//! Testes focados do runtime de tarefas (MX-142 Hub-B): migração, outbox por
//! trigger, gate de responsável, agenda idempotente e `mark_answered`.
use super::store;
use super::*;
use super::{acordar, avisos};
use rusqlite::params;

const HUB: &str = "00000000-0000-4000-8000-0000000000aa";
const SPACE: &str = "00000000-0000-4000-8000-000000000001";
const PROJECT: &str = "00000000-0000-4000-8000-0000000000bb";
const TASK: &str = "00000000-0000-4000-8000-0000000000cc";
const BOT: &str = "00000000-0000-4000-8000-0000000000dd";
const REQUEST: &str = "00000000-0000-4000-8000-0000000000ee";

fn fixture() -> rusqlite::Connection {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    // A tabela de comentários precisa existir ANTES do migrate para o trigger
    // do outbox ser criado (mesma ordem de fork::migrate: dados → runtime).
    c.execute_batch(
        "CREATE TABLE donna_task_comments(
            seq INTEGER PRIMARY KEY AUTOINCREMENT, hub_id TEXT, space_id TEXT,
            project_id TEXT, task_id TEXT, comment_id TEXT, request_id TEXT,
            payload TEXT, entry TEXT);
         CREATE TABLE entries(id TEXT, bot_id TEXT, session TEXT, turn INTEGER, kind TEXT, data TEXT);",
    )
    .unwrap();
    // entries precisa existir ANTES do migrate: os guards TOCTOU do runtime
    // são triggers BEFORE INSERT nela.
    migrate(&c).unwrap();
    // A coluna da proposta na agenda é da frente de propostas; a ordem aqui
    // é a mesma do fork::migrate.
    crate::fork::propostas::migrate(&c).unwrap();
    c.execute_batch(
        "CREATE TABLE kv(k TEXT PRIMARY KEY, v TEXT);
         CREATE TABLE donna_projects(hub_id TEXT, space_id TEXT, id TEXT, project TEXT, tasks TEXT);
         CREATE TABLE bots(id TEXT PRIMARY KEY, config TEXT, deleted INTEGER DEFAULT 0);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO kv VALUES('donna.spaces.v1',?1)",
        [serde_json::json!({
            "version":1,"personalSpaceId":SPACE,"revision":1,
            "spaces":[{"id":SPACE,"kind":"personal","name":"Pessoal","position":0,"revision":1,
                "appearance":{"wallpaper":"none","blur":0.0,"transparency":50.0,"veilLight":0.18,"veilDark":0.32}}],
        })
        .to_string()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO bots(id,config) VALUES(?1,?2)",
        params![BOT, r#"{"name":"Berga","spaceId":"00000000-0000-4000-8000-000000000001"}"#],
    )
    .unwrap();
    let project = serde_json::json!({"id":PROJECT,"name":"Geral","state":"active","coordinatorBotId":BOT});
    let tasks = serde_json::json!([{"id":TASK,"title":"Subtarefa","assigneeBotId":BOT,"state":"backlog"}]);
    c.execute(
        "INSERT INTO donna_projects VALUES(?1,?2,?3,?4,?5)",
        params![HUB, SPACE, PROJECT, project.to_string(), tasks.to_string()],
    )
    .unwrap();
    c
}

#[test]
fn migra_cria_tabelas_e_trigger_do_outbox() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE donna_task_comments(hub_id TEXT, space_id TEXT, project_id TEXT, task_id TEXT, comment_id TEXT, entry TEXT);
         CREATE TABLE entries(id TEXT, bot_id TEXT, session TEXT, turn INTEGER, kind TEXT, data TEXT);",
    )
    .unwrap();
    migrate(&c).unwrap();
    for table in ["donna_task_agenda", "donna_task_receipts", "donna_task_sessions", "donna_task_deliveries"] {
        let exists: bool = c
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(exists, "{table} ausente");
    }
    let trigger: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='donna_task_deliveries_outbox')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(trigger);
}

#[test]
fn gate_exige_responsavel_e_bot_do_space() {
    let c = fixture();
    match store::gate(&c, HUB, SPACE, PROJECT, TASK).unwrap() {
        store::Gate::Fire { bot_id, .. } => assert_eq!(bot_id, BOT),
        store::Gate::Refused(_) => panic!("deveria disparar"),
    }
    // Sem responsável: refused, nunca start implícito.
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![serde_json::json!([{"id":TASK,"title":"X","state":"backlog"}]).to_string(), PROJECT],
    )
    .unwrap();
    match store::gate(&c, HUB, SPACE, PROJECT, TASK).unwrap() {
        store::Gate::Refused(reason) => assert_eq!(reason, "no_assignee"),
        store::Gate::Fire { .. } => panic!("sem responsável não dispara"),
    }
}

