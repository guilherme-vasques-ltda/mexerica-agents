//! RPCs do app para o `WhatsApp`. Só o dono, como o editor de bot; nenhuma
//! ferramenta do bot/motor chega aqui.
use super::{
    messages,
    number::{self, State},
    store::{self, BotPatch, HubPatch},
    wacli::{self, Method},
};
use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

/// Ajustes (R1).
pub(crate) const SETTINGS: [&str; 4] =
    ["donnaWhatsapp", "donnaWhatsappSave", "donnaWhatsappBotSave", "donnaWhatsappWacliRetry"];
/// Número (R2).
pub(crate) const NUMBER: [&str; 3] = ["donnaWhatsappPair", "donnaWhatsappPairCancel", "donnaWhatsappDisconnect"];

/// Caixa do `WhatsApp` do bot (R3): só o dono, para a aba própria do bot (nunca o chat).
pub(crate) const INBOX: [&str; 3] = ["donnaWhatsappChats", "donnaWhatsappMessages", "donnaWhatsappMarkRead"];
/// R8: "Apagar esta conversa deste computador" (caixa do Hub e cópia do wacli).
pub(crate) const CHAT_ERASE: &str = "donnaWhatsappChatErase";
/// MX-18: gesto do dono no cartão de rascunho (só Local), envio direto do dono no
/// mini `WhatsApp` (só Local) e a fila de saída.
pub(crate) const SEND_DRAFT: &str = "donnaWhatsappSendDraft";
pub(crate) const OWNER_SEND: &str = "donnaWhatsappSend";
pub(crate) const CHAT_MARK: &str = "donnaWhatsappChatMark";
pub(crate) const GROUP_AUTO: &str = "donnaWhatsappGroupAuto";
pub(crate) const QUEUE: [&str; 2] = ["donnaWhatsappQueue", "donnaWhatsappQueueDecide"];

