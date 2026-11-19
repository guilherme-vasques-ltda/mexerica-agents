//! D70: produtores/stream reais e fronteiras canônicas; sem iniciar motores ou harnesses.
use crate::{
    LockExt,
    api::{devices::Caller, dispatch, scoped_events_stream},
    fork::{activity_code::ActivityKind, spaces::PERSONAL},
    hub::{BotStatus, Hub, Runtime},
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Store},
};
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use std::{path::PathBuf, pin::Pin, sync::Arc, time::Duration};

const FIELDS: [&str; 6] = ["status", "activity", "activityCode", "startedAt", "resumeAt", "workingChat"];

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
    donna: String,
    observer: String,
    loja_bot: String,
    loja: String,
    personal_chat: String,
    loja_chat: String,
}

impl Fixture {
    async fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-runtime-audiencia-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "audiencia-fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let donna = uuid::Uuid::new_v4().to_string();
        let observer = uuid::Uuid::new_v4().to_string();
        let loja_bot = uuid::Uuid::new_v4().to_string();
        hub.store.kv_set("team.settings.v1", &json!({"primaryBotId":donna,"permission":"ask"}).to_string()).unwrap();
        let state = hub.store.spaces().unwrap();
        let spaces = dispatch(
            &hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({"hubId":hub.host_id,"expectedRevision":state.revision,"name":"Loja",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"LJ"}}}),
        )
        .await
        .unwrap();
        let loja = spaces["spaces"][1]["id"].as_str().unwrap().to_owned();
        for (id, space) in [(&donna, PERSONAL), (&observer, PERSONAL), (&loja_bot, loja.as_str())] {
            let cfg: BotConfig = serde_json::from_value(json!({
                "id":id,"name":"Bot da fixture","spaceId":space,"backend":"fixture","cwd":dir,
                "description":"CONFIG_AUTORIZADA","harnessMemory":true,
            }))
            .unwrap();
            hub.store.save_bot(&cfg).unwrap();
        }
        let mut chats = Vec::new();
        for space in [PERSONAL, loja.as_str()] {
            let response = dispatch(
                &hub,
                &Caller::Local,
                "donnaConversationCreate",
                json!({"hubId":hub.host_id,"spaceId":space,"botId":donna,"title":"Conversa da fixture"}),
            )
            .await
            .unwrap();
            chats.push(response["conversation"]["id"].as_str().unwrap().to_owned());
        }
        Self { hub, dir, donna, observer, loja_bot, loja, personal_chat: chats.remove(0), loja_chat: chats.remove(0) }
    }

    fn runtime(&self, id: &str, chat: Option<&str>, status: BotStatus) {
        self.hub.set_runtime(id, |rt| {
            *rt = Runtime {
                status,
                started_at: Some(1_730_000_000_123),
                resume_at: Some(1_730_000_000_999),
                lane: chat.map(Lane::main),
                ..Runtime::default()
            };
            rt.set_activity("ATIVIDADE_PRIVADA", Some(ActivityKind::Working));
        });
    }

    fn bot(&self, id: &str) -> Value {
        self.hub.bot_json(&self.hub.store.bot(id).unwrap().unwrap())
    }

    fn group(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":id,"name":"Grupo da fixture","kind":"group","members":[self.donna],
            "backend":"fixture","cwd":self.dir,
        }))
        .unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
        id
    }

    fn marker(&self) {
        self.hub.events.send(json!({"type":"resync","marker":"fim-runtime"})).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

async fn until_marker<S: Stream<Item = Value>>(mut stream: Pin<&mut S>) -> Vec<Value> {
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), stream.next()).await.unwrap().unwrap();
        if event["marker"] == "fim-runtime" {
            return events;
        }
        events.push(event);
    }
}

// Diagnóstico das fixtures: um tipo por evento, sem filtrar nem imprimir corpo ou identidade.
fn event_types(events: &[Value]) -> Vec<&'static str> {
    events
        .iter()
        .map(|event| match event["type"].as_str() {
            Some("bot") => "bot",
            Some("donnaControlTowerChanged") => "donnaControlTowerChanged",
            Some("screen") => "screen",
            Some("entry") => "entry",
            Some("conversation") => "conversation",
            Some("session") => "session",
            Some("resync") => "resync",
            Some(_) => "outro",
            None => "sem-tipo",
        })
        .collect()
}

