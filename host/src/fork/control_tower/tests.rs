#![allow(clippy::unwrap_used)]
use super::{EVENT, METHOD};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::spaces::PERSONAL,
    hub::{BotStatus, Hub},
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, EntryKind, Lane, Scope, Store, now_ms},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-torre-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "torre".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        Self { hub, dir }
    }
    fn bot(&self, id: &str, space: &str, kind: &str) {
        self.bot_with_avatar(id, space, kind, &crate::fork_avatars::preset("Fonte", "", false));
    }
    fn bot_with_avatar(&self, id: &str, space: &str, kind: &str, avatar: &crate::fork_avatars::AvatarConfig) {
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":id,"name":id,"spaceId":space,"kind":kind,"backend":"fixture",
            "title":"papel-reservado","description":"alma-reservada","cwd":"/pasta-reservada",
            "avatarConfig":avatar
        }))
        .unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
    }
    async fn space(&self) -> String {
        let initial = self.hub.store.spaces().unwrap();
        let created = dispatch(
            &self.hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId":self.hub.host_id,"expectedRevision":initial.revision,"name":"Trabalho",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}
            }),
        )
        .await
        .unwrap();
        created["spaces"][1]["id"].as_str().unwrap().into()
    }
    async fn read(&self, caller: &Caller, space: &str) -> anyhow::Result<Value> {
        dispatch(&self.hub, caller, METHOD, json!({"hubId":self.hub.host_id,"spaceId":space})).await
    }
    fn device(&self, source: DeviceSource, scopes: Vec<Scope>, lease: Option<i64>) -> Caller {
        self.hub
            .store
            .put_device(&Device {
                key: "controle".into(),
                name: "fixture".into(),
                platform: "fixture".into(),
                source,
                grant_id: None,
                scopes: scopes.clone(),
                lease_until: lease,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        Caller::Device { key: "controle".into(), scopes, autoridade: None }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

#[tokio::test]
async fn snapshot_filtra_roster_e_ausencia_nao_vira_idle_ou_zero() {
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("bot", PERSONAL, "agent");
    f.bot("grupo", PERSONAL, "group");
    f.bot("privado", &work, "agent");
    let snapshot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(snapshot["bots"].as_array().unwrap().len(), 1);
    let bot = &snapshot["bots"][0];
    assert_eq!(bot["runtime"], json!({"availability":"unavailable"}));
    assert_eq!(bot["queue"], json!({"availability":"unavailable"}));
    for key in ["dailyBotTokens", "subscriptionLimit", "contextTokens"] {
        assert_eq!(bot["usage"][key], json!({"availability":"unavailable"}));
    }
    let wire = snapshot.to_string();
    for secret in ["privado", "grupo", "alma-reservada", "/pasta-reservada", "acp"] {
        assert!(!wire.contains(secret), "não serializar {secret}");
    }
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, json!({"spaceId":PERSONAL})).await.is_err());
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, json!({"hubId":"outro","spaceId":PERSONAL})).await.is_err());
    assert!(f.read(&Caller::Local, "inexistente").await.is_err());
    assert!(
        dispatch(&f.hub, &Caller::Local, METHOD, json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"inventado":true}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn avatar_oficial_preserva_fonte_canonica_sem_cruzar_spaces() {
    let f = Fixture::new();
    let work = f.space().await;
    let visible = crate::fork_avatars::preset("Fonte", "", false);
    let private = crate::fork_avatars::preset("Amazon", "", false);
    assert_ne!(visible, private);
    f.bot_with_avatar("visivel", PERSONAL, "agent", &visible);
    f.bot_with_avatar("privado", &work, "agent", &private);
    let (profile, avatar): (String, String) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT config,avatar_config FROM bots WHERE id='visivel'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(serde_json::from_str::<Value>(&profile).unwrap().get("avatarConfig").is_none());
    assert_eq!(serde_json::from_str::<Value>(&avatar).unwrap(), serde_json::to_value(&visible).unwrap());
    let canonical = f.hub.store.bot("visivel").unwrap().unwrap().config;
    assert_eq!(canonical.avatar_config, Some(visible));
    let snapshot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    let bots = snapshot["bots"].as_array().unwrap();
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0]["botId"], "visivel");
    assert_eq!(bots[0]["avatarConfig"], serde_json::to_value(canonical.avatar_config).unwrap());
    assert_ne!(bots[0]["avatarConfig"], serde_json::to_value(&private).unwrap());
    assert!(!snapshot.to_string().contains("privado"));
    let other = f.read(&Caller::Local, &work).await.unwrap();
    let bots = other["bots"].as_array().unwrap();
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0]["botId"], "privado");
    assert_eq!(bots[0]["avatarConfig"], serde_json::to_value(private).unwrap());
    assert!(!other.to_string().contains("visivel"));
}