#[test]
fn schedule_grava_e_limpa_com_recibo_idempotente() {
    let c = fixture();
    let at = crate::store::now_ms() + 60_000;
    let (answer, changed) = store::call(
        &c,
        HUB,
        "donnaTaskSchedule",
        serde_json::json!({
            "hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK,
            "clientRequestId":REQUEST,"startAt":at,
        }),
    )
    .unwrap();
    assert!(changed);
    assert_eq!(answer["agenda"]["state"], "scheduled");
    assert_eq!(answer["agenda"]["startAt"], at);
    // Replay do mesmo pedido devolve o resultado gravado sem reescrever.
    let (again, changed_again) = store::call(
        &c,
        HUB,
        "donnaTaskSchedule",
        serde_json::json!({
            "hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK,
            "clientRequestId":REQUEST,"startAt":at,
        }),
    )
    .unwrap();
    assert!(!changed_again);
    assert_eq!(again, answer);
    // Limpar: startAt null remove a agenda.
    let other = "00000000-0000-4000-8000-0000000000ff";
    let (cleared, _) = store::call(
        &c,
        HUB,
        "donnaTaskSchedule",
        serde_json::json!({
            "hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK,
            "clientRequestId":other,"startAt":null,
        }),
    )
    .unwrap();
    assert!(cleared["agenda"].is_null());
    let count: i64 = c.query_row("SELECT COUNT(*) FROM donna_task_agenda", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn outbox_so_entrega_mencao_autenticada_do_dono() {
    let c = fixture();
    let comment = |author: &str, only_note: bool, mentions: Value| {
        serde_json::json!({
            "id":"c1","at":1,"author":author,"text":"oi","mentions":mentions,"onlyNote":only_note,
        })
        .to_string()
    };
    let insert = |entry: String| {
        c.execute(
            "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry)
             VALUES(?1,?2,?3,?4,'c1','r1','{}',?5)",
            params![HUB, SPACE, PROJECT, TASK, entry],
        )
        .unwrap();
    };
    let deliveries = || -> i64 { c.query_row("SELECT COUNT(*) FROM donna_task_deliveries", [], |r| r.get(0)).unwrap() };
    // Dono + @ + sem onlyNote → uma entrega queued.
    insert(comment("owner", false, serde_json::json!([BOT])));
    assert_eq!(deliveries(), 1);
    // Só anotar → nada.
    c.execute("DELETE FROM donna_task_comments", []).unwrap();
    c.execute("DELETE FROM donna_task_deliveries", []).unwrap();
    insert(comment("owner", true, serde_json::json!([BOT])));
    assert_eq!(deliveries(), 0);
    // Sem menção → nada (regra: SEM @ nunca entrega).
    insert(comment("owner", false, serde_json::json!([])));
    assert_eq!(deliveries(), 0);
    // Autor bot → nunca desperta outro bot.
    c.execute("DELETE FROM donna_task_comments", []).unwrap();
    insert(comment("bot:outro", false, serde_json::json!([BOT])));
    assert_eq!(deliveries(), 0);
}

#[test]
fn mark_answered_exige_prova_causal_entry_turno() {
    let c = fixture();
    c.execute(
        "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
         VALUES(?1,?2,?3,?4,'c1',?5,'sess-1','sending',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    // Poke da entrega consumido pela execução: clientNonce da delivery,
    // sessão e turno da execução que comenta.
    let nonce = format!("mx142-task-aviso-c1-{BOT}");
    c.execute(
        "INSERT INTO entries(bot_id,session,turn,kind,data) VALUES(?1,'sess-1',7,'user',?2)",
        params![BOT, serde_json::json!({"clientNonce": nonce}).to_string()],
    )
    .unwrap();
    // Sem prova (turno errado) não marca.
    // Marca 'sent' (e session_id NULL, linha legada) DEPOIS do aceite.
    c.execute("UPDATE donna_task_deliveries SET status='sent',session_id=NULL", []).unwrap();
    // Sem prova (turno errado) não marca.
    assert!(!store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c1", BOT, "sess-1", 9).unwrap());
    // Com prova exata marca uma vez; replay é idempotente.
    assert!(store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c1", BOT, "sess-1", 7).unwrap());
    let status: String =
        c.query_row("SELECT status FROM donna_task_deliveries WHERE comment_id='c1'", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "answered");
    assert!(!store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c1", BOT, "sess-1", 7).unwrap());
}

fn agenda_row(c: &rusqlite::Connection, task: &str, nonce: &str) {
    c.execute(
        "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,1,1,'starting',?6,0,1,1)",
        params![HUB, SPACE, PROJECT, task, BOT, nonce],
    )
    .unwrap();
}

#[test]
fn restore_starting_prova_nonce_e_nunca_deadlocka() {
    let c = fixture();
    // 'starting' com nonce cujo poke JÁ foi aceito (entry clientNonce) -> started.
    agenda_row(&c, TASK, "n-ok");
    // Guard de aceite: nonce 'starting' + sessão vinculada à tarefa.
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-r',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-r','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n-ok"}).to_string()],
    )
    .unwrap();
    // 'starting' sem evidência -> volta a scheduled (mesma nonce, dedupe real).
    agenda_row(&c, "t2", "n-pending");
    // Fogo via bot órfão COM evidência D220 -> uncertain; SEM -> failed.
    // As tarefas t3/t4 precisam existir p/ o guard de admissão aceitar a
    // notice de evidência (ela representa uma admissão já commitada).
    agenda_row(&c, "t3", "mx142-bot-r1");
    agenda_row(&c, "t4", "mx142-bot-r2");
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([
                {"id":TASK,"title":"X","assigneeBotId":BOT,"state":"backlog"},
                {"id":"t3","title":"Y","assigneeBotId":BOT,"state":"backlog"},
                {"id":"t4","title":"Z","assigneeBotId":BOT,"state":"backlog"},
            ])
            .to_string(),
            PROJECT
        ],
    )
    .unwrap();
    // Evidência AUTENTICADA: taskDispatch estruturado (só o caminho interno
    // grava) com o nonce do claim t3.
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'main','notice',?2)",
        params![
            BOT,
            serde_json::json!({"botMessage": {
                "sourceBotId": "coord", "targetBotId": BOT,
                "taskDispatch": {"nonce": "mx142-bot-r1", "hubId": HUB, "spaceId": SPACE,
                    "projectId": PROJECT, "taskId": "t3"}}})
            .to_string()
        ],
    )
    .unwrap();
    // ISCA (TEC R2-22): notice com o marcador textual do claim t4 mas SEM
    // taskDispatch — texto livre não é prova; t4 deve ficar 'failed'.
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'main','notice',?2)",
        params![
            BOT,
            serde_json::json!({"botMessage": {"text": "trabalho\n\nReferência de despacho: mx142-bot-r2"}}).to_string()
        ],
    )
    .unwrap();
    // Roda duas vezes: idempotente e prova que não trava (deadlock seria hang).
    acordar::restore_starting(&c, HUB).unwrap();
    acordar::restore_starting(&c, HUB).unwrap();
    let states: Vec<String> = {
        let mut st = c.prepare("SELECT task_id||':'||state FROM donna_task_agenda ORDER BY task_id").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(
        states,
        vec![
            format!("{TASK}:started"),
            "t2:scheduled".to_string(),
            "t3:uncertain".to_string(),
            "t4:failed".to_string(),
        ]
    );
}

#[test]
fn restore_sending_prova_nonce_e_volta_a_fila() {
    let c = fixture();
    for comment in ["ca", "cb"] {
        c.execute(
            "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,'sess-'||?5,'sending',1)",
            params![HUB, SPACE, PROJECT, TASK, comment, BOT],
        )
        .unwrap();
    }
    // Só 'ca' tem poke aceito -> sent; 'cb' volta a queued.
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-ca','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": format!("mx142-task-aviso-ca-{BOT}")}).to_string()],
    )
    .unwrap();
    avisos::restore_sending(&c, HUB).unwrap();
    let states: Vec<String> = {
        let mut st =
            c.prepare("SELECT comment_id||':'||status FROM donna_task_deliveries ORDER BY comment_id").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(states, vec!["ca:sent".to_string(), "cb:queued".to_string()]);
}

#[test]
fn conclusao_de_claim_velho_nao_sobrescreve_reagendamento() {
    let c = fixture();
    // Claim em voo: agenda 'starting' com nonce N1.
    agenda_row(&c, TASK, "n-old");
    // Dono reagenda: schedule() grava 'scheduled' com nonce NOVO (fencing).
    let at = crate::store::now_ms() + 60_000;
    store::call(
        &c,
        HUB,
        "donnaTaskSchedule",
        serde_json::json!({
            "hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK,
            "clientRequestId":REQUEST,"startAt":at,
        }),
    )
    .unwrap();
    let fire_velho = acordar::Fire {
        space_id: SPACE.to_owned(),
        project_id: PROJECT.to_owned(),
        task_id: TASK.to_owned(),
        bot_id: BOT.to_owned(),
        title: "Subtarefa".to_owned(),
        project_name: "Geral".to_owned(),
        nonce: "n-old".to_owned(),
        attempts: 0,
        session_id: None,
        origin: String::new(),
        brief: String::new(),
    };
    // A conclusão do claim velho tenta 'started' — fenced pelo nonce.
    acordar::mark(&c, HUB, &fire_velho, "started", None, false).unwrap();
    let (state, start_at): (String, i64) =
        c.query_row("SELECT state,start_at FROM donna_task_agenda", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(state, "scheduled");
    assert_eq!(start_at, at);
    // Com o nonce NOVO a mesma marcação vale (claim corrente não é bloqueado).
    let nonce_novo: String = c.query_row("SELECT nonce FROM donna_task_agenda", [], |r| r.get(0)).unwrap();
    let fire_novo = acordar::Fire { nonce: nonce_novo, ..fire_velho };
    acordar::mark(&c, HUB, &fire_novo, "started", None, false).unwrap();
    let state: String = c.query_row("SELECT state FROM donna_task_agenda", [], |r| r.get(0)).unwrap();
    assert_eq!(state, "started");
}

#[test]
fn restore_sending_recupera_sessao_e_permite_ack_causal() {
    let c = fixture();
    c.execute(
        "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
         VALUES(?1,?2,?3,?4,'c9',?5,'sess-9','sending',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    // Poke aceito antes do crash: entry 'user' com o clientNonce da entrega,
    // na sessão extra da tarefa e no turno que a consumiu.
    c.execute(
        "INSERT INTO entries(bot_id,session,turn,kind,data) VALUES(?1,'sess-9',3,'user',?2)",
        params![BOT, serde_json::json!({"clientNonce": format!("mx142-task-aviso-c9-{BOT}")}).to_string()],
    )
    .unwrap();
    avisos::restore_sending(&c, HUB).unwrap();
    let (status, session): (String, Option<String>) =
        c.query_row("SELECT status,session_id FROM donna_task_deliveries", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(status, "sent");
    assert_eq!(session.as_deref(), Some("sess-9"));
    // O ack causal agora consegue a prova: session+turn da execução que comenta.
    assert!(store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c9", BOT, "sess-9", 3).unwrap());
    let status: String = c.query_row("SELECT status FROM donna_task_deliveries", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "answered");
}

#[test]
fn hub_divergente_e_recusado_em_leituras() {
    let c = fixture();
    let outro_hub = "00000000-0000-4000-8000-0000000000ff";
    // Agenda e Sessions recebem hubId do corpo: UUID válido mas divergente
    // do hub da conexão precisa ser recusado (antes era ignorado).
    for method in ["donnaTaskAgenda", "donnaTaskSessions"] {
        let body = if method == "donnaTaskAgenda" {
            serde_json::json!({"hubId": outro_hub, "spaceId": SPACE})
        } else {
            serde_json::json!({"hubId": outro_hub, "spaceId": SPACE, "projectId": PROJECT, "taskId": TASK})
        };
        let err = store::call(&c, HUB, method, body).unwrap_err().to_string();
        assert!(err.contains("forbidden"), "{method}: {err}");
    }
    // E o hub correto passa do gate de hub (pode falhar depois por outro
    // motivo, mas nunca por hub divergente).
    let ok = store::call(&c, HUB, "donnaTaskAgenda", serde_json::json!({"hubId": HUB, "spaceId": SPACE}));
    assert!(ok.is_ok(), "{:?}", ok.err());
}

#[test]
fn sessions_redigem_nome_de_bot_oculto_removido_ou_movido() {
    let c = fixture();
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-1',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    let title = |c: &rusqlite::Connection| {
        let (v, _) = store::call(
            c,
            HUB,
            "donnaTaskSessions",
            serde_json::json!({"hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK}),
        )
        .unwrap();
        v["sessions"][0]["title"].as_str().unwrap().to_owned()
    };
    let set_cfg = |c: &rusqlite::Connection, cfg: &str, deleted: i64| {
        c.execute("UPDATE bots SET config=?1, deleted=?2 WHERE id=?3", params![cfg, deleted, BOT]).unwrap();
    };
    // Bot vivo, visível e do mesmo space: o nome aparece (compatibilidade).
    assert_eq!(title(&c), "Berga");
    // Oculto: nome redigido, sessão continua listada.
    set_cfg(&c, r#"{"name":"Berga","spaceId":"00000000-0000-4000-8000-000000000001","hidden":true}"#, 0);
    assert_eq!(title(&c), BOT);
    // Removido logicamente: redigido.
    set_cfg(&c, r#"{"name":"Berga","spaceId":"00000000-0000-4000-8000-000000000001"}"#, 1);
    assert_eq!(title(&c), BOT);
    // Vivo e visível, mas movido para outro Space: redigido.
    set_cfg(&c, r#"{"name":"Berga","spaceId":"00000000-0000-4000-8000-0000000000ff"}"#, 0);
    assert_eq!(title(&c), BOT);
    // Grupo nunca é alvo de tarefa e segue a mesma regra de visibilidade.
    set_cfg(&c, r#"{"name":"Berga","spaceId":"00000000-0000-4000-8000-000000000001","kind":"group"}"#, 0);
    assert_eq!(title(&c), BOT);
    // Bot sem spaceId no config pertence ao space pessoal (default do serde):
    // continua visível quando a tarefa é do Pessoal.
    set_cfg(&c, r#"{"name":"Berga"}"#, 0);
    assert_eq!(title(&c), "Berga");
}

#[test]
fn gate_recusa_tarefa_done_ou_cancelled() {
    let c = fixture();
    let set_state = |state: &str| {
        c.execute(
            "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
            params![
                serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":state}]).to_string(),
                PROJECT
            ],
        )
        .unwrap();
    };
    for state in ["done", "cancelled"] {
        set_state(state);
        match store::gate(&c, HUB, SPACE, PROJECT, TASK).unwrap() {
            store::Gate::Refused(reason) => assert_eq!(reason, "task_closed", "{state}"),
            store::Gate::Fire { .. } => panic!("{state} não pode disparar"),
        }
    }
    // Estados ativos continuam disparando.
    for state in ["backlog", "in_progress", "review", "blocked"] {
        set_state(state);
        assert!(matches!(store::gate(&c, HUB, SPACE, PROJECT, TASK).unwrap(), store::Gate::Fire { .. }), "{state}");
    }
}

#[test]
fn ack_vale_mesmo_antes_da_marca_sent() {
    let c = fixture();
    // Estado REAL gerado pelo fluxo: session_id já gravada (sessão resolvida
    // antes do send) e status 'sending' — a resposta do bot chega antes da
    // marca 'sent'. A prova causal é a ENTRY (clientNonce+session+turn),
    // não a coluna de status da delivery.
    c.execute(
        "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
         VALUES(?1,?2,?3,?4,'c1',?5,'sess-1','sending',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    let nonce = format!("mx142-task-aviso-c1-{BOT}");
    c.execute(
        "INSERT INTO entries(bot_id,session,turn,kind,data) VALUES(?1,'sess-1',7,'user',?2)",
        params![BOT, serde_json::json!({"clientNonce": nonce}).to_string()],
    )
    .unwrap();
    assert!(store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c1", BOT, "sess-1", 7).unwrap());
    let status: String =
        c.query_row("SELECT status FROM donna_task_deliveries WHERE comment_id='c1'", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "answered");
    // 'queued' sem prova continua intocado.
    c.execute(
        "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
         VALUES(?1,?2,?3,?4,'c2',?5,NULL,'queued',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    assert!(!store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c2", BOT, "sess-1", 7).unwrap());
    let status: String =
        c.query_row("SELECT status FROM donna_task_deliveries WHERE comment_id='c2'", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "queued");
}

#[test]
fn task_brief_resume_descricao_e_aceite_com_truncamento() {
    let task = serde_json::json!({
        "description": "Fazer X", "acceptance": "Y funciona", "extra": "ignorado"
    });
    let brief = acordar::task_brief(&task);
    assert!(brief.contains("Fazer X"));
    assert!(brief.contains("Critério de aceite: Y funciona"));
    let longo = serde_json::json!({"description": "d".repeat(5000)});
    assert_eq!(acordar::task_brief(&longo).chars().count(), 1200);
}

#[test]
fn notify_summary_agrega_por_destinatario_sem_mentir() {
    use avisos::notify_summary;
    let row = |b: &str, s: &str, r: Option<&str>| (b.to_owned(), s.to_owned(), r.map(str::to_owned));
    // Vazio -> None (sem entregas registradas).
    assert!(notify_summary(&[]).is_none());
    // Todos responderam -> responded.
    let v = notify_summary(&[row(BOT, "answered", None), row("b2", "answered", None)]).unwrap();
    assert_eq!(v["state"], "responded");
    assert_eq!(v["recipients"].as_array().unwrap().len(), 2);
    // Ninguém recebeu -> not_notified + delivery_failed.
    let v = notify_summary(&[row(BOT, "failed", Some("mention_not_in_text"))]).unwrap();
    assert_eq!(v["state"], "not_notified");
    assert_eq!(v["reason"], "delivery_failed");
    assert_eq!(v["recipients"][0]["reason"], "mention_not_in_text");
    // Mistura answered+failed -> partial, NUNCA not_notified.
    let v = notify_summary(&[row(BOT, "answered", None), row("b2", "failed", Some("bot_unavailable"))]).unwrap();
    assert_eq!(v["state"], "partial");
    assert!(v["reason"].is_null());
    assert_eq!(v["recipients"][1]["state"], "failed");
    assert_eq!(v["recipients"][1]["reason"], "bot_unavailable");
    // Pendência vence: sent+failed -> notifying (ainda pode responder).
    let v = notify_summary(&[row(BOT, "sent", None), row("b2", "failed", None)]).unwrap();
    assert_eq!(v["state"], "notifying");
    // uncertain -> uncertain.
    let v = notify_summary(&[row(BOT, "uncertain", None)]).unwrap();
    assert_eq!(v["state"], "uncertain");
}

#[test]
fn claim_due_tarefa_removida_vira_refused_sem_envenenar_lote() {
    let c = fixture();
    let ghost: &str = "00000000-0000-4000-8000-0000000000ff";
    // Duas linhas vencidas: a tarefa real e uma cujo id não existe mais no
    // JSON do projeto (exclusão/recriação — TEC-18).
    for (task, nonce) in [(TASK, "n-live"), (ghost, "n-ghost")] {
        c.execute(
            "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,1,1,'scheduled',?6,0,1,1)",
            params![HUB, SPACE, PROJECT, task, BOT, nonce],
        )
        .unwrap();
    }
    let fires = acordar::claim_due(&c, HUB, 1000).unwrap();
    // O fogo válido dispara; o fantasma vira refused terminal.
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].task_id, TASK);
    let states: Vec<(String, String)> = {
        let mut st = c.prepare("SELECT task_id,state FROM donna_task_agenda ORDER BY task_id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(states, vec![(TASK.to_string(), "starting".to_string()), (ghost.to_string(), "refused".to_string())]);
    // E o refused é terminal: próximo tick não tenta de novo.
    assert!(acordar::claim_due(&c, HUB, 2000).unwrap().is_empty());
}

#[test]
fn sessao_excluida_apaga_prova_e_ack_falha_honesto() {
    let c = fixture();
    // excluir.rs de sessões apaga entries da sessão — a prova causal some
    // junto. A delivery permanece 'sent' (não mente 'answered').
    c.execute(
        "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
         VALUES(?1,?2,?3,?4,'c1',?5,'sess-1','sending',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    let nonce = format!("mx142-task-aviso-c1-{BOT}");
    c.execute(
        "INSERT INTO entries(bot_id,session,turn,kind,data) VALUES(?1,'sess-1',7,'user',?2)",
        params![BOT, serde_json::json!({"clientNonce": nonce}).to_string()],
    )
    .unwrap();
    // Marca 'sent' depois do aceite (fluxo real).
    c.execute("UPDATE donna_task_deliveries SET status='sent'", []).unwrap();
    // Sessão excluída -> entries removidas (mesma operação do excluir.rs).
    c.execute("DELETE FROM entries WHERE session='sess-1'", []).unwrap();
    assert!(!store::mark_answered_in_session(&c, HUB, SPACE, PROJECT, TASK, "c1", BOT, "sess-1", 7).unwrap());
    let status: String =
        c.query_row("SELECT status FROM donna_task_deliveries WHERE comment_id='c1'", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "sent");
}

#[test]
fn revalidate_on_recusa_apos_exclusao_ou_troca_de_responsavel() {
    let c = fixture();
    let fire = |bot: &str| acordar::Fire {
        space_id: SPACE.to_owned(),
        project_id: PROJECT.to_owned(),
        task_id: TASK.to_owned(),
        bot_id: bot.to_owned(),
        title: "T".to_owned(),
        project_name: "P".to_owned(),
        nonce: "n".to_owned(),
        attempts: 0,
        session_id: None,
        origin: String::new(),
        brief: String::new(),
    };
    // Válido passa.
    assert!(acordar::revalidate_on(&c, HUB, &fire(BOT)).is_ok());
    // Troca de responsável na janela -> recusado.
    assert!(acordar::revalidate_on(&c, HUB, &fire("outro-bot")).is_err());
    // Tarefa removida do JSON -> invalid_field.
    c.execute("UPDATE donna_projects SET tasks='[]' WHERE id=?1", [PROJECT]).unwrap();
    let err = acordar::revalidate_on(&c, HUB, &fire(BOT)).unwrap_err().to_string();
    assert!(err.contains("invalid_field"), "{err}");
    // done/cancelled também recusam na janela.
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":"done"}]).to_string(), PROJECT],
    )
    .unwrap();
    assert!(acordar::revalidate_on(&c, HUB, &fire(BOT)).is_err());
}

#[test]
fn trigger_guard_valida_alvo_no_ponto_de_aceite() {
    let c = fixture();
    // Vínculo tarefa->sessão->bot que o trigger resolve.
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-g',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    // Entregas de aviso 'sending' na sessão vinculada (fluxo real: session_id
    // gravada antes do send). O nonce reconstruído precisa casar.
    for comment in ["c1", "c2", "c3"] {
        c.execute(
            "INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,session_id,status,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,'sess-g','sending',1)",
            params![HUB, SPACE, PROJECT, TASK, comment, BOT],
        )
        .unwrap();
    }
    // Agenda tem PK por tarefa: um nonce por vez — helper reseta a linha.
    let agenda = |nonce: &str| {
        c.execute("DELETE FROM donna_task_agenda", []).unwrap();
        c.execute(
            "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,1,1,'starting',?6,0,1,1)",
            params![HUB, SPACE, PROJECT, TASK, BOT, nonce],
        )
        .unwrap();
    };
    let insert = |nonce: &str| {
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-g','user',?2)",
            params![BOT, serde_json::json!({"clientNonce": nonce}).to_string()],
        )
    };
    // Claim corrente n1 + tarefa ativa + BOT responsável -> start e aviso passam.
    agenda("n1");
    insert("mx142-task-start-n1").unwrap();
    insert(&format!("mx142-task-aviso-c1-{BOT}")).unwrap();
    // Nonce estranho -> trigger nem dispara.
    insert("outro-nonce").unwrap();
    // Tarefa vira done -> START aborta (vedado iniciar); AVISO também aborta
    // aqui só porque a entrega 'sending' exige... não: aviso não checa estado.
    // O que aborta o aviso é task ausente — coberto abaixo. Para done, o
    // start é o único vedado; aviso passa (comentário @ em concluída vale).
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":"done"}]).to_string(), PROJECT],
    )
    .unwrap();
    assert!(insert("mx142-task-start-n2").is_err());
    // Aviso @ em tarefa done -> ACEITO (só o início é vedado em done/cancelled).
    insert(&format!("mx142-task-aviso-c2-{BOT}")).unwrap();
    // Recriada como backlog -> volta a aceitar (recreate não deixa resíduo).
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":"backlog"}]).to_string(),
            PROJECT
        ],
    )
    .unwrap();
    agenda("n3");
    insert("mx142-task-start-n3").unwrap();
    // Start exige responsável: entry de outro bot com nonce válido aborta.
    agenda("n4");
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES('bot-x','sess-g','user',?1)",
        [serde_json::json!({"clientNonce": "mx142-task-start-n4"}).to_string()],
    )
    .unwrap_err();
    // Sessão sem vínculo task_sessions -> aborta (nonce nosso sem prova).
    agenda("n5");
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-livre','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n5"}).to_string()],
    )
    .unwrap_err();
    // Sub-prefixo desconhecido/forjado dentro do namespace -> aborta sempre.
    assert!(insert("mx142-task-qualquer-coisa").is_err());
    // Bot revogado (deleted=1) -> start e aviso abortam no aceite.
    c.execute("UPDATE bots SET deleted=1 WHERE id=?1", [BOT]).unwrap();
    agenda("n6");
    assert!(insert("mx142-task-start-n6").is_err());
    assert!(insert(&format!("mx142-task-aviso-c3-{BOT}")).is_err());
    c.execute("UPDATE bots SET deleted=0 WHERE id=?1", [BOT]).unwrap();
    // FENCE DE CLAIM (TEC delta): agenda reagendada troca o nonce — o
    // clientNonce do claim VELHO aborta no aceite, o novo passa.
    agenda("n7");
    c.execute("UPDATE donna_task_agenda SET nonce='n8',state='scheduled'", []).unwrap();
    assert!(insert("mx142-task-start-n7").is_err());
    c.execute("UPDATE donna_task_agenda SET state='starting'", []).unwrap();
    insert("mx142-task-start-n8").unwrap();
    // Ordenação serializada: entry aceita ANTES da exclusão permanece.
    c.execute("UPDATE donna_projects SET tasks='[]' WHERE id=?1", [PROJECT]).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.clientNonce')='mx142-task-start-n8'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    // E novo insert depois da exclusão aborta — janela fechada no aceite.
    assert!(insert("mx142-task-start-n9").is_err());
    // Aviso também morre com a tarefa removida (prova some junto).
    assert!(insert(&format!("mx142-task-aviso-c1-{BOT}")).is_err());
}

#[test]
fn data_nao_json_nao_quebra_guards_nem_dispara() {
    let c = fixture();
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-g',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    // entries.data é TEXT livre: linhas legadas/armazenamento podem não ser
    // JSON. Sem json_valid no WHEN, json_extract abortaria TODO insert com
    // 'malformed JSON' (regressão: armazenamento::two_bots_utf8_*).
    for kind in ["user", "notice"] {
        c.execute(
            "INSERT INTO entries(bot_id,session,turn,kind,data) VALUES(?1,'sess-g',1,?2,'texto puro sem json')",
            params![BOT, kind],
        )
        .unwrap();
    }
    // E a linha legada não-JSON não pode derrubar o subselect de dedupe do
    // start_guard (json_extract(e2.data) sobre ela levantaria o mesmo erro).
    c.execute(
        "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,1,1,'starting','n-json',0,1,1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-g','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n-json"}).to_string()],
    )
    .unwrap();
}

#[test]
fn deleg_guard_valida_kickoff_d220_na_admissao() {
    let c = fixture();
    // Agenda 'starting' do kickoff delegado: nonce mx142-bot-<req>.
    c.execute(
        "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,1,1,'starting','mx142-bot-req1',0,1,1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    let notice = |nonce: &str, coords: bool| {
        let dispatch = if coords {
            serde_json::json!({"nonce": nonce, "hubId": HUB, "spaceId": SPACE,
                "projectId": PROJECT, "taskId": TASK})
        } else {
            serde_json::json!({"nonce": nonce})
        };
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES(?2,'main','notice',?1)",
            params![
                serde_json::json!({
                    "delegationId": "dlg-1",
                    "botMessage": {"sourceBotId": "coord", "targetBotId": BOT,
                        "text": format!("Início de tarefa delegado.\n\nReferência de despacho: {nonce}"),
                        "taskDispatch": dispatch}
                })
                .to_string(),
                BOT
            ],
        )
    };
    // Cópia do REMETENTE do par D220: taskDispatch presente mas a entry
    // pousa na lane do source — o guard não dispara e o insert passa
    // (era o bug: abortava o par inteiro na primeira inserção).
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES('coord','main','notice',?1)",
        params![
            serde_json::json!({
                "delegationId": "dlg-0",
                "botMessage": {"sourceBotId": "coord", "targetBotId": BOT,
                    "taskDispatch": {"nonce": "mx142-bot-req1", "hubId": HUB,
                        "spaceId": SPACE, "projectId": PROJECT, "taskId": TASK}}
            })
            .to_string()
        ],
    )
    .unwrap();
    // Alvo válido -> notice aceito (a admissão D220 prossegue).
    notice("mx142-bot-req1", true).unwrap();
    // taskDispatch sem coordenadas de referência -> aborta (incompleto).
    assert!(notice("mx142-bot-req1", false).is_err());
    // Nonce de despacho desconhecido/forjado -> aborta (sem prova na agenda).
    assert!(notice("mx142-bot-inexistente", true).is_err());
    // Agenda saiu de 'starting' (já concluída) -> aborta replay tardio.
    c.execute("UPDATE donna_task_agenda SET state='started'", []).unwrap();
    assert!(notice("mx142-bot-req1", true).is_err());
    c.execute("UPDATE donna_task_agenda SET state='starting'", []).unwrap();
    // Reagendamento troca o nonce -> despacho do claim VELHO aborta.
    c.execute("UPDATE donna_task_agenda SET nonce='mx142-bot-novo'", []).unwrap();
    assert!(notice("mx142-bot-req1", true).is_err());
    notice("mx142-bot-novo", true).unwrap();
    c.execute("UPDATE donna_task_agenda SET nonce='mx142-bot-req1'", []).unwrap();
    // Coordenada de referência divergente -> aborta.
    let bad = serde_json::json!({"nonce": "mx142-bot-req1", "hubId": HUB, "spaceId": SPACE,
        "projectId": "outro-projeto", "taskId": TASK});
    assert!(
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES(?2,'main','notice',?1)",
            params![serde_json::json!({"delegationId":"dlg-2","botMessage":{"taskDispatch":bad}}).to_string(), BOT],
        )
        .is_err()
    );
    // Reassign na janela -> aborta.
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":"outro","state":"backlog"}]).to_string(),
            PROJECT
        ],
    )
    .unwrap();
    assert!(notice("mx142-bot-req1", true).is_err());
    // done -> aborta.
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":"done"}]).to_string(), PROJECT],
    )
    .unwrap();
    assert!(notice("mx142-bot-req1", true).is_err());
    // R2-22 — INTERCEPTAÇÃO: mensagem comum que CITA o marcador textual,
    // sem taskDispatch, NÃO dispara o guard e é aceita normalmente.
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":"backlog"}]).to_string(),
            PROJECT
        ],
    )
    .unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?2,'main','notice',?1)",
        params![
            serde_json::json!({
                "delegationId": "dlg-3",
                "botMessage": {"sourceBotId": "coord", "targetBotId": BOT,
                    "text": "Você conhece a string 'Referência de despacho: mx142-bot-req1'?"}
            })
            .to_string(),
            BOT
        ],
    )
    .unwrap();
    // FORJA 1: targetBotId divergente do responsável da agenda -> aborta
    // (a.bot_id=BOT != 'bot-alvo'; não há bypass silencioso).
    assert!(
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES('bot-alvo','main','notice',?1)",
            params![
                serde_json::json!({
                    "delegationId": "dlg-4",
                    "botMessage": {"sourceBotId": "coord", "targetBotId": "bot-alvo",
                        "taskDispatch": {"nonce": "mx142-bot-req1", "hubId": HUB,
                            "spaceId": SPACE, "projectId": PROJECT, "taskId": TASK}}
                })
                .to_string()
            ],
        )
        .is_err()
    );
    // FORJA 2: bot_id da entry fora de {sourceBotId,targetBotId} -> aborta.
    assert!(
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES('bot-x','main','notice',?1)",
            params![
                serde_json::json!({
                    "delegationId": "dlg-5",
                    "botMessage": {"sourceBotId": "coord", "targetBotId": BOT,
                        "taskDispatch": {"nonce": "mx142-bot-req1", "hubId": HUB,
                            "spaceId": SPACE, "projectId": PROJECT, "taskId": TASK}}
                })
                .to_string()
            ],
        )
        .is_err()
    );
    // PAR REAL INTEIRO: os dois notices do par passam na sequência —
    // remetente ('coord') e destinatário (BOT) — mesma tx lógica do
    // insert_team_notices; nenhum aborta.
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES('coord','main','notice',?1)",
        params![
            serde_json::json!({
                "delegationId": "dlg-6",
                "botMessage": {"sourceBotId": "coord", "targetBotId": BOT,
                    "taskDispatch": {"nonce": "mx142-bot-req1", "hubId": HUB,
                        "spaceId": SPACE, "projectId": PROJECT, "taskId": TASK}}
            })
            .to_string()
        ],
    )
    .unwrap();
    notice("mx142-bot-req1", true).unwrap();
}

