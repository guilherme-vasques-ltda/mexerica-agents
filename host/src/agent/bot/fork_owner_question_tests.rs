//! MB-1b D70: `ask_owner` no ator — origem do turno, pergunta nova, Precisa de você e reinício/limpeza.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::LockExt;
use crate::fork::sessoes::bindings;
use crate::store::Store;
use std::path::PathBuf;

struct Fixture {
    hub: Arc<Hub>,
    cfg: BotConfig,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-ask-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"fixture","cwd":dir,"notify":false}))
                .unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(
            store,
            "ask-owner".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        Self { hub, cfg, dir }
    }
    /// Turno vivo do próprio bot na conversa principal.
    fn turn(&self, actor: &mut Actor) -> crate::fork::sessoes::ExecutionRef {
        actor.turn = Some(1);
        bindings::fixture_execution(&self.hub, "bot")
    }
    fn end_turn(&self, actor: &mut Actor) {
        actor.turn = None;
        self.hub.set_runtime("bot", |r| {
            r.status = BotStatus::Idle;
            r.lane = None;
            r.clear_activity();
            r.started_at = None;
            r.memory_turn_token = None;
        });
    }
    fn cards(&self) -> Vec<Entry> {
        self.hub
            .store
            .history("bot", i64::MAX, 50)
            .unwrap()
            .into_iter()
            .filter(|e| e.data["card"] == "ownerQuestion")
            .collect()
    }
    fn state(&self, card: &Entry) -> Value {
        self.hub.store.entry(&card.id).unwrap().data["ownerQuestion"]["status"].clone()
    }
    fn shown(&self) -> Value {
        self.hub.bot_json(&self.hub.store.bot("bot").unwrap().unwrap())
    }
    fn revision(&self) -> i64 {
        self.hub.store.space_conversation(crate::fork::spaces::PERSONAL, "bot").unwrap().revision
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn recusa_pedido_entre_bots_rotina_e_execucao_ausente_sem_cartao() {
    let f = Fixture::new();
    let mut actor = new_actor(f.hub.clone(), f.cfg.clone());
    let execution = f.turn(&mut actor);
    assert!(actor.ask_owner("Sem execução?".into(), None).is_err());
    let (request, _answer) = crate::chat::team::BotRequest::for_test("pedido");
    actor.active_request = Some(request);
    let error = actor.ask_owner("Pergunta de Times?".into(), Some(&execution)).unwrap_err();
    assert!(error.to_string().contains("outro bot ou de uma equipe"));
    actor.active_request = None;
    actor.active_routine = Some("rotina".into());
    let error = actor.ask_owner("Pergunta de rotina?".into(), Some(&execution)).unwrap_err();
    assert!(error.to_string().contains("rotina"));
    assert!(f.cards().is_empty(), "nenhuma recusa grava cartão");
    assert_eq!(f.hub.runtime("bot").pending_questions, Vec::new());
}

#[tokio::test]
async fn pergunta_nova_retira_a_anterior_e_reinicio_ou_limpeza_retiram_a_pendente() {
    let f = Fixture::new();
    let mut actor = new_actor(f.hub.clone(), f.cfg.clone());
    let execution = f.turn(&mut actor);
    actor.ask_owner("Qual pasta?".into(), Some(&execution)).unwrap();
    actor.ask_owner("Qual ramo?".into(), Some(&execution)).unwrap();
    let cards = f.cards();
    assert_eq!(cards.len(), 2);
    let (first, second) = (&cards[0], &cards[1]);
    assert_eq!(f.state(first), "withdrawn", "pergunta nova na mesma conversa retira a anterior");
    assert_eq!(f.state(second), "pending");
    assert_eq!(second.data["ownerQuestion"]["chatId"], "bot");
    assert_eq!(second.data["ownerQuestion"]["sessionId"], "bot");
    assert_eq!(actor.sent, ["Qual pasta?", "Qual ramo?"], "a resposta final não repete a pergunta");

    // Sem turno, a pergunta pendente aparece como Precisa de você; o estado interno segue Livre.
    f.end_turn(&mut actor);
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Idle);
    let shown = f.shown();
    assert_eq!(shown["status"], "needsInput");
    assert_eq!(shown["pendingQuestions"][0]["entryId"], second.id.as_str());
    assert_eq!(shown["pendingQuestions"][0]["text"], "Qual ramo?");

    // Ajuste A: limpar a conversa (reset) retira.
    actor.context_control("bot", f.revision(), None, true).await.unwrap();
    assert_eq!(f.state(second), "withdrawn");
    assert_eq!(f.shown()["status"], "idle");
    assert_eq!(f.shown()["pendingQuestions"], json!([]));

    // Ajuste A: Nova sessão da principal (newSession) também retira.
    let execution = f.turn(&mut actor);
    actor.ask_owner("Continua?".into(), Some(&execution)).unwrap();
    let third = f.cards().pop().unwrap();
    f.end_turn(&mut actor);
    let autoridade = crate::fork::autoridade::Autoridade::local();
    actor.primary_new_session("bot", f.revision(), &autoridade).await.unwrap();
    assert_eq!(f.state(&third), "withdrawn");
    assert_eq!(f.hub.runtime("bot").pending_questions, Vec::new());
}