// Consome só a notificação exata e na posição prevista; nenhum outro evento é descartado.
fn expect_tower_change(events: &mut Vec<Value>, index: usize, hub: &Hub, space: &str) {
    assert!(index < events.len(), "tipos na ordem recebida: {:?}", event_types(events));
    assert!(
        events[index] == json!({"type":"donnaControlTowerChanged","hubId":hub.host_id,"spaceId":space}),
        "invalidação fora do contrato; tipos na ordem recebida: {:?}",
        event_types(events)
    );
    let _ = events.remove(index);
}

fn unavailable(bot: &Value) {
    assert_eq!(bot["status"], "unavailable");
    assert_eq!(bot["activity"], "");
    assert!(bot["activityCode"].is_null(), "o código sai junto do texto na redação: {bot}");
    for field in ["startedAt", "resumeAt", "workingChat"] {
        assert!(bot[field].is_null(), "{field}: {bot}");
    }
}

fn visible(bot: &Value, chat: &str, status: &str) {
    assert_eq!(bot["status"], status);
    assert!(bot["activity"].as_str().unwrap().contains("ATIVIDADE_PRIVADA"));
    assert_eq!(bot["activityCode"]["kind"], "working", "{bot}");
    assert_eq!(bot["startedAt"], 1_730_000_000_123_i64);
    assert_eq!(bot["resumeAt"], 1_730_000_000_999_i64);
    assert_eq!(bot["workingChat"], chat);
}

#[tokio::test]
async fn ausencia_de_observacao_no_snapshot_e_stream_nao_vira_idle() {
    let f = Fixture::new().await;
    let group = f.group();
    let before = f.hub.store.current_rev();
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    for id in [&f.donna, &group] {
        unavailable(&f.bot(id));
        f.hub.emit_bot(id);
    }
    for since in [0, before] {
        let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
        tokio::pin!(stream);
        f.marker();
        let events = until_marker(stream.as_mut()).await;
        assert_eq!(events[0]["type"], "hello");
        for id in [&f.donna, &group] {
            let event = events.iter().find(|e| e["type"] == "bot" && e["bot"]["id"] == *id).unwrap();
            unavailable(&event["bot"]);
        }
    }
    let since = f.hub.store.current_rev();
    let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    for id in [&f.donna, &group] {
        f.hub.emit_bot(id);
    }
    f.marker();
    let events = until_marker(stream.as_mut()).await;
    assert_eq!(events.len(), 2, "tipos na ordem recebida: {:?}", event_types(&events));
    for event in events {
        unavailable(&event["bot"]);
    }
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    // Leituras repetidas continuam indisponíveis: a projeção não insere um default no mapa.
    unavailable(&f.bot(&f.donna));
    unavailable(&f.bot(&group));

    // O bot legado não é emitido pelo default igual; a Torre invalida a primeira observação explícita.
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime::default());
    for id in [&f.donna, &group] {
        assert_eq!(f.bot(id)["status"], "idle");
        f.hub.emit_bot(id);
    }
    f.marker();
    let mut events = until_marker(stream.as_mut()).await;
    expect_tower_change(&mut events, 0, &f.hub, PERSONAL);
    assert_eq!(events.len(), 2, "tipos na ordem recebida: {:?}", event_types(&events));
    for event in events {
        let bot = &event["bot"];
        assert_eq!(bot["status"], "idle");
        assert_eq!(bot["activity"], "");
        for field in ["startedAt", "resumeAt", "workingChat"] {
            assert!(bot[field].is_null());
        }
    }
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    let foreign = format!("{:?}", f.hub.runtime(&f.donna));
    f.marker();
    let mut events = until_marker(stream.as_mut()).await;
    expect_tower_change(&mut events, 0, &f.hub, PERSONAL);
    assert_eq!(events.len(), 1, "tipos na ordem recebida: {:?}", event_types(&events));
    unavailable(&events[0]["bot"]);
    unavailable(&f.bot(&f.donna));
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), foreign);
    // Uma mudança privada posterior continua indisponível no Pessoal: nenhum hint extra.
    f.hub.set_runtime(&f.donna, |rt| rt.activity = "ATIVIDADE_PRIVADA_POSTERIOR".into());
    f.marker();
    let events = until_marker(stream.as_mut()).await;
    assert_eq!(events.len(), 1, "tipos na ordem recebida: {:?}", event_types(&events));
    assert_eq!(events[0]["type"], "bot");
    unavailable(&events[0]["bot"]);
}