#[test]
fn mention_authorized_exige_marcador_e_lista() {
    // Marcador 2 + ID na lista -> autorizado (rename-safe: sem @Nome).
    let ok = serde_json::json!({"mentionCheck": 2, "mentions": ["b1", BOT], "text": "sem arroba"});
    assert!(avisos::mention_authorized(&ok, BOT));
    // Legado SEM marcador + mentions -> fail-closed (lista não verificada).
    let legacy = serde_json::json!({"mentions": [BOT], "text": "@Berga oi"});
    assert!(!avisos::mention_authorized(&legacy, BOT));
    // Marcador errado/ausente variantes.
    assert!(!avisos::mention_authorized(&serde_json::json!({"mentionCheck": 1, "mentions": [BOT]}), BOT));
    assert!(!avisos::mention_authorized(&serde_json::json!({"mentionCheck": "2", "mentions": [BOT]}), BOT));
    // Marcador 2 mas ID fora da lista -> rejeita.
    assert!(!avisos::mention_authorized(&serde_json::json!({"mentionCheck": 2, "mentions": ["outro"]}), BOT));
    // Marcador 2 sem lista -> rejeita.
    assert!(!avisos::mention_authorized(&serde_json::json!({"mentionCheck": 2}), BOT));
}

#[test]
fn reagendamento_apos_troca_responsavel_reconcilia_alvo_sem_reusar_sessao() {
    let c = fixture();
    let bot2 = "00000000-0000-4000-8000-0000000000f1";
    c.execute(
        "INSERT INTO bots(id,config) VALUES(?1,?2)",
        params![bot2, r#"{"name":"Nova","spaceId":"00000000-0000-4000-8000-000000000001"}"#],
    )
    .unwrap();
    // Agenda inicial do BOT.
    store::call(
        &c,
        HUB,
        "donnaTaskSchedule",
        serde_json::json!({
            "hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK,
            "clientRequestId":REQUEST,"startAt":1,
        }),
    )
    .unwrap();
    // Claim antigo em voo: 'starting' com nonce velho + sessão do BOT.
    let old_nonce: String = c.query_row("SELECT nonce FROM donna_task_agenda", [], |r| r.get(0)).unwrap();
    c.execute("UPDATE donna_task_agenda SET state='starting',session_id='sess-old'", []).unwrap();
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-old',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    // Dono troca o responsável e REAGENDAR explicitamente (novo gesto).
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([{"id":TASK,"title":"Subtarefa","assigneeBotId":bot2,"state":"backlog"}]).to_string(),
            PROJECT
        ],
    )
    .unwrap();
    let req2 = "00000000-0000-4000-8000-0000000000aa";
    store::call(
        &c,
        HUB,
        "donnaTaskSchedule",
        serde_json::json!({
            "hubId":HUB,"spaceId":SPACE,"projectId":PROJECT,"taskId":TASK,
            "clientRequestId":req2,"startAt":2,
        }),
    )
    .unwrap();
    // Agenda nova: bot_id=bot2, session_id limpo, nonce novo.
    let (nb, ns, nn): (String, Option<String>, String) = c
        .query_row("SELECT bot_id,session_id,nonce FROM donna_task_agenda", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(nb, bot2);
    assert_eq!(ns, None);
    assert_ne!(nn, old_nonce);
    // A sessão do bot anterior permanece em donna_task_sessions (auditoria).
    let kept: i64 = c
        .query_row("SELECT COUNT(*) FROM donna_task_sessions WHERE bot_id=?1 AND session_id='sess-old'", [BOT], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, 1);
    // Replay do claim VELHO: poke com nonce antigo aborta no guard.
    assert!(
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-old','user',?2)",
            params![BOT, serde_json::json!({"clientNonce": format!("mx142-task-start-{old_nonce}")}).to_string()],
        )
        .is_err()
    );
    // Tick: claim_due reconcilia bot_id com o gate e limpa sessão divergente.
    let fires = acordar::claim_due(&c, HUB, 100).unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].bot_id, bot2);
    assert_eq!(fires[0].session_id, None);
    let (ab, asess): (String, Option<String>) =
        c.query_row("SELECT bot_id,session_id FROM donna_task_agenda", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(ab, bot2);
    assert_eq!(asess, None);
    // Vínculo de sessão do NOVO bot + poke do novo claim -> aceito.
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-new',1)",
        params![HUB, SPACE, PROJECT, TASK, bot2],
    )
    .unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-new','user',?2)",
        params![bot2, serde_json::json!({"clientNonce": format!("mx142-task-start-{nn}")}).to_string()],
    )
    .unwrap();
}

