//! R2/R3: suspensões nos handlers reais, antes do commit de permissão e team.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::LockExt;
use crate::fork::spaces::tool_context::{self, tests::Fixture};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn actor(f: &Fixture) -> Actor {
    let mut actor = new_actor(f.hub.clone(), f.cfg.clone());
    actor.lane = Lane::main(f.cfg.session_owner());
    actor.turn = Some(1);
    actor.turn_session = Some("sessao-A".into());
    actor
}

#[tokio::test]
async fn mx106_leitura_de_rotina_marca_sessao_memoria_e_gestao_antes_da_criacao() {
    use crate::fork::sessoes::bindings;
    let f = Fixture::new();
    let bot = f.cfg.id.clone();
    let cfg = f.hub.store.bot(&bot).unwrap().unwrap().config;
    let binding = bindings::issue(&f.hub.store, &bot, &bot, &bot, None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-mx106").unwrap();
    let entry =
        f.hub.store.insert_entry(&Lane::main(&bot), EntryKind::User, 1, &json!({"text":"Pedido sintético"})).unwrap();
    f.hub
        .start_bot_turn(&bot, |r| {
            r.lane = Some(Lane::main(&bot));
            r.message = Some(entry.id);
            r.session = Some("acp-mx106".into());
        })
        .unwrap();
    f.hub.store.memory_session_start(&bot, "acp-mx106", false).unwrap();
    let (acp, rx) = Acp::spawn("cat", f.dir.to_str().unwrap(), &[]).unwrap();
    let mut a = new_actor(f.hub.clone(), cfg);
    a.lane = Lane::main(&bot);
    a.turn = Some(1);
    a.turn_session = Some("acp-mx106".into());
    a.conn = Some(Conn {
        ponte: None,
        curated: crate::fork::conectores::Pool::default(),
        acp,
        rx,
        skill_reads: tokio::task::JoinSet::new(),
        sessions: SessionCaps { load: true, close: false, system_prompt: false },
        adapter: Adapter::Claude,
        adapter_version: None,
        codex_config: false,
        config_warnings: vec![],
        loaded: HashMap::new(),
        started: Instant::now(),
        fast_options: HashMap::new(),
        reports_compaction: false,
        compaction_usage: CompactionUsage::Idle,
        usage: HashMap::new(),
    });
    crate::fork::memory_auto::transport::new_generation(&f.hub, &a.cfg, &bot, "claude").unwrap();
    let (definitions, _) = a.mcp_servers(None, &binding).unwrap();
    let definitions = crate::fork::sessoes::contexto_ferramenta::without_seal(&definitions);
    let exe = std::env::current_exe().unwrap();
    let known = crate::guard_hub::tool_origin::internal_servers(
        &definitions,
        &exe,
        &bot,
        (f.hub.port, None),
        &a.cfg.space_id,
        None,
        tool_context::definition_binding(&definitions),
        Some(binding.id.as_str()),
    );
    assert!(known.contains("routines"), "definição real do Hub deve ser reconhecida");
    a.hub_tool_servers.insert("acp-mx106".into(), known);
    a.on_update(&json!({"sessionUpdate":"tool_call","toolCallId":"save-1","kind":"other",
        "_meta":{"claudeCode":{"toolName":"mcp__routines__save_routine"}},"rawInput":{}}));
    assert!(!crate::fork::sessoes::exposure::context_marked(&f.hub.store.db.locked(), &bot, &bot, "acp-mx106"));
    let clean_execution = bindings::capture(&f.hub, &bot, binding.id.as_str()).unwrap();
    let source = crate::routines::call_invoked_in_execution(
        &f.hub,
        &bot,
        "save_routine",
        &json!({"name":"Origem", "instruction":"Sintética", "triggers":[{"type":"interval","seconds":3600}]}),
        &clean_execution,
        Some("mx106-clean-save"),
    )
    .unwrap();
    assert_eq!(source["routine"]["enabled"], true);
    let source_id = source["routine"]["id"].as_str().unwrap();
    f.hub
        .routines
        .enqueue(&f.hub.store, &bot, source_id, json!({"body":"evento externo sintético"}), None, true)
        .unwrap();
    let listing = f.hub.routines.list(&bot);
    assert!(listing.to_string().contains("evento externo sintético"));
    a.on_update(&json!({"sessionUpdate":"tool_call","toolCallId":"list-1","kind":"other",
        "_meta":{"claudeCode":{"toolName":"mcp__routines__list_routines"}},"rawInput":{},
        "content":[{"type":"content","content":{"type":"text","text":listing.to_string()}}]}));
    assert!(crate::fork::sessoes::exposure::context_marked(&f.hub.store.db.locked(), &bot, &bot, "acp-mx106"));
    assert!(f.hub.store.guard_management_exposed(&bot, false).unwrap());
    let execution = bindings::capture(&f.hub, &bot, binding.id.as_str()).unwrap();
    let saved = crate::routines::call_invoked_in_execution(
        &f.hub,
        &bot,
        "save_routine",
        &json!({"name":"Resumo","instruction":"Resumo sintético","triggers":[{"type":"interval","seconds":3600}]}),
        &execution,
        Some("mx106-list-then-save"),
    )
    .unwrap();
    assert_eq!(saved["routine"]["enabled"], false);
}

fn revoke(f: &Fixture) {
    f.hub.update_bot_by_user(&json!({"id":f.cfg.id,"computer":true})).unwrap();
    assert!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.computer);
}

