//! DON-907 C9a: a troca do motor ou do modelo da conversa principal (texto leigo da linha: D375).
//!
//! Um gancho só, `engine_switched`, chamado onde o ator descobre que o motor da conversa mudou. Com a D372 (`UI01b`),
//! a principal do Pessoal tem a própria seleção: o ponto da troca é o início do turno humano novo que promove a
//! seleção nova do dono (`turn.rs`, `turn_config` → `engine_switched_on_turn`). Editar o padrão do bot
//! (`queue.rs`, `Cmd::Reconfigure`) só troca a conversa que não tem seleção própria (retrato ilegível).
//! - Mesmo provedor (só o modelo): o Hub retoma a sessão inteira; a linha diz `carried: "session"`.
//! - Outro provedor: a sessão do motor antigo não serve ao novo. A IA nova recebe a passagem dos rollovers
//!   (`chat::handoff`, motivo `EngineSwitch`), que o usuário vê e pode editar, mas não recusar.
//! - Um pedido esperando o limite (D72) já leva a passagem ao ser retomado (`switch_waiting_model`): só a linha.
//!
//! A linha é uma entrada de aviso com `modelSwitch {from, to, kind, carried, resentTokens}`; `resentTokens` fica
//! `null` até o primeiro `usage_update` da sessão nova (`engine_switch_usage`). Só a conversa principal.

use super::Actor;
use crate::chat::handoff;
use crate::store::{BotConfig, EntryKind, Lane};
use serde_json::{Value, json};

/// A linha de troca que espera o primeiro uso da sessão nova.
fn pending_key(bot: &str) -> String {
    format!("modelSwitch.pending.{bot}")
}

/// A passagem de uma troca feita durante um turno, montada quando ele acaba.
fn later_key(bot: &str) -> String {
    format!("modelSwitch.handoff.{bot}")
}

/// O cartão pendente é o da troca (a passagem que a mensagem deste turno leva).
pub(super) fn pending_switch(store: &crate::store::Store, bot: &str) -> bool {
    handoff::pending(store, bot).is_some_and(|(id, _)| {
        store.entry(&id).is_some_and(|e| e.data["handoff"]["reason"] == json!(handoff::Reason::EngineSwitch))
    })
}

/// `provider` quando muda o motor, `model` quando muda só o modelo; `None` sem troca.
pub(super) fn switch_kind(old: &BotConfig, new: &BotConfig) -> Option<&'static str> {
    if old.backend != new.backend {
        Some("provider")
    } else if old.model != new.model {
        Some("model")
    } else {
        None
    }
}

/// O nome que a linha mostra: o motor e, quando escolhido, o modelo.
fn label(cfg: &BotConfig) -> String {
    let engine = crate::agent::backends::harness(&cfg.backend).map_or(cfg.backend.as_str(), |h| h.name);
    match cfg.model.as_deref().filter(|m| !m.is_empty()) {
        Some(model) => format!("{engine} {model}"),
        None => engine.to_owned(),
    }
}

fn engine(cfg: &BotConfig) -> Value {
    json!({"backend": cfg.backend, "model": cfg.model, "label": label(cfg)})
}

impl Actor {
    /// O motor ou o modelo da conversa principal vai mudar de `self.cfg` para `new`. Chame antes de esquecer a
    /// sessão antiga: a passagem lê o resumo dela. `waiting`: há um pedido esperando o limite, que leva a passagem.
    pub(super) fn engine_switched(&mut self, new: &BotConfig, waiting: bool) {
        // Sem conversa no motor antigo, não há o que passar.
        let Some(session) = self.session_id.clone().or_else(|| self.turn_session.clone()) else { return };
        self.switch_line(new, waiting, Some(&session));
    }

    /// D372: a seleção nova da principal, promovida no início de um turno humano novo (`turn_config`), antes de
    /// fechar a conexão antiga. Durante a chamada, `self.cfg` é a seleção efetiva da principal antes da promoção.
    /// A mensagem que abre o turno já é a "próxima": entre provedores, a passagem vai nela. Se a fila já tirou para
    /// ela um cartão pendente (rascunho de rollover, talvez editado), esse cartão vira o da troca e é a passagem: só
    /// a linha.
    pub(super) fn engine_switched_on_turn(&mut self, new: &BotConfig, lane: &Lane) {
        if self.turn_handoff(lane).is_some() {
            self.sent_card_is_switch();
            self.switch_line(new, true, None);
            return;
        }
        // A sessão da principal (slot principal), nunca `turn_session`, que pode ser da última extra/rotina (R7).
        let Some(session) = self.session_id.clone() else { return };
        self.switch_line(new, false, Some(&session));
        if switch_kind(&self.cfg, new) == Some("provider") && pending_switch(&self.hub.store, &self.cfg.id) {
            self.handoff = self.take_handoff();
            self.handoff_chat = Some(lane.clone());
        }
    }