#[tokio::test]
async fn estados_medidos_e_abrir_somente_navegacao_sem_executionref() {
    let f = Fixture::new();
    f.bot("bot", PERSONAL, "agent");
    for (status, expected) in [
        (BotStatus::Idle, "idle"),
        (BotStatus::Working, "working"),
        (BotStatus::NeedsInput, "needsInput"),
        (BotStatus::Waiting, "waiting"),
        (BotStatus::Error, "error"),
    ] {
        f.hub.set_runtime("bot", |rt| {
            rt.status = status;
            rt.lane = Some(Lane::main("bot"));
            rt.session = Some("acp-nao-e-sessionref".into());
            rt.set_activity("atividade medida", Some(crate::fork::activity_code::ActivityKind::Working));
            rt.started_at = Some(123);
            rt.resume_at = Some(456);
        });
        let rev = f.hub.store.current_rev();
        let before = f.hub.store.history("bot", i64::MAX, 10).unwrap();
        let mut rx = f.hub.events.subscribe();
        let snapshot = f.read(&Caller::Local, PERSONAL).await.unwrap();
        let bot = &snapshot["bots"][0];
        assert_eq!(bot["runtime"]["value"]["status"], expected);
        assert_eq!(
            bot["runtime"]["value"]["startedAt"],
            if status == BotStatus::Working { json!(123) } else { Value::Null }
        );
        assert_eq!(
            bot["runtime"]["value"]["activity"],
            if status == BotStatus::Working { json!("atividade medida") } else { Value::Null }
        );
        assert_eq!(
            bot["runtime"]["value"]["activityCode"],
            if status == BotStatus::Working { json!({"kind":"working"}) } else { Value::Null }
        );
        assert_eq!(bot["open"]["label"], "Abrir conversa do bot");
        assert_eq!(bot["open"]["conversation"]["conversationId"], "bot");
        assert_eq!(bot["open"]["sessionRef"], json!({"id":"bot","chatId":"bot"}));
        assert!(bot.get("paused").is_none(), "pausa não tem produtor público nesta base");
        assert!(!snapshot.to_string().contains("acp-nao-e-sessionref"));
        assert!(bot.get("executionRef").is_none());
        assert_eq!(f.hub.store.current_rev(), rev, "ler não cria sessão, mensagem ou execução");
        assert_eq!(f.hub.store.history("bot", i64::MAX, 10).unwrap().len(), before.len());
        assert!(rx.try_recv().is_err(), "consulta não emite efeitos");
    }
    f.hub.set_runtime("bot", |rt| {
        rt.status = BotStatus::Error;
        rt.lane = None;
    });
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["availability"], "unavailable");
}