fn inherited_permission_fixture() -> Fixture {
    let f = Fixture::with_inherited_permission();
    f.context().check(&f.hub).unwrap();
    f
}

// Corte do produtor real antes da Connection: sem tempo/sleep para ordenar a disputa.
async fn suspend_team_settings(
    f: &Fixture,
) -> (std::sync::mpsc::Sender<()>, tokio::task::JoinHandle<Result<Vec<BotConfig>>>) {
    let (entered, at_cut) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::channel();
    f.hub.store.test_before_team_settings_write(move || {
        entered.send(()).unwrap();
        resume.recv().unwrap();
    });
    let hub = f.hub.clone();
    let changing = tokio::task::spawn_blocking(move || hub.store.set_team_settings(&json!({"permission":"ask"})));
    at_cut.await.unwrap();
    assert!(
        f.hub.store.memory_boundary.try_lock().is_err(),
        "setTeamSettings deve reservar a mesma barreira antes de alterar o perfil"
    );
    assert_eq!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.permission, crate::store::Permission::Auto);
    f.context().check(&f.hub).unwrap();
    (release, changing)
}

#[tokio::test]
async fn r4_team_settings_serializa_revogacao_com_o_commit_acp() {
    let f = inherited_permission_fixture();
    let context = f.context();
    let (release, changing) = suspend_team_settings(&f).await;
    let (mut writer, mut reader) = tokio::io::duplex(1024);
    let hub = f.hub.clone();
    let delivery = tokio::task::spawn_blocking(move || {
        futures::executor::block_on(crate::fork::spaces::tool_effects::write_checked(
            &mut writer,
            b"{\"id\":\"r4\",\"result\":{\"outcome\":{\"optionId\":\"allow\"}}}\n",
            &hub.store.memory_boundary,
            || context.check(&hub),
        ))
    });
    release.send(()).unwrap();
    changing.await.unwrap().unwrap();
    assert!(delivery.await.unwrap().is_err());
    let mut bytes = vec![];
    reader.read_to_end(&mut bytes).await.unwrap();
    assert!(bytes.is_empty(), "nenhum byte de allow após a revogação herdada");
    assert_eq!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.permission, crate::store::Permission::Ask);
}

#[tokio::test]
async fn r4_team_settings_serializa_revogacao_com_admissao_do_ator() {
    for name in ["ask_bot", "message_bot"] {
        let f = inherited_permission_fixture();
        let mut source = f.hub.test_bot_commands(&f.cfg.id);
        let target = uuid::Uuid::new_v4().to_string();
        let cfg: BotConfig = serde_json::from_value(json!({"id":target,"name":"Destino","cwd":f.dir})).unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
        let mut recipient = f.hub.test_bot_commands(&target);
        f.hub.team.start_turn(&f.cfg.id, None);
        let mut actor = actor(&f);
        let mut body = f.body.clone();
        body["name"] = name.into();
        body["arguments"] = json!({"botId":target,"message":"Pedido durante revogação herdada"});
        let mut call = Box::pin(tool_context::route(&f.hub, "teamCall", &body));
        assert!(futures::poll!(call.as_mut()).is_pending());
        let admission = source.try_recv().unwrap();
        assert!(matches!(admission, Cmd::ContextTeam { .. }));
        let (release, changing) = suspend_team_settings(&f).await;
        let (done, _) = mpsc::unbounded_channel();
        let admitted = tokio::task::spawn_blocking(move || futures::executor::block_on(actor.on_cmd(admission, &done)));
        release.send(()).unwrap();
        changing.await.unwrap().unwrap();
        admitted.await.unwrap();
        assert!(call.await.is_err());
        assert!(recipient.try_recv().is_err(), "nenhum pedido depois da mudança de permissão");
        assert_eq!(f.hub.team.test_pending_counts(), (0, 0), "nenhuma reserva ou aresta");
        assert!(f.hub.store.history(&target, i64::MAX, 20).unwrap().is_empty());
        assert_eq!(f.hub.store.history(f.cfg.session_owner(), i64::MAX, 20).unwrap().len(), 1);
    }
}