#[tokio::test]
async fn hello_completo_e_sync_incremental_redigem_trabalho_de_outro_space() {
    let f = Fixture::new().await;
    let before = f.hub.store.current_rev();
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    f.runtime(&f.loja_bot, Some(&f.loja_bot), BotStatus::Working);
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    for space in [PERSONAL, f.loja.as_str()] {
        // A RPC hello não carrega bots: anuncia o contexto; os bots vêm no catch-up do stream.
        let hello = dispatch(&f.hub, &Caller::Local, "hello", json!({"spaceId":space})).await.unwrap();
        assert_eq!(hello["spaceId"], space);
        assert!(hello.get("bots").is_none());
        assert!(!hello.to_string().contains("ATIVIDADE_PRIVADA"));
        for since in [0, before] {
            let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(space)).unwrap();
            tokio::pin!(stream);
            f.marker();
            let events = until_marker(stream.as_mut()).await;
            assert_eq!(events[0]["type"], "hello");
            assert_eq!(events[0]["spaceId"], space);
            let bots: Vec<_> = events.iter().filter(|e| e["type"] == "bot").map(|e| &e["bot"]).collect();
            if space == PERSONAL {
                let donna = bots.iter().find(|b| b["id"] == f.donna).unwrap();
                unavailable(donna);
                assert!(!events.iter().any(|e| e.to_string().contains("ATIVIDADE_PRIVADA")));
                assert!(!events.iter().any(|e| e.to_string().contains(&f.loja_chat)));
            } else {
                assert!(!bots.iter().any(|b| b["id"] == f.donna));
                visible(bots.iter().find(|b| b["id"] == f.loja_bot).unwrap(), &f.loja_bot, "working");
            }
        }
    }
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    let bots = crate::chat::team::call(&f.hub, &f.observer, "list_bots", &json!({})).await.unwrap();
    assert_eq!(bots["bots"].as_array().unwrap().len(), 1);
    assert_eq!(bots["bots"][0]["id"], f.donna);
    assert_eq!(bots["bots"][0]["status"], "unavailable");
}

#[tokio::test]
async fn evento_real_filtrado_redige_necessidade_espera_e_erro_estrangeiros() {
    let f = Fixture::new().await;
    let since = f.hub.store.current_rev();
    let personal = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
    let loja = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(&f.loja)).unwrap();
    tokio::pin!(personal, loja);
    assert_eq!(personal.next().await.unwrap()["type"], "hello");
    assert_eq!(loja.next().await.unwrap()["type"], "hello");
    for status in [BotStatus::NeedsInput, BotStatus::Waiting, BotStatus::Error] {
        f.runtime(&f.donna, Some(&f.loja_chat), status);
        f.marker();
        let events = until_marker(personal.as_mut()).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["bot"]["id"], f.donna);
        unavailable(&events[0]["bot"]);
        let mut loja_events = until_marker(loja.as_mut()).await;
        expect_tower_change(&mut loja_events, 0, &f.hub, &f.loja);
        assert_eq!(loja_events.len(), 0, "tipos na ordem recebida: {:?}", event_types(&loja_events));
        assert_eq!(f.hub.runtime(&f.donna).status, status);
    }
    // O hint é capturado com o vínculo válido, mas a conversa fecha antes do consumo.
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_space_conversations SET closed=1 WHERE id=?1", [&f.loja_chat])
        .unwrap();
    f.marker();
    let events = until_marker(personal.as_mut()).await;
    assert_eq!(events.len(), 1, "tipos na ordem recebida: {:?}", event_types(&events));
    assert_eq!(events[0]["type"], "bot");
    unavailable(&events[0]["bot"]);
    let loja_events = until_marker(loja.as_mut()).await;
    assert_eq!(loja_events.len(), 0, "tipos na ordem recebida: {:?}", event_types(&loja_events));
}

