//! Sono 2b1c: atalho literal (`auto` com recibo e Desfazer; `suggest` sem tocar `memory_entries`), validação da
//! linhagem inteira, lote que não trava (R2-D1) e o motivo NULL na reserva. Casos da linha 2b1 da tabela.
use super::*;
use crate::fork::{
    autoridade::Autoridade,
    memory_auto::{sleep, sleep_checks, suggestions},
    modelos_funcao::{catalogo::FunctionId, config},
};
use codync_mem::nature::{Hint, KindNature};

fn later() -> i64 {
    crate::store::now_ms() + sleep::IDLE_MS + 60_000
}

/// A seleção de fundo do Pessoal (conta Claude por assinatura), como em `background_memory_auto`: sem ela o sono não
/// tem autorização do Space e não aplica nem sugere o atalho.
pub(super) fn authorize(f: &Fixture) {
    let account = format!("ac_{}", "a".repeat(64));
    let registry = json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
        "id":account,"backend":"claude","billing":"subscription","state":"connected",
        "launch":{"kind":"cli_profile","executable":"/bin/false","directory":"/fixture/profile"},
        "scope":"owner","quotaIdentity":account,"verifiedAt":1,
        "subscriptionProof":{"method":"claude_auth_status","plan":"max"},
        "models":[{"id":"haiku","options":[]}]}]});
    f.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
    config::initialize(&f.hub.store).unwrap();
    let binding = config::Binding {
        account_id: account,
        model_id: "haiku".into(),
        model_options: std::collections::BTreeMap::new(),
    };
    let revision = config::load(&f.hub.store).unwrap().revision;
    let input = config::SelectionInput::Custom { binding };
    config::save(&f.hub.store, revision, Some(FunctionId::MemoryConsolidation), Some(input)).unwrap();
}

fn inferred(f: &Fixture, text: &str) -> Operation {
    Operation::Remember {
        text: text.into(),
        shared: false,
        evidence: vec![f.message.clone()],
        nature: Some(Hint::Inferred),
    }
}