async fn pending_permission(f: &Fixture, rpc_id: Value, option: &str) -> (Actor, Arc<Acp>, std::path::PathBuf, Entry) {
    pending_permission_opts(f, rpc_id, json!([{"optionId":option,"name":"Permitir","kind":"allow_once"}])).await
}

async fn pending_permission_opts(
    f: &Fixture,
    rpc_id: Value,
    options: Value,
) -> (Actor, Arc<Acp>, std::path::PathBuf, Entry) {
    let path = f.dir.join("respostas-acp.jsonl");
    let script = "import os,sys\nwith open(os.environ['ACP_CAPTURE'], 'w') as out:\n for line in sys.stdin:\n  out.write(line); out.flush()\n";
    let command = format!("python3 -u -c {}", crate::shell::quote(script));
    let (acp, rx) =
        Acp::spawn(&command, f.dir.to_str().unwrap(), &[("ACP_CAPTURE".into(), path.to_string_lossy().into_owned())])
            .unwrap();
    let mut actor = actor(f);
    actor.conn = Some(Conn {
        ponte: None,
        curated: crate::fork::conectores::Pool::default(),
        acp: acp.clone(),
        rx,
        skill_reads: tokio::task::JoinSet::new(),
        sessions: SessionCaps { load: true, close: false, system_prompt: false },
        adapter: Adapter::Other,
        adapter_version: None,
        codex_config: false,
        config_warnings: vec![],
        loaded: HashMap::new(),
        started: Instant::now(),
        fast_options: HashMap::new(),
        reports_compaction: false,
        compaction_usage: CompactionUsage::Idle,
        usage: HashMap::new(),
    });
    actor
        .on_permission(
            rpc_id,
            json!({
                "sessionId":"sessao-A", "toolCall":{"title":"Ação nativa", "kind":"execute"},
                "options": options
            }),
        )
        .await;
    let card = f
        .hub
        .store
        .history(f.cfg.session_owner(), i64::MAX, 20)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "permission")
        .unwrap();
    (actor, acp, path, card)
}

#[tokio::test]
async fn regra_recusar_invalida_cartao_pendente_e_impede_entrega() {
    use crate::fork::rules::rule::{Effect, Rule};
    for suspended in [false, true] {
        let f = Fixture::new();
        let (mut actor, acp, path, card) = pending_permission(&f, json!("mx121"), "allow").await;
        let input = if suspended { Some(acp.hold_input().await) } else { None };
        let mut answer = Box::pin(actor.answer_permission(&card.id, Some("allow".into())));
        if suspended {
            assert!(futures::poll!(answer.as_mut()).is_pending());
        }
        let revision = f.hub.store.donna_rules().unwrap().revision;
        f.hub
            .store
            .donna_rule_save(
                Rule {
                    id: String::new(),
                    effect: Effect::Deny,
                    field: "ferramenta".into(),
                    op: "igual".into(),
                    value: "execute".into(),
                    bot_id: None,
                    tool: None,
                    scope: None,
                    enabled: true,
                    revision: 1,
                    created_at: 0,
                    updated_at: 0,
                },
                revision,
                true,
            )
            .unwrap();
        drop(input);
        assert!(answer.await.is_err());
        acp.kill_and_wait().await;
        let output = std::fs::read_to_string(path).unwrap();
        assert!(!output.contains("allow"), "aprovação não pode ser entregue: {output}");
        let entry = f.hub.store.entry(&card.id).unwrap();
        assert_eq!(entry.data["status"], "expired");
        assert!(entry.data["detail"].as_str().unwrap().contains("política"));
    }
}

#[tokio::test]
async fn opcao_sem_kind_allow_recai_na_revalidacao_e_expira_com_recusar() {
    use crate::fork::rules::rule::{Effect, Rule};
    for kind in [json!("other"), Value::Null] {
        let f = Fixture::new();
        let mut option = json!({"optionId":"prosseguir","name":"Prosseguir"});
        if !kind.is_null() {
            option["kind"] = kind;
        }
        let (mut actor, acp, path, card) = pending_permission_opts(&f, json!("mx121-kind"), json!([option])).await;
        let revision = f.hub.store.donna_rules().unwrap().revision;
        f.hub
            .store
            .donna_rule_save(
                Rule {
                    id: String::new(),
                    effect: Effect::Deny,
                    field: "ferramenta".into(),
                    op: "igual".into(),
                    value: "execute".into(),
                    bot_id: None,
                    tool: None,
                    scope: None,
                    enabled: true,
                    revision: 1,
                    created_at: 0,
                    updated_at: 0,
                },
                revision,
                true,
            )
            .unwrap();
        assert!(actor.answer_permission(&card.id, Some("prosseguir".into())).await.is_err());
        acp.kill_and_wait().await;
        let output = std::fs::read_to_string(path).unwrap();
        assert!(!output.contains("prosseguir"), "kind não-allow não pode entregar aprovação: {output}");
        let entry = f.hub.store.entry(&card.id).unwrap();
        assert_eq!(entry.data["status"], "expired");
        assert!(entry.data["detail"].as_str().unwrap().contains("política"));
    }
}