/// D539 P2: o mesmo bot conserva o perfil no domínio do dono, mas nunca o envia ao Canal.
#[tokio::test]
async fn owner_profile_is_absent_in_channel_and_present_in_owner_turns() {
    use crate::fork::sessoes::{origin::SessionOrigin, store as sessions};
    use codync_mem::store::{self as mem, New, Writer};
    let f = Fixture::new();
    f.hub
        .store
        .memory_write(|db, now, rev| {
            mem::add(
                db,
                Writer::User,
                New { bot: Some("bot"), text: "Prefere respostas curtas", room: None, evidence: &[] },
                now,
                rev,
            )
        })
        .unwrap();
    let mut actor = new_actor(f.hub.clone(), f.cfg.clone());
    let check = |prompt: &str| {
        assert!(prompt.contains("<owner-memory-profile>"));
        assert!(prompt.contains("Prefere respostas curtas"));
    };
    check(&actor.compose("pessoal", "Mensagem pessoal").await.unwrap());
    actor.active_routine = Some("rotina".into());
    check(&actor.compose("rotina", "Scheduled routine: teste\nMensagem").await.unwrap());
    actor.active_routine = None;
    let (request, _answer) = crate::chat::team::BotRequest::for_test("pedido");
    actor.active_request = Some(request);
    check(&actor.compose("pedido", "Request from outro bot\nMensagem").await.unwrap());
    actor.active_request = None;
    // Grupo no catálogo do dono é diferente do produtor GroupTurn, que representa Canal.
    let group = sessions::create(
        &f.hub.store.db.locked(),
        &sessions::NewSession {
            chat: "bot".into(),
            origin: SessionOrigin::Group,
            source_session: None,
            parent_session: None,
            title: "Grupo do dono".into(),
            runs_on: None,
        },
    )
    .unwrap();
    actor.lane = Lane { chat: "bot".into(), session: Some(group.id) };
    check(&actor.compose("grupo", "Mensagem do grupo").await.unwrap());
    let channel_lane = Lane::main("canal");
    let (reply, _answer) = tokio::sync::oneshot::channel();
    actor.active_group = Some(GroupTurn {
        lane: channel_lane.clone(),
        prompt: "[Channel: Sala]\nMensagem".into(),
        shown: vec![],
        epoch: f.hub.groups.reserve_epoch_for_test(&channel_lane),
        authority: None,
        reply,
    });
    actor.lane = channel_lane;
    f.hub.store.db.locked().execute("DELETE FROM donna_memory_profiles", []).unwrap();
    let prompt = actor.compose("canal", "[Channel: Sala]\nMensagem").await.unwrap();
    assert!(!prompt.contains("<owner-memory-profile>"), "perfil pessoal não chega ao Canal");
    assert!(!prompt.contains("Prefere respostas curtas"), "o conteúdo pessoal também não chega");
    let cached: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_memory_profiles", [], |r| r.get(0)).unwrap();
    assert_eq!(cached, 0, "Canal nem carrega/gera o perfil");
    actor.active_group = None;
    actor.lane = Lane::main("bot");
    let prompt = actor.compose("pessoal", "De volta ao dono").await.unwrap();
    check(&prompt);
    assert!(
        crate::fork::memory::profile::turn(&f.hub.store, &f.cfg, false).chars().count()
            <= codync_mem::profile::MAX_CHARS + 2
    );
}
