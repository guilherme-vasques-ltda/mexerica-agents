//! D70 do PR5 nas fronteiras de Sessões: fechamento manual atômico e só da extra, cerca do despacho, vínculo,
//! continuação (cartão por sessão, resumo só da origem como dado, exposição herdada, D1), arquivamento automático
//! (P4: relê config e pendências na Tx) e a configuração. O lado do ator: `agent/bot/fork_sessoes_tests.rs`.
#![allow(clippy::unwrap_used)]
use super::encerrar;
use super::{bindings, exposure, origin::SessionOrigin, store};
use crate::{
    LockExt,
    api::{self, devices::Caller},
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Store, now_ms},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const DAY: i64 = 24 * 60 * 60 * 1000;

fn hub() -> (Arc<Hub>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("sessoes-pr5-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(Path::new(":memory:")).unwrap();
    for id in ["b", "c"] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id": id, "name": id, "backend": "claude", "cwd": "/tmp"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    (hub, dir)
}

fn extra(hub: &Hub, chat: &str) -> String {
    let c = hub.store.db.locked();
    store::create(
        &c,
        &store::NewSession {
            chat: chat.into(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: "Origem".into(),
            runs_on: None,
        },
    )
    .unwrap()
    .id
}

fn lane(session: &str) -> Lane {
    Lane { chat: "b".into(), session: Some(session.into()) }
}

fn status(hub: &Hub, id: &str) -> Value {
    hub.store.entry(id).unwrap().data["status"].clone()
}

fn revision(hub: &Hub, id: &str) -> i64 {
    store::get(&hub.store.db.locked(), id).unwrap().unwrap().revision
}

fn close(hub: &Hub, id: &str) -> anyhow::Result<encerrar::Closed> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction().unwrap();
    let closed = encerrar::close_on(&tx, "b", id, revision_on(&tx, id))?;
    tx.commit().unwrap();
    Ok(closed)
}

fn revision_on(c: &rusqlite::Connection, id: &str) -> i64 {
    store::get(c, id).unwrap().unwrap().revision
}

fn idle(hub: &Hub, id: &str) -> Option<store::SessionRow> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction().unwrap();
    let closed = encerrar::close_idle_on(&tx, "b", id, now_ms()).unwrap();
    tx.commit().unwrap();
    closed
}

/// Tudo da sessão fica "antigo": criação e entradas há `days` dias.
fn age(hub: &Hub, id: &str, days: i64) {
    let at = now_ms() - days * DAY;
    let c = hub.store.db.locked();
    c.execute("UPDATE donna_bot_sessions SET created_at=?2,last_used_at=?2 WHERE id=?1", rusqlite::params![id, at])
        .unwrap();
    c.execute("UPDATE entries SET created_at=?2,updated_at=?2 WHERE session=?1", rusqlite::params![id, at]).unwrap();
}