#[tokio::test]
async fn regra_nova_recusa_entrega_adiada_por_leitura_de_skill() {
    use crate::fork::rules::rule::{Effect, Rule};
    let f = Fixture::new();
    let (mut actor, acp, path, card) = pending_permission(&f, json!("mx121-adiado"), "allow").await;
    actor.conn.as_mut().unwrap().skill_reads.spawn(std::future::pending::<()>());
    let input = acp.hold_input().await;
    actor.answer_permission(&card.id, Some("allow".into())).await.unwrap();
    let revision = f.hub.store.donna_rules().unwrap().revision;
    f.hub
        .store
        .donna_rule_save(
            Rule {
                id: String::new(),
                effect: Effect::Deny,
                field: "ferramenta".into(),
                op: "igual".into(),
                value: "execute".into(),
                bot_id: None,
                tool: None,
                scope: None,
                enabled: true,
                revision: 1,
                created_at: 0,
                updated_at: 0,
            },
            revision,
            true,
        )
        .unwrap();
    drop(input);
    tokio::time::timeout(Duration::from_secs(5), actor.conn.as_mut().unwrap().skill_reads.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let entry = f.hub.store.entry(&card.id).unwrap();
    assert_eq!(entry.data["status"], "expired");
    assert!(entry.data["detail"].as_str().unwrap().contains("política"));
    actor.conn.as_mut().unwrap().skill_reads.shutdown().await;
    acp.kill_and_wait().await;
    assert!(!std::fs::read_to_string(path).unwrap().contains("allow"));
}

#[tokio::test]
async fn r2_permissao_suspensa_no_pipe_nao_entrega_allow_depois_de_update_bot() {
    for revoked in [true, false] {
        let f = Fixture::new();
        let mut commands = f.hub.test_bot_commands(&f.cfg.id);
        let (mut actor, acp, path, card) = pending_permission(&f, json!("permissao-r2"), "allow").await;
        let input = acp.hold_input().await;
        let mut answer = Box::pin(actor.answer_permission(&card.id, Some("allow".into())));
        assert!(futures::poll!(answer.as_mut()).is_pending());
        assert!(f.hub.store.memory_boundary.try_lock().is_ok(), "barreira não atravessa espera do pipe");
        if revoked {
            revoke(&f);
            assert!(matches!(commands.try_recv().unwrap(), Cmd::Reconfigure(_)));
        }
        drop(input);
        assert_eq!(answer.await.is_err(), revoked);
        acp.kill_and_wait().await;
        let lines = std::fs::read_to_string(&path).unwrap();
        if revoked {
            assert!(lines.is_empty(), "nenhuma concessão pode chegar ao ACP: {lines}");
            assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "cancelled");
        } else {
            let response: Value = serde_json::from_str(lines.trim()).unwrap();
            assert_eq!(response["result"]["outcome"]["optionId"], "allow");
        }
    }
}

#[tokio::test]
async fn r3_aba_do_perfil_e_delete_recusam_permissao_ja_suspensa() {
    for deleted in [false, true] {
        let f = Fixture::new();
        let _commands = f.hub.test_bot_commands(&f.cfg.id);
        let context = f.context();
        let (mut actor, acp, path, card) = pending_permission(&f, json!("permissao-r3"), "allow").await;
        let input = acp.hold_input().await;
        let mut answer = Box::pin(actor.answer_permission(&card.id, Some("allow".into())));
        assert!(futures::poll!(answer.as_mut()).is_pending());
        if deleted {
            f.hub.delete_bot(&f.cfg.id).unwrap();
        } else {
            revoke(&f);
            f.hub.update_bot_by_user(&json!({"id":f.cfg.id,"computer":false})).unwrap();
            assert!(!f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.computer);
        }
        assert!(context.check(&f.hub).is_err(), "captura não revive após ABA nem exclusão");
        drop(input);
        assert!(answer.await.is_err());
        acp.kill_and_wait().await;
        assert_eq!(std::fs::read_to_string(path).unwrap(), "");
        if !deleted {
            assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "cancelled");
        }
    }
}

