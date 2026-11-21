//! D70: o cartão pessoal mantém a captura da API, inclusive com reemissão na mesma ACP.
use super::*;
use crate::fork::sessoes::{bindings, call_tests};
use crate::store::Store;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

struct Fixture {
    hub: Arc<Hub>,
    cfg: BotConfig,
    binding: bindings::BindingRef,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("approval-origin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let store = Store::open(std::path::Path::new(":memory:"))?;
        store.kv_set("team.settings.v1", r#"{"primaryBotId":"lead","permission":"ask"}"#)?;
        let cfg = serde_json::from_value(json!({"id":"lead","name":"Donna","backend":"claude",
            "cwd":dir,"manageTeam":true,"permission":"ask","permissionChosenByUser":true,"notify":false}))?;
        store.save_bot(&cfg)?;
        store.save_bot(&serde_json::from_value(
            json!({"id":"dev","name":"Dev","backend":"claude","cwd":dir,"notify":false}),
        )?)?;
        store.memory_session_start("lead", "approval-acp", false)?;
        let hub = Hub::new(
            store,
            "approval-test".into(),
            crate::remote::identity::Identity::load_or_create(&dir)?,
            "test".into(),
            0,
        );
        let entry = hub
            .add_entry(&Lane::main("lead"), EntryKind::User, 1, &json!({"text":"Gerencie meu time"}))
            .ok_or_else(|| anyhow!("entrada humana ausente"))?;
        let binding = bindings::issue(&hub.store, "lead", "lead", "lead", None)?;
        bindings::activate_for_config(&hub.store, &binding, "approval-acp", &cfg)?;
        hub.start_bot_turn("lead", |r| {
            r.lane = Some(Lane::main("lead"));
            r.message = Some(entry.id);
            r.session = Some("approval-acp".into());
        })?;
        Ok(Self { hub, cfg, binding, dir })
    }
    fn actor(&self) -> Actor {
        let mut actor = new_actor(self.hub.clone(), self.cfg.clone());
        actor.turn = Some(1);
        actor.turn_session = Some("approval-acp".into());
        actor
    }
    fn body(&self) -> Value {
        json!({"botId":"lead","sessionBinding":self.binding.id.as_str(),"name":"update_bot",
            "arguments":{"botId":"dev","name":"Alterado"}})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn manage_pessoal_recusa_reemissao_apos_queue_check_sem_adotar_b_e_aceita_a() -> Result<()> {
    for phase in ["permission_card_origin", "permission_card_commit", "valid"] {
        let f = Fixture::new()?;
        let mut commands = f.hub.test_bot_commands("lead");
        let _target = f.hub.test_bot_commands("dev");
        let mut actor = f.actor();
        let execution = bindings::capture(&f.hub, "lead", f.binding.id.as_str())?;
        let segment = if phase == "permission_card_origin" {
            let entry = f
                .hub
                .add_entry(&Lane::main("lead"), EntryKind::Agent, 1, &json!({"text":"antes","final":false}))
                .ok_or_else(|| anyhow!("segmento ausente"))?;
            actor.seg = Seg::Open {
                kind: SegKind::Text,
                entry_id: entry.id.clone(),
                buf: "não publicar após revogar".into(),
                flushed: Instant::now(),
                dirty: true,
                solta: false,
            };
            Some(entry)
        } else {
            None
        };
        let mut events = f.hub.events.subscribe();
        let mut call =
            Box::pin(crate::api::dispatch(&f.hub, &crate::api::devices::Caller::Local, "manageCall", f.body()));
        assert!(futures::poll!(call.as_mut()).is_pending());
        let command = commands.try_recv()?;
        assert!(matches!(&command, Cmd::Approve { execution: Some(original), .. } if original == &execution));
        let hub = f.hub.clone();
        let cfg = f.cfg.clone();
        let reached = (phase != "valid").then(|| {
            call_tests::interleave(&f.hub.store, phase, move || {
                let replacement = (|| -> Result<()> {
                    bindings::revoke(&hub.store, "lead", Some("approval-acp"))?;
                    let next = bindings::issue(&hub.store, "lead", "lead", "lead", None)?;
                    bindings::activate_for_config(&hub.store, &next, "approval-acp", &cfg)?;
                    assert_ne!(Some(next.id.as_str()), execution.acp_binding());
                    Ok(())
                })();
                assert!(replacement.is_ok(), "{replacement:?}");
            })
        });
        let (done, _) = mpsc::unbounded_channel();
        actor.on_cmd(command, &done).await;
        if let Some(reached) = reached {
            assert!(reached.load(Ordering::SeqCst));
            assert!(call.await.is_err());
            assert!(actor.perms.is_empty());
            assert!(f.hub.store.history("lead", i64::MAX, 50)?.iter().all(|e| e.kind != "permission"));
            assert_eq!(f.hub.runtime("lead").status, BotStatus::Working);
            while let Ok(event) = events.try_recv() {
                assert_ne!(event["type"], "entry", "sem publicação antes da guarda: {event}");
            }
            if let Some(segment) = segment {
                let stored = f.hub.store.entry(&segment.id).ok_or_else(|| anyhow!("segmento ausente"))?;
                assert_eq!(stored.data, segment.data);
                assert!(matches!(actor.seg, Seg::Open { dirty: true, .. }));
            }
        } else {
            assert_eq!(actor.perms.len(), 1);
            let card = f
                .hub
                .store
                .history("lead", i64::MAX, 50)?
                .into_iter()
                .find(|e| e.kind == "permission")
                .ok_or_else(|| anyhow!("cartão válido ausente"))?;
            assert!(f.hub.store.kv_prefix("donna.tools.card.").is_empty(), "pessoal não recebe selo contextual");
            actor.answer_permission(&card.id, Some("allow".into())).await?;
            let result = call.await;
            assert!(
                result.is_ok(),
                "manageCall após Allow ({phase}): {}",
                result.as_ref().err().map(|error| format!("{error:#}")).unwrap_or_default()
            );
            assert_eq!(f.hub.store.bot("dev")?.ok_or_else(|| anyhow!("destino ausente"))?.config.name, "Alterado");
        }
    }
    let f = Fixture::new()?;
    let mut actor = f.actor();
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    let (done, _) = mpsc::unbounded_channel();
    actor
        .on_cmd(Cmd::Approve { title: "Sem captura".into(), detail: String::new(), execution: None, reply }, &done)
        .await;
    assert!(matches!(answer.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Closed)));
    assert!(actor.perms.is_empty());
    assert!(f.hub.store.history("lead", i64::MAX, 50)?.iter().all(|e| e.kind != "permission"));
    Ok(())
}