#[tokio::test]
async fn contexto_proprio_e_grupo_preservam_runtime_e_campos_autorizados() {
    let f = Fixture::new().await;
    f.hub
        .store
        .insert_entry(
            &Lane::main(&f.donna),
            EntryKind::Agent,
            1,
            &json!({"author":f.donna,"final":true,"text":"ULTIMA_MENSAGEM_AUTORIZADA"}),
        )
        .unwrap();
    let row = f.hub.store.bot(&f.donna).unwrap().unwrap();
    let mut base = f.hub.bot_json(&row);
    assert_eq!(base["description"], "CONFIG_AUTORIZADA");
    assert_eq!(base["lastMessage"], "ULTIMA_MENSAGEM_AUTORIZADA");
    for field in FIELDS {
        base.as_object_mut().unwrap().remove(field);
    }
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    let mut foreign = f.hub.bot_json(&row);
    unavailable(&foreign);
    for field in FIELDS {
        foreign.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(foreign, base, "redação estrangeira preserva os demais campos autorizados");
    for (status, wire) in [
        (BotStatus::Working, "working"),
        (BotStatus::NeedsInput, "needsInput"),
        (BotStatus::Waiting, "waiting"),
        (BotStatus::Error, "error"),
    ] {
        f.runtime(&f.donna, Some(&f.personal_chat), status);
        let mut own = f.hub.bot_json(&row);
        visible(&own, &f.personal_chat, wire);
        for field in FIELDS {
            own.as_object_mut().unwrap().remove(field);
        }
        assert_eq!(own, base, "projeção não muda configuração, memória, preview ou capacidades");
    }
    let group = f.group();
    let mut cfg = f.hub.store.bot(&group).unwrap().unwrap().config;
    cfg.members.push(f.observer.clone());
    f.hub.store.save_bot(&cfg).unwrap();
    // Outro membro ainda sem observação não apaga o runtime real escolhido pelo agregador.
    f.runtime(&f.donna, Some(&group), BotStatus::NeedsInput);
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    visible(&f.bot(&f.donna), &group, "needsInput");
    visible(&f.bot(&group), &group, "needsInput");
    assert_eq!(f.bot(&group)["activity"], format!("{}: ATIVIDADE_PRIVADA", f.hub.bot_name(&f.donna)));
    assert_eq!(
        f.bot(&group)["activityCode"],
        json!({"kind":"working","member":f.hub.bot_name(&f.donna)}),
        "o código do membro agregado sobe identificado, como o prefixo do texto"
    );
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime::default());
    assert_eq!(f.bot(&f.donna)["status"], "idle");
    unavailable(&f.bot(&group));
    f.hub.set_runtime(&f.observer, |rt| *rt = Runtime::default());
    assert_eq!(f.bot(&group)["status"], "idle");
}

#[tokio::test]
async fn grupo_com_row_antigo_nao_afirma_idle_apos_incluir_membro_sem_observacao() {
    let f = Fixture::new().await;
    let group = f.group();
    let old_row = f.hub.store.bot(&group).unwrap().unwrap();
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime::default());
    assert_eq!(f.hub.bot_json(&old_row)["status"], "idle");
    let mut cfg = old_row.config.clone();
    cfg.members.push(f.observer.clone());
    f.hub.store.save_bot(&cfg).unwrap();
    assert!(f.hub.observed_runtime(&f.observer).is_none());

    // A interleaving é explícita: projetar o MESMO row [A], depois de salvar [A,B].
    let stale = f.hub.bot_json(&old_row);
    unavailable(&stale);
    assert_eq!(stale["members"], json!([f.donna]));
    unavailable(&f.bot(&group));
    assert!(f.hub.observed_runtime(&f.observer).is_none(), "projeção não observa B por fallback");
    f.hub.set_runtime(&f.observer, |rt| *rt = Runtime::default());
    let current = f.hub.store.bot(&group).unwrap().unwrap();
    assert_eq!(current.config.members, vec![f.donna.clone(), f.observer.clone()]);
    assert_eq!(f.hub.bot_json(&current)["status"], "idle");
    unavailable(&f.hub.bot_json(&old_row));
    for member in [&f.donna, &f.observer] {
        assert_eq!(format!("{:?}", f.hub.observed_runtime(member).unwrap()), format!("{:?}", Runtime::default()));
    }
}

