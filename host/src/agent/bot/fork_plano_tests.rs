//! Plano de código só para conversa (Kimi Code): rotina, acordar de tarefa agendada, aviso de comentário, pedido de
//! outro bot, canal e compactação com o dono ausente não abrem o processo do OpenCode com a chave do plano. A prova
//! passa pelo `send` real da API (por onde tarefa e aviso entram, com a marca Hub-side) e pelo `ensure_session` real
//! do ator, o mesmo ponto por onde todo turno começa.
//!
//! Limite conhecido: os processos de teste (`cat`) seguem o padrão dos demais testes do crate e não são portáveis ao
//! Windows, onde o crate de testes não é alvo do CI.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::LockExt;
use crate::fork::planos_codigo::Uso;
use zeroize::Zeroizing;

const CHAVE: &str = "chave-ficticia-do-plano-kimi-123";

fn com_plano() -> (Arc<Hub>, BotConfig) {
    let dir =
        std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("plano-conversa-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = crate::store::Store::open(&dir.join("test.db")).unwrap();
    *store.secret_key.locked() = Some(Zeroizing::new(vec![7; 32]));
    let cfg: BotConfig = serde_json::from_value(json!({
        "id": "kimi", "name": "Kimi", "backend": "opencode", "model": "kimi-code-plan-global/k3",
        "cwd": "/tmp", "spaceId": crate::fork::spaces::PERSONAL, "notify": false
    }))
    .unwrap();
    store.save_bot(&cfg).unwrap();
    crate::market::vault::write(&store, "plano-codigo/kimi-code", CHAVE).unwrap();
    let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
    (Hub::new(store, "fixture".into(), identity, "fixture".into(), 0), cfg)
}

/// O relatório que o app declara. Não é prova de nada (quem chama `send` pode construir um igual); a regra do plano
/// não o usa. Serve só para provar que ele não abre caminho sozinho.
fn relatorio_do_app(texto: &str) -> Value {
    let len = texto.encode_utf16().count();
    json!({
        "schema": 1, "draftId": uuid::Uuid::new_v4().to_string(), "revision": 2, "textLength": len,
        "deviceId": uuid::Uuid::new_v4().to_string(), "channel": "desktopComposer", "interaction": "send",
        "segments": [{"start": 0, "end": len, "kind": "typed"}]
    })
}

/// Um `send` pela API, como o app do dono (chamador local autenticado) ou como o Hub interno (`automatico`).
async fn enviar(hub: &Arc<Hub>, texto: &str, automatico: bool) -> Entry {
    let nonce = uuid::Uuid::new_v4().to_string();
    let body = json!({
        "botId": "kimi", "text": texto, "clientNonce": nonce, "attachments": [],
        "contentOrigin": relatorio_do_app(texto)
    });
    let _ = if automatico {
        crate::api::dispatch_do_hub(hub, "send", body).await
    } else {
        crate::api::dispatch(hub, &crate::api::devices::Caller::Local, "send", body).await
    };
    hub.store.find_by_nonce("kimi", &nonce).expect("a entrada foi gravada")
}

#[tokio::test]
async fn so_a_palavra_do_dono_pelo_ingresso_do_hub_e_conversa() {
    let (hub, cfg) = com_plano();
    let mut actor = new_actor(hub.clone(), cfg);

    // Turno sem entrada nenhuma (oculto, retomada) não é conversa.
    assert_eq!(actor.uso_do_plano(), Uso::Automatico);

    // A palavra do dono entra pelo chamador local autenticado do app dele: conversa, com ou sem relatório do app.
    let dono = enviar(&hub, "me ajuda com esse código", false).await;
    assert_eq!(dono.data["contentOrigin"]["ingress"]["caller"], "local");
    actor.turn_entries = vec![dono.id.clone()];
    assert_eq!(actor.uso_do_plano(), Uso::Conversa);

    // Acordar de tarefa e aviso de comentário entram pela marca Hub-side, mesmo forjando o relatório do app:
    // o corpo do pedido não define nem apaga a marca.
    let aviso = enviar(&hub, "Comentário do dono na tarefa «x»", true).await;
    assert_eq!(aviso.data["contentOrigin"]["hubOrigin"], "automatico", "a marca é do Hub, não do corpo");
    actor.turn_entries = vec![aviso.id.clone()];
    assert_eq!(actor.uso_do_plano(), Uso::Automatico, "aviso marcado pelo Hub não é conversa");
    actor.turn_entries = vec![dono.id.clone(), aviso.id.clone()];
    assert_eq!(actor.uso_do_plano(), Uso::Automatico, "lote com uma entrada automática nega o turno");

    // Marcadores vencem o ingresso: rotina, pedido de outro bot e canal não são conversa.
    actor.turn_entries = vec![dono.id.clone()];
    actor.active_routine = Some("rotina".into());
    assert_eq!(actor.uso_do_plano(), Uso::Automatico);
    actor.active_routine = None;

    let (request, _resposta) = crate::chat::team::BotRequest::for_test("pedido");
    actor.active_request = Some(request);
    assert_eq!(actor.uso_do_plano(), Uso::Automatico);
    actor.active_request = None;

    let canal = Lane::main("canal");
    let (reply, _resposta) = tokio::sync::oneshot::channel();
    actor.active_group = Some(GroupTurn {
        lane: canal.clone(),
        prompt: "[Channel: Sala]
Mensagem"
            .into(),
        shown: Vec::new(),
        epoch: hub.groups.reserve_epoch_for_test(&canal),
        authority: None,
        reply,
    });
    assert_eq!(actor.uso_do_plano(), Uso::Automatico);
    actor.active_group = None;

    // Compactação: pedida pelo dono (ou no fim do turno dele) segue a conversa; com o dono ausente é automática.
    actor.compacting = Some(true);
    assert_eq!(actor.uso_do_plano(), Uso::Automatico, "compactação com o dono ausente não é conversa");
    actor.compacting = Some(false);
    assert_eq!(
        actor.uso_do_plano(),
        Uso::Automatico,
        "compactação sem pedido do dono nem turno dele antes não é conversa"
    );
    actor.ultimo_turno_do_dono = true;
    assert_eq!(actor.uso_do_plano(), Uso::Conversa, "compactação ao fim de um turno do dono continua a conversa dele");
    actor.ultimo_turno_do_dono = false;
    actor.context_compaction = Some(crate::fork::contexto::ActiveCompaction {
        target: crate::fork::contexto::Target {
            bot: "kimi".into(),
            hub_id: "hub".into(),
            space_id: crate::fork::spaces::PERSONAL.into(),
            epoch: 0,
            lane: Lane::main("kimi"),
            acp: None,
            engine: json!({}),
        },
        owner: None,
    });
    assert_eq!(actor.uso_do_plano(), Uso::Conversa, "compactar a pedido do dono pela API continua a conversa dele");
}

#[tokio::test]
async fn rotina_em_outra_conta_nao_herda_a_origem_da_conversa_do_dono() {
    let (hub, cfg) = com_plano();
    let mut actor = new_actor(hub.clone(), cfg);
    let antes = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);

    // 1. Conversa do dono no plano: o `ensure_session` real registra a origem dela.
    let dono = enviar(&hub, "me ajuda com esse código", false).await;
    actor.turn_entries = vec![dono.id.clone()];
    let _ = actor.ensure_session(None).await;
    assert!(actor.ultimo_turno_do_dono, "a conversa do dono foi registrada");

    // 2. A rotina roda com uma conta explicitamente selecionada: esse ramo nunca passava pela lógica antiga, e a
    //    falha na conta desconhecida vem DEPOIS do registro, então a limpeza só existe com a correção.
    actor.cfg.fork_editor.engine_account_id = Some("conta-outra".into());
    actor.turn_entries = vec![];
    actor.active_routine = Some("rotina".into());
    let _ = actor.ensure_session(None).await;
    assert!(!actor.ultimo_turno_do_dono, "a rotina não herda a origem da conversa");
    actor.active_routine = None;
    actor.cfg.fork_editor.engine_account_id = None;

    // 3. A compactação enfileirada pelo turno da rotina (sem pedido do dono) é automática pela fila real.
    actor.queue.push_back(Queued::Compact { idle: false, manual: None, dono: actor.ultimo_turno_do_dono });
    let Some(Queued::Compact { idle, manual, dono }) = actor.queue.pop_front() else { panic!("enfileirada") };
    assert!(!idle && manual.is_none() && !dono);

    // 4. O bot volta ao plano antes da compactação rodar: ela é recusada, sem processo nem leitura do cofre.
    actor.cfg.backend = "opencode".into();
    actor.cfg.model = "kimi-code-plan-global/k3".into();
    actor.compactacao_do_dono = manual.is_some() || dono;
    actor.compacting = Some(false);
    let erro = actor.ensure_session(None).await.expect_err("compactação motivada por rotina").to_string();
    assert!(erro.contains("plano_so_conversa"), "{erro}");
    assert!(actor.conn.is_none(), "nenhum processo do OpenCode foi aberto");
    assert_eq!(
        hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst),
        antes,
        "o cofre nem foi lido"
    );
}