#[tokio::test]
async fn runtime_em_outro_space_nao_revela_atividade_titulo_pendencia_ou_contagem() {
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("donna", PERSONAL, "agent");
    f.hub.store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    let created = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationCreate",
        json!({
            "hubId":f.hub.host_id,"spaceId":work,"botId":"donna","title":"titulo-privado"
        }),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    let pending = f
        .hub
        .store
        .insert_entry(
            &Lane::main(chat),
            EntryKind::Permission,
            1,
            &json!({
                "status":"pending","text":"comando-privado"
            }),
        )
        .unwrap();
    let mut events = f.hub.events.subscribe();
    f.hub.set_runtime("donna", |rt| {
        rt.status = BotStatus::NeedsInput;
        rt.lane = Some(Lane::main(chat));
        rt.activity = "atividade-privada".into();
        rt.started_at = Some(987_654);
    });
    for activity in [None, Some("outra-atividade-privada")] {
        if let Some(activity) = activity {
            f.hub.set_runtime("donna", |rt| rt.activity = activity.into());
        }
        let mut changed = false;
        while let Ok(event) = events.try_recv() {
            if event["type"] == EVENT {
                changed = true;
                assert_eq!(event["spaceId"], work, "primeira observação e mudanças privadas só invalidam Loja");
                assert_eq!(
                    super::event(&f.hub, &Caller::Local, &work, &event).unwrap(),
                    json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":work})
                );
                assert!(super::event(&f.hub, &Caller::Local, PERSONAL, &event).is_none());
            }
            if event["type"] == "bot" && event["bot"]["id"] == "donna" {
                assert_eq!(event["bot"]["status"], "unavailable", "legado preserva produtor #232");
                assert!(event["bot"]["workingChat"].is_null());
                assert!(event["bot"]["startedAt"].is_null());
                assert_eq!(event["bot"]["activity"], "");
            }
        }
        assert!(changed, "audiência autorizada recebe hint real do Hub");
    }
    let personal = f.read(&Caller::Local, PERSONAL).await.unwrap();
    let wire = personal.to_string();
    for secret in [chat, "titulo-privado", "atividade-privada", "987654", &pending.id, "comando-privado"] {
        assert!(!wire.contains(secret), "audiência bloqueia {secret}");
    }
    assert_eq!(personal["bots"][0]["runtime"], json!({"availability":"unavailable"}));
    assert_eq!(personal["bots"][0]["pending"]["value"].as_array().unwrap().as_slice(), &[] as &[Value]);
    let allowed = f.read(&Caller::Local, &work).await.unwrap();
    let bot = &allowed["bots"][0];
    assert_eq!(bot["runtime"]["value"]["status"], "needsInput");
    assert_eq!(bot["runtime"]["value"]["conversation"]["conversationId"], chat);
    assert!(bot["title"].is_null(), "roster pessoal não libera função privada no Space");
    assert_eq!(bot["pending"]["value"].as_array().unwrap().len(), 1);
    assert_eq!(bot["pending"]["value"][0]["entryId"], pending.id);
    assert_eq!(
        bot["pending"]["value"][0]["sessionRef"],
        json!({"id":chat,"chatId":chat}),
        "catálogo original, sem ACP"
    );
    assert!(!allowed.to_string().contains("comando-privado"));
    // Revogação/fechamento são rechecados em cada consulta, sem fallback de conversa.
    f.hub.store.db.locked().execute("UPDATE donna_space_conversations SET closed=1 WHERE id=?1", [chat]).unwrap();
    let closed = f.read(&Caller::Local, &work).await.unwrap();
    assert_eq!(closed["bots"][0]["runtime"]["availability"], "unavailable");
    assert!(closed["bots"][0]["open"].is_null());
    assert_eq!(closed["bots"][0]["pending"], json!({"availability":"unavailable"}));
}

#[tokio::test]
async fn pendencias_sao_referencias_unicas_com_sessao_original_valida() {
    let f = Fixture::new();
    f.bot("bot", PERSONAL, "agent");
    let original = f
        .hub
        .store
        .insert_entry(
            &Lane::main("bot"),
            EntryKind::Permission,
            1,
            &json!({
                "status":"pending","command":"conteudo-privado"
            }),
        )
        .unwrap();
    f.hub.store.insert_entry(&Lane::main("bot"), EntryKind::Permission, 2, &json!({"status":"approved"})).unwrap();
    let unknown =
        f.hub.store.insert_entry(&Lane::main("bot"), EntryKind::Permission, 3, &json!({"status":"pending"})).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET session='sessao-inexistente' WHERE id=?1", [&unknown.id])
        .unwrap();
    let snapshot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    let pending = snapshot["bots"][0]["pending"]["value"].as_array().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["entryId"], original.id);
    assert!(!snapshot.to_string().contains("conteudo-privado"));
}