#[tokio::test]
async fn grupo_com_row_antigo_fica_indisponivel_apos_reorder_e_remocao() {
    let f = Fixture::new().await;
    let group = f.group();
    let mut cfg = f.hub.store.bot(&group).unwrap().unwrap().config;
    cfg.members.push(f.observer.clone());
    f.hub.store.save_bot(&cfg).unwrap();
    f.runtime(&f.donna, Some(&group), BotStatus::NeedsInput);
    f.runtime(&f.observer, Some(&group), BotStatus::Working);
    let internal = [&f.donna, &f.observer].map(|id| format!("{:?}", f.hub.runtime(id)));
    let old_row = f.hub.store.bot(&group).unwrap().unwrap();
    visible(&f.hub.bot_json(&old_row), &group, "needsInput");

    // O conjunto é igual, mas a prioridade do primeiro membro ativo mudou.
    cfg.members.reverse();
    f.hub.store.save_bot(&cfg).unwrap();
    unavailable(&f.hub.bot_json(&old_row));
    let reordered = f.hub.store.bot(&group).unwrap().unwrap();
    visible(&f.hub.bot_json(&reordered), &group, "working");

    cfg.members = vec![f.donna.clone()];
    f.hub.store.save_bot(&cfg).unwrap();
    unavailable(&f.hub.bot_json(&reordered));
    visible(&f.bot(&group), &group, "needsInput");
    assert_eq!([&f.donna, &f.observer].map(|id| format!("{:?}", f.hub.runtime(id))), internal);
}

#[tokio::test]
async fn sem_lane_contexto_desconhecido_e_relogio_sem_origem_nao_inventam_idle_ou_erro() {
    let f = Fixture::new().await;
    for status in [BotStatus::Working, BotStatus::Waiting, BotStatus::Error, BotStatus::Idle] {
        f.runtime(&f.donna, None, status);
        let before = format!("{:?}", f.hub.runtime(&f.donna));
        unavailable(&f.bot(&f.donna));
        assert_eq!(before, format!("{:?}", f.hub.runtime(&f.donna)));
    }
    f.runtime(&f.donna, Some("conversa-inexistente"), BotStatus::Error);
    unavailable(&f.bot(&f.donna));
    f.runtime(&f.donna, Some(&f.observer), BotStatus::Working);
    unavailable(&f.bot(&f.donna));
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime { status: BotStatus::Error, ..Runtime::default() });
    unavailable(&f.bot(&f.donna));
    f.hub.store.db.locked().execute("DELETE FROM donna_space_conversations WHERE id=?1", [&f.donna]).unwrap();
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime::default());
    unavailable(&f.bot(&f.donna));
}

#[tokio::test]
async fn fechamento_e_rebinding_redigem_snapshot_e_evento_ja_enfileirado() {
    for changed_binding in [false, true] {
        let f = Fixture::new().await;
        // Estado Error permite o fechamento real pelo contrato principal; não altera seu estado interno.
        f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Error);
        let since = f.hub.store.current_rev();
        let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
        tokio::pin!(stream);
        assert_eq!(stream.next().await.unwrap()["type"], "hello");
        f.hub.emit_bot(&f.donna);
        if changed_binding {
            // Simula vínculo persistido revogado, sem permitir essa mutação como API nova.
            f.hub
                .store
                .db
                .locked()
                .execute(
                    "UPDATE donna_space_conversations SET closed=0,bot_id=?2 WHERE id=?1",
                    [&f.personal_chat, &f.observer],
                )
                .unwrap();
        } else {
            let c = f.hub.store.space_conversation(PERSONAL, &f.personal_chat).unwrap();
            dispatch(
                &f.hub,
                &Caller::Local,
                "donnaConversationUpdate",
                json!({
                    "hubId":f.hub.host_id,"spaceId":PERSONAL,"conversationId":f.personal_chat,
                    "expectedRevision":c.revision,"closed":true,
                }),
            )
            .await
            .unwrap();
        }
        f.marker();
        let events = until_marker(stream.as_mut()).await;
        let bot = events.iter().find(|e| e["type"] == "bot").unwrap();
        unavailable(&bot["bot"]);
        unavailable(&f.bot(&f.donna));
        assert_eq!(f.hub.runtime(&f.donna).status, BotStatus::Error);
    }
}

#[tokio::test]
async fn remocao_de_membro_revoga_lane_e_evento_agregado_ja_enfileirado() {
    let f = Fixture::new().await;
    let group = f.group();
    f.runtime(&f.donna, Some(&group), BotStatus::Working);
    let since = f.hub.store.current_rev();
    let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    f.hub.emit_bot(&f.donna);
    f.hub.emit_bot(&group);
    let mut cfg = f.hub.store.bot(&group).unwrap().unwrap().config;
    cfg.members = vec![f.observer.clone()];
    f.hub.store.save_bot(&cfg).unwrap();
    f.marker();
    let events = until_marker(stream.as_mut()).await;
    assert_eq!(events.len(), 2);
    for event in events {
        unavailable(&event["bot"]);
    }
    unavailable(&f.bot(&f.donna));
    assert_eq!(f.hub.runtime(&f.donna).status, BotStatus::Working);
}