#[tokio::test]
async fn r3_id_e_option_id_longos_entregam_e_conservam_a_sessao() {
    for (id, option) in [
        (json!("i".repeat(8192)), "allow".to_owned()),
        (json!("id-curto"), "a".repeat(500)),
        (json!("id\"\n".repeat(2048)), "opção\"\n".repeat(256)),
    ] {
        let f = Fixture::new();
        let _commands = f.hub.test_bot_commands(&f.cfg.id);
        let (mut actor, acp, path, card) = pending_permission(&f, id.clone(), &option).await;
        actor.answer_permission(&card.id, Some(option.clone())).await.unwrap();
        acp.notify("sonda-posterior", json!({})).await.unwrap();
        acp.kill_and_wait().await;
        let lines = std::fs::read_to_string(path).unwrap();
        let responses: Vec<Value> = lines.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(responses.len(), 2, "opção válida não fecha nem corrompe o pipe");
        assert_eq!(responses[0]["id"], id);
        assert_eq!(responses[0]["result"]["outcome"]["optionId"], option);
        assert_eq!(responses[1]["method"], "sonda-posterior");
    }
}

// Suspende exatamente antes do fechamento obrigatório, após escrever todo o corpo.
#[derive(Default)]
struct BeforeCommit(Vec<u8>);
impl tokio::io::AsyncWrite for BeforeCommit {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if bytes == b"}\n" {
            std::task::Poll::Pending
        } else {
            self.0.extend_from_slice(bytes);
            std::task::Poll::Ready(Ok(bytes.len()))
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn r3_revogacao_antes_do_fecho_recusa_json_longo_inclusive_no_eof() {
    for deleted in [false, true] {
        let f = Fixture::new();
        let _commands = f.hub.test_bot_commands(&f.cfg.id);
        let context = f.context();
        let mut line =
            serde_json::to_vec(&json!({"id":"i".repeat(8192),"result":{"outcome":{"optionId":"a".repeat(500)}}}))
                .unwrap();
        line.push(b'\n');
        let mut writer = BeforeCommit::default();
        let mut delivery = Box::pin(crate::fork::spaces::tool_effects::write_checked(
            &mut writer,
            &line,
            &f.hub.store.memory_boundary,
            || context.check(&f.hub),
        ));
        assert!(futures::poll!(delivery.as_mut()).is_pending());
        assert!(f.hub.store.memory_boundary.try_lock().is_ok());
        if deleted {
            f.hub.delete_bot(&f.cfg.id).unwrap();
        } else {
            revoke(&f);
            f.hub.update_bot_by_user(&json!({"id":f.cfg.id,"computer":false})).unwrap();
        }
        assert!(delivery.await.is_err());
        assert_eq!(writer.0, line[..line.len() - 2]);
        assert!(serde_json::from_slice::<Value>(&writer.0).is_err(), "EOF não completa concessão");
        writer.0.extend_from_slice(b"}\n");
        assert!(serde_json::from_slice::<Value>(&writer.0).is_ok(), "faltava só o commit retido");
    }
}

#[tokio::test]
async fn r2_pipe_com_backpressure_nao_entrega_concessao() {
    let f = Fixture::new();
    let _commands = f.hub.test_bot_commands(&f.cfg.id);
    let context = f.context();
    let (mut writer, mut reader) = tokio::io::duplex(8);
    writer.write_all(b"anterior").await.unwrap();
    let line = b"{\"result\":{\"outcome\":\"allow\"}}\n";
    let mut delivery = Box::pin(crate::fork::spaces::tool_effects::write_checked(
        &mut writer,
        line,
        &f.hub.store.memory_boundary,
        || context.check(&f.hub),
    ));
    assert!(futures::poll!(delivery.as_mut()).is_pending());
    assert!(f.hub.store.memory_boundary.try_lock().is_ok(), "Pending libera a barreira");
    revoke(&f);
    assert!(delivery.await.is_err());
    writer.shutdown().await.unwrap();
    let mut bytes = vec![];
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"anterior", "nenhum byte da concessão após revogar durante Pending");
}

#[tokio::test]
async fn r2_team_na_fila_do_ator_recusa_stop_ou_reconfiguracao_antes_de_reservar() {
    for name in ["ask_bot", "message_bot"] {
        for stop in [true, false] {
            let f = Fixture::new();
            let mut source = f.hub.test_bot_commands(&f.cfg.id);
            let target = uuid::Uuid::new_v4().to_string();
            let cfg: BotConfig = serde_json::from_value(json!({"id":target,"name":"Destino","cwd":f.dir})).unwrap();
            f.hub.store.save_bot(&cfg).unwrap();
            let mut recipient = f.hub.test_bot_commands(&target);
            f.hub.team.start_turn(&f.cfg.id, None);
            let mut actor = actor(&f);
            if stop {
                f.hub.send_cmd(&f.cfg.id, Cmd::Stop).unwrap();
            }
            let mut body = f.body.clone();
            body["name"] = name.into();
            body["arguments"] = json!({"botId":target,"message":"EFEITO_QUE_NAO_PODE_ENTRAR"});
            let mut call = Box::pin(tool_context::route(&f.hub, "teamCall", &body));
            assert!(
                futures::poll!(call.as_mut()).is_pending(),
                "handler validou a origem e aguarda admissão pelo ator"
            );
            let (done, _) = mpsc::unbounded_channel();
            if stop {
                let stop = source.try_recv().unwrap();
                assert!(matches!(stop, Cmd::Stop));
                actor.on_cmd(stop, &done).await;
            } else {
                revoke(&f);
            }
            let admission = source.try_recv().unwrap();
            assert!(matches!(admission, Cmd::ContextTeam { .. }));
            actor.on_cmd(admission, &done).await;
            assert!(call.await.is_err());
            assert!(recipient.try_recv().is_err(), "destinatário não pode receber pedido");
            assert_eq!(f.hub.team.test_pending_counts(), (0, 0), "não reservar aresta nem capacidade");
            assert!(f.hub.store.history(&target, i64::MAX, 20).unwrap().is_empty());
            assert_eq!(
                f.hub.store.history(f.cfg.session_owner(), i64::MAX, 20).unwrap().len(),
                1,
                "não publicar avisos"
            );
        }
    }
}

#[tokio::test]
async fn r3_delete_mantem_a_barreira_ate_invalidar_e_recusa_team_na_fila() {
    for name in ["ask_bot", "message_bot"] {
        let f = Fixture::new();
        let mut source = f.hub.test_bot_commands(&f.cfg.id);
        let target = uuid::Uuid::new_v4().to_string();
        let cfg: BotConfig = serde_json::from_value(json!({"id":target,"name":"Destino","cwd":f.dir})).unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
        let mut recipient = f.hub.test_bot_commands(&target);
        f.hub.team.start_turn(&f.cfg.id, None);
        let mut actor = actor(&f);
        let mut body = f.body.clone();
        body["name"] = name.into();
        body["arguments"] = json!({"botId":target,"message":"Pedido antes de deleteBot"});
        let mut call = Box::pin(tool_context::route(&f.hub, "teamCall", &body));
        assert!(futures::poll!(call.as_mut()).is_pending());
        let admission = source.try_recv().unwrap();
        assert!(matches!(admission, Cmd::ContextTeam { .. }));

        let (entered, at_cut) = tokio::sync::oneshot::channel();
        let (release, resume) = std::sync::mpsc::channel();
        f.hub.test_before_bot_deletion(move || {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        });
        let hub = f.hub.clone();
        let id = f.cfg.id.clone();
        let deleting = tokio::task::spawn_blocking(move || hub.delete_bot(&id));
        at_cut.await.unwrap();
        assert!(f.hub.store.memory_boundary.try_lock().is_err(), "exclusão reservou a barreira externa");
        assert!(!f.hub.store.bot(&f.cfg.id).unwrap().unwrap().deleted, "corte anterior ao commit");
        assert!(source.try_recv().is_err(), "nenhum Shutdown antes do commit durável");
        assert!(recipient.try_recv().is_err());
        assert_eq!(f.hub.team.test_pending_counts(), (0, 0));
        release.send(()).unwrap();
        deleting.await.unwrap().unwrap();
        assert!(matches!(source.try_recv().unwrap(), Cmd::Shutdown));

        let (done, _) = mpsc::unbounded_channel();
        actor.on_cmd(admission, &done).await;
        assert!(call.await.is_err());
        assert!(recipient.try_recv().is_err(), "nenhum pedido ao destinatário");
        assert_eq!(f.hub.team.test_pending_counts(), (0, 0));
        assert!(f.hub.store.history(&target, i64::MAX, 20).unwrap().is_empty());
        assert!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().deleted);
    }
}