#[test]
fn fechar_cancela_so_a_extra_na_mesma_tx_e_recusa_principal_cas_e_repeticao() {
    let (hub, dir) = hub();
    let (a, irma) = (extra(&hub, "b"), extra(&hub, "b"));
    let fila_a = hub.store.insert_entry(&lane(&a), EntryKind::User, 1, &json!({"text":"A","status":"queued"})).unwrap();
    let perm_a =
        hub.store.insert_entry(&lane(&a), EntryKind::Permission, 1, &json!({"author":"b","status":"pending"})).unwrap();
    let fila_irma =
        hub.store.insert_entry(&lane(&irma), EntryKind::User, 2, &json!({"text":"B","status":"queued"})).unwrap();
    let fila_main =
        hub.store.insert_entry(&Lane::main("b"), EntryKind::User, 3, &json!({"text":"P","status":"queued"})).unwrap();
    // Checkpoint de espera da extra (retomada pendente): some como retomada na MESMA Tx do fechamento.
    let waiting = json!({"phase":"waiting","input":{"chat":"b","visibleSession":a}});
    hub.store.kv_set("turn.waiting.b", &waiting.to_string()).unwrap();
    let before = revision(&hub, &a);

    {
        let mut db = hub.store.db.locked();
        let tx = db.transaction().unwrap();
        for (id, rev, code) in [
            ("b", revision_on(&tx, "b"), "principal_not_closable"),
            (a.as_str(), before + 1000, "session_revision_conflict"),
            ("inexistente", 0, "unknown_session"),
        ] {
            let error = encerrar::close_on(&tx, "b", id, rev).err().unwrap();
            assert!(format!("{error:#}").contains(code), "{error:#}");
        }
        // Sessão de outro bot pelo chat errado.
        let other = store::create(
            &tx,
            &store::NewSession {
                chat: "c".into(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: String::new(),
                runs_on: None,
            },
        )
        .unwrap();
        let rev = revision_on(&tx, &other.id);
        let error = encerrar::close_on(&tx, "b", &other.id, rev).err().unwrap();
        assert!(format!("{error:#}").contains("unknown_session"));
    }
    assert_eq!(status(&hub, &fila_a.id), "queued", "recusa não tem efeito");

    let closed = close(&hub, &a).unwrap();
    assert_eq!((closed.session.state.as_str(), closed.session.closed_reason.as_deref()), ("closed", Some("owner")));
    assert!(closed.session.revision > before && closed.session.closed_at.is_some());
    assert_eq!(closed.cancelled, vec![fila_a.id.clone()]);
    assert_eq!(status(&hub, &fila_a.id), "cancelled");
    assert_eq!(status(&hub, &perm_a.id), "cancelled");
    // Irmã e principal intactas; histórico mantido.
    assert_eq!(status(&hub, &fila_irma.id), "queued");
    assert_eq!(status(&hub, &fila_main.id), "queued");
    assert!(hub.store.entry(&fila_a.id).is_some());
    assert_eq!(store::get(&hub.store.db.locked(), &irma).unwrap().unwrap().state, "open");
    let saved: Value = serde_json::from_str(&hub.store.kv_get("turn.waiting.b").unwrap()).unwrap();
    assert_eq!(saved["phase"], "finished", "tombstone na Tx: queda depois do commit não retoma");
    // Checkpoint de OUTRA sessão não é tocado pelo fechamento.
    let other = json!({"phase":"waiting","input":{"chat":"b","visibleSession":irma}});
    hub.store.kv_set("turn.waiting.b", &other.to_string()).unwrap();
    let terceira = extra(&hub, "b");
    close(&hub, &terceira).unwrap();
    let saved: Value = serde_json::from_str(&hub.store.kv_get("turn.waiting.b").unwrap()).unwrap();
    assert_eq!(saved["phase"], "waiting");
    // Repetir não reabre nada.
    assert!(format!("{:#}", close(&hub, &a).err().unwrap()).contains("session_closed"));
    let _ = std::fs::remove_dir_all(dir);
}

/// Execução viva de uma extra pelo produtor canônico: vínculo emitido e ativado no slot dela, runtime do turno dela.
fn live_execution(hub: &Hub, session: &str, acp: &str) -> bindings::ExecutionRef {
    let slot = format!("session:{session}");
    let binding = bindings::issue_in(&hub.store, "b", "b", session, Some(&slot), Some(acp)).unwrap();
    bindings::activate(&hub.store, &binding, acp).unwrap();
    hub.set_runtime("b", |r| {
        r.status = crate::hub::BotStatus::Working;
        r.lane = Some(lane(session));
        r.session = Some(acp.into());
        r.memory_turn_token = Some("turno-extra".into());
    });
    bindings::capture(hub, "b", binding.id.as_str()).unwrap()
}

#[test]
fn extra_fechada_nao_recebe_dispatch_resume_permissao_nem_pergunta_atrasada() {
    let (hub, dir) = hub();
    let (a, irma) = (extra(&hub, "b"), extra(&hub, "b"));
    let execution = live_execution(&hub, &a, "acp-a");
    let cfg = hub.store.bot("b").unwrap().unwrap().config;
    // Antes de fechar, os consumidores reais aceitam a execução viva (o fixture é válido).
    let asked = crate::fork::owner_question::ask(&hub, "b", &lane(&a), 1, "Qual cor?", &execution).unwrap();
    close(&hub, &a).unwrap();
    // A pergunta pendente foi retirada na Tx do fechamento.
    assert_ne!(hub.store.entry(&asked.id).unwrap().data["ownerQuestion"]["status"], "pending");
    let count = || -> i64 {
        hub.store.db.locked().query_row("SELECT COUNT(*) FROM entries WHERE session=?1", [&a], |r| r.get(0)).unwrap()
    };
    let before = count();
    // Pedido de permissão e pergunta atrasados da sessão fechada: recusados sem efeito (S4).
    let card = json!({"author":"b","status":"pending","options":[]});
    assert!(hub.add_permission_card(&cfg, &lane(&a), 1, &card, &execution, None).is_err());
    assert!(crate::fork::owner_question::ask(&hub, "b", &lane(&a), 1, "De novo?", &execution).is_err());
    assert_eq!(count(), before, "nenhuma entrada nova na sessão fechada");
    // Entrada que chegou à extra por fora (atrasada) e a da irmã.
    let atrasada =
        hub.store.insert_entry(&lane(&a), EntryKind::User, 1, &json!({"text":"tarde","status":"queued"})).unwrap();
    let viva = json!({"text":"ok","status":"queued"});
    let viva = hub.store.insert_entry(&lane(&irma), EntryKind::User, 2, &viva).unwrap();
    let c = hub.store.db.locked();
    let atrasadas = std::slice::from_ref(&atrasada.id);
    for checkpoint in [None, Some(r#"{"executionToken":"t","phase":"retrying","input":{"chat":"b"}}"#)] {
        let error = Store::claim_user_turn_on(&c, "b", "b", checkpoint, atrasadas, Some(&a), "t").err().unwrap();
        assert!(format!("{error:#}").contains("session_closed"), "{error:#}");
    }
    let status: String = c
        .query_row("SELECT json_extract(data,'$.status') FROM entries WHERE id=?1", [&atrasada.id], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "queued");
    assert!(Store::claim_user_turn_on(&c, "b", "b", None, std::slice::from_ref(&viva.id), Some(&irma), "t").is_ok());
    drop(c);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn continuar_cria_outra_extra_com_cartao_proprio_resumo_canonico_da_origem_como_dado_e_d1() {
    let (hub, dir) = hub();
    let (a, irma) = (extra(&hub, "b"), extra(&hub, "b"));
    let ins = |l: &Lane, kind, data: Value| hub.store.insert_entry(l, kind, 1, &data).unwrap();
    ins(&lane(&a), EntryKind::User, json!({"text":"PEDIDO-A","status":"sent"}));
    ins(&lane(&a), EntryKind::Agent, json!({"text":"RESPOSTA-A com CONTEUDO-EXTERNO lido do site","final":true}));
    ins(&lane(&irma), EntryKind::User, json!({"text":"SEGREDO-IRMA","status":"sent"}));
    ins(&Lane::main("b"), EntryKind::User, json!({"text":"SEGREDO-PRINCIPAL","status":"sent"}));
    {
        let c = hub.store.db.locked();
        // Origem com seleção própria (D1: não copiada).
        let own = json!({"schemaVersion":2,"source":"own","accountId":format!("ac_{}", "a".repeat(64)),
            "backend":"claude","model":"sonnet","modelOptions":{},"fastMode":false});
        c.execute("UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1", rusqlite::params![a, own.to_string()])
            .unwrap();
        // A ACP da origem e o conteúdo externo pelos produtores reais: ligação ACP → sessão visível e marca da
        // linhagem, que o gatilho copia para a exposição da sessão visível.
        exposure::bind(&c, "b", "acp-irma", &irma).unwrap();
        exposure::bind(&c, "b", "acp-a", &a).unwrap();
        bindings::mark(&c, "b", "acp-a", "conteúdo externo").unwrap();
    }
    assert!(exposure::exposed(&hub.store.db.locked(), "b", &a));
    // Resumo do motor guardado para a ACP canônica da origem (e um de outra sessão, que não pode entrar).
    hub.store.kv_set(&crate::chat::handoff::summary_key("b", "acp-a"), "RESUMO-DO-MOTOR-A").unwrap();
    hub.store.kv_set(&crate::chat::handoff::summary_key("b", "acp-irma"), "SEGREDO-RESUMO-IRMA").unwrap();
    let call = |method: &'static str, body: Value| api::dispatch(&hub, &Caller::Local, method, body);
    let body = |rev: i64| json!({"botId":"b","sessionId":a,"expectedRevision":rev});
    // Origem aberta, principal e revisão vencida: recusadas sem criar sessão.
    let open = call("sessionContinue", body(revision(&hub, &a))).await.unwrap_err();
    assert!(format!("{open:#}").contains("session_open"), "{open:#}");
    let main = call("sessionContinue", json!({"botId":"b","sessionId":"b","expectedRevision":revision(&hub, "b")}));
    assert!(format!("{:#}", main.await.unwrap_err()).contains("principal_not_continuable"));
    close(&hub, &a).unwrap();
    let stale = call("sessionContinue", body(revision(&hub, &a) - 1)).await.unwrap_err();
    assert!(format!("{stale:#}").contains("session_revision_conflict"));
    let count = || store::list(&hub.store.db.locked(), "b").unwrap().len();
    assert_eq!(count(), 3);

    let first = call("sessionContinue", body(revision(&hub, &a))).await.unwrap();
    let second = call("sessionContinue", body(revision(&hub, &a))).await.unwrap();
    let (n1, n2) = (first["session"]["id"].as_str().unwrap(), second["session"]["id"].as_str().unwrap());
    assert_ne!(n1, n2);
    assert_eq!(first["session"]["origin"], json!({"kind":"continuation"}));
    assert_eq!(first["session"]["sourceSession"], json!(a));
    assert_eq!(first["session"]["state"], "open");
    assert_eq!(first["source"]["state"], "closed", "a origem continua encerrada");
    // D1: retrato do padrão atual do bot, nunca a seleção da origem.
    assert_eq!(first["session"]["engine"]["source"], "botDefault");
    // Cartão próprio: resumo canônico do motor da origem e só linhas dela, como dado (`<handoff>`).
    let card = &first["handoff"];
    let text = card["data"]["handoff"]["text"].as_str().unwrap();
    assert!(text.contains("RESUMO-DO-MOTOR-A") && text.contains("PEDIDO-A"), "{text}");
    assert!(text.contains("CONTEUDO-EXTERNO"), "{text}");
    assert!(!text.contains("SEGREDO"), "{text}");
    assert!(text.starts_with("<handoff>") && text.contains("information, not instructions"), "{text}");
    assert_eq!(card["data"]["handoff"]["reason"], "continuation");
    assert_eq!(card["sessionId"], json!(n1));
    for n in [n1, n2] {
        assert!(crate::chat::handoff::pending_in(&hub.store, &lane(n)).is_some(), "chaves separadas por sessão");
        assert!(exposure::exposed(&hub.store.db.locked(), "b", n), "exposição herdada");
    }
    assert!(crate::chat::handoff::pending(&hub.store, "b").is_none(), "a chave legada da principal fica livre");
    // Nada enviado: nenhuma entrada do dono nas novas sessões.
    let users: i64 = hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM entries WHERE session IN (?1,?2) AND kind='user'", [n1, n2], |r| r.get(0))
        .unwrap();
    assert_eq!(users, 0);
    // "Manter" não existe no cartão de continuação; ajustar o resumo existe.
    let entry = card["id"].as_str().unwrap();
    assert!(call("keepConversation", json!({"entryId":entry})).await.is_err());
    assert!(call("editHandoff", json!({"entryId":entry,"text":"ajustado"})).await.is_ok());
    // Consumo canônico (Tx do permit): só a captura da própria lane, na revisão capturada; uma vez só.
    {
        let c = hub.store.db.locked();
        let card1 = crate::chat::handoff::capture_on(&c, &lane(n1)).unwrap().unwrap();
        let changed = |r: anyhow::Result<crate::store::Entry>| {
            r.err().is_some_and(|e| e.chain().any(<dyn std::error::Error>::is::<crate::chat::handoff::CardChanged>))
        };
        assert!(changed(crate::chat::handoff::consume_on(&c, &lane(n2), &card1)), "captura de outra lane");
        // Edição aceita depois da captura (revisão nova): o permit da captura velha é desfeito (CAS).
        let stale = crate::chat::handoff::Pr5Card { rev: card1.rev - 1, ..card1.clone() };
        assert!(changed(crate::chat::handoff::consume_on(&c, &lane(n1), &stale)));
        let consumed = crate::chat::handoff::consume_on(&c, &lane(n1), &card1).unwrap();
        assert_eq!(consumed.data["handoff"]["status"], "sent");
        assert!(changed(crate::chat::handoff::consume_on(&c, &lane(n1), &card1)), "uma vez só");
    }
    assert!(crate::chat::handoff::pending_in(&hub.store, &lane(n1)).is_none());
    assert!(crate::chat::handoff::pending_in(&hub.store, &lane(n2)).is_some());
    // A ACP nova da continuação exposta nasce marcada como externa (D125).
    exposure::carry(&hub.store, "b", &lane(n2), "acp-n2").unwrap();
    let marked: Option<i64> = hub
        .store
        .db
        .locked()
        .query_row("SELECT marked_at FROM donna_memory_sessions WHERE bot='b' AND session='acp-n2'", [], |r| r.get(0))
        .unwrap();
    assert!(marked.is_some());
    // Fechar a continuação mata o cartão: não pode mais ser editado.
    close(&hub, n2).unwrap();
    assert!(crate::chat::handoff::pending_in(&hub.store, &lane(n2)).is_none());
    let card2 = second["handoff"]["id"].as_str().unwrap();
    let before = hub.store.entry(card2).unwrap();
    assert!(call("editHandoff", json!({"entryId":card2,"text":"x"})).await.is_err());
    assert!(call("keepConversation", json!({"entryId":card2})).await.is_err());
    let after = hub.store.entry(card2).unwrap();
    assert_eq!((after.rev, &after.data), (before.rev, &before.data), "sessão fechada: nenhum efeito");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn arquivamento_automatico_so_fecha_extra_inativa_sem_pendencia_e_rele_config_na_tx() {
    let (hub, dir) = hub();
    let a = extra(&hub, "b");
    // Recente: não fecha. Principal nunca é candidata.
    assert!(idle(&hub, &a).is_none());
    age(&hub, &a, 4);
    {
        let c = hub.store.db.locked();
        assert_eq!(encerrar::idle_candidates_for_test(&c, now_ms()).unwrap(), vec![("b".into(), a.clone())]);
        // Renomear e trocar revisão não renovam atividade (L1).
        let rev = revision_on(&c, &a);
        store::rename(&c, "b", &a, "Novo nome", rev).unwrap();
        assert_eq!(encerrar::idle_candidates_for_test(&c, now_ms()).unwrap().len(), 1);
    }
    // Cada pendência durável segura a sessão; nada é cancelado. Ilegível conta como pendente (adia).
    let pergunta = json!({"card":"ownerQuestion","ownerQuestion":{"status":"pending"}});
    let espera = json!({"phase":"waiting","input":{"chat":"b","visibleSession":a}}).to_string();
    let pendentes = [
        ("fila", Some((EntryKind::User, json!({"status":"queued"}))), None),
        ("permissão", Some((EntryKind::Permission, json!({"status":"pending"}))), None),
        ("pergunta", Some((EntryKind::Notice, pergunta)), None),
        ("espera", None, Some(("turn.waiting.b", espera))),
        ("espera ilegível", None, Some(("turn.waiting.b", "{quebrado".to_owned()))),
        ("admissão sem entradas", None, Some(("turn.inflight.b", json!({"admissionVersion":1}).to_string()))),
        ("admissão ilegível", None, Some(("turn.inflight.b", "{quebrado".to_owned()))),
        ("nunca", None, Some(("session.autoclose.v1", r#"{"days":null,"revision":1}"#.to_owned()))),
    ];
    for (what, entry, kv) in pendentes {
        let id = entry.map(|(kind, data)| hub.store.insert_entry(&lane(&a), kind, 1, &data).unwrap().id);
        if let Some((key, value)) = &kv {
            hub.store.kv_set(key, value).unwrap();
        }
        age(&hub, &a, 4);
        assert!(idle(&hub, &a).is_none(), "{what}");
        assert_eq!(store::get(&hub.store.db.locked(), &a).unwrap().unwrap().state, "open", "{what}");
        let c = hub.store.db.locked();
        if let Some(id) = id {
            c.execute("DELETE FROM entries WHERE id=?1", [&id]).unwrap();
        }
        if let Some((key, _)) = kv {
            c.execute("DELETE FROM kv WHERE k=?1", [key]).unwrap();
        }
    }
    // Cartão de continuação ainda em rascunho é trabalho preservado: não fecha e o cartão continua.
    let draft = json!({"handoff":{"status":"draft","reason":"continuation","text":"resumo"}});
    let draft = hub.store.insert_entry(&lane(&a), EntryKind::Notice, 1, &draft).unwrap();
    let key = crate::chat::handoff::key_for(&lane(&a));
    hub.store.kv_set(&key, &draft.id).unwrap();
    age(&hub, &a, 4);
    assert!(idle(&hub, &a).is_none());
    assert_eq!(hub.store.kv_get(&key).as_deref(), Some(draft.id.as_str()));
    assert_eq!(hub.store.entry(&draft.id).unwrap().data["handoff"]["status"], "draft");
    hub.store.db.locked().execute("DELETE FROM kv WHERE k=?1", [&key]).unwrap();
    // Escopo canônico relido na Tx: bot apagado não tem extras arquivadas por aqui.
    hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='b'", []).unwrap();
    age(&hub, &a, 4);
    assert!(idle(&hub, &a).is_none());
    let candidates = encerrar::idle_candidates_for_test(&hub.store.db.locked(), now_ms()).unwrap();
    assert_eq!(candidates, Vec::<(String, String)>::new());
    hub.store.db.locked().execute("UPDATE bots SET deleted=0 WHERE id='b'", []).unwrap();
    // Prazo maior relido na Tx: um comando antigo não fecha.
    hub.store.kv_set("session.autoclose.v1", r#"{"days":30,"revision":2}"#).unwrap();
    assert!(idle(&hub, &a).is_none());
    hub.store.db.locked().execute("DELETE FROM kv WHERE k='session.autoclose.v1'", []).unwrap();
    // Atividade concorrente entre a leitura da varredura e a Tx.
    hub.store.insert_entry(&lane(&a), EntryKind::Agent, 1, &json!({"text":"agora","final":true})).unwrap();
    assert!(idle(&hub, &a).is_none());
    age(&hub, &a, 4);
    let closed = idle(&hub, &a).unwrap();
    assert_eq!((closed.state.as_str(), closed.closed_reason.as_deref()), ("closed", Some("idle")));
    assert!(idle(&hub, "b").is_none(), "principal nunca");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn configuracao_padrao_tres_dias_faixa_nunca_e_cas() {
    let (hub, dir) = hub();
    let call = |method: &'static str, body: Value| api::dispatch(&hub, &Caller::Local, method, body);
    let read = call("sessionAutoCloseConfig", json!({})).await.unwrap();
    assert_eq!(read["config"]["days"], 3);
    let rev = read["config"]["revision"].as_i64().unwrap();
    for days in [json!(0), json!(91), json!("3")] {
        assert!(call("sessionAutoCloseSave", json!({"days":days,"expectedRevision":rev})).await.is_err());
    }
    assert!(call("sessionAutoCloseSave", json!({"expectedRevision":rev})).await.is_err(), "days é obrigatório");
    let never = call("sessionAutoCloseSave", json!({"days":null,"expectedRevision":rev})).await.unwrap();
    assert_eq!(never["config"]["days"], Value::Null);
    let stale = call("sessionAutoCloseSave", json!({"days":5,"expectedRevision":rev})).await.unwrap_err();
    assert!(format!("{stale:#}").contains("autoclose_revision_conflict"));
    let rev = never["config"]["revision"].as_i64().unwrap();
    let five = call("sessionAutoCloseSave", json!({"days":5,"expectedRevision":rev})).await.unwrap();
    assert_eq!(five["config"]["days"], 5);
    let _ = std::fs::remove_dir_all(dir);
}