#[tokio::test]
async fn device_revalida_escopo_lease_e_revogacao_depois_da_espera() {
    let f = Fixture::new();
    f.bot("bot", PERSONAL, "agent");
    for caller in [
        Caller::Device { key: "ausente".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Pairing { key: "par".into() },
        Caller::Bootstrap { key: "boot".into(), bootstrap_id: "bootstrap".into() },
        f.device(DeviceSource::Local, vec![Scope::OwnerDevices], None),
        f.device(DeviceSource::Local, vec![Scope::Executor], None),
        f.device(DeviceSource::Local, vec![Scope::Screen], None),
    ] {
        assert!(f.read(&caller, PERSONAL).await.is_err());
    }
    let limited = f.device(DeviceSource::Local, vec![], None);
    f.device(DeviceSource::Local, vec![Scope::Control], None);
    assert!(f.read(&limited, PERSONAL).await.is_err(), "concessão nova não amplia o Caller original");
    let stale = f.device(DeviceSource::Local, vec![Scope::Control], None);
    assert!(f.read(&stale, PERSONAL).await.is_ok());
    f.device(DeviceSource::Local, vec![], None);
    assert!(f.read(&stale, PERSONAL).await.is_err(), "scopes do Caller não preservam concessão velha");
    let leased = f.device(DeviceSource::Account, vec![Scope::Control], Some(now_ms() - 1));
    f.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    assert!(f.read(&leased, PERSONAL).await.is_err());
    let current = f.device(DeviceSource::Local, vec![Scope::Control], None);
    let guard = f.hub.store.team_lock.lock().await;
    let hub = f.hub.clone();
    let task =
        tokio::spawn(
            async move { dispatch(&hub, &current, METHOD, json!({"hubId":hub.host_id,"spaceId":PERSONAL})).await },
        );
    tokio::task::yield_now().await;
    f.hub.store.db.locked().execute("DELETE FROM devices WHERE key='controle'", []).unwrap();
    drop(guard);
    assert!(task.await.unwrap().is_err());
    let revoked = Caller::Device { key: "controle".into(), scopes: vec![Scope::Control], autoridade: None };
    let error = f.read(&revoked, "space-nao-revelar").await.unwrap_err();
    assert!(error.is::<crate::api::devices::Forbidden>(), "revogado não consulta nem existência de Space");
}

#[tokio::test]
async fn canal_revalida_audiencia_e_entrega_apenas_invalidacao_sem_corpo() {
    use futures::StreamExt;
    let f = Fixture::new();
    f.bot("bot", PERSONAL, "agent");
    let caller = f.device(DeviceSource::Local, vec![Scope::Control], None);
    let stream =
        crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &caller, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap()["type"],
        "hello"
    );
    let input =
        json!({"type":EVENT,"spaceId":PERSONAL,"botId":"bot","conversationId":"bot","activity":"nao-encaminhar"});
    let _ = f.hub.events.send(json!({"type":EVENT,"spaceId":"outro-space","botId":"bot","conversationId":"bot"}));
    let _ = f.hub.events.send(input.clone());
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap(),
        json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL})
    );
    f.device(DeviceSource::Local, vec![], None);
    let _ = f.hub.events.send(input.clone());
    let _ = f.hub.events.send(json!({"type":"resync","marker":"fim"}));
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap()["marker"],
        "fim",
        "revogado não recebe a projeção"
    );
    f.hub.store.delete_bot("bot").unwrap();
    assert!(super::event(&f.hub, &Caller::Local, PERSONAL, &input).is_none());
}