#[tokio::test]
async fn r2_team_admitido_entrega_uma_vez_e_conserva_destino_e_cotas() {
    for name in ["ask_bot", "message_bot"] {
        let f = Fixture::new();
        let mut source = f.hub.test_bot_commands(&f.cfg.id);
        let target = uuid::Uuid::new_v4().to_string();
        let cfg: BotConfig = serde_json::from_value(json!({"id":target,"name":"Destino","cwd":f.dir})).unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
        let mut recipient = f.hub.test_bot_commands(&target);
        f.hub.team.start_turn(&f.cfg.id, None);
        let mut actor = actor(&f);
        let mut body = f.body.clone();
        body["name"] = name.into();
        body["arguments"] = json!({"botId":target,"message":"Pedido válido"});
        let mut call = Box::pin(tool_context::route(&f.hub, "teamCall", &body));
        assert!(futures::poll!(call.as_mut()).is_pending());
        let (done, _) = mpsc::unbounded_channel();
        actor.on_cmd(source.try_recv().unwrap(), &done).await;
        let Cmd::BotRequest(request) = recipient.try_recv().unwrap() else { panic!("pedido esperado") };
        request.complete(Ok("Resposta válida".into()), false);
        assert!(call.await.is_ok());
        while let Ok(command) = recipient.try_recv() {
            assert!(!matches!(command, Cmd::BotRequest(_)), "não duplicar pedido aceito");
        }
        assert_eq!(f.hub.team.test_pending_counts(), (0, 0));
        assert!(f.hub.store.history(&f.cfg.id, i64::MAX, 20).unwrap().is_empty());
        assert!(
            f.hub
                .store
                .history(f.cfg.session_owner(), i64::MAX, 20)
                .unwrap()
                .iter()
                .any(|e| e.data["delegationId"].is_string())
        );
    }
}