#[test]
fn restore_starting_prova_por_nonce_sobrevive_a_reatribuicao() {
    let c = fixture();
    // t10: poke dono aceito sob 'bot-a' ANTES da reatribuição para BOT.
    // t9: kickoff D220 admitido sob 'bot-a' (notice do remetente com
    // taskDispatch) antes da reatribuição.
    agenda_row(&c, "t9", "mx142-bot-r9");
    agenda_row(&c, "t10", "n-old");
    c.execute("UPDATE donna_task_agenda SET bot_id='bot-a' WHERE task_id IN ('t9','t10')", []).unwrap();
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([
                {"id":TASK,"title":"X","assigneeBotId":BOT,"state":"backlog"},
                {"id":"t9","title":"Y","assigneeBotId":"bot-a","state":"backlog"},
                {"id":"t10","title":"Z","assigneeBotId":"bot-a","state":"backlog"},
            ])
            .to_string(),
            PROJECT
        ],
    )
    .unwrap();
    c.execute(
        "INSERT INTO bots(id,config) VALUES('bot-a','{\"spaceId\":\"00000000-0000-4000-8000-000000000001\"}')",
        [],
    )
    .unwrap();
    // Vínculo de sessão do bot-a para o guard do poke dono.
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,'t10','bot-a','sess-a',1)",
        params![HUB, SPACE, PROJECT],
    )
    .unwrap();
    // Poke dono ACEITO sob bot-a (entry real atravessa o start_guard).
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES('bot-a','sess-a','user',?1)",
        params![serde_json::json!({"clientNonce": "mx142-task-start-n-old"}).to_string()],
    )
    .unwrap();
    // Par D220 admitido: cópia do remetente com taskDispatch (prova do commit).
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES('coord','main','notice',?1)",
        params![
            serde_json::json!({
                "delegationId": "dlg-r9",
                "botMessage": {"sourceBotId": "coord", "targetBotId": "bot-a",
                    "taskDispatch": {"nonce": "mx142-bot-r9", "hubId": HUB, "spaceId": SPACE,
                        "projectId": PROJECT, "taskId": "t9"}}
            })
            .to_string()
        ],
    )
    .unwrap();
    // REATRIBUIÇÃO: agenda e tarefa passam a BOT (claim reconciliado).
    c.execute("UPDATE donna_task_agenda SET bot_id=?1", [BOT]).unwrap();
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([
                {"id":TASK,"title":"X","assigneeBotId":BOT,"state":"backlog"},
                {"id":"t9","title":"Y","assigneeBotId":BOT,"state":"backlog"},
                {"id":"t10","title":"Z","assigneeBotId":BOT,"state":"backlog"},
            ])
            .to_string(),
            PROJECT
        ],
    )
    .unwrap();
    acordar::restore_starting(&c, HUB).unwrap();
    // Prova POR NONCE: a evidência sob bot-a continua valendo — 'started'
    // (poke entregue) e 'uncertain' (D220 admitido), nunca retry/failed
    // que duplicaria o efeito.
    let states: Vec<String> = {
        let mut st = c
            .prepare("SELECT task_id||':'||state||':'||bot_id FROM donna_task_agenda WHERE task_id IN ('t9','t10') ORDER BY task_id")
            .unwrap();
        st.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(states, vec!["t10:started:".to_string() + BOT, "t9:uncertain:".to_string() + BOT]);
}

