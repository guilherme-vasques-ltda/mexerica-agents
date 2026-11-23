//! 0I: validade do cartão na conversa e no ator que mantém a espera viva.
use crate::{
    LockExt,
    agent::bot::Cmd,
    hub::Hub,
    store::{Entry, Lane},
};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::Arc;

#[path = "permission_card/store.rs"]
mod store;

/// Fotografia imutável da espera; nunca reconstruída a partir da resposta do cliente.
/// Alterar conteúdo/revisão exige encerrar esta espera e publicar outro cartão, com outro ID.
pub(crate) struct PendingContext {
    card: Entry,
    session: Option<String>,
}

/// Recibo privado da única transição confirmada por esta espera; não é nova autoridade.
pub(crate) struct AnsweredContext(PendingContext);

impl AnsweredContext {
    pub(crate) fn check(
        &self,
        card: &Entry,
        bot: &str,
        lane: &Lane,
        turn: Option<i64>,
        session: Option<&str>,
    ) -> Result<()> {
        self.0.check_status(card, bot, lane, turn, session, "answered")
    }
}

impl PendingContext {
    pub(crate) fn capture(card: &Entry, session: Option<&str>) -> Self {
        Self { card: card.clone(), session: session.map(str::to_owned) }
    }

    pub(crate) fn check(
        &self,
        card: &Entry,
        bot: &str,
        lane: &Lane,
        turn: Option<i64>,
        session: Option<&str>,
    ) -> Result<()> {
        self.check_status(card, bot, lane, turn, session, "pending")
    }

    /// Conserva a revisão devolvida pela própria escrita, sem aceitar edição intermediária.
    pub(crate) fn answered(&self, card: &Entry, option: &str) -> Result<AnsweredContext> {
        let mut expected = self.card.data.clone();
        expected["status"] = json!("answered");
        expected["selected"] = json!(option);
        if card.id != self.card.id
            || card.seq != self.card.seq
            || card.bot_id != self.card.bot_id
            || card.session_id != self.card.session_id
            || card.kind != self.card.kind
            || card.turn != self.card.turn
            || card.created_at != self.card.created_at
            || card.rev <= self.card.rev
            || card.data != expected
        {
            bail!("A revisão deste cartão mudou. Atualize a conversa antes de responder.");
        }
        Ok(AnsweredContext(Self { card: card.clone(), session: self.session.clone() }))
    }

    fn check_status(
        &self,
        card: &Entry,
        bot: &str,
        lane: &Lane,
        turn: Option<i64>,
        session: Option<&str>,
        status: &str,
    ) -> Result<()> {
        if card.id != self.card.id
            || card.bot_id != lane.chat
            || card.bot_id != self.card.bot_id
            || card.session_id != self.card.session_id
            || card.session_id != lane.session.as_deref().unwrap_or(&lane.chat)
            || card.data["author"].as_str() != Some(bot)
            || card.data["author"] != self.card.data["author"]
        {
            bail!("Este cartão pertence a outro bot ou conversa. Abra a conversa correta.");
        }
        if turn != Some(self.card.turn) || card.turn != self.card.turn {
            bail!("O turno deste cartão mudou. Confira a conversa antes de responder.");
        }
        if session.is_none() || session != self.session.as_deref() {
            bail!("A sessão deste cartão mudou. Confira a conversa antes de responder.");
        }
        if card.kind != "permission" || card.data["status"] != status {
            bail!("Este cartão não está mais pendente. Confira a conversa antes de responder.");
        }
        if card.rev != self.card.rev || card.data != self.card.data {
            bail!("A revisão deste cartão mudou. Atualize a conversa antes de responder.");
        }
        Ok(())
    }
}

/// Antes inclusive dos atalhos sem cartão: uma sessão antiga não autoriza a atual.
pub(crate) fn request_session(params: &Value, session: Option<&str>) -> Result<()> {
    if session.is_none() || params["sessionId"].as_str() != session {
        bail!("O pedido de permissão pertence a outra sessão. Confira a conversa.");
    }
    Ok(())
}