#[tokio::test]
async fn compactacao_enfileirada_ao_fim_da_conversa_conserva_a_origem_na_fila() {
    let (hub, cfg) = com_plano();
    let mut actor = new_actor(hub.clone(), cfg);
    // A conversa do dono terminou de verdade: o `ensure_session` real reconheceu a entrada dele.
    let dono = enviar(&hub, "me ajuda com esse código", false).await;
    actor.turn_entries = vec![dono.id.clone()];
    assert_eq!(actor.uso_do_plano(), Uso::Conversa);
    let uso = actor.uso_do_plano();
    actor.ultimo_turno_do_dono = uso == Uso::Conversa;
    assert!(actor.ultimo_turno_do_dono);

    // O fim do turno dela enfileira a compactação, preservando a origem. O push/pop manual copia o contrato da fila
    // (`queue.rs` retira o item e repassa `dono` a `start_requested_compaction`); o laço completo de `next_in_queue`
    // não é exercitado porque abrir um turno de verdade exige um motor de verdade, fora deste teste.
    actor.queue.push_back(Queued::Compact { idle: false, manual: None, dono: actor.ultimo_turno_do_dono });
    let Some(Queued::Compact { idle, manual, dono }) = actor.queue.pop_front() else {
        panic!("a compactação foi enfileirada")
    };
    assert!(!idle && manual.is_none());
    assert!(dono, "a origem é a do enfileiramento, não do estado do momento");

    // Um turno automático cruza antes da execução e zera o estado do turno anterior: o preservado na fila é que vale.
    actor.ultimo_turno_do_dono = false;
    actor.compactacao_do_dono = false;
    actor.compacting = Some(false);
    // É o que `start_requested_compaction` grava ao executar, antes do `ensure_session`.
    actor.compactacao_do_dono = manual.is_some() || dono;
    assert_eq!(
        actor.uso_do_plano(),
        Uso::Conversa,
        "a compactação enfileirada pela conversa do dono continua a conversa dele"
    );
}