#[test]
fn start_guard_dedupe_atomica_impede_segundo_poke_cross_bot() {
    let c = fixture();
    let bot_a = "00000000-0000-4000-8000-0000000000a1";
    // BOT já existe na fixture — só o bot antigo precisa ser criado.
    c.execute(
        "INSERT INTO bots(id,config) VALUES(?1,?2)",
        params![bot_a, format!(r#"{{"name":"Antiga","spaceId":"{SPACE}"}}"#)],
    )
    .unwrap();
    // Claim 'n-dup' começando sob bot_a (responsável na época do envio).
    c.execute(
        "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,1,1,'starting','n-dup',0,1,1)",
        params![HUB, SPACE, PROJECT, TASK, bot_a],
    )
    .unwrap();
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":bot_a,"state":"backlog"}]).to_string(),
            PROJECT
        ],
    )
    .unwrap();
    for (bot, sess) in [(bot_a, "sess-a"), (BOT, "sess-b")] {
        c.execute(
            "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,1)",
            params![HUB, SPACE, PROJECT, TASK, bot, sess],
        )
        .unwrap();
    }
    let poke = |bot: &str, sess: &str| {
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,?2,'user',?3)",
            params![bot, sess, serde_json::json!({"clientNonce": "mx142-task-start-n-dup"}).to_string()],
        )
    };
    // CASO 1 — A commita primeiro: poke de bot_a aceito.
    poke(bot_a, "sess-a").unwrap();
    // Reatribuição + erro pós-commit: agenda e tarefa passam a BOT.
    c.execute("UPDATE donna_task_agenda SET bot_id=?1 WHERE nonce='n-dup'", [BOT]).unwrap();
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![
            serde_json::json!([{"id":TASK,"title":"X","assigneeBotId":BOT,"state":"backlog"}]).to_string(),
            PROJECT
        ],
    )
    .unwrap();
    // O retry de BOT com o MESMO nonce aborta no guard — primeiro a
    // commitar vence; sem consulta prévia nem janela check->send.
    assert!(poke(BOT, "sess-b").is_err());
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.clientNonce')='mx142-task-start-n-dup'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "nonce duplicado cross-bot não pode gerar segunda entry");
    // P2-30 — MESMO bot: claim novo sob BOT commita; uma SEGUNDA entry com
    // o mesmo clientNonce (dois sends concorrentes que atravessaram as
    // consultas) aborta no guard — dedupe do nonce é por identidade da
    // tentativa, não por bot. O replay legítimo nunca chega aqui porque o
    // send deduplica antes do INSERT.
    c.execute("UPDATE donna_task_agenda SET nonce='n-dup3' WHERE task_id=?1", [TASK]).unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-b','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n-dup3"}).to_string()],
    )
    .unwrap();
    assert!(
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-b','user',?2)",
            params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n-dup3"}).to_string()],
        )
        .is_err(),
        "segundo insert mesmo bot/mesmo nonce deve abortar (P2-30)"
    );
}