#[tokio::test]
async fn exclusao_preserva_tombstone_e_nao_reenvia_runtime_antigo() {
    let f = Fixture::new().await;
    f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Working);
    let old_row = f.hub.store.bot(&f.donna).unwrap().unwrap();
    let since = f.hub.store.current_rev();
    let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    f.hub.emit_bot(&f.donna);
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    f.hub.delete_bot(&f.donna).unwrap();
    f.marker();
    let mut events = until_marker(stream.as_mut()).await;
    expect_tower_change(&mut events, 2, &f.hub, PERSONAL);
    assert_eq!(events.len(), 2, "tipos na ordem recebida: {:?}", event_types(&events));
    unavailable(&events[0]["bot"]);
    assert_eq!(events[1]["bot"]["deleted"], true);
    assert!(events[1]["bot"].get("status").is_none());
    unavailable(&f.hub.bot_json(&old_row));
}

/// MX-147: código estável flui junto do texto legado no snapshot e no evento,
/// troca e limpa nas mesmas transições, e um título livre passa cru em `tool`.
#[tokio::test]
async fn activity_code_flui_substitui_e_limpa_com_o_texto() {
    let f = Fixture::new().await;
    let since = f.hub.store.current_rev();
    let stream = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");

    for (text, kind, wire) in [
        ("Typing…", ActivityKind::Typing, json!({"kind":"typing"})),
        (
            "Needs approval: rodar testes",
            ActivityKind::NeedsApproval { title: "rodar testes".into() },
            json!({"kind":"needs_approval","title":"rodar testes"}),
        ),
        (
            "Downloading Codex…",
            ActivityKind::Downloading { item: "Codex".into() },
            json!({"kind":"downloading","item":"Codex"}),
        ),
        ("Terminal", ActivityKind::Tool { title: "Terminal".into() }, json!({"kind":"tool","title":"Terminal"})),
    ] {
        f.hub.set_runtime(&f.donna, |rt| {
            rt.status = BotStatus::Working;
            rt.lane = Some(Lane::main(&f.personal_chat));
            rt.set_activity(text, Some(kind));
        });
        let bot = f.bot(&f.donna);
        assert_eq!(bot["activity"], text);
        assert_eq!(bot["activityCode"], wire);
        f.marker();
        let events = until_marker(stream.as_mut()).await;
        let event =
            events.iter().find(|e| e["type"] == "bot" && e["bot"]["id"] == f.donna).expect("evento bot da fixture");
        assert_eq!(event["bot"]["activity"], text);
        assert_eq!(event["bot"]["activityCode"], wire);
    }

    // Só o código muda (mesmo texto): a comparação do runtime ainda emite o bot.
    f.hub.set_runtime(&f.donna, |rt| rt.set_activity("Terminal", Some(ActivityKind::Working)));
    f.marker();
    let events = until_marker(stream.as_mut()).await;
    let event = events
        .iter()
        .find(|e| e["type"] == "bot" && e["bot"]["id"] == f.donna)
        .expect("mudança só de código emite o bot");
    assert_eq!(event["bot"]["activity"], "Terminal");
    assert_eq!(event["bot"]["activityCode"], json!({"kind":"working"}));

    // Transição de limpeza apaga texto e código juntos no snapshot e no evento.
    f.hub.set_runtime(&f.donna, |rt| {
        rt.status = BotStatus::Idle;
        rt.lane = None;
        rt.clear_activity();
    });
    let bot = f.bot(&f.donna);
    assert_eq!(bot["activity"], "");
    assert!(bot["activityCode"].is_null());
    f.marker();
    let events = until_marker(stream.as_mut()).await;
    let event = events
        .iter()
        .find(|e| e["type"] == "bot" && e["bot"]["id"] == f.donna)
        .expect("a limpeza emite o bot atualizado");
    assert_eq!(event["bot"]["activity"], "");
    assert!(event["bot"]["activityCode"].is_null());
}

// R3: eventos em fila e snapshots revalidados na entrega.
mod events;

// R1: caminhos de entrega focados, com transporte/broker de fixture, sem rede ou motor.
mod live_activity;
mod screen;
