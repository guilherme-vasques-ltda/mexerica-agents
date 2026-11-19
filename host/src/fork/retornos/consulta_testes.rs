#![allow(clippy::unwrap_used)]
//! D70 do R2a pelo percurso real: pedidos produzidos por `ask_bot`/`message_bot` no fake engine
//! e consultados por `teamCall` (a mesma rota do MCP `team`). Identidade, recusa indistinguível,
//! fronteira de alcance, reinício, falha fechada e ausência de efeito.
use super::consulta::FERRAMENTA;
use crate::LockExt;
use crate::api::devices::Caller;
use crate::fork::sessoes::{
    bindings,
    origin::SessionOrigin,
    store::{NewSession, create},
};
use crate::hub::{BotStatus, Hub};
use crate::store::{BotConfig, Lane, Store};
use anyhow::Result;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const SENTINELA: &str = "SENTINELA-R2A-PRIVADA";
const INDISPONIVEL: &str = "retorno_pedido_indisponivel";
const FORA: &str = "retorno_consulta_fora_do_alcance";
const INVALIDO: &str = "retorno_registro_invalido";

type Aviso = (i64, Option<String>, (usize, usize));
type Efeitos = (String, (i64, i64, i64), [Aviso; 2], usize);

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
    principal: String,
}

impl Fixture {
    fn new() -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let dir = std::env::temp_dir().join(format!("donna-r2a-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
        for id in ["a", "b", "c"] {
            let cwd = dir.join(id);
            std::fs::create_dir_all(&cwd).unwrap();
            let cfg: BotConfig = serde_json::from_value(json!({
                "id": id, "name": id, "backend": "fixture", "cwd": cwd,
                "command": crate::shell::python(&agent), "notify": false, "permission": "ask",
            }))
            .unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(
            store,
            "test-host".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "test-token".into(),
            19222,
        );
        hub.start().unwrap();
        bindings::fixture_execution(&hub, "a");
        hub.team.start_turn("a", None);
        let principal = bindings::for_acp(&hub.store, "a", "fixture-acp").unwrap().id.as_str().to_owned();
        Self { hub, dir, principal }
    }

    /// `ask_bot` real da principal de "a"; devolve o ID registrado.
    async fn perguntar(&self, texto: &str) -> String {
        let _ = call(&self.hub, "a", &self.principal, "ask_bot", &json!({"botId": "b", "message": texto})).await;
        self.ultimo_pedido()
    }

    async fn mandar(&self, binding: &str, texto: &str) -> String {
        let aceito = call(&self.hub, "a", binding, "message_bot", &json!({"botId": "b", "message": texto})).await;
        aceito.unwrap()["requestId"].as_str().unwrap().to_owned()
    }

    fn ultimo_pedido(&self) -> String {
        self.hub
            .store
            .db
            .locked()
            .query_row("SELECT id FROM donna_session_requests ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0))
            .unwrap()
    }

    fn prompts(&self) -> usize {
        std::fs::read_to_string(self.dir.join("b/prompts.jsonl")).unwrap_or_default().lines().count()
    }

    /// Espera o prompt bloqueado chegar ao motor (`running` já foi gravado antes dele).
    async fn bloqueado(&self) -> usize {
        let ultimo = || {
            let log = std::fs::read_to_string(self.dir.join("b/prompts.jsonl")).unwrap_or_default();
            log.lines().last().is_some_and(|l| l.contains("BLOCK"))
        };
        self.until(ultimo).await;
        self.prompts()
    }

    fn mensagens(&self) -> usize {
        self.hub.team.test_pending_counts().1
    }

    async fn until(&self, condition: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(30), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture condition timed out");
    }

    async fn ocioso(&self) {
        self.until(|| self.mensagens() == 0 && self.hub.runtime("b").status == BotStatus::Idle).await;
    }

    /// Nova execução de `bot` numa sessão do catálogo, com vínculo ativo e turno próprio.
    fn entrar(&self, bot: &str, lane: Lane, acp: &str, token: &str) -> String {
        let session = lane.session.clone().unwrap_or_else(|| lane.chat.clone());
        let binding = bindings::issue(&self.hub.store, bot, &lane.chat, &session, None).unwrap();
        bindings::activate(&self.hub.store, &binding, acp).unwrap();
        self.hub.set_runtime(bot, |r| {
            r.status = BotStatus::Working;
            r.lane = Some(lane);
            r.session = Some(acp.into());
            r.memory_turn_token = Some(token.into());
        });
        binding.id.as_str().to_owned()
    }

    fn voltar_principal(&self) {
        let binding = bindings::for_acp(&self.hub.store, "a", "fixture-acp").unwrap();
        bindings::activate(&self.hub.store, &binding, "fixture-acp").unwrap();
        self.hub.set_runtime("a", |r| {
            r.status = BotStatus::Working;
            r.lane = Some(Lane::main("a"));
            r.session = Some("fixture-acp".into());
            r.memory_turn_token = Some("fixture-turn-principal".into());
        });
    }

    fn adicional(&self, titulo: &str) -> Lane {
        let extra = create(
            &self.hub.store.db.locked(),
            &NewSession {
                chat: "a".into(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: titulo.into(),
                runs_on: None,
            },
        )
        .unwrap();
        Lane { chat: "a".into(), session: Some(extra.id) }
    }

    /// Tudo que a consulta não pode alterar: pedidos/resultados, D220, avisos, turno, fila e prompts.
    fn efeitos(&self) -> Efeitos {
        let (dump, chains) = {
            let db = self.hub.store.db.locked();
            let chains = db
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM donna_chain_roots),(SELECT COUNT(*) FROM donna_chain_admissions),
                       (SELECT COALESCE(SUM(used),0) FROM donna_chain_roots)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            (super::testes::despejo(&db), chains)
        };
        let snapshot = |bot| crate::chat::team::admission_tests::snapshot(&self.hub, bot).unwrap();
        (dump, chains, [snapshot("a"), snapshot("b")], self.prompts())
    }

    /// IDs internos que nunca podem aparecer na resposta.
    fn internos(&self) -> Vec<String> {
        let db = self.hub.store.db.locked();
        let mut st = db
            .prepare(
                "SELECT origin_entry_id FROM donna_session_requests
                 UNION SELECT target_entry_id FROM donna_session_requests
                 UNION SELECT final_entry_id FROM donna_session_results WHERE final_entry_id IS NOT NULL
                 UNION SELECT requester_execution_ref FROM donna_session_requests
                 UNION SELECT digest FROM donna_session_results UNION SELECT admission_id FROM donna_session_requests
                 UNION SELECT chain_id FROM donna_session_requests UNION SELECT id FROM donna_session_bindings",
            )
            .unwrap();
        st.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    async fn shutdown(self) {
        self.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(self.dir);
    }
}

async fn call(hub: &Arc<Hub>, bot: &str, binding: &str, name: &str, arguments: &Value) -> Result<Value> {
    crate::api::dispatch(
        hub,
        &Caller::Local,
        "teamCall",
        json!({"botId": bot, "sessionBinding": binding, "name": name, "arguments": arguments}),
    )
    .await
}

async fn consultar(f: &Fixture, bot: &str, binding: &str, id: &str) -> Result<Value> {
    call(&f.hub, bot, binding, FERRAMENTA, &json!({"requestId": id})).await
}

/// Resposta fechada: só as chaves do contrato e nenhum dado interno ou texto do pedido.
fn conferir_fechada(f: &Fixture, resposta: &Value) {
    let chaves = |v: &Value| {
        let mut chaves: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        chaves.sort();
        chaves
    };
    assert_eq!(chaves(resposta), ["note", "outcome", "reason", "requestId", "state"]);
    if resposta["outcome"].is_object() {
        assert_eq!(chaves(&resposta["outcome"]), ["reason", "result"]);
    }
    let texto = resposta.to_string();
    assert!(!texto.contains(SENTINELA) && !texto.contains("fixture") && !texto.contains("acp:"), "{texto}");
    for interno in f.internos() {
        assert!(!texto.contains(&interno), "{texto}");
    }
}

fn estado(resposta: &Value) -> (&str, Option<&str>, Option<(&str, &str)>) {
    let outcome =
        resposta["outcome"].as_object().map(|o| (o["result"].as_str().unwrap(), o["reason"].as_str().unwrap()));
    (resposta["state"].as_str().unwrap(), resposta["reason"].as_str(), outcome)
}

/// Estados próprios pelo MCP real, novo turno da mesma sessão e ausência de efeito da consulta.
#[tokio::test]
async fn r2a_estado_proprio_pelo_mcp_sem_efeito_e_novo_turno_le_pedido_antigo() {
    let f = Fixture::new();
    let vp = f.principal.clone();
    let concluido = f.perguntar(&format!("{SENTINELA} pergunta")).await;
    let falhou = f.perguntar(&format!("FAIL {SENTINELA}")).await;
    let recusado = f.perguntar("REFUSE").await;
    let rodando = f.mandar(&vp, &format!("BLOCK {SENTINELA}")).await;
    let enviados = f.bloqueado().await;
    let fila = f.mandar(&vp, "na fila").await;

    let antes = f.efeitos();
    let mut respostas = Vec::new();
    for id in [&concluido, &falhou, &recusado, &rodando, &fila] {
        let resposta = consultar(&f, "a", &vp, id).await.unwrap();
        assert_eq!(resposta["requestId"], id.as_str());
        conferir_fechada(&f, &resposta);
        respostas.push(resposta);
    }
    // Repetir a consulta não muda nada: pedidos, resultados, D220, avisos, turno, fila e prompts.
    for id in [&concluido, &rodando, &fila] {
        consultar(&f, "a", &vp, id).await.unwrap();
    }
    assert_eq!(f.efeitos(), antes);

    assert_eq!(estado(&respostas[0]), ("finished", None, Some(("completed", "end_turn"))));
    assert_eq!(estado(&respostas[1]), ("finished", None, Some(("failed", "engine_error"))));
    assert_eq!(estado(&respostas[2]), ("finished", None, Some(("incomplete", "refusal"))));
    assert_eq!(estado(&respostas[3]), ("running", None, None));
    assert_eq!(estado(&respostas[4]), ("accepted", None, None));
    // Falha e incompletude nunca recebem a nota de conclusão normal.
    for falha in &respostas[1..] {
        assert_ne!(falha["note"], respostas[0]["note"]);
    }

    // Novo turno legítimo da mesma sessão lê o pedido do turno anterior; a execução antiga, não.
    let antiga = bindings::capture(&f.hub, "a", &vp).unwrap();
    f.hub.set_runtime("a", |r| r.memory_turn_token = Some("fixture-turn-2".into()));
    assert_eq!(estado(&consultar(&f, "a", &vp, &concluido).await.unwrap()).0, "finished");
    let args = json!({"requestId": concluido});
    assert!(crate::chat::team::call_in_execution(&f.hub, "a", FERRAMENTA, &args, Some(&antiga)).await.is_err());

    // Parar o destinatário: a parada não vira sucesso e o pedido na fila não afirma envio.
    f.hub.send_cmd("b", crate::agent::bot::Cmd::Stop).unwrap();
    f.until(|| f.mensagens() == 0).await;
    let parado = consultar(&f, "a", &vp, &rodando).await.unwrap();
    match estado(&parado) {
        ("finished", None, Some((resultado, _))) => assert_eq!(resultado, "stopped"),
        (state, _, outcome) => assert!(state == "cancelled" && outcome.is_none(), "{parado}"),
    }
    let cancelado = consultar(&f, "a", &vp, &fila).await.unwrap();
    assert_eq!(estado(&cancelado).0, "cancelled");
    assert!(cancelado["outcome"].is_null());
    assert_eq!(f.prompts(), enviados, "a consulta não envia nem repete trabalho");
    f.shutdown().await;
}

/// Outra conversa do mesmo bot (nos dois sentidos), outra adicional, outro bot e ID inexistente:
/// a mesma recusa, sem revelar existência. Grupo fica fora do alcance.
#[tokio::test]
async fn r2a_outra_conversa_outro_bot_e_inexistente_tem_a_mesma_recusa() {
    let f = Fixture::new();
    let principal = f.mandar(&f.principal, &format!("{SENTINELA} principal")).await;
    f.ocioso().await;
    let inexistente = uuid::Uuid::new_v4().to_string();

    let extra = f.adicional("Extra");
    let vx = f.entrar("a", extra, "acp-extra", "turno-extra");
    let adicional = f.mandar(&vx, &format!("{SENTINELA} adicional")).await;
    f.ocioso().await;
    let proprio = consultar(&f, "a", &vx, &adicional).await.unwrap();
    conferir_fechada(&f, &proprio);
    assert_eq!(estado(&proprio).0, "finished");

    let recusa = format!("{:#}", consultar(&f, "a", &vx, &inexistente).await.unwrap_err());
    assert!(recusa.contains(INDISPONIVEL), "{recusa}");
    let mut outras = vec![format!("{:#}", consultar(&f, "a", &vx, &principal).await.unwrap_err())];

    let segunda = f.adicional("Outra");
    let vy = f.entrar("a", segunda, "acp-outra", "turno-outra");
    for id in [&adicional, &principal] {
        outras.push(format!("{:#}", consultar(&f, "a", &vy, id).await.unwrap_err()));
    }

    f.voltar_principal();
    let vp = f.principal.clone();
    outras.push(format!("{:#}", consultar(&f, "a", &vp, &adicional).await.unwrap_err()));
    assert_eq!(estado(&consultar(&f, "a", &vp, &principal).await.unwrap()).0, "finished");

    let vc = {
        bindings::fixture_execution(&f.hub, "c");
        bindings::for_acp(&f.hub.store, "c", "fixture-acp").unwrap().id.as_str().to_owned()
    };
    for id in [&principal, &adicional] {
        outras.push(format!("{:#}", consultar(&f, "c", &vc, id).await.unwrap_err()));
    }
    for erro in &outras {
        assert_eq!(erro, &recusa, "recusa distinguível");
        assert!(!erro.contains(SENTINELA));
    }

    // Execução de "a" num grupo: fora do alcance, sem fallback à conversa própria.
    let grupo: BotConfig =
        serde_json::from_value(json!({"id": "", "kind": "group", "name": "G", "members": ["a", "b"]})).unwrap();
    let grupo = f.hub.create_bot(grupo).unwrap()["id"].as_str().unwrap().to_owned();
    let vg = f.entrar("a", Lane::main(&grupo), "acp-grupo", "turno-grupo");
    let erro = format!("{:#}", consultar(&f, "a", &vg, &principal).await.unwrap_err());
    assert!(!erro.contains(INDISPONIVEL) && !erro.contains(SENTINELA), "{erro}");
    f.shutdown().await;
}

/// Argumentos fora do esquema, ausência de execução, fora do Pessoal, sessão fechada e vínculo revogado.
#[tokio::test]
async fn r2a_argumento_execucao_ausente_fora_do_pessoal_sessao_fechada_e_revogado_recusam() {
    let f = Fixture::new();
    let vp = f.principal.clone();
    let id = f.mandar(&vp, "primeiro").await;
    f.ocioso().await;
    let antes = f.efeitos();

    for args in [
        json!({}),
        json!({"requestId": id, "botId": "b"}),
        json!({"requestId": 5}),
        json!({"requestId": "nao-e-uuid"}),
        json!({"requestId": id.to_uppercase()}),
        json!({"requestId": format!("{{{id}}}")}),
        json!([id]),
    ] {
        let erro = format!("{:#}", call(&f.hub, "a", &vp, FERRAMENTA, &args).await.unwrap_err());
        assert!(erro.contains("retorno_argumento_invalido"), "{erro}");
    }

    // Chamada interna sem ExecutionRef: nada é lido.
    let args = json!({"requestId": id});
    let erro = format!("{:#}", crate::chat::team::call(&f.hub, "a", FERRAMENTA, &args).await.unwrap_err());
    assert!(erro.contains(FORA), "{erro}");

    // Bot fora do Pessoal, com vínculo e execução ainda válidos.
    let execucao = bindings::capture(&f.hub, "a", &vp).unwrap();
    let config: String =
        f.hub.store.db.locked().query_row("SELECT config FROM bots WHERE id='a'", [], |r| r.get(0)).unwrap();
    let mover = "UPDATE bots SET config=json_set(config,'$.spaceId','outro-space') WHERE id='a'";
    f.hub.store.db.locked().execute(mover, []).unwrap();
    let erro = crate::chat::team::call_in_execution(&f.hub, "a", FERRAMENTA, &args, Some(&execucao)).await;
    assert!(format!("{:#}", erro.unwrap_err()).contains(FORA));
    f.hub.store.db.locked().execute("UPDATE bots SET config=?1 WHERE id='a'", [config]).unwrap();
    assert_eq!(estado(&consultar(&f, "a", &vp, &id).await.unwrap()).0, "finished");

    // Sessão adicional fechada: o gatilho de Sessões revoga o vínculo.
    let extra = f.adicional("Fechar");
    let sessao = extra.session.clone().unwrap();
    let vx = f.entrar("a", extra, "acp-fechar", "turno-fechar");
    let proprio = f.mandar(&vx, "na adicional").await;
    f.ocioso().await;
    let execucao_x = bindings::capture(&f.hub, "a", &vx).unwrap();
    let fechar = "UPDATE donna_bot_sessions SET state='closed' WHERE id=?1";
    f.hub.store.db.locked().execute(fechar, [&sessao]).unwrap();
    assert!(consultar(&f, "a", &vx, &proprio).await.is_err());
    let args_x = json!({"requestId": proprio});
    assert!(crate::chat::team::call_in_execution(&f.hub, "a", FERRAMENTA, &args_x, Some(&execucao_x)).await.is_err());

    // Vínculo revogado: nem a chamada MCP nem a execução capturada antes leem.
    f.voltar_principal();
    let execucao = bindings::capture(&f.hub, "a", &vp).unwrap();
    bindings::revoke(&f.hub.store, "a", None).unwrap();
    assert!(consultar(&f, "a", &vp, &id).await.is_err());
    assert!(crate::chat::team::call_in_execution(&f.hub, "a", FERRAMENTA, &args, Some(&execucao)).await.is_err());
    // As recusas não enviaram nada: só a mensagem da adicional chegou ao motor.
    assert_eq!(f.prompts(), antes.3 + 1);
    f.shutdown().await;
}

/// Reinício: incerto continua incerto e antes do envio não afirma envio, sem prompt novo.
/// Versão, estado ou combinação desconhecidos falham fechados, sem ecoar o valor.
#[tokio::test]
async fn r2a_reinicio_mantem_incerto_e_registro_invalido_falha_fechado() {
    let f = Fixture::new();
    let vp = f.principal.clone();
    let concluido = f.perguntar("antes").await;
    let rodando = f.mandar(&vp, "BLOCK em andamento").await;
    let enviados = f.bloqueado().await;
    let fila = f.mandar(&vp, "na fila").await;
    assert_eq!(estado(&consultar(&f, "a", &vp, &rodando).await.unwrap()).0, "running");

    let reaberta = Store::open(&f.dir.join("test.db")).unwrap();
    assert_eq!(super::recuperar(&reaberta).unwrap(), 2);
    drop(reaberta);
    let incerto = consultar(&f, "a", &vp, &rodando).await.unwrap();
    assert_eq!(estado(&incerto), ("interrupted", Some("restart_uncertain"), None));
    let antes_do_envio = consultar(&f, "a", &vp, &fila).await.unwrap();
    assert_eq!(estado(&antes_do_envio), ("interrupted", Some("restart_before_start"), None));
    assert_eq!(estado(&consultar(&f, "a", &vp, &concluido).await.unwrap()).2, Some(("completed", "end_turn")));

    // A conclusão tardia do runtime não transforma o incerto em concluído; nada é reenviado.
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.mensagens() == 0).await;
    assert_eq!(consultar(&f, "a", &vp, &rodando).await.unwrap(), incerto);
    assert_eq!(f.prompts(), enviados);

    // (alteração inválida, restauração, pedido consultado)
    let invalidos = [
        ("UPDATE donna_session_requests SET schema_version=2 WHERE id=?1", "schema_version=1", &concluido),
        ("UPDATE donna_session_requests SET state='finished' WHERE id=?1", "state='interrupted'", &rodando),
        ("UPDATE donna_session_requests SET state='cancelled' WHERE id=?1", "state='interrupted'", &rodando),
        (
            "UPDATE donna_session_requests SET end_reason='consumed' WHERE id=?1",
            "end_reason='restart_uncertain'",
            &rodando,
        ),
        (
            "UPDATE donna_session_requests SET state='waiting',end_reason=NULL WHERE id=?1",
            "state='interrupted',end_reason='restart_before_start'",
            &fila,
        ),
    ];
    let original = super::testes::despejo(&f.hub.store.db.locked());
    for (alterar, restaurar, id) in invalidos {
        ignorando_checks(&f, alterar, id);
        let erro = format!("{:#}", consultar(&f, "a", &vp, id).await.unwrap_err());
        assert!(erro.contains(INVALIDO) && !erro.contains("consumed") && !erro.contains("waiting"), "{erro}");
        let sql = format!("UPDATE donna_session_requests SET {restaurar} WHERE id=?1");
        f.hub.store.db.locked().execute(&sql, [id]).unwrap();
    }
    for (alterar, restaurar) in [
        ("UPDATE donna_session_results SET outcome='delivered' WHERE request_id=?1", "outcome='completed'"),
        ("UPDATE donna_session_results SET schema_version=9 WHERE request_id=?1", "schema_version=1"),
    ] {
        ignorando_checks(&f, alterar, &concluido);
        let erro = format!("{:#}", consultar(&f, "a", &vp, &concluido).await.unwrap_err());
        assert!(erro.contains(INVALIDO) && !erro.contains("delivered"), "{erro}");
        let sql = format!("UPDATE donna_session_results SET {restaurar} WHERE request_id=?1");
        f.hub.store.db.locked().execute(&sql, [&concluido]).unwrap();
    }
    assert_eq!(super::testes::despejo(&f.hub.store.db.locked()), original);
    assert_eq!(estado(&consultar(&f, "a", &vp, &rodando).await.unwrap()), estado(&incerto));
    f.shutdown().await;
}

/// Grava um valor fora do esquema, como uma versão futura ou dado corrompido faria.
fn ignorando_checks(f: &Fixture, sql: &str, id: &str) {
    let db = f.hub.store.db.locked();
    db.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
    db.execute(sql, [id]).unwrap();
    db.execute_batch("PRAGMA ignore_check_constraints=OFF").unwrap();
}

/// R2b: o erro do `ask_bot` depois da admissão traz o requestId certo, sem texto do pedido, e a
/// consulta mostra o registro sem conclusão. Sucesso fica igual; recusa antes da admissão, sem ID.
#[tokio::test]
async fn r2b_erro_do_ask_bot_traz_request_id_e_a_consulta_nao_mostra_conclusao() {
    let f = Fixture::new();
    let vp = f.principal.clone();
    let pedidos = || -> i64 {
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_session_requests", [], |r| r.get(0)).unwrap()
    };
    let conferir = |erro: &str, id: &str| {
        assert!(erro.ends_with(&format!(" (requestId: {id})")), "{erro}");
        assert!(!erro.contains(SENTINELA) && !erro.contains("BLOCK"), "{erro}");
    };
    let ask_em_fundo = |texto: String| {
        let (hub, binding) = (f.hub.clone(), vp.clone());
        tokio::spawn(async move {
            let args = json!({"botId": "b", "message": texto});
            format!("{:#}", call(&hub, "a", &binding, "ask_bot", &args).await.unwrap_err())
        })
    };

    let ok = call(&f.hub, "a", &vp, "ask_bot", &json!({"botId": "b", "message": "oi"})).await.unwrap();
    let mut chaves: Vec<String> = ok.as_object().unwrap().keys().cloned().collect();
    chaves.sort();
    assert_eq!(chaves, ["botId", "name", "reply", "requestId"]);
    assert_eq!((ok["requestId"].as_str().unwrap(), &ok["reply"]), (f.ultimo_pedido().as_str(), &json!("reply: oi")));

    let antes = pedidos();
    for args in [json!({"botId": "missing", "message": "x"}), json!({"botId": "b", "message": " "})] {
        let erro = format!("{:#}", call(&f.hub, "a", &vp, "ask_bot", &args).await.unwrap_err());
        assert!(!erro.contains("requestId"), "{erro}");
    }
    assert_eq!(pedidos(), antes, "recusa antes da admissão não grava pedido");

    // Prazo: o mesmo ask_bot da execução do MCP, só com prazo curto.
    let execucao = bindings::capture(&f.hub, "a", &vp).unwrap();
    let args = json!({"botId": "b", "message": format!("BLOCK {SENTINELA} prazo")});
    let prazo = crate::chat::team::ask_in_execution(&f.hub, "a", &args, &execucao, Duration::from_millis(200));
    let erro = format!("{:#}", prazo.await.unwrap_err());
    let id = f.ultimo_pedido();
    conferir(&erro, &id);
    assert_eq!(estado(&consultar(&f, "a", &vp, &id).await.unwrap()), ("cancelled", Some("timeout"), None));
    f.ocioso().await;

    // Cancelamento do solicitante enquanto o destinatário trabalha.
    let enviados = f.prompts();
    let pedido = ask_em_fundo(format!("BLOCK {SENTINELA} cancelar"));
    f.until(|| f.prompts() > enviados).await;
    let id = f.ultimo_pedido();
    f.hub.team.cancel_from("a");
    conferir(&pedido.await.unwrap(), &id);
    assert_eq!(estado(&consultar(&f, "a", &vp, &id).await.unwrap()), ("cancelled", Some("cancelled"), None));
    f.ocioso().await;
    f.hub.team.start_turn("a", None);

    // Parada do destinatário: nunca vira concluído.
    let enviados = f.prompts();
    let pedido = ask_em_fundo(format!("BLOCK {SENTINELA} parar"));
    f.until(|| f.prompts() > enviados).await;
    let id = f.ultimo_pedido();
    f.hub.send_cmd("b", crate::agent::bot::Cmd::Stop).unwrap();
    conferir(&pedido.await.unwrap(), &id);
    let parado = consultar(&f, "a", &vp, &id).await.unwrap();
    conferir_fechada(&f, &parado);
    match estado(&parado) {
        ("finished", None, Some((resultado, _))) => assert_eq!(resultado, "stopped"),
        (state, Some(_), None) => assert!(matches!(state, "cancelled" | "interrupted"), "{parado}"),
        _ => panic!("{parado}"),
    }
    f.shutdown().await;
}