pub(crate) fn handles(method: &str) -> bool {
    SETTINGS.contains(&method)
        || NUMBER.contains(&method)
        || INBOX.contains(&method)
        || QUEUE.contains(&method)
        || method == CHAT_ERASE
        || method == SEND_DRAFT
        || method == OWNER_SEND
        || method == CHAT_MARK
        || method == GROUP_AUTO
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Chat {
    bot_id: String,
    /// JID da conversa no `WhatsApp`. Campo próprio: `chatId` é conversa do Hub (guarda dos Spaces).
    whatsapp_chat_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Page {
    bot_id: String,
    whatsapp_chat_id: String,
    before: Option<messages::Before>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BotSave {
    bot_id: String,
    settings: BotPatch,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pair {
    bot_id: String,
    method: PairMethod,
    phone: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PairMethod {
    Code,
    Qr,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BotOnly {
    bot_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disconnect {
    bot_id: String,
    #[serde(default)]
    erase: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueRead {
    bot_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueDecide {
    bot_id: String,
    item_id: String,
    decision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendDraft {
    draft_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerSend {
    bot_id: String,
    /// JID da conversa no `WhatsApp`: `whatsappChatId`, nunca `chatId` (conversa do Hub, guarda dos Spaces).
    whatsapp_chat_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChatMark {
    bot_id: String,
    /// JID da conversa no `WhatsApp`: `whatsappChatId`, nunca `chatId` (conversa do Hub, guarda dos Spaces).
    whatsapp_chat_id: String,
    marked: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GroupAuto {
    bot_id: String,
    whatsapp_chat_id: String,
    auto: bool,
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    if INBOX.contains(&method) {
        return inbox(hub, caller, method, body);
    }
    if method == CHAT_ERASE {
        return erase_chat(hub, caller, body).await;
    }
    if method == SEND_DRAFT {
        // Gesto do dono no cartão: só o app Local, nunca Device/voz/ACS4.
        anyhow::ensure!(matches!(caller, Caller::Local), "not_allowed: enviar o rascunho exige o app no computador.");
        let request: SendDraft = parse(body)?;
        return crate::fork::conectores::whatsapp_human::send(hub, &request.draft_id).await;
    }
    if method == GROUP_AUTO {
        anyhow::ensure!(
            matches!(caller, Caller::Local),
            "not_allowed: alterar envio autônomo exige o app no computador."
        );
    }
    if method == OWNER_SEND {
        // O dono escreve no mini WhatsApp: gesto local, fora da fila e dos limites do bot.
        anyhow::ensure!(
            matches!(caller, Caller::Local),
            "not_allowed: escrever no WhatsApp exige o app no computador."
        );
        let request: OwnerSend = parse(body)?;
        let sent = super::send::owner_send(hub, &request.bot_id, &request.whatsapp_chat_id, &request.text).await?;
        return Ok(match sent {
            wacli::Sent::Yes { id } => serde_json::json!({"state": "sent", "id": id}),
            wacli::Sent::NotStarted => {
                anyhow::bail!("not_started: nada saiu; confira a conversa e tente de novo.")
            }
            wacli::Sent::Uncertain => serde_json::json!({"state": "uncertain"}),
        });
    }
    let mut view = dispatch(hub, caller, method, body).await?;
    // R7: com o plugin ligado, o Hub baixa o wacli oficial em segundo plano se faltar.
    let enabled = view["plugin"]["enabled"] == true;
    let download = super::binario::follow(hub, enabled, method == "donnaWhatsappWacliRetry");
    if let (Some(wacli), Value::Object(fields)) = (view["plugin"]["wacli"].as_object_mut(), download) {
        wacli.extend(fields);
    }
    Ok(view)
}

async fn dispatch(hub: &Arc<Hub>, caller: &Caller, method: &str, mut body: Value) -> Result<Value> {
    // A origem vem do transporte autenticado; Device só com Control, sem virar Local.
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    if let Some(object) = body.as_object_mut() {
        object.remove("spaceId");
    }
    let view = || store::read(&hub.store, &hub.host_id, &hub.whatsapp);
    match method {
        "donnaWhatsappPair" => {
            let request: Pair = parse(body)?;
            let (method, phone) = match request.method {
                PairMethod::Code => {
                    let phone = request
                        .phone
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("invalid_field: informe o número para receber o código."))?;
                    (Method::Code, Some(wacli::phone(phone)?))
                }
                PairMethod::Qr => {
                    anyhow::ensure!(request.phone.is_none(), "invalid_field: o QR não usa número.");
                    (Method::Qr, None)
                }
            };
            number::pair(hub, &request.bot_id, method, phone).await?;
            view()
        }
        "donnaWhatsappPairCancel" => {
            let request: BotOnly = parse(body)?;
            number::cancel(hub, &request.bot_id);
            view()
        }
        "donnaWhatsappDisconnect" => {
            let request: Disconnect = parse(body)?;
            let logout = number::disconnect(hub, &request.bot_id, request.erase).await?;
            let mut result = view()?;
            result["disconnect"] = logout.json();
            Ok(result)
        }
        _ => {
            // R8: trocar o prazo muda o que a caixa mostra; o app relê (inclusive o já carregado).
            let saves_retention = method == "donnaWhatsappBotSave" && body["settings"].get("retentionDays").is_some();
            let retention = body["botId"].as_str().filter(|_| saves_retention).map(str::to_owned);
            let result = handle(&hub.store, &hub.host_id, &hub.whatsapp, caller, method, body)?;
            if let Some(bot) = retention {
                hub.whatsapp.notify(&hub.events, &bot);
            }
            if !matches!(method, "donnaWhatsapp" | "donnaWhatsappWacliRetry") {
                // Ligar/desligar, modo ou pausa mudam quem pode ter o wacli de pé.
                number::reconcile(hub);
            }
            Ok(result)
        }
    }
}

pub(super) fn handle(
    store: &crate::store::Store,
    host_id: &str,
    numbers: &State,
    caller: &Caller,
    method: &str,
    mut body: Value,
) -> Result<Value> {
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    if let Some(object) = body.as_object_mut() {
        object.remove("spaceId");
    }
    match method {
        "donnaWhatsapp" | "donnaWhatsappWacliRetry" => {
            anyhow::ensure!(
                body.as_object().is_some_and(serde_json::Map::is_empty),
                "invalid_field: consulta inválida."
            );
            store::read(store, host_id, numbers)
        }
        "donnaWhatsappSave" => store::save_hub(store, host_id, numbers, parse::<HubPatch>(body)?),
        "donnaWhatsappBotSave" => {
            let request: BotSave = parse(body)?;
            store::save_bot(store, host_id, numbers, &request.bot_id, &request.settings)
        }
        "donnaWhatsappQueue" => {
            let request: QueueRead = parse(body)?;
            super::outbox::list(store, &request.bot_id)
        }
        "donnaWhatsappQueueDecide" => {
            let request: QueueDecide = parse(body)?;
            super::outbox::decide(store, &request.bot_id, &request.item_id, &request.decision)
        }
        "donnaWhatsappGroupAuto" => {
            let request: GroupAuto = parse(body)?;
            store::set_group_auto(store, host_id, numbers, &request.bot_id, &request.whatsapp_chat_id, request.auto)
        }
        "donnaWhatsappChatMark" => {
            let request: ChatMark = parse(body)?;
            store::mark_chat(store, host_id, numbers, &request.bot_id, &request.whatsapp_chat_id, request.marked)
        }
        _ => anyhow::bail!("invalid_field: pedido do WhatsApp desconhecido."),
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow::anyhow!("invalid_field: pedido do WhatsApp inválido."))
}

/// Apaga uma conversa: exclusão inicial do Hub, drena o leitor e reapaga antes da
/// cópia do wacli. Timeout ao aguardar/parar ou erro no reapagar retorna `failed`,
/// sem cleanup e sem confirmar exclusão completa. Nada no celular nem no `WhatsApp` da pessoa.
/// Mesma audiência da caixa.
async fn erase_chat(hub: &Arc<Hub>, caller: &Caller, mut body: Value) -> Result<Value> {
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    if let Some(object) = body.as_object_mut() {
        object.remove("spaceId");
    }
    let request: Chat = parse(body)?;
    store::inbox_bot(&hub.store, &request.bot_id)?;
    let (bot, chat) = (request.bot_id, request.whatsapp_chat_id);
    anyhow::ensure!(!chat.is_empty() && chat.len() <= 200, "invalid_field: conversa inválida.");
    let erased = messages::erase_chat(&hub.store.db.locked(), &bot, &chat)?;
    hub.whatsapp.notify(&hub.events, &bot);
    let wacli = number::erase_chat(hub, &bot, &chat).await;
    let unread = messages::unread(&hub.store.db.locked(), &bot)?;
    let mut result = serde_json::json!({"botId": bot, "whatsappChatId": chat, "erased": erased, "unread": unread});
    if let (Value::Object(out), Value::Object(fields)) = (&mut result, wacli.json()) {
        out.extend(fields);
    }
    Ok(result)
}

/// As três funções da caixa. Audiência: só o dono (transporte autenticado, como o editor
/// de bot) e só bots agentes vivos do espaço Pessoal, onde o número pode ter concessão.
/// Nenhum bot, motor, ferramenta ou outro membro chega aqui.
fn inbox(hub: &Hub, caller: &Caller, method: &str, mut body: Value) -> Result<Value> {
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    if let Some(object) = body.as_object_mut() {
        object.remove("spaceId");
    }
    match method {
        "donnaWhatsappChats" => {
            let request: BotOnly = parse(body)?;
            store::inbox_bot(&hub.store, &request.bot_id)?;
            messages::chats(&hub.store.db.locked(), &request.bot_id, None)
        }
        "donnaWhatsappMessages" => {
            let request: Page = parse(body)?;
            store::inbox_bot(&hub.store, &request.bot_id)?;
            let c = hub.store.db.locked();
            messages::page(&c, &request.bot_id, &request.whatsapp_chat_id, None, request.before.as_ref(), request.limit)
        }
        "donnaWhatsappMarkRead" => {
            let request: Chat = parse(body)?;
            store::inbox_bot(&hub.store, &request.bot_id)?;
            let c = hub.store.db.locked();
            let (changed, unread) = messages::mark_read(&c, &request.bot_id, &request.whatsapp_chat_id)?;
            drop(c);
            if changed > 0 {
                hub.whatsapp.notify(&hub.events, &request.bot_id);
            }
            let chat = request.whatsapp_chat_id;
            Ok(serde_json::json!({"botId": request.bot_id, "whatsappChatId": chat, "unread": unread}))
        }
        _ => anyhow::bail!("invalid_field: pedido do WhatsApp desconhecido."),
    }
}