#[tokio::test]
async fn mudanca_privada_nao_invalida_outro_space_e_grupo_nao_e_executor() {
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("bot", PERSONAL, "agent");
    f.bot("grupo", PERSONAL, "group");
    f.bot("outro", &work, "agent");
    let mut rx = f.hub.events.subscribe();
    f.hub.set_runtime("bot", |r| {
        r.status = BotStatus::Working;
        r.lane = Some(Lane::main("outro"));
    });
    f.hub.set_runtime("bot", |r| {
        r.activity = "atividade secreta".into();
        r.started_at = Some(123);
    });
    while let Ok(v) = rx.try_recv() {
        if v["type"] == EVENT {
            assert_ne!(v["spaceId"], PERSONAL, "atividade privada seguinte não sinaliza Pessoal");
            assert!(
                super::event(&f.hub, &Caller::Local, &work, &v).is_none(),
                "bot sem vínculo não aparece no outro roster"
            );
        }
    }
    let mut group = f.hub.store.bot("grupo").unwrap().unwrap().config;
    group.members = vec!["bot".into()];
    f.hub.store.save_bot(&group).unwrap();
    f.hub.set_runtime("bot", |r| {
        r.lane = Some(Lane::main("grupo"));
    });
    let snapshot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(snapshot["bots"].as_array().unwrap().len(), 1, "grupo não vira executor");
    assert_eq!(snapshot["bots"][0]["runtime"]["value"]["conversation"]["conversationId"], "grupo");
    assert_eq!(
        snapshot["bots"][0]["open"]["conversation"]["conversationId"], "bot",
        "abrir bot não assume execução de grupo"
    );
}

#[tokio::test]
async fn stream_real_invalida_spaces_apos_troca_e_exclusao_da_principal() {
    use futures::StreamExt;
    for operation in ["troca", "exclusao"] {
        let f = Fixture::new();
        let work = f.space().await;
        let principal = format!("torre-r2-{}", uuid::Uuid::new_v4());
        let successor = format!("torre-r2-{}", uuid::Uuid::new_v4());
        for id in [&principal, &successor] {
            f.bot(id, PERSONAL, "agent");
            f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
            f.hub.store.set_team_settings(&json!({"primaryBotId":id})).unwrap();
            dispatch(
                &f.hub,
                &Caller::Local,
                "donnaConversationCreate",
                json!({
                    "hubId":f.hub.host_id,"spaceId":work,"botId":id,"title":"titulo-nao-encaminhar"
                }),
            )
            .await
            .unwrap();
        }
        f.hub.store.set_team_settings(&json!({"primaryBotId":principal})).unwrap();
        let before = f.read(&Caller::Local, &work).await.unwrap();
        assert_eq!(before["bots"].as_array().unwrap().len(), 1);
        assert_eq!(before["bots"][0]["botId"], principal);
        let caller = f.device(DeviceSource::Local, vec![Scope::Control], None);
        let since = f.hub.store.current_rev();
        let live = crate::api::scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(&work)).unwrap();
        let revoked = crate::api::scoped_events_stream(&f.hub, since, None, &caller, Some(&work)).unwrap();
        tokio::pin!(live, revoked);
        assert_eq!(live.next().await.unwrap()["type"], "hello");
        assert_eq!(revoked.next().await.unwrap()["type"], "hello");
        if operation == "troca" {
            dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"primaryBotId":successor})).await.unwrap();
        } else {
            f.hub.store.sem_donna_fixa(); // D366: exclusão legada da principal.
            dispatch(&f.hub, &Caller::Local, "deleteBot", json!({"botId":principal})).await.unwrap();
        }
        f.device(DeviceSource::Local, vec![], None);
        let next = tokio::time::timeout(std::time::Duration::from_secs(1), live.next()).await.unwrap().unwrap();
        assert_eq!(next, json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":work}));
        let current = f.read(&Caller::Local, &work).await.unwrap();
        let bots = current["bots"].as_array().unwrap();
        if operation == "troca" {
            assert_eq!(bots.len(), 1);
            assert_eq!(bots[0]["botId"], successor);
        } else {
            assert_eq!(bots.as_slice(), &[] as &[Value]);
        }
        // Só o marcador genérico é artificial; a invalidação vem da API real acima.
        let _ = f.hub.events.send(json!({"type":"resync","marker":"fim"}));
        let next = tokio::time::timeout(std::time::Duration::from_secs(1), revoked.next()).await.unwrap().unwrap();
        assert_eq!(next["marker"], "fim", "Device revogado não recebe invalidação capturada antes");
    }
}