#[tokio::test]
async fn team_contextual_revalida_o_par_na_transacao_sem_admissao_parcial() {
    use crate::chat::team::admission_tests::{fail_recipient, no_notices, snapshot};
    use crate::fork::sessoes::{bindings, call_tests};
    use std::sync::atomic::Ordering;

    for name in ["ask_bot", "message_bot"] {
        for mode in ["swap", "revoke", "sql"] {
            let f = Fixture::new();
            let mut source = f.hub.test_bot_commands(&f.cfg.id);
            let target = uuid::Uuid::new_v4().to_string();
            let cfg: BotConfig = serde_json::from_value(json!({"id":target,"name":"Destino","cwd":f.dir})).unwrap();
            f.hub.store.save_bot(&cfg).unwrap();
            let mut recipient = f.hub.test_bot_commands(&target);
            f.hub.team.start_turn(&f.cfg.id, None);
            let mut actor = actor(&f);
            let before = snapshot(&f.hub, &target).unwrap();
            let mut events = f.hub.events.subscribe();
            let mut body = f.body.clone();
            body["name"] = name.into();
            body["arguments"] = json!({"botId":target,"message":"Par contextual"});
            let mut call =
                Box::pin(crate::api::dispatch(&f.hub, &crate::api::devices::Caller::Local, "teamCall", body.clone()));
            assert!(futures::poll!(call.as_mut()).is_pending());
            let hub = f.hub.clone();
            let original = f.cfg.clone();
            let reached = if mode == "sql" {
                fail_recipient(&f.hub, &target).unwrap();
                None
            } else {
                Some(call_tests::interleave(&f.hub.store, "team_notices_commit", move || {
                    if mode == "revoke" {
                        bindings::revoke(&hub.store, &original.id, Some("sessao-A")).unwrap();
                    } else {
                        let next = bindings::issue(
                            &hub.store,
                            &original.id,
                            original.session_owner(),
                            original.session_owner(),
                            Some("sessao-B"),
                        )
                        .unwrap();
                        bindings::activate_for_config(&hub.store, &next, "sessao-B", &original).unwrap();
                        hub.set_runtime(&original.id, |r| r.session = Some("sessao-B".into()));
                    }
                }))
            };
            let (done, _) = mpsc::unbounded_channel();
            actor.on_cmd(source.try_recv().unwrap(), &done).await;
            let error = call.await.unwrap_err();
            if let Some(reached) = reached {
                assert!(reached.load(Ordering::SeqCst));
            } else {
                assert!(error.to_string().contains("team second insert"), "{error:#}");
            }
            assert_eq!(snapshot(&f.hub, &target).unwrap(), before, "{name}/{mode}");
            assert!(recipient.try_recv().is_err(), "nenhum pedido nem CancelAsk na admissão negada");
            assert!(source.try_recv().is_err());
            no_notices(&mut events);
            if mode == "sql" {
                f.hub.store.db.locked().execute_batch("DROP TRIGGER team_second_insert").unwrap();
                let mut valid =
                    Box::pin(crate::api::dispatch(&f.hub, &crate::api::devices::Caller::Local, "teamCall", body));
                assert!(futures::poll!(valid.as_mut()).is_pending());
                actor.on_cmd(source.try_recv().unwrap(), &done).await;
                let Cmd::BotRequest(request) = recipient.try_recv().unwrap() else { panic!("pedido esperado") };
                let after = snapshot(&f.hub, &target).unwrap();
                assert_eq!(after.0, before.0 + 2);
                assert_eq!(after.1.as_deref(), Some("1"));
                assert_eq!(after.2, if name == "ask_bot" { (1, 0) } else { (0, 1) });
                request.complete(Ok("Resposta válida".into()), false);
                assert!(valid.await.is_ok());
                assert_eq!(f.hub.team.test_pending_counts(), (0, 0));
                while let Ok(command) = recipient.try_recv() {
                    assert!(!matches!(command, Cmd::BotRequest(_)));
                }
            }
        }
    }
}