#[test]
fn migrate_substitui_trigger_legado_de_banco_existente() {
    // P2-29/P3-32: banco migrado por versão anterior tem o trigger VELHO
    // com IF NOT EXISTS — sem DROP+CREATE a definição antiga sobreviveria
    // e a dedupe nova nunca existiria. Schema REAL das dependências para
    // que o comportamento do guard seja o do banco de produção.
    let c = rusqlite::Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE donna_task_comments(hub_id TEXT, space_id TEXT, project_id TEXT, task_id TEXT, comment_id TEXT, entry TEXT);
         CREATE TABLE entries(id TEXT, bot_id TEXT, session TEXT, turn INTEGER, kind TEXT, data TEXT);
         CREATE TABLE kv(k TEXT PRIMARY KEY, v TEXT);
         CREATE TABLE donna_projects(hub_id TEXT, space_id TEXT, id TEXT, project TEXT, tasks TEXT);
         CREATE TABLE bots(id TEXT PRIMARY KEY, config TEXT, deleted INTEGER DEFAULT 0);
         -- Versão velha do trigger: no-op, ocupa o nome.
         CREATE TRIGGER donna_task_start_guard BEFORE INSERT ON entries BEGIN SELECT 1; END;",
    )
    .unwrap();
    migrate(&c).unwrap();
    // A definição NOVA está presente (não apenas o nome): a cláusula de
    // dedupe por nonce é parte do guard atual.
    let ddl: String = c
        .query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='donna_task_start_guard'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(ddl.contains("donna_task_agenda"), "guard novo não instalado: {ddl}");
    assert!(ddl.contains("clientNonce"), "dedupe ausente no guard: {ddl}");
    // Comportamento: poke com nonce desconhecido aborta — o no-op antigo
    // deixaria passar e o migrador antigo nem criaria o trigger.
    assert!(
        c.execute(
            "INSERT INTO entries(bot_id,session,kind,data) VALUES('b','s','user',?1)",
            params![serde_json::json!({"clientNonce": "mx142-task-start-inexistente"}).to_string()],
        )
        .is_err(),
        "trigger legado no-op não pode sobreviver à migração"
    );
}