#[tokio::test]
async fn projecao_reusa_audiencia_canonica_para_idle_e_membros_atuais() {
    use futures::StreamExt;
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("bot", PERSONAL, "agent");
    f.bot("grupo", PERSONAL, "group");
    f.bot("privado", &work, "agent");
    f.hub.set_runtime("bot", |_| {});
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["value"]["status"], "idle");
    f.hub.set_runtime("bot", |r| r.activity = "sem-conversa-capturada".into());
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["availability"], "unavailable");
    f.hub.set_runtime("bot", |r| {
        r.status = BotStatus::Working;
        r.lane = Some(Lane::main("grupo"));
    });
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["availability"], "unavailable");
    let mut group = f.hub.store.bot("grupo").unwrap().unwrap().config;
    group.members = vec!["bot".into()];
    f.hub.store.save_bot(&group).unwrap();
    let snapshot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(snapshot["bots"][0]["runtime"]["value"]["status"], "working");
    let stream =
        crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(PERSONAL))
            .unwrap()
            .filter(|event| std::future::ready(event["type"] == EVENT || event["type"] == "resync"));
    tokio::pin!(stream);
    let expected = json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL});
    // Controle positivo: produtor e consumo reais, com associação atual preservada.
    f.hub.set_runtime("bot", |r| r.activity = "membro-atual".into());
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap(),
        expected
    );
    // Sair da conversa não revoga sua associação. O contexto anterior ainda
    // precisa do hint para retirar o trabalho, mesmo com outra lane atual.
    let mut queued = f.hub.events.subscribe();
    f.hub.set_runtime("bot", |r| r.lane = Some(Lane::main("privado")));
    let previous = queued.try_recv().unwrap();
    assert_eq!(previous["type"], EVENT);
    assert_eq!(previous["conversationId"], "grupo");
    assert_eq!(f.hub.observed_runtime("bot").unwrap().lane.unwrap().chat, "privado");
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["availability"], "unavailable");
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap(),
        expected
    );
    // A1: o membro é autorizado quando o produtor real enfileira, mas perde
    // a associação antes de o stream consumir. Roster e conversa seguem vivos.
    let mut queued = f.hub.events.subscribe();
    f.hub.set_runtime("bot", |r| r.lane = Some(Lane::main("grupo")));
    let captured = queued.try_recv().unwrap();
    assert_eq!(captured["type"], EVENT);
    assert_eq!(captured["botId"], "bot");
    assert_eq!(captured["conversationId"], "grupo");
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["value"]["status"], "working");
    group.members.clear();
    f.hub.store.save_bot(&group).unwrap();
    assert_eq!(f.read(&Caller::Local, PERSONAL).await.unwrap()["bots"][0]["runtime"]["availability"], "unavailable");
    {
        let c = f.hub.store.db.locked();
        assert!(super::projection::roster(&c, PERSONAL).unwrap().iter().any(|bot| bot.id == "bot"));
        assert!(
            super::projection::conversations(&c, PERSONAL)
                .unwrap()
                .iter()
                .any(|chat| chat.id == "grupo" && !chat.closed)
        );
    }
    assert!(super::event(&f.hub, &Caller::Local, PERSONAL, &captured).is_none());
    let _ = f.hub.events.send(json!({"type":"resync","marker":"membro-removido"}));
    let next = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap();
    assert_eq!(next["marker"], "membro-removido", "hint enfileirado não sobrevive à revogação da associação");
    f.hub.store.db.locked().execute("DELETE FROM donna_bot_sessions WHERE chat_id='bot'", []).unwrap();
    let missing = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert!(missing["bots"][0]["open"]["sessionRef"].is_null(), "catálogo ausente não se infere da Lane");
    assert_eq!(missing["bots"][0]["pending"]["availability"], "unavailable");
    assert!(!crate::fork::bot_editor::authorization::retain_tool(&json!({"name":METHOD})));
    assert!(
        dispatch(&f.hub, &Caller::Local, "chatCall", json!({"botId":"bot","name":METHOD,"arguments":{}}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn exclusao_invalida_principal_atual_sem_sinalizar_vinculo_apenas_historico() {
    use futures::StreamExt;
    for current in [true, false] {
        let f = Fixture::new();
        let work = f.space().await;
        let principal = format!("torre-vinculo-{}", uuid::Uuid::new_v4());
        let successor = format!("torre-vinculo-{}", uuid::Uuid::new_v4());
        for id in [&principal, &successor] {
            f.bot(id, PERSONAL, "agent");
        }
        f.hub.store.set_team_settings(&json!({"primaryBotId":principal})).unwrap();
        dispatch(
            &f.hub,
            &Caller::Local,
            "donnaConversationCreate",
            json!({"hubId":f.hub.host_id,"spaceId":work,"botId":principal,"title":"Conversa da fixture"}),
        )
        .await
        .unwrap();
        if !current {
            f.hub.store.sem_donna_fixa(); // D366: troca legada da principal.
            f.hub.store.set_team_settings(&json!({"primaryBotId":successor})).unwrap();
        }
        let before = f.read(&Caller::Local, &work).await.unwrap();
        assert_eq!(before["bots"].as_array().unwrap().len(), usize::from(current));
        let stream =
            crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(&work))
                .unwrap();
        tokio::pin!(stream);
        assert_eq!(stream.next().await.unwrap()["type"], "hello");
        f.hub.store.sem_donna_fixa(); // D366: exclusão legada da principal.
        dispatch(&f.hub, &Caller::Local, "deleteBot", json!({"botId":principal})).await.unwrap();
        assert_eq!(f.read(&Caller::Local, &work).await.unwrap()["bots"].as_array().unwrap().len(), 0);
        let _ = f.hub.events.send(json!({"type":"resync","marker":"fim-vinculo"}));
        let mut next = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap();
        if current {
            assert!(
                next == json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":work}),
                "a principal atual recebe somente a invalidação mínima após a exclusão"
            );
            next = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap();
        }
        assert!(next["marker"] == "fim-vinculo", "vínculo apenas histórico não recebe sinal da exclusão");
    }
}

#[tokio::test]
async fn caller_original_nao_revive_na_espera_ou_no_stream_apos_reconcessao() {
    use futures::StreamExt;
    let f = Fixture::new();
    f.bot("bot", PERSONAL, "agent");
    let _legacy = f.device(DeviceSource::Local, vec![Scope::Control], None);
    let device = f.hub.store.device("controle").unwrap();
    let (_, original) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(f.read(&original, PERSONAL).await.is_ok());
    let stream =
        crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &original, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    let guard = f.hub.store.team_lock.lock().await;
    let waiting = f.read(&original, PERSONAL);
    tokio::pin!(waiting);
    assert!(futures::poll!(&mut waiting).is_pending(), "consulta parou na trava depois da validação inicial");
    let _ = f.hub.events.send(json!({"type":EVENT,"spaceId":PERSONAL,"botId":"bot","conversationId":"bot"}));
    assert!(f.hub.store.remove_device(&device.key).unwrap());
    f.hub.store.put_device(&device).unwrap();
    drop(guard);
    assert!(waiting.await.is_err(), "mesma chave e escopos não recuperam o pedido anterior");
    let _ = f.hub.events.send(json!({"type":"resync","marker":"fim-reconcessao"}));
    let next = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().unwrap();
    assert!(next["marker"] == "fim-reconcessao", "stream antigo não entrega o hint pela concessão nova");
    let (_, current) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(f.read(&current, PERSONAL).await.is_ok(), "nova autenticação continua permitida");
}