/// A mensagem M do primeiro turno passa a ter `text`, inteira digitada (mesma revisão: o `Context` continua valendo).
fn say(f: &Fixture, text: &str) {
    let data = json!({"text":text,"contentOrigin":{"status":"appReported","report":{"channel":"desktopComposer"},
        "segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}});
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET data=?2 WHERE id=?1", params![f.message, data.to_string()])
        .unwrap();
}

/// Aplica as operações no trabalho do primeiro turno e devolve as lembranças da camada. M diz, em frases inteiras,
/// "Moro em Curitiba." e "Prefiro café sem açúcar.".
fn learn(f: &Fixture, ops: Vec<Operation>) -> Vec<mem::Entry> {
    say(f, "Moro em Curitiba. Prefiro café sem açúcar.");
    let r = success(f);
    apply::complete(&f.hub, &r, ExtractionPlan { operations: ops }, |_, _| Ok(())).unwrap();
    f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap()
}

fn get(f: &Fixture, id: i64) -> mem::Entry {
    f.hub.store.memory_read(|c| mem::get(c, id)).unwrap().unwrap()
}

fn by_text(entries: &[mem::Entry], text: &str) -> mem::Entry {
    entries.iter().find(|e| e.text == text).unwrap().clone()
}

fn tick(f: &Fixture) {
    sleep::try_tick(&f.hub, "bot", later()).unwrap();
}

fn rows(f: &Fixture) -> String {
    let db = f.hub.store.db.locked();
    let mut stmt = db.prepare("SELECT id,rev,text,nature,evidence,state FROM memory_entries ORDER BY id").unwrap();
    let rows: Vec<String> = stmt
        .query_map([], |r| {
            Ok(format!(
                "{}:{}:{}:{}:{}:{}",
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows.join("|")
}

/// `auto` só aplica o atalho literal: a inferência cujo texto está na mensagem limpa vira "Você disse", com recibo e
/// cartão; a que não está literal fica como estava. O Desfazer volta à inferência e o sono não confirma de novo.
#[test]
fn auto_aplica_so_o_atalho_literal_com_recibo_e_desfazer() {
    let f = fixture(false);
    authorize(&f);
    let entries = learn(&f, vec![inferred(&f, "Moro em Curitiba"), inferred(&f, "Gosta de cidades frias")]);
    let city = by_text(&entries, "Moro em Curitiba");
    let other = by_text(&entries, "Gosta de cidades frias");
    tick(&f);
    let confirmed = get(&f, city.id);
    assert_eq!(confirmed.nature, KindNature::Said);
    assert_eq!(confirmed.origin.kind, Kind::User, "a origem é a fala do dono");
    assert!(confirmed.rev > city.rev, "versão nova");
    assert_eq!(get(&f, other.id).nature, KindNature::Inferred, "sem literal, nada muda");
    let notice: String = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT id FROM donna_memory_auto_receipts WHERE entry=?1 AND json_extract(operation,'$.action')='confirm'",
            [city.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(f.hub.store.entry(&notice).unwrap().data["memory"]["status"], "done");
    assert_eq!(
        apply::undo(&f.hub, &notice, &Autoridade::local()).unwrap()["entry"]["data"]["memory"]["status"],
        "undone"
    );
    assert_eq!(get(&f, city.id).nature, KindNature::Inferred, "o Desfazer volta à inferência");
    f.hub.store.db.locked().execute("DELETE FROM donna_memory_sleep_jobs", []).unwrap_or_default();
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap(), "há o que provar");
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().expect("a segunda rodada roda");
    sleep::run(&f.hub, &r, later(), None).unwrap();
    let done: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_memory_sleep_jobs WHERE status='done'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(done, 1, "a segunda rodada terminou");
    assert_eq!(get(&f, city.id).nature, KindNature::Inferred, "desfeito, o sono não confirma de novo");
}

/// `suggest`: vira sugestão e não toca `memory_entries`; aprovada pelo dono, aplica como o automático.
#[test]
fn suggest_vira_sugestao_sem_tocar_memory_entries_e_o_dono_aprova() {
    let f = fixture(false);
    authorize(&f);
    let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "suggest").unwrap();
    let before = rows(&f);
    tick(&f);
    assert_eq!(rows(&f), before, "suggest não toca memory_entries");
    let list = suggestions::list(&f.hub.store, &f.cfg).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["action"], "confirm");
    assert!(!sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", &f.message).unwrap());
    let id = list[0]["id"].as_str().unwrap().to_owned();
    suggestions::approve(&f.hub, &f.cfg, &id, &Autoridade::local()).unwrap();
    assert_eq!(get(&f, city.id).nature, KindNature::Said);
}

/// Mensagem mista (trecho digitado + colado): o literal está no trecho digitado, mas a mensagem não é limpa.
#[test]
fn mensagem_mista_nao_vira_said() {
    let f = fixture(false);
    authorize(&f);
    let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    let data = json!({"text":"Moro em Curitiba. Prefiro café sem açúcar.",
        "contentOrigin":{"status":"appReported","report":{"channel":"desktopComposer"},
        "segments":[{"kind":"typed","start":0,"end":17},{"kind":"pasted","start":17,"end":42}]}});
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET data=?2 WHERE id=?1", params![f.message, data.to_string()])
        .unwrap();
    tick(&f);
    assert_eq!(get(&f, city.id).nature, KindNature::Inferred);
}

/// Conflito com o literal antigo: o texto conservado ("Moro em Curitiba") está na mensagem, mas conflito nunca usa o
/// atalho (a alternativa de N não some).
#[test]
fn conflito_com_literal_antigo_nao_reafirma() {
    let f = fixture(false);
    authorize(&f);
    let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    let (n, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let revise = Operation::Revise {
        entry: city.id,
        expected_rev: city.rev,
        text: "Moro em Recife".into(),
        evidence: vec![n],
        nature: None,
    };
    apply::complete(&f.hub, &r, plan(revise), |_, _| Ok(())).unwrap();
    assert_eq!(get(&f, city.id).nature, KindNature::Conflict);
    let before = rows(&f);
    tick(&f);
    assert_eq!(rows(&f), before, "conflito não é reafirmado");
}

/// Linhagem: uma fonte de outra conversa da mesma memória exclui o candidato; uma versão sem recibo (fixar e
/// desafixar pelo dono) também.
#[test]
fn fonte_de_outro_chat_ou_versao_sem_recibo_exclui_o_candidato() {
    for case in ["outro chat", "sem recibo"] {
        let f = fixture(false);
        authorize(&f);
        let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
        if case == "outro chat" {
            let other = f
                .hub
                .add_entry(&Lane::main("outro"), EntryKind::User, 1, &json!({"text":"Moro em Curitiba"}))
                .unwrap()
                .id;
            let origin = json!({"schema":1,"kind":"conversation","external":false,
                "sources":[{"messageId":f.message,"kind":"user"},{"messageId":other,"kind":"user"}]});
            f.hub
                .store
                .db
                .locked()
                .execute("UPDATE memory_entries SET origin=?2 WHERE id=?1", params![city.id, origin.to_string()])
                .unwrap();
        } else {
            for pinned in [true, false] {
                f.hub.store.memory_write(|c, now, rev| mem::pin(c, city.id, pinned, now, rev)).unwrap();
            }
        }
        tick(&f);
        assert_eq!(get(&f, city.id).nature, KindNature::Inferred, "{case}");
    }
}

/// D412: M → nota L e fato F; `covered` de M; conflito de L por N → a C7 recusa M.
#[test]
fn conflito_da_nota_da_linhagem_faz_a_c7_recusar_m() {
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let bot = format!("sono-c-{}", uuid::Uuid::new_v4());
    let f = fixture_for_bot(false, &bot);
    let r = success(&f);
    let note = |content: &str, evidence: &str, nature| Operation::Note {
        file: "viagem.md".into(),
        content: Some(format!("---\ndescription: Viagem\n---\n- {content}\n")),
        shared: false,
        expected_hash: None,
        evidence: vec![evidence.to_owned()],
        nature,
    };
    let fact = Operation::Remember {
        text: "Moro em Curitiba".into(),
        shared: false,
        evidence: vec![f.message.clone()],
        nature: None,
    };
    apply::complete(
        &f.hub,
        &r,
        ExtractionPlan { operations: vec![note("Curitiba.", &f.message, None), fact] },
        |_, _| Ok(()),
    )
    .unwrap();
    sleep::try_tick(&f.hub, &bot, later()).unwrap();
    let cleanable =
        |f: &Fixture| sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), &bot, &bot, &f.message).unwrap();
    assert!(!cleanable(&f), "nota na linhagem retém M");
    // Conflito da nota L por N (evidência só N): M continua retida.
    let (n, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let current = f.hub.store.memory_read(|db| codync_mem::files::current(db, Some(&bot), "viagem.md")).unwrap();
    let mut conflict = note("Recife.", &n, Some(Hint::Conflict));
    if let Operation::Note { expected_hash, .. } = &mut conflict {
        *expected_hash = current.as_deref().map(codync_mem::files::hash);
    }
    apply::complete(&f.hub, &r, plan(conflict), |_, _| Ok(())).unwrap();
    assert!(!cleanable(&f), "conflito de L por N: a C7 recusa M");
    let _ = std::fs::remove_dir_all(crate::chat::memory::files::folder(Some(&bot)).parent().unwrap());
}

/// R2-D1: 501 mensagens que não passaram pela memória antes de N não ocupam o lote; N, aplicada depois, entra.
#[test]
fn mensagens_seguradas_nao_ocupam_o_lote() {
    let f = fixture(false);
    learn(&f, vec![remember(&f, "Moro em Curitiba")]);
    for i in 0..501 {
        f.hub.add_entry(&Lane::main("bot"), EntryKind::User, 1, &json!({"text":format!("ok {i}")})).unwrap();
    }
    let (n, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    apply::complete(&f.hub, &r, ExtractionPlan::default(), |_, _| Ok(())).unwrap();
    tick(&f);
    let covered: bool = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_checks WHERE message=?1 AND status='covered')",
            [&n],
            |r| r.get(0),
        )
        .unwrap();
    assert!(covered);
}

/// Ressalva da Segurança: extração esperando com `attempts>0` e motivo NULL não é a "presa sem seleção"; segura o sono.
#[test]
fn extracao_com_motivo_nulo_e_tentada_segura_o_sono() {
    let f = fixture(false);
    learn(&f, vec![remember(&f, "Moro em Curitiba")]);
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let (_, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_memory_auto_jobs SET attempts=1,reason=NULL WHERE id=?1", [&job])
        .unwrap();
    assert!(sleep::reserve(&f.hub.store, "bot", later()).unwrap().is_none());
}

/// `confirm` é do Hub: vinda no plano do extrator, é ignorada (nada muda e o lote não falha).
#[test]
fn confirmar_no_plano_do_extrator_e_ignorado() {
    let f = fixture(false);
    let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    let (_, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let op = Operation::Confirm { entry: city.id, expected_rev: city.rev, evidence: vec![f.message.clone()] };
    assert_eq!(apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap().len(), 0);
    assert_eq!(get(&f, city.id).nature, KindNature::Inferred);
    // SEG-507-02: o plano só tinha a operação ignorada; o `noChange` fica sem a prova e N não conta como lida.
    let proof: Option<String> = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT proof FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| r.get(0))
        .unwrap();
    assert_eq!(proof.as_deref(), Some("unproven"));
    tick(&f);
    let n = f.hub.store.entry(&second_message(&f, &job)).unwrap();
    assert!(!sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", &n.id).unwrap());
    let covered: bool = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_checks WHERE message=?1)", [&n.id], |r| r.get(0))
        .unwrap();
    assert!(!covered);
}

fn second_message(f: &Fixture, job: &str) -> String {
    let raw: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [job], |r| r.get(0))
        .unwrap();
    serde_json::from_str::<queue::Context>(&raw).unwrap().messages[0].id.clone()
}

/// R1-01: o fato precisa ser uma FRASE INTEIRA da mensagem limpa; trecho dentro de negação ou condição não vale.
#[test]
fn so_frase_inteira_confirma() {
    use crate::chat::memory::write_gate::whole_sentence;
    let msg = |text: &str| json!({ "text": text });
    assert!(!whole_sentence(&msg("Não moro em Curitiba; moro em Recife."), "moro em Curitiba"));
    assert!(!whole_sentence(&msg("Se eu mudar de trabalho, moro em Curitiba."), "moro em Curitiba"));
    assert!(whole_sentence(&msg("Moro em Curitiba."), "Moro em Curitiba"));
    assert!(whole_sentence(&msg("Não moro em Curitiba; moro em Recife."), "moro em Recife"));
    assert!(whole_sentence(&msg("Oi!\nMoro em Curitiba"), "Moro em Curitiba."));
    for (message, fact, confirmed) in [
        ("Não moro em Curitiba; moro em Recife.", "moro em Curitiba", false),
        ("Se eu mudar de trabalho, moro em Curitiba.", "moro em Curitiba", false),
        ("Não moro em Curitiba; moro em Recife.", "moro em Recife", true),
    ] {
        let f = fixture(false);
        authorize(&f);
        let r = success(&f);
        say(&f, message);
        apply::complete(&f.hub, &r, plan(inferred(&f, fact)), |_, _| Ok(())).unwrap();
        let entry = f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().remove(0);
        tick(&f);
        let nature = if confirmed { KindNature::Said } else { KindNature::Inferred };
        assert_eq!(get(&f, entry.id).nature, nature, "{message} / {fact}");
    }
}

/// R1-02 = SEG-507-01: literal extraído limpo, mas a sessão foi marcada como exposta depois → não confirma.
#[test]
fn sessao_exposta_depois_da_extracao_nao_confirma() {
    let f = fixture(false);
    authorize(&f);
    let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    f.hub.store.memory_session_mark("bot", "session", "entrada externa").unwrap();
    tick(&f);
    assert_eq!(get(&f, city.id).nature, KindNature::Inferred);
}

/// R1-03: 20 candidatas sem literal à frente não travam a seleção; a 21ª entra na rodada seguinte.
#[test]
fn a_selecao_avanca_depois_de_vinte_candidatas_sem_literal() {
    let f = fixture(false);
    authorize(&f);
    let mut ops: Vec<Operation> = (0..20).map(|i| inferred(&f, &format!("Gosta do número {i}"))).collect();
    ops.push(inferred(&f, "Moro em Curitiba"));
    let city = by_text(&learn(&f, ops), "Moro em Curitiba");
    tick(&f);
    assert_eq!(get(&f, city.id).nature, KindNature::Inferred, "fora das 20 vagas na primeira rodada");
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().expect("segunda rodada");
    sleep::run(&f.hub, &r, later(), None).unwrap();
    assert_eq!(get(&f, city.id).nature, KindNature::Said, "a 21ª entra na rodada seguinte");
}

/// "Sobre você" ligado no bot e num segundo bot B (`b2`), sem trocar a revisão do vínculo do pedido já admitido,
/// com a inferência "Moro em Curitiba" aprendida de M na camada compartilhada.
fn shared_fact(f: &Fixture) -> mem::Entry {
    // Compartilhamento ligado sem trocar a revisão do vínculo do pedido já admitido.
    let revision: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT revision FROM donna_memory_auto_bot_bindings WHERE bot='bot'", [], |r| r.get(0))
        .unwrap();
    let mut cfg = f.cfg.clone();
    cfg.shared_memory = Some(true);
    f.hub.store.save_bot(&cfg).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_memory_auto_bot_bindings SET revision=?1 WHERE bot='bot'", [revision])
        .unwrap();
    let other: BotConfig =
        serde_json::from_value(json!({"id":"b2","name":"B","backend":"claude","sharedMemory":true})).unwrap();
    f.hub.store.save_bot(&other).unwrap();
    let op = Operation::Remember {
        text: "Moro em Curitiba".into(),
        shared: true,
        evidence: vec![f.message.clone()],
        nature: Some(Hint::Inferred),
    };
    learn(f, vec![op]);
    f.hub.store.memory_read(|c| mem::layer(c, None)).unwrap().remove(0)
}

/// R2-507-01: a marca de candidata recusada vale só para a conversa conferida. B recusou pela linhagem (marca na
/// rodada de B); A, que tem a prova, ainda seleciona e confirma.
#[test]
fn marca_de_outra_conversa_nao_esconde_a_candidata() {
    let f = fixture(false);
    authorize(&f);
    let fact = shared_fact(&f);
    {
        let db = f.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_memory_sleep_jobs(id,bot,chat,space,day,context,source,status,created_at)
             VALUES('sono-b2','b2','b2','personal','2026-11-07','{}','x','done',0)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO donna_memory_sleep_facts(entry,rev,run,verdict,reason) VALUES(?1,?2,'sono-b2','literal','lineage')",
            params![fact.id, fact.rev],
        )
        .unwrap();
    }
    tick(&f);
    assert_eq!(get(&f, fact.id).nature, KindNature::Said);
}

/// SEG-507-02 (R2): plano só com arquivar uma lembrança já arquivada (`apply_one` sem efeito) → sem a prova e sem
/// `covered`.
#[test]
fn arquivar_o_ja_arquivado_nao_prova_a_leitura() {
    let f = fixture(false);
    let city = by_text(&learn(&f, vec![remember(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    f.hub.store.memory_write(|c, now, rev| mem::archive(c, mem::Writer::User, city.id, None, now, rev)).unwrap();
    let archived = get(&f, city.id);
    let (n, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let op = Operation::Archive { entry: city.id, expected_rev: archived.rev, evidence: vec![n.clone()], nature: None };
    assert_eq!(apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap().len(), 0);
    let (status, proof): (String, Option<String>) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT status,proof FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((status.as_str(), proof.as_deref()), ("noChange", Some("unproven")));
    tick(&f);
    let covered: bool = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_checks WHERE message=?1)", [&n], |r| r.get(0))
        .unwrap();
    assert!(!covered);
    assert!(!sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", &n).unwrap());
}

/// R1-04: lembrança de "Sobre você" confirmada avisa os outros bots que veem a camada (a revisão de B avança).
#[test]
fn confirmar_em_sobre_voce_avisa_os_outros_bots() {
    let f = fixture(false);
    authorize(&f);
    let fact = shared_fact(&f);
    let before = f.hub.store.kv_get("memory.revision.b2").unwrap_or_default();
    tick(&f);
    assert_eq!(get(&f, fact.id).nature, KindNature::Said);
    assert_ne!(f.hub.store.kv_get("memory.revision.b2").unwrap_or_default(), before, "a revisão de B avança");
}

/// A extração por turno usa a mesma régua de "Você disse" (frase inteira): trecho dentro de negação ou condição
/// vira inferência; a frase inteira continua "Você disse".
#[test]
fn extracao_so_da_voce_disse_para_frase_inteira() {
    for (message, fact, nature) in [
        ("Não moro em Curitiba; moro em Recife.", "moro em Curitiba", KindNature::Inferred),
        ("Se eu mudar de trabalho, moro em Curitiba.", "moro em Curitiba", KindNature::Inferred),
        ("Não moro em Curitiba; moro em Recife.", "moro em Recife", KindNature::Said),
    ] {
        let f = fixture(false);
        say(&f, message);
        let r = success(&f);
        let op =
            Operation::Remember { text: fact.into(), shared: false, evidence: vec![f.message.clone()], nature: None };
        apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap();
        let entry = f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().remove(0);
        assert_eq!(entry.nature, nature, "{message} / {fact}");
    }
}

/// SEG-507-02 caso 2: lote misto, uma escrita válida com uma operação ignorada. O job fica `applied` com a marca
/// `unproven`: a escrita e o recibo ficam, mas N não vira `covered` nem pode ser apagada pela C7.
#[test]
fn lote_misto_salva_a_escrita_mas_nao_prova_a_leitura() {
    let f = fixture(false);
    let city = by_text(&learn(&f, vec![inferred(&f, "Moro em Curitiba")]), "Moro em Curitiba");
    let (n, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let ops = vec![
        Operation::Remember {
            text: "Gosta de chá verde".into(),
            shared: false,
            evidence: vec![n.clone()],
            nature: None,
        },
        Operation::Confirm { entry: city.id, expected_rev: city.rev, evidence: vec![n.clone()] },
    ];
    assert_eq!(apply::complete(&f.hub, &r, ExtractionPlan { operations: ops }, |_, _| Ok(())).unwrap().len(), 1);
    let (status, proof): (String, Option<String>) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT status,proof FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((status.as_str(), proof.as_deref()), ("applied", Some("unproven")));
    let texts: Vec<String> =
        f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().into_iter().map(|e| e.text).collect();
    assert!(texts.contains(&"Gosta de chá verde".to_owned()), "a escrita válida continua salva");
    tick(&f);
    let covered: bool = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_checks WHERE message=?1)", [&n], |r| r.get(0))
        .unwrap();
    assert!(!covered);
    assert!(!sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", &n).unwrap());
}