    /// O cartão que a fila acabou de enviar com esta mensagem passa a ser o da troca (sem "Manter esta conversa"):
    /// pelo ID exato guardado por `take_handoff`, e só se for o cartão desta passagem (mesmo texto, já enviado).
    fn sent_card_is_switch(&mut self) {
        let Some(id) = self.handoff_card.as_deref() else { return };
        let Some(mut card) = self.hub.store.entry(id) else { return };
        let sent = card.data["handoff"]["status"] == json!(handoff::Status::Sent);
        if sent && card.data["handoff"]["text"].as_str() == self.handoff.as_deref() {
            card.data["handoff"]["reason"] = json!(handoff::Reason::EngineSwitch);
            self.hub.set_entry(id, &card.data);
        }
    }

    /// A linha de troca e, entre provedores, o cartão da passagem de `session`. `waiting`: a passagem já vai por outro
    /// caminho (pedido esperando o limite, ou cartão levado pela mensagem deste turno); só a linha.
    fn switch_line(&mut self, new: &BotConfig, waiting: bool, session: Option<&str>) {
        if self.cfg.context.is_some() {
            return;
        }
        let Some(kind) = switch_kind(&self.cfg, new) else { return };
        let carried = if waiting || kind == "provider" { "handoff" } else { "session" };
        let (from, to) = (label(&self.cfg), label(new));
        let data = json!({
            "text": format!("Modelo trocado de {from} para {to}. A conversa foi passada para o novo modelo."),
            "style": "info",
            "author": self.cfg.id,
            "modelSwitch": {"from": engine(&self.cfg), "to": engine(new), "kind": kind, "carried": carried, "resentTokens": null},
        });
        let main = Lane::main(&self.cfg.id);
        let turn = self.hub.store.max_turn(&self.cfg.id);
        if let Some(posted) = self.hub.add_entry(&main, EntryKind::Notice, turn, &data) {
            if let Err(error) = self.hub.store.kv_set(&pending_key(&self.cfg.id), &posted.id) {
                tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't remember the switch line");
            }
        } else {
            tracing::warn!(bot = %self.cfg.id, "couldn't post the switch line");
        }
        // O pedido esperando o limite já leva a passagem (`switch_waiting_model`).
        if kind == "provider"
            && !waiting
            && let Some(session) = session
        {
            let says = format!(
                "O motor mudou de {from} para {to}. Sua próxima mensagem começa no motor novo levando este resumo."
            );
            if self.turn.is_some() {
                // A resposta em curso entra na passagem: ela é montada quando o turno acaba (`engine_switch_after_turn`).
                let later = json!({"session": session, "says": says}).to_string();
                if let Err(error) = self.hub.store.kv_set(&later_key(&self.cfg.id), &later) {
                    tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't defer the switch handoff");
                }
            } else {
                self.engine_switch_handoff(session, &says);
            }
        }
    }

    /// Depois do turno que estava em curso na troca, antes do próximo envio (`next_in_queue`): a passagem adiada.
    pub(super) fn engine_switch_after_turn(&mut self) {
        let key = later_key(&self.cfg.id);
        let Some(later) = self.hub.store.kv_get(&key) else { return };
        let _ = self.hub.store.kv_delete(&key);
        let later: Value = serde_json::from_str(&later).unwrap_or_default();
        if let (Some(session), Some(says)) = (later["session"].as_str(), later["says"].as_str()) {
            self.engine_switch_handoff(session, says);
        }
    }

    /// O cartão da passagem. Um rascunho pendente (rollover) vira o da troca, com o texto que o usuário já editou:
    /// a conversa antiga é de outro motor, então ele não pode mais "manter a conversa".
    pub(super) fn engine_switch_handoff(&mut self, session: &str, says: &str) {
        if let Some((id, _)) = handoff::pending(&self.hub.store, &self.cfg.id) {
            // Sessões PR5: o cartão do recomeço já leva a passagem e não tem "Manter"; continua o mesmo cartão.
            if handoff::is_pr5_card(&self.hub.store, &id) {
                return;
            }
            if let Some(mut entry) = self.hub.store.entry(&id) {
                entry.data["handoff"]["reason"] = json!(handoff::Reason::EngineSwitch);
                entry.data["text"] = says.into();
                self.hub.set_entry(&id, &entry.data);
            }
            return;
        }
        if let Err(error) = handoff::draft_saying(&self.hub, &self.cfg, session, handoff::Reason::EngineSwitch, says) {
            tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't prepare the engine switch handoff");
        }
    }