/// Classifica apenas a apresentação; o nome não comprova a definição MCP efetiva.
pub(crate) fn memory_tool_request(params: &Value, entry: Option<&Entry>, turn: Option<i64>, chat: &str) -> bool {
    let tool = &params["toolCall"];
    params["_meta"]["is_mcp_tool_approval"] == true
        && tool["kind"] == "execute"
        && tool["rawInput"].is_null()
        && entry.is_some_and(|entry| {
            entry.kind == "tool"
                && Some(entry.turn) == turn
                && entry.bot_id == chat
                && entry.data["toolKind"] == "execute"
                && matches!(entry.data["status"].as_str(), Some("pending" | "in_progress"))
                && entry.data["hubToolOrigin"] == "internal"
                && entry.data["mcpServer"] == "memory"
        })
}

pub(crate) async fn respond(hub: &Arc<Hub>, body: &Value) -> Result<Value> {
    // CF-14: a origem ORIGINAL é capturada uma vez, antes de qualquer espera e dos atalhos sem ator;
    // segue para a fila e para o cancelamento sem ator, nunca recapturada.
    let origem = crate::fork::donna_fixa::pedido::atual();
    crate::fork::conectores::api::intercept(hub, body)?;
    let entry_id = body["entryId"].as_str().ok_or_else(|| anyhow!("Informe o cartão de permissão."))?;
    let chat =
        body["botId"].as_str().ok_or_else(|| anyhow!("Atualize o app para responder ao cartão com segurança."))?;
    let revision = body["entryRev"]
        .as_i64()
        .filter(|rev| *rev > 0)
        .ok_or_else(|| anyhow!("Atualize o app para responder ao cartão com a revisão atual."))?;
    let entry = hub.store.entry(entry_id).ok_or_else(|| anyhow!("Cartão não encontrado."))?;
    if chat != entry.bot_id {
        bail!("Este cartão pertence a outra conversa. Abra a conversa correta.");
    }
    let voice = match body.get("source") {
        None | Some(Value::Null) => false,
        Some(Value::String(source)) if source == "app" => false,
        Some(Value::String(source)) if source == "voice" => true,
        _ => bail!("A origem da resposta ao cartão é inválida. Responda pelo app."),
    };
    let option_id = match &body["optionId"] {
        Value::Null => None,
        Value::String(id) => Some(id.clone()),
        _ => bail!("Opção inválida: informe o identificador de uma opção do cartão."),
    };
    let refusal = option_id.as_deref().is_none_or(|id| {
        entry.data["options"].as_array().is_some_and(|options| {
            options.iter().any(|option| {
                option["optionId"] == id && matches!(option["kind"].as_str(), Some("reject_once" | "reject_always"))
            })
        })
    });
    // Confirmar uma resposta já aplicada não concede autorização nem chega ao motor (#43).
    if crate::agent::engine_permissions::repeated_answer(&entry, option_id.as_deref())
        || (!voice
            && entry.kind == "permission"
            && entry.data["status"] == "cancelled"
            && refusal
            && entry.data["selected"] == json!(option_id))
    {
        return Ok(json!({}));
    }
    if entry.kind != "permission" || entry.data["status"] != "pending" {
        bail!("Este cartão não está mais pendente. Confira a conversa antes de responder.");
    }
    if revision != entry.rev {
        bail!("A revisão deste cartão mudou. Atualize a conversa antes de responder.");
    }
    // D98: a voz não seleciona, nega nem cancela cartões que exigem o app.
    if voice && !crate::agent::bot::voice_answer_allowed(&entry.data, option_id.as_deref()) {
        bail!("Aprove no app");
    }
    let bot = entry.data["author"].as_str().ok_or_else(|| anyhow!("O bot deste cartão não foi identificado."))?;
    let Some(author) = hub.store.bot(bot)?.filter(|row| !row.deleted && !row.config.is_group()) else {
        if refusal {
            if entry.data["toolKind"] == "screen_access" && entry.data["screenAccess"].is_object() {
                return crate::screen::fork_permission::respond(hub, &entry, option_id.as_deref(), origem.as_ref());
            }
            return cancel_unavailable(hub, &entry, option_id.as_deref(), origem.as_ref());
        }
        bail!("O bot deste cartão não está mais disponível.");
    };
    // Retirar o membro revoga liberações; negar/cancelar ainda deve encerrar a espera.
    let context_conversation = hub
        .store
        .space_conversation(body["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL), chat)
        .ok()
        .filter(|c| c.bot_id == bot && !c.closed);
    if chat != author.config.id && context_conversation.is_none() && !refusal {
        let room = hub
            .store
            .bot(chat)?
            .filter(|row| !row.deleted && row.config.is_group())
            .ok_or_else(|| anyhow!("A sala deste cartão não está mais disponível."))?;
        if !room.config.members.iter().any(|member| member == bot) {
            bail!("O bot deste cartão não participa mais desta sala.");
        }
    }
    if entry.data["toolKind"] == "screen_access" && entry.data["screenAccess"].is_object() {
        return crate::screen::fork_permission::respond(hub, &entry, option_id.as_deref(), origem.as_ref());
    }
    if entry.data["restoredFrom"].is_string() {
        return answer_restored(hub, &entry, option_id.as_deref(), refusal, origem.as_ref());
    }
    let (reply, result) = tokio::sync::oneshot::channel();
    // CF-14: a autoridade original do pedido segue na fila até a Tx do cartão no ator.
    let cmd =
        Cmd::Permission { entry_id: entry_id.to_owned(), option_id: option_id.clone(), reply, origem: origem.clone() };
    if hub.send_cmd(bot, cmd).is_err() {
        if refusal {
            return cancel_unavailable(hub, &entry, option_id.as_deref(), origem.as_ref());
        }
        bail!("O bot deste cartão não está mais disponível.");
    }
    result.await.context("Não foi possível responder ao cartão. Tente novamente.")??;
    Ok(json!({}))
}

/// CF-14: grava a resposta na Tx da guarda da origem ORIGINAL (`emit_lock` → Cloud → DB), sem await;
/// recusa da guarda não grava. Evento só depois do commit. Fora de pedido (Hub interno): Tx comum.
pub(crate) fn commit(
    hub: &Hub,
    id: &str,
    data: &Value,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
) -> Result<Option<Entry>> {
    commit_checked(hub, id, data, execution, origem, |_| Ok(()))
}

/// `commit` com uma conferência síncrona na mesma `Connection` da guarda, antes de gravar.
fn commit_checked(
    hub: &Hub,
    id: &str,
    data: &Value,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
    check: impl FnOnce(&rusqlite::Connection) -> Result<()>,
) -> Result<Option<Entry>> {
    let _emit = hub.emit_lock.locked();
    let entry = commit_original(hub, id, origem, |c| {
        check(c)?;
        Ok((data.clone(), execution.cloned(), ()))
    })
    .map(|(entry, ())| entry);

    if let Err(error) = &entry {
        tracing::error!(error = format!("{error:#}"), entry = id, "couldn't update transcript entry");
    }
    if let Ok(Some(entry)) = &entry {
        let _ = hub.events.send(json!({"type": "entry", "rev": entry.rev, "entry": entry}));
    }
    entry
}

/// Corpo único do #567: a transição e seus dados usam a origem ORIGINAL até o commit.
/// O chamador segura emit; `extra` pode reter screen.control até a confirmação real.
pub(crate) fn commit_original<T>(
    hub: &Hub,
    id: &str,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
    transition: impl FnOnce(&rusqlite::Connection) -> Result<(Value, Option<crate::fork::sessoes::ExecutionRef>, T)>,
) -> Result<(Option<Entry>, T)> {
    crate::fork::donna_fixa::pedido::gravar_com(&hub.store, origem, |c| {
        let (data, execution, extra) = transition(c)?;
        let entry = crate::store::Store::update_entry_on(c, id, &data, execution.as_ref())?;
        Ok((entry, extra))
    })
}

/// Pergunta restaurada depois de um reinício: nenhum motor espera por ela. A resposta nova só é registrada; aprovar
/// não executa a ação (ela nunca rodou, o motor que pediu morreu no reinício) nem vira permissão para outro turno.
fn answer_restored(
    hub: &Hub,
    entry: &Entry,
    option_id: Option<&str>,
    refusal: bool,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
) -> Result<Value> {
    if refusal {
        return cancel_unavailable(hub, entry, option_id, origem);
    }
    let offered = option_id.is_some_and(|option| {
        entry.data["options"]
            .as_array()
            .is_some_and(|options| options.iter().any(|o| o["optionId"] == option && o["kind"] == "allow_once"))
    });
    let Some(option) = option_id.filter(|_| offered) else {
        tracing::warn!(entry = entry.id, option = ?option_id, "opção recusada no cartão restaurado");
        bail!("Opção inválida: escolha uma das opções disponíveis neste cartão.");
    };
    let mut data = entry.data.clone();
    data["status"] = json!("answered");
    data["selected"] = json!(option);
    let check = |c: &rusqlite::Connection| {
        let current = crate::store::Store::entry_on(c, &entry.id)?;
        if !current.is_some_and(|card| card.data["status"] == "pending" && card.rev == entry.rev) {
            bail!("A revisão deste cartão mudou. Atualize a conversa antes de responder.");
        }
        Ok(())
    };
    commit_checked(hub, &entry.id, &data, None, origem, check)?
        .ok_or_else(|| anyhow!("Não foi possível registrar a resposta. Tente novamente."))?;
    let lane = Lane {
        chat: entry.bot_id.clone(),
        session: (entry.session_id != entry.bot_id).then(|| entry.session_id.clone()),
    };
    hub.add_entry_once(
        &format!("{}-registrada", entry.id),
        &lane,
        crate::store::EntryKind::Notice,
        entry.turn,
        &json!({"text":"Aprovação registrada. Como o Hub reiniciou enquanto esta pergunta esperava, a ação não foi executada. Peça de novo ao bot se ainda quiser que ele a faça.","style":"info","author":entry.data["author"]}),
    )?;
    Ok(json!({}))
}

/// Sem ator para receber a recusa, encerra só o cartão salvo e publica a nova revisão ao app.
/// CF-14: grava na guarda da origem ORIGINAL; recusa ou cartão alterado não grava nem publica.
fn cancel_unavailable(
    hub: &Hub,
    entry: &Entry,
    option_id: Option<&str>,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
) -> Result<Value> {
    let mut data = entry.data.clone();
    data["status"] = json!("cancelled");
    data["selected"] = json!(option_id);
    let check = |c: &rusqlite::Connection| {
        let current = crate::store::Store::entry_on(c, &entry.id)?;
        let pending = |card: &Entry| card.kind == "permission" && card.data["status"] == "pending";
        if !current.is_some_and(|card| pending(&card) && card.rev == entry.rev) {
            bail!("A revisão deste cartão mudou. Atualize a conversa antes de responder.");
        }
        Ok(())
    };
    commit_checked(hub, &entry.id, &data, None, origem, check)?
        .ok_or_else(|| anyhow!("Não foi possível encerrar o cartão. Tente novamente."))?;
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{EntryKind, Store};

    #[test]
    fn espera_recusa_sessao_trocada_e_sessao_ausente() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let lane = Lane::main("bot");
        let card =
            store.insert_entry(&lane, EntryKind::Permission, 1, &json!({"author":"bot", "status":"pending"})).unwrap();
        let pending = PendingContext::capture(&card, Some("sessao-original"));
        assert!(pending.check(&card, "bot", &lane, Some(1), Some("sessao-original")).is_ok());
        for session in [Some("sessao-nova"), None] {
            assert!(pending.check(&card, "bot", &lane, Some(1), session).unwrap_err().to_string().contains("sessão"));
        }
    }

    #[test]
    fn espera_recusa_turno_vivo_trocado_e_ausente() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let lane = Lane::main("bot");
        let card =
            store.insert_entry(&lane, EntryKind::Permission, 1, &json!({"author":"bot", "status":"pending"})).unwrap();
        let pending = PendingContext::capture(&card, Some("sessao"));
        for turn in [Some(2), None] {
            assert!(
                pending.check(&card, "bot", &lane, turn, Some("sessao")).unwrap_err().to_string().contains("turno")
            );
        }
    }

    #[test]
    fn resposta_confere_transicao_propria_e_recusa_revisao_ou_conteudo_posterior() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let lane = Lane::main("bot");
        let card = store
            .insert_entry(&lane, EntryKind::Permission, 1, &json!({"author":"bot","status":"pending","selected":null}))
            .unwrap();
        let pending = PendingContext::capture(&card, Some("sessao"));
        let mut data = card.data.clone();
        data["status"] = json!("answered");
        data["selected"] = json!("allow");
        let confirmed = store.update_entry(&card.id, &data).unwrap().unwrap();
        assert!(pending.check(&confirmed, "bot", &lane, Some(1), Some("sessao")).is_err());
        let answered = pending.answered(&confirmed, "allow").unwrap();
        assert!(answered.check(&confirmed, "bot", &lane, Some(1), Some("sessao")).is_ok());
        assert!(pending.answered(&confirmed, "deny").is_err());
        let later = store.update_entry(&card.id, &data).unwrap().unwrap();
        assert!(answered.check(&later, "bot", &lane, Some(1), Some("sessao")).is_err());
        let mut changed = confirmed.clone();
        changed.data["author"] = json!("outro");
        assert!(answered.check(&changed, "bot", &lane, Some(1), Some("sessao")).is_err());
        assert!(pending.answered(&changed, "allow").is_err());
        changed = confirmed.clone();
        changed.data["selected"] = json!("deny");
        assert!(answered.check(&changed, "bot", &lane, Some(1), Some("sessao")).is_err());
        assert!(pending.answered(&changed, "allow").is_err());
    }

    #[tokio::test]
    async fn autor_sem_runtime_recusa_allow_e_publica_cancelamento_idempotente() {
        let dir = std::env::temp_dir().join(format!("donnabot-card-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let bot = serde_json::from_value(json!({"id":"bot", "name":"Bot", "backend":"custom"})).unwrap();
        store.save_bot(&bot).unwrap();
        let hub = Hub::new(
            store,
            "card-test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            0,
        );
        // Não inicia o ator: o autor existe no store, mas send_cmd não tem destinatário.
        let mut events = hub.events.subscribe();
        for option in [Value::Null, json!("deny")] {
            let card = hub
                .store
                .insert_entry(
                    &Lane::main("bot"),
                    EntryKind::Permission,
                    1,
                    &json!({"author":"bot", "status":"pending", "options":[
                        {"optionId":"allow", "kind":"allow_once"}, {"optionId":"deny", "kind":"reject_once"}
                    ]}),
                )
                .unwrap();
            let mut body = json!({"botId":"bot", "entryId":card.id, "entryRev":card.rev, "optionId":"allow"});
            assert_eq!(
                respond(&hub, &body).await.unwrap_err().to_string(),
                "O bot deste cartão não está mais disponível."
            );
            assert_eq!(hub.store.entry(&card.id).unwrap().data["status"], "pending");
            body["optionId"] = option.clone();
            respond(&hub, &body).await.unwrap();
            let closed = hub.store.entry(&card.id).unwrap();
            assert!(closed.rev > card.rev);
            assert_eq!(closed.data["status"], "cancelled");
            assert_eq!(closed.data["selected"], option);
            let event = events.try_recv().unwrap();
            assert_eq!(event["type"], "entry");
            assert_eq!(event["rev"], closed.rev);
            assert_eq!(event["entry"]["data"], closed.data);
            respond(&hub, &body).await.unwrap();
            assert_eq!(hub.store.entry(&card.id).unwrap().rev, closed.rev);
            assert!(events.try_recv().is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

fn informative_title(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    !lower.is_empty() && !matches!(lower.as_str(), "use a tool" | "tool" | "run a tool" | "run tool" | "use tool")
}

/// Identifica o pedido pelo que o motor informou; campos ausentes não são adivinhados.
pub fn action_title(tool: &Value, recorded: Option<&Value>) -> String {
    let raw = &tool["rawInput"];
    let action = tool["name"]
        .as_str()
        .or_else(|| raw["tool"].as_str())
        .or_else(|| recorded.and_then(|data| data["title"].as_str()).filter(|s| s.starts_with("mcp.")))
        .filter(|s| informative_title(s))
        .unwrap_or(match tool["kind"].as_str() {
            Some("edit") => "editar arquivo",
            Some("execute") => "executar ferramenta",
            Some("read") => "ler arquivo",
            Some("delete") => "apagar",
            _ => "usar ferramenta não identificada",
        });
    let command =
        raw["command"].as_array().map(|parts| parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "));
    // O alvo é só apresentação: primeiro o que o adaptador alegou como servidor
    // (`mcpServerClaimed`/`mcpServer`), nunca os campos `server`/`serverName` dos
    // argumentos — esses ficam como último recurso e não provam origem. Quando o
    // alvo é um servidor, o cartão só o mostra como verificado se a origem gravada
    // for `internal`; qualquer outra alegação sai marcada como não verificada.
    let target = [
        raw["command"].as_str().map(|s| (s, false)),
        command.as_deref().map(|s| (s, false)),
        raw["path"].as_str().map(|s| (s, false)),
        raw["file_path"].as_str().map(|s| (s, false)),
        tool["locations"][0]["path"].as_str().map(|s| (s, false)),
        recorded.and_then(|data| data["mcpServerClaimed"].as_str()).map(|s| (s, true)),
        recorded.and_then(|data| data["mcpServer"].as_str()).map(|s| (s, true)),
        recorded.and_then(|data| data["locations"][0]["path"].as_str()).map(|s| (s, false)),
        raw["serverName"].as_str().map(|s| (s, true)),
        raw["server"].as_str().map(|s| (s, true)),
        tool["title"].as_str().filter(|s| informative_title(s)).map(|s| (s, false)),
        recorded.and_then(|data| data["title"].as_str()).filter(|s| informative_title(s)).map(|s| (s, false)),
    ]
    .into_iter()
    .flatten()
    .find(|(s, _)| !s.trim().is_empty());
    let (target, server_claim) = target.unwrap_or(("alvo não informado", false));
    let verified = server_claim && recorded.is_some_and(|data| data["hubToolOrigin"] == "internal");
    let suffix = if server_claim && !verified { " (servidor não verificado)" } else { "" };
    format!("Autorizar {action}: {target}{suffix}")
}

#[cfg(test)]
mod title_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn titulo_identifica_acao_e_alvo_sem_ingles_generico() {
        assert_eq!(
            action_title(&json!({"kind":"edit", "rawInput":{"file_path":"/tmp/teste"}}), None),
            "Autorizar editar arquivo: /tmp/teste"
        );
        assert_eq!(
            action_title(&json!({"kind":"execute", "rawInput":{"command":["rm", "arquivo"]}}), None),
            "Autorizar executar ferramenta: rm arquivo"
        );
        assert_eq!(
            action_title(&json!({"title":"Use a tool"}), None),
            "Autorizar usar ferramenta não identificada: alvo não informado"
        );
        // Servidor com origem internal verificada sai sem marca.
        assert_eq!(
            action_title(
                &json!({"kind":"execute"}),
                Some(&json!({"title":"mcp.memory.test", "mcpServer":"memory", "hubToolOrigin":"internal"}))
            ),
            "Autorizar mcp.memory.test: memory"
        );
        // O rótulo alegado pelo adaptador vem antes dos campos de argumento —
        // mas sem origem internal o servidor sai marcado como não verificado.
        assert_eq!(
            action_title(
                &json!({"kind":"execute", "rawInput":{"server":"manage", "serverName":"email"}}),
                Some(&json!({"title":"mcp.memory.test", "mcpServerClaimed":"memory", "hubToolOrigin":"unknown"}))
            ),
            "Autorizar mcp.memory.test: memory (servidor não verificado)"
        );
        // Prova de identidade sem origem internal também não é "verificada".
        assert_eq!(
            action_title(
                &json!({"kind":"execute"}),
                Some(&json!({"title":"mcp.external.test", "mcpServer":"external", "hubToolOrigin":"unknown"}))
            ),
            "Autorizar mcp.external.test: external (servidor não verificado)"
        );
        // Sem registro algum, o servidor dos argumentos também sai marcado.
        assert_eq!(
            action_title(&json!({"kind":"execute", "rawInput":{"server":"manage"}}), None),
            "Autorizar executar ferramenta: manage (servidor não verificado)"
        );
    }
}