#[tokio::test]
async fn compactacao_depois_de_turno_automatico_e_recusada_pelo_fluxo_real() {
    let (hub, cfg) = com_plano();
    let mut actor = new_actor(hub.clone(), cfg);
    let antes = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);

    // Uma rotina termina e o fim do turno dela enfileira a compactação (`idle: false`, sem pedido do dono).
    actor.active_routine = Some("rotina".into());
    assert!(actor.ensure_session(None).await.expect_err("rotina").to_string().contains("plano_so_conversa"));
    actor.active_routine = None;
    assert!(!actor.ultimo_turno_do_dono);

    actor.compacting = Some(false);
    let erro = actor.ensure_session(None).await.expect_err("compactação depois da rotina").to_string();
    assert!(erro.contains("plano_so_conversa"), "{erro}");
    assert!(actor.conn.is_none(), "nenhum processo do OpenCode foi aberto");
    assert_eq!(
        hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst),
        antes,
        "o cofre nem foi lido"
    );
}

#[tokio::test]
async fn acordar_de_tarefa_e_aviso_nao_abrem_o_opencode_do_kimi() {
    let (hub, cfg) = com_plano();
    let mut actor = new_actor(hub.clone(), cfg);
    let antes = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);

    // O acordar de tarefa e o aviso de comentário chegam com a marca Hub-side: recusados antes do cofre.
    let aviso = enviar(&hub, "Comentário do dono na tarefa «acordar» (projeto «p»):", true).await;
    actor.turn_entries = vec![aviso.id.clone()];
    let erro = actor.ensure_session(None).await.expect_err("aviso do Hub").to_string();
    assert!(erro.contains("plano_so_conversa"), "{erro}");
    assert!(!erro.contains(CHAVE));

    // A delegação (pedido de outro bot) também não abre, mesmo com a palavra do dono no lote.
    let dono = enviar(&hub, "só conversando", false).await;
    actor.turn_entries = vec![dono.id.clone()];
    let (request, _resposta) = crate::chat::team::BotRequest::for_test("pedido");
    actor.active_request = Some(request);
    assert!(actor.ensure_session(None).await.expect_err("pedido").to_string().contains("plano_so_conversa"));
    actor.active_request = None;

    actor.compacting = Some(true);
    assert!(
        actor.ensure_session(None).await.expect_err("compactação ausente").to_string().contains("plano_so_conversa")
    );
    actor.compacting = None;

    assert!(actor.conn.is_none(), "nenhum processo do OpenCode foi aberto");
    assert_eq!(
        hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst),
        antes,
        "a chave do plano não foi nem lida"
    );
}