    /// O primeiro uso de uma sessão (`track_usage`) na principal: a linha de troca que esperava ganha os tokens
    /// que a sessão nova leu.
    pub(super) fn engine_switch_usage(&mut self, used: u64) {
        // Só a sessão principal: rotinas usam a mesma via com sessão ACP própria; grupos e pedidos, outra via.
        // Com o reinício pendente (troca durante um turno), o uso ainda é do modelo antigo (R2-1).
        if self.restart_after_turn
            || self.cfg.context.is_some()
            || self.lane != Lane::main(&self.cfg.id)
            || self.turn_session.is_none()
            || self.turn_session != self.session_id
        {
            return;
        }
        let key = pending_key(&self.cfg.id);
        let Some(id) = self.hub.store.kv_get(&key).filter(|id| !id.is_empty()) else { return };
        let _ = self.hub.store.kv_delete(&key);
        if let Some(mut entry) = self.hub.store.entry(&id) {
            entry.data["modelSwitch"]["resentTokens"] = json!(used);
            self.hub.set_entry(&id, &entry.data);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::{Actor, Cmd, new_actor};
    use crate::api::devices::Caller;
    use crate::chat::handoff;
    use crate::hub::Hub;
    use crate::store::{BotConfig, EntryKind, Lane, Store};
    use serde_json::{Value, json};
    use std::sync::Arc;

    struct Fixture {
        hub: Arc<Hub>,
        dir: std::path::PathBuf,
    }

    fn account(letter: char) -> String {
        format!("ac_{}", letter.to_string().repeat(64))
    }

    impl Fixture {
        /// Bot do Pessoal no Claude. `managed`: com a conta Claude `a` e a conta Codex `b` cadastradas (padrão `a`);
        /// sem ela, legado sem conta. A principal nasce com o retrato do padrão (D372).
        fn new(model: Option<&str>, managed: bool) -> Self {
            let dir = std::env::temp_dir().join(format!("donnabot-troca-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let store = Store::open(&dir.join("test.db")).unwrap();
            let mut cfg = json!({
                "id": "bot", "name": "Bot", "backend": "claude", "model": model, "cwd": dir, "notify": false,
            });
            if managed {
                cfg["forkEditor"] = json!({"engineAccountId": account('a')});
                let conta = |letter: char, backend: &str| {
                    json!({"id":account(letter),"backend":backend,"billing":"paid_api","state":"connected",
                        "launch":{"kind":"vault_profile","slot":format!("PRIVATE_{letter}")},"scope":"owner",
                        "quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[]})
                };
                let registry = json!({"salt": uuid::Uuid::new_v4().to_string(),
                    "accounts": [conta('a', "claude"), conta('b', "codex")]});
                store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
            }
            store.save_bot(&serde_json::from_value(cfg).unwrap()).unwrap();
            store
                .insert_entry(
                    &Lane::main("bot"),
                    EntryKind::User,
                    1,
                    &json!({"text":"SENTINELA pedido em curso","status":"sent"}),
                )
                .unwrap();
            let hub = Hub::new(
                store,
                "test".into(),
                crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
                "test".into(),
                19422,
            );
            Self { hub, dir }
        }

        /// `sessionEngineSet` real na principal (vale no próximo turno humano novo).
        async fn choose(&self, account: Option<&str>, backend: &str, model: Option<&str>, options: &Value) {
            let listed = crate::api::dispatch(&self.hub, &Caller::Local, "sessions", json!({"botId":"bot"})).await;
            let revision = listed.unwrap()["sessions"][0]["revision"].clone();
            let engine = json!({"accountId":account,"backend":backend,"model":model,"modelOptions":options,
                "fastMode":false});
            let body = json!({"botId":"bot","sessionId":null,"expectedRevision":revision,"engine":engine});
            crate::api::dispatch(&self.hub, &Caller::Local, "sessionEngineSet", body).await.unwrap();
        }

        /// O início de um turno humano novo na principal (`turn_config`, o mesmo de `start_prepared_turn`, que promove
        /// a seleção e chama a troca); depois o ator adota a configuração, como `start_prepared_turn` faz.
        fn new_turn(&self, actor: &mut Actor, text: &str) -> bool {
            let main = Lane::main("bot");
            let turn = self.hub.store.alloc_turn("bot").unwrap();
            let data = json!({"text":text,"status":"queued"});
            let entry = self.hub.store.insert_entry(&main, EntryKind::User, turn, &data).unwrap();
            let (cfg, promoted) = actor.turn_config(&main, &[entry.id], false, None).unwrap();
            actor.cfg = cfg;
            promoted
        }

        fn cfg(&self) -> BotConfig {
            self.hub.store.bot("bot").unwrap().unwrap().config
        }

        fn notices(&self) -> Vec<Value> {
            let entries = self.hub.store.history_in_session("bot", Some("bot"), i64::MAX, 50).unwrap();
            entries.into_iter().filter(|e| e.kind == EntryKind::Notice.as_str()).map(|e| e.data).collect()
        }

        fn switch_lines(&self) -> Vec<Value> {
            self.notices().into_iter().filter(|d| d["modelSwitch"].is_object()).collect()
        }

        fn cards(&self) -> Vec<Value> {
            self.notices().into_iter().filter(|d| d["handoff"].is_object()).collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[tokio::test]
    async fn troca_de_provedor_cria_a_passagem_que_a_proxima_mensagem_leva_ao_motor_novo() {
        let f = Fixture::new(None, true);
        let mut actor = new_actor(f.hub.clone(), f.cfg());
        actor.session_id = Some("sessao-claude".into());
        f.choose(Some(account('b').as_str()), "codex", None, &json!({})).await;
        assert!(f.switch_lines().is_empty(), "a troca da conversa vale no próximo turno");
        assert!(f.new_turn(&mut actor, "próxima"));

        let lines = f.switch_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0]["text"],
            "Modelo trocado de Claude Code para Codex. A conversa foi passada para o novo modelo."
        );
        let switch = &lines[0]["modelSwitch"];
        assert_eq!((switch["kind"].as_str(), switch["carried"].as_str()), (Some("provider"), Some("handoff")));
        assert_eq!(
            (switch["from"]["backend"].as_str(), switch["to"]["backend"].as_str()),
            (Some("claude"), Some("codex"))
        );
        assert!(switch["resentTokens"].is_null(), "até o primeiro usage_update");
        let cards = f.cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["handoff"]["reason"], "engineSwitch");
        assert!(actor.session_id.is_none(), "a sessão do Claude não serve ao Codex");

        // A mensagem que abriu o turno leva a passagem (`take_handoff`, como nos rollovers).
        let carried = actor.handoff.clone().unwrap();
        assert!(carried.contains("SENTINELA pedido em curso"), "{carried}");
        assert_eq!(actor.handoff_chat, Some(Lane::main("bot")));
        assert_eq!(f.cards()[0]["handoff"]["status"], "sent");

        // Uma rotina (mesma via, sessão ACP própria) não completa a linha (R1-3).
        actor.session_id = Some("sessao-codex".into());
        actor.turn_session = Some("rotina-1".into());
        actor.engine_switch_usage(777);
        assert!(f.switch_lines()[0]["modelSwitch"]["resentTokens"].is_null());
        // Com o motor antigo ainda por reiniciar (troca durante um turno), o uso é do antigo: não completa (R2-1).
        actor.turn_session = Some("sessao-codex".into());
        actor.restart_after_turn = true;
        actor.engine_switch_usage(555);
        assert!(f.switch_lines()[0]["modelSwitch"]["resentTokens"].is_null());
        actor.restart_after_turn = false;
        // O primeiro uso da sessão principal nova completa a linha, uma vez.
        actor.engine_switch_usage(12_345);
        actor.engine_switch_usage(99);
        assert_eq!(f.switch_lines()[0]["modelSwitch"]["resentTokens"], 12_345);
    }

    #[tokio::test]
    async fn troca_de_modelo_no_mesmo_provedor_retoma_a_sessao_sem_passagem() {
        let f = Fixture::new(Some("opus"), false);
        let mut actor = new_actor(f.hub.clone(), f.cfg());
        actor.session_id = Some("sessao-claude".into());
        f.choose(None, "claude", Some("haiku"), &json!({})).await;
        assert!(f.new_turn(&mut actor, "próxima"));

        let lines = f.switch_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0]["text"],
            "Modelo trocado de Claude Code opus para Claude Code haiku. A conversa foi passada para o novo modelo."
        );
        assert_eq!(
            (lines[0]["modelSwitch"]["kind"].as_str(), lines[0]["modelSwitch"]["carried"].as_str()),
            (Some("model"), Some("session"))
        );
        assert!(f.cards().is_empty() && handoff::pending(&f.hub.store, "bot").is_none());
        assert_eq!(actor.session_id.as_deref(), Some("sessao-claude"), "a sessão continua");

        // Sem troca de modelo (só o esforço, por exemplo), nenhuma linha.
        f.choose(None, "claude", Some("haiku"), &json!({"effort":"high"})).await;
        assert!(f.new_turn(&mut actor, "outra"));
        assert_eq!(f.switch_lines().len(), 1);
    }

    #[tokio::test]
    async fn rascunho_pendente_vira_o_da_troca_sem_duplicar_e_nao_pode_ser_recusado() {
        let f = Fixture::new(None, true);
        handoff::draft(&f.hub, &f.cfg(), "sessao-claude", handoff::Reason::Full).unwrap();
        let (id, _) = handoff::pending(&f.hub.store, "bot").unwrap();
        let local = crate::fork::donna_fixa::pedido::Origem::Local;
        let edit = json!({"entryId": id, "text": "SENTINELA editado pelo usuário"});
        handoff::call_as(&f.hub, "editHandoff", &edit, &local).unwrap();
        let mut actor = new_actor(f.hub.clone(), f.cfg());
        actor.session_id = Some("sessao-claude".into());
        f.choose(Some(account('b').as_str()), "codex", None, &json!({})).await;
        // A fila tira o cartão pendente para a mensagem antes de abrir o turno (`next_in_queue`).
        actor.handoff = actor.take_handoff();
        actor.handoff_chat = Some(Lane::main("bot"));
        assert!(f.new_turn(&mut actor, "próxima"));
        let cards = f.cards();
        assert_eq!(cards.len(), 1, "o rascunho pendente vira o da troca");
        assert_eq!(cards[0]["handoff"]["reason"], "engineSwitch");
        assert_eq!(cards[0]["handoff"]["text"], "SENTINELA editado pelo usuário");
        assert_eq!(f.switch_lines().len(), 1);
        // Manter a conversa antiga não existe na troca (R1-1): o Hub recusa e a mensagem leva a passagem.
        assert!(handoff::call_as(&f.hub, "keepConversation", &json!({"entryId": id}), &local).is_err());
        assert_eq!(actor.handoff.as_deref(), Some("SENTINELA editado pelo usuário"));
    }

    #[tokio::test]
    async fn troca_durante_um_turno_monta_a_passagem_depois_dele_com_a_resposta() {
        let f = Fixture::new(None, true);
        let mut actor = new_actor(f.hub.clone(), f.cfg());
        actor.session_id = Some("sessao-claude".into());
        actor.turn = Some(1);
        f.choose(Some(account('b').as_str()), "codex", None, &json!({})).await;
        assert!(f.switch_lines().is_empty() && f.cards().is_empty(), "a resposta em curso ainda não terminou");
        // A resposta termina; o turno seguinte monta a passagem com ela antes de abrir o motor novo (R1-2).
        f.hub
            .store
            .insert_entry(
                &Lane::main("bot"),
                EntryKind::Agent,
                1,
                &json!({"text":"SENTINELA resposta final","final":true}),
            )
            .unwrap();
        actor.turn = None;
        assert!(f.new_turn(&mut actor, "próxima"));
        assert_eq!(f.switch_lines().len(), 1);
        let cards = f.cards();
        assert_eq!(cards.len(), 1);
        assert!(cards[0]["handoff"]["text"].as_str().unwrap().contains("SENTINELA resposta final"), "{cards:?}");
        assert!(!f.new_turn(&mut actor, "outra"), "nada novo a promover");
        assert_eq!(f.cards().len(), 1, "uma vez só");
        assert_eq!(f.switch_lines().len(), 1);
    }

    /// D372: editar o padrão do bot não troca a conversa principal que tem retrato próprio: nem linha, nem passagem,
    /// e o próximo turno continua no motor dela.
    #[tokio::test]
    async fn editar_o_padrao_nao_troca_a_conversa_principal() {
        let f = Fixture::new(None, true);
        let mut actor = new_actor(f.hub.clone(), f.cfg());
        actor.session_id = Some("sessao-claude".into());
        let mut codex = f.cfg();
        codex.backend = "codex".into();
        codex.fork_editor.engine_account_id = Some(account('b'));
        f.hub.store.save_bot(&codex).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        actor.on_cmd(Cmd::Reconfigure(Box::new(codex)), &tx).await;
        assert!(f.switch_lines().is_empty() && f.cards().is_empty());
        assert_eq!(actor.session_id.as_deref(), Some("sessao-claude"), "a sessão da conversa continua");
        assert!(!f.new_turn(&mut actor, "próxima"));
        assert_eq!(actor.cfg.backend, "claude");
        assert_eq!(actor.cfg.fork_editor.engine_account_id, Some(account('a')));
        assert!(f.switch_lines().is_empty() && f.cards().is_empty());
    }
}