#[test]
fn revalidacao_apos_suspensao_reconcilia_com_prova_de_nonce() {
    // TEC P2-31 interleaving: fire de B suspenso -> tick recupera claim
    // para A -> A aceita N -> B retoma e revalidate_before_effect falha
    // (responsável mudou). A reconciliação por nonce TEM que vencer a
    // recusa — sem ela o erro viraria 'failed'/'scheduled' durável sobre
    // prova existente.
    let c = fixture();
    // TASK já existe no JSON da fixture com assignee BOT — 't-rec' seria
    // recusado pelo guard (tarefa inexistente) antes das asserções.
    agenda_row(&c, TASK, "n-rec");
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-r',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    // A (BOT, responsável atual) aceita N — entry real via guard.
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-r','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n-rec"}).to_string()],
    )
    .unwrap();
    // Fire do bot antigo suspenso retoma: gate dá BOT, fire diz 'bot-b'.
    let fire_b = acordar::Fire {
        space_id: SPACE.into(),
        project_id: PROJECT.into(),
        task_id: TASK.into(),
        bot_id: "bot-b".into(),
        title: "T".into(),
        project_name: "P".into(),
        nonce: "n-rec".into(),
        attempts: 0,
        session_id: None,
        origin: String::new(),
        brief: String::new(),
    };
    // A revalidação FALHA (responsável mudou) — é exatamente o erro que
    // o wiring agora reconcilia antes de propagar.
    assert!(acordar::revalidate_on(&c, HUB, &fire_b).is_err());
    // Mas a prova de aceite por nonce existe -> o wiring retorna
    // Ok(true)/'started', nunca 'failed' sobre prova.
    assert!(acordar::nonce_accepted(&c, "mx142-task-start-n-rec").unwrap());
    // E sem prova o erro propagaria intacto (não mascaramos):
    assert!(!acordar::nonce_accepted(&c, "mx142-task-start-inexistente").unwrap());
}

#[test]
fn reconcile_delivery_prova_vence_recusa_e_erro_sem_prova_intacto() {
    // TEC P2-34: exercita a função REAL que perform chama nos três
    // caminhos de erro — remover a reconciliação muda este resultado.
    let c = fixture();
    agenda_row(&c, TASK, "n-rec2");
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,'sess-r',1)",
        params![HUB, SPACE, PROJECT, TASK, BOT],
    )
    .unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES(?1,'sess-r','user',?2)",
        params![BOT, serde_json::json!({"clientNonce": "mx142-task-start-n-rec2"}).to_string()],
    )
    .unwrap();
    // Com prova: a recusa (ex.: 'responsável mudou') vira entrega.
    assert!(
        acordar::reconcile_delivery(
            &c,
            "mx142-task-start-n-rec2",
            anyhow::anyhow!("invalid_field: responsável da tarefa mudou.")
        )
        .unwrap()
    );
    // Sem prova: o MESMO erro propaga intacto — não mascaramos falha.
    let err = acordar::reconcile_delivery(
        &c,
        "mx142-task-start-sem-prova",
        anyhow::anyhow!("invalid_field: responsável da tarefa mudou."),
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "invalid_field: responsável da tarefa mudou.");
    // Replay: a decisão é idempotente — mesma prova, mesmo resultado.
    assert!(acordar::reconcile_delivery(&c, "mx142-task-start-n-rec2", anyhow::anyhow!("outra recusa")).unwrap());
}

/// Linha da agenda com proposta e estado escolhidos, para os testes do teto.
fn proposal_agenda_row(c: &rusqlite::Connection, task: &str, bot: &str, state: &str, proposal: Option<&str>) {
    c.execute(
        "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,nonce,attempts,proposal_id,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,1,1,?6,?7,0,?8,1,1)",
        params![HUB, SPACE, PROJECT, task, bot, state, format!("n-{task}"), proposal],
    )
    .unwrap();
}

/// Tarefas do Projeto reescritas por (id, bot, estado).
fn proposal_tasks(c: &rusqlite::Connection, rows: &[(&str, &str, &str)]) {
    let tasks: Vec<_> = rows
        .iter()
        .map(|(id, bot, state)| serde_json::json!({"id": id, "title": id, "assigneeBotId": bot, "state": state}))
        .collect();
    c.execute(
        "UPDATE donna_projects SET tasks=?1 WHERE id=?2",
        params![serde_json::to_string(&tasks).unwrap(), PROJECT],
    )
    .unwrap();
}

fn proposal_bots(c: &rusqlite::Connection, ids: &[&str]) {
    for id in ids {
        c.execute(
            "INSERT INTO bots(id,config) VALUES(?1,?2)",
            params![id, format!(r#"{{"name":"{id}","spaceId":"{SPACE}"}}"#)],
        )
        .unwrap();
    }
}

fn agenda_snapshot(c: &rusqlite::Connection) -> std::collections::BTreeMap<String, (String, i64)> {
    let mut st = c.prepare("SELECT task_id,state,attempts FROM donna_task_agenda").unwrap();
    let rows: Vec<(String, String, i64)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    rows.into_iter().map(|(task, state, attempts)| (task, (state, attempts))).collect()
}

#[test]
fn claim_respeita_teto_de_duas_ativas_no_space() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2", "b3"]);
    proposal_tasks(&c, &[("t1", "b1", "backlog"), ("t2", "b2", "backlog"), ("t3", "b3", "backlog")]);
    for (task, bot) in [("t1", "b1"), ("t2", "b2"), ("t3", "b3")] {
        proposal_agenda_row(&c, task, bot, "scheduled", Some("prop-a"));
    }
    // Três vencidas, três bots: só duas claimadas.
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 2);
    let snapshot = agenda_snapshot(&c);
    assert_eq!(snapshot["t1"].0, "starting");
    assert_eq!(snapshot["t2"].0, "starting");
    // A terceira fica scheduled e não gasta tentativa.
    assert_eq!(snapshot["t3"], ("scheduled".to_owned(), 0));
    // Próximo tick: as duas ativas seguem e a terceira continua de fora.
    let again = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert!(again.is_empty());
    assert_eq!(agenda_snapshot(&c)["t3"].0, "scheduled");
}