#[tokio::test]
async fn permissao_nativa_conserva_captura_e_cartao_selo_marcador_sao_atomicos() {
    use crate::fork::sessoes::{bindings, call_tests};
    use std::sync::atomic::Ordering;

    for mode in ["reissue", "seal"] {
        let f = Fixture::new();
        let mut actor = actor(&f);
        let original = f.context().turn.execution.unwrap();
        let params = json!({"sessionId":"sessao-A","toolCall":{"title":"Nativo","kind":"execute"},
            "options":[{"optionId":"allow","name":"Permitir","kind":"allow_once"}]});
        let reached = if mode == "reissue" {
            let hub = f.hub.clone();
            let cfg = f.cfg.clone();
            Some(call_tests::interleave(&f.hub.store, "permission_card_origin", move || {
                bindings::revoke(&hub.store, &cfg.id, Some("sessao-A")).unwrap();
                let next =
                    bindings::issue(&hub.store, &cfg.id, cfg.session_owner(), cfg.session_owner(), None).unwrap();
                bindings::activate_for_config(&hub.store, &next, "sessao-A", &cfg).unwrap();
                assert_ne!(Some(next.id.as_str()), original.acp_binding());
            }))
        } else {
            // Transcrito legado passivo: não cria sessão extra de Space, binding ou turno.
            // O marcador precisa reconciliar essa linha antiga com a principal canônica.
            let legacy = f
                .hub
                .store
                .insert_entry(
                    &Lane { chat: f.cfg.session_owner().into(), session: Some("historico-legado".into()) },
                    EntryKind::Notice,
                    1,
                    &json!({"text":"Histórico da outra sessão"}),
                )
                .unwrap();
            assert!(
                crate::fork::sessoes::store::session_ref_of_entry(
                    &f.hub.store.db.locked(),
                    f.cfg.session_owner(),
                    &legacy.id,
                )
                .is_err(),
                "transcrito antigo não emite identidade de sessão"
            );
            assert!(crate::fork::sessoes::marker::needed(&f.hub.store.db.locked(), &actor.lane).unwrap().is_some());
            f.hub
                .store
                .db
                .locked()
                .execute_batch(
                    "CREATE TRIGGER card_seal_failure BEFORE INSERT ON kv WHEN NEW.k LIKE 'donna.tools.card.%'
                 BEGIN SELECT RAISE(ABORT,'card seal failure'); END;",
                )
                .unwrap();
            None
        };
        let before = f.hub.store.history(f.cfg.session_owner(), i64::MAX, 50).unwrap();
        let rev = f.hub.store.current_rev();
        let mut events = f.hub.events.subscribe();
        actor.on_permission(json!("native-original"), params.clone()).await;
        if let Some(reached) = reached {
            assert!(reached.load(Ordering::SeqCst));
        } else {
            assert_eq!(f.hub.store.current_rev(), rev, "rollback inclui rev do marcador e do cartão");
        }
        assert!(actor.perms.is_empty());
        assert_eq!(f.hub.runtime(&f.cfg.id).status, BotStatus::Working);
        assert_eq!(f.hub.store.history(f.cfg.session_owner(), i64::MAX, 50).unwrap().len(), before.len());
        assert_eq!(f.hub.store.kv_prefix("donna.tools.card."), [] as [String; 0]);
        while let Ok(event) = events.try_recv() {
            assert_ne!(event["type"], "entry", "sem cartão/marcador publicado na recusa: {event}");
        }
        if mode == "seal" {
            f.hub.store.db.locked().execute_batch("DROP TRIGGER card_seal_failure").unwrap();
            actor.on_permission(json!("native-valid"), params).await;
            let entries = f.hub.store.history(f.cfg.session_owner(), i64::MAX, 50).unwrap();
            assert_eq!(entries.len(), before.len() + 2, "marcador e cartão confirmados juntos");
            let card = entries.iter().find(|e| e.kind == "permission").unwrap();
            assert!(f.hub.store.kv_read(&format!("donna.tools.card.{}", card.id)).unwrap().is_some());
            assert_eq!(actor.perms.len(), 1);
            assert_eq!(f.hub.runtime(&f.cfg.id).status, BotStatus::NeedsInput);
            assert!(
                tool_context::route(&f.hub, "manageCall", &f.body).await.is_err(),
                "gestão contextual continua negada"
            );
        }
    }
}