#[tokio::test]
async fn recusa_de_uso_automatico_nao_mata_a_sessao_viva_do_dono() {
    let (hub, cfg) = com_plano();
    let mut actor = new_actor(hub.clone(), cfg);
    // Sessão viva do dono: um processo de verdade rodando, como fica depois de uma conversa dele.
    let (acp, rx) = Acp::spawn("cat", std::env::temp_dir().to_str().unwrap(), &[]).unwrap();
    let pid = acp.pid();
    actor.conn = Some(Conn {
        ponte: None,
        plano: None,
        curated: crate::fork::conectores::Pool::default(),
        acp,
        rx,
        skill_reads: tokio::task::JoinSet::new(),
        sessions: SessionCaps { load: true, close: true, system_prompt: true },
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
    let antes = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);

    // Um aviso do Hub no meio da sessão viva: recusado sem encerrar a sessão do dono nem ler o cofre.
    let aviso = enviar(&hub, "Comentário do dono na tarefa «x»:", true).await;
    actor.turn_entries = vec![aviso.id.clone()];
    let erro = actor.ensure_session(None).await.expect_err("aviso na sessão viva").to_string();
    assert!(erro.contains("plano_so_conversa"), "{erro}");
    let conn = actor.conn.as_ref().expect("a sessão do dono segue viva");
    assert_eq!(conn.acp.pid(), pid, "o processo do dono não foi trocado nem morto");
    assert_eq!(
        hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst),
        antes,
        "a recusa vem antes de qualquer leitura do cofre"
    );

    // E a conversa dele continua valendo: a entrada pelo app dele segue sendo conversa.
    let dono = enviar(&hub, "continuando", false).await;
    actor.turn_entries = vec![dono.id.clone()];
    assert_eq!(actor.uso_do_plano(), Uso::Conversa);
    if let Some(conn) = actor.conn.take() {
        conn.kill().await;
    }
}