#[test]
fn claim_respeita_teto_de_uma_ativa_por_bot() {
    let c = fixture();
    proposal_bots(&c, &["b1"]);
    proposal_tasks(&c, &[("t1", "b1", "backlog"), ("t2", "b1", "backlog")]);
    proposal_agenda_row(&c, "t1", "b1", "scheduled", Some("prop-b"));
    proposal_agenda_row(&c, "t2", "b1", "scheduled", Some("prop-b"));
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 1);
    let snapshot = agenda_snapshot(&c);
    let states: std::collections::BTreeSet<String> = snapshot.values().map(|(state, _)| state.clone()).collect();
    assert!(states.contains("starting"));
    assert!(states.contains("scheduled"));
}

#[test]
fn orfa_de_proposta_reatribuida_espera_a_vaga_do_novo_bot() {
    let c = fixture();
    proposal_bots(&c, &["b1"]);
    // t1 era de um bot antigo (removido do exemplo), foi reatribuída a b1 e
    // ficou órfã em `starting`; b1 já tem t2 ativa de proposta.
    proposal_tasks(&c, &[("t1", "b1", "backlog"), ("t2", "b1", "backlog")]);
    proposal_agenda_row(&c, "t1", "b1-old", "starting", Some("prop-e"));
    proposal_agenda_row(&c, "t2", "b1", "started", Some("prop-e"));
    // A recuperação NÃO transfere t1 para b1: ele já ocupa a vaga dele.
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert!(fires.is_empty());
    let snapshot = agenda_snapshot(&c);
    assert_eq!(snapshot["t1"], ("starting".to_owned(), 0));
    // A vaga de b1 libera (t2 vai para review) e a transferência acontece.
    proposal_tasks(&c, &[("t1", "b1", "backlog"), ("t2", "b1", "review")]);
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].task_id, "t1");
    assert_eq!(fires[0].bot_id, "b1");
}

#[test]
fn orfas_trocadas_sem_entrega_nao_bloqueiam_uma_a_outro() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2"]);
    // t1 era de b2 e foi reatribuída a b1; t2 era de b1 e foi a b2. Sem
    // entrega provada, as reservas velhas não podem travar a recuperação.
    proposal_tasks(&c, &[("t1", "b1", "backlog"), ("t2", "b2", "backlog")]);
    proposal_agenda_row(&c, "t1", "b2", "starting", Some("prop-f"));
    proposal_agenda_row(&c, "t2", "b1", "starting", Some("prop-f"));
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 2);
    let snapshot = agenda_snapshot(&c);
    assert_eq!(snapshot["t1"].0, "starting");
    assert_eq!(snapshot["t2"].0, "starting");
}

#[test]
fn orfa_de_proposta_com_tarefa_em_review_e_recusada_sem_consumir_vaga() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2", "b3"]);
    // t1 em review com linha de proposta órfã; o teto já está cheio com
    // entregas reais. A recuperação não pode furar o teto: a linha órfã
    // é recusada (o início automático não faz mais sentido) e libera a vaga.
    proposal_tasks(&c, &[("t1", "b1", "review"), ("t2", "b2", "in_progress"), ("t3", "b3", "in_progress")]);
    proposal_agenda_row(&c, "t1", "b1", "starting", Some("prop-g"));
    proposal_agenda_row(&c, "t2", "b2", "started", Some("prop-g"));
    proposal_agenda_row(&c, "t3", "b3", "started", Some("prop-g"));
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert!(fires.is_empty());
    assert_eq!(agenda_snapshot(&c)["t1"], ("refused".to_owned(), 0));
}

#[test]
fn orfa_com_entrega_provada_continua_reservando_a_vaga_do_bot() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2"]);
    // t1 órfã em `starting`, reatribuída a b1; b1 já entregou t2 (poke aceito).
    // A entrega provada segue reservando a vaga de b1.
    proposal_tasks(&c, &[("t1", "b1", "backlog"), ("t2", "b1", "in_progress")]);
    proposal_agenda_row(&c, "t1", "b2", "starting", Some("prop-h"));
    proposal_agenda_row(&c, "t2", "b1", "starting", Some("prop-h"));
    // Vínculo de sessão exigido pelo guard de início; depois, a prova de
    // entrega do poke de t2.
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,'t2','b1','sess-b1',1)",
        params![HUB, SPACE, PROJECT],
    )
    .unwrap();
    c.execute(
        "INSERT INTO entries(bot_id,session,kind,data) VALUES('b1','sess-b1','user',?1)",
        params![serde_json::json!({"clientNonce": "mx142-task-start-n-t2"}).to_string()],
    )
    .unwrap();
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    // t1 (reatribuída a b1) NÃO é transferida: b1 tem entrega provada em t2.
    // t2 é recuperada normalmente (o dedupe por nonce na hora do disparo não
    // repete o poke já entregue).
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].task_id, "t2");
    assert_eq!(fires[0].bot_id, "b1");
    let t1_bot: String =
        c.query_row("SELECT bot_id FROM donna_task_agenda WHERE task_id='t1'", [], |r| r.get(0)).unwrap();
    assert_eq!(t1_bot, "b2");
    assert_eq!(agenda_snapshot(&c)["t1"], ("starting".to_owned(), 0));
}

#[test]
fn vaga_libera_quando_a_tarefa_va_para_review() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2", "b3"]);
    proposal_tasks(&c, &[("t1", "b1", "in_progress"), ("t2", "b2", "in_progress"), ("t3", "b3", "backlog")]);
    proposal_agenda_row(&c, "t1", "b1", "started", Some("prop-c"));
    proposal_agenda_row(&c, "t2", "b2", "started", Some("prop-c"));
    proposal_agenda_row(&c, "t3", "b3", "scheduled", Some("prop-c"));
    // Teto cheio: nada novo é claimado.
    assert!(acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap().is_empty());
    // t1 vai para review: a vaga dela libera e t3 entra.
    proposal_tasks(&c, &[("t1", "b1", "review"), ("t2", "b2", "in_progress"), ("t3", "b3", "backlog")]);
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].task_id, "t3");
}

#[test]
fn inicio_manual_do_dono_continua_fora_do_teto() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2", "b3"]);
    proposal_tasks(
        &c,
        &[("t1", "b1", "in_progress"), ("t2", "b2", "in_progress"), ("t3", "b3", "backlog"), ("t4", "b3", "backlog")],
    );
    proposal_agenda_row(&c, "t1", "b1", "started", Some("prop-d"));
    proposal_agenda_row(&c, "t2", "b2", "started", Some("prop-d"));
    // Agendamentos manuais (sem proposta) passam com o teto de proposta cheio.
    proposal_agenda_row(&c, "t3", "b3", "scheduled", None);
    proposal_agenda_row(&c, "t4", "b3", "scheduled", None);
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 2);
    let ids: std::collections::BTreeSet<&str> = fires.iter().map(|f| f.task_id.as_str()).collect();
    assert!(ids.contains("t3") && ids.contains("t4"));
}

#[test]
fn starting_orfa_de_proposta_recupera_sem_pedir_vaga_nova() {
    let c = fixture();
    proposal_bots(&c, &["b1", "b2"]);
    proposal_tasks(&c, &[("t1", "b1", "in_progress"), ("t2", "b2", "in_progress")]);
    // Duas 'starting' órfãs: já são as duas ativas do teto; a recuperação
    // prossegue porque não pede vaga nova.
    proposal_agenda_row(&c, "t1", "b1", "starting", Some("prop-e"));
    proposal_agenda_row(&c, "t2", "b2", "starting", Some("prop-e"));
    let fires = acordar::claim_due(&c, HUB, 1_000_000_000_000).unwrap();
    assert_eq!(fires.len(), 2);
}
