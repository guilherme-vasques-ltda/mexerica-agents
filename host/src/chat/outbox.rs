//! How a bot talks to the user, Grok Bot's way: in its chat the user sees
//! only the messages the bot sends with the built-in `chat` MCP server's `send_message`,
//! each one its own bubble. What the agent writes as its reply stays in the trace, unless
//! it sent nothing in the turn: then its last text becomes the reply, as before.

use crate::agent::bot::Cmd;
use crate::hub::Hub;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// How to talk to the user: part of every bot's instructions (`context`), and the `chat` server's.
pub const INSTRUCTIONS: &str = "O Hub vincula estas ferramentas à sessão de origem; chamadas revogadas ou de outra sessão em andamento são recusadas como chamada vencida. Na conversa adicional própria, mensagens, reações e arquivos são entregues nessa conversa; arquivos relativos usam a pasta dela. Contexto alterado é recusado, sem enviar na principal. In your chat with the user, use send_message to deliver your messages. \
If you send none, your final reply is delivered as a fallback. Send a message whenever you have something worth telling them: \
short and plain, one thought per message, in the user's language. Lead with the result, then what needs the user. \
Structure a longer message with Markdown: a short ## heading for each part, lists for steps or parallel items, and a blank line between blocks; keep short messages plain. \
An emoji may open a heading or a key line when it adds meaning (⚠️ only for a real risk), a few per message at most, never in place of words. \
No modo de motor separado, send_file lê com os direitos do motor e não segue links em nenhum componente; não oferece acesso aos dados privados do Hub. Use send_file(path, name?) to share a generated regular file (any type, up to 100 MiB); a local path in text is not a download. Don't send just to acknowledge, to say you started or are still working, or to repeat yourself. \
Before closing, check every requested result and say what is missing. When send_message is available and you already used it, send the conclusion with it too. Share files through send_file when available. A saved note or another bot's acceptance is not delivery. When a channel, routine or another bot's request lacks send_message, deliver the final reply instead; never require absent tools. \
Reactions are rare: react(emoji) only where a person would (❤️ to thanks, 👍 to a plain OK, 🎉 to big news, 😂 to a real joke), \
never to a question or a request. When a message needs no words, the reaction is the whole reply; when it needs neither, \
end your turn by replying exactly \"(pass)\".";

/// Longest message accepted, in bytes.
const MAX_MESSAGE_BYTES: usize = 32 * 1024;
/// How long the bot's actor has to take the message.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

pub fn tools() -> Value {
    json!([{
        "name": "send_message",
        "description": "Envia uma mensagem Markdown ao dono na sessão de origem. O Hub recusa como chamada vencida se o vínculo foi revogado ou a sessão de origem não está em andamento.",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string", "description": "The message, as the user should read it."}},
            "required": ["text"],
        },
        "annotations": {"readOnlyHint": false, "idempotentHint": false},
    }, {
        "name": "send_file",
        "description": "Share one regular file with the user in your own chat. Any file type, including empty files, up to 100 MiB. The host saves an immutable copy. Paths are absolute or relative to your working directory. A successful response means the file message was published; do not repeat it. O Hub recusa como chamada vencida se o vínculo foi revogado ou a sessão de origem não está em andamento. No modo de motor separado, a leitura usa os direitos do motor e recusa links na folha ou em ancestrais; dados privados do Hub não podem ser enviados.",
        "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "name": {"type": "string", "description": "Optional display filename, without folders."}}, "required": ["path"]},
        "annotations": {"readOnlyHint": false, "idempotentHint": false},
    }])
}

pub async fn call(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<Value> {
    call_bound(hub, bot, name, args, None, execution, None).await
}
pub(crate) async fn call_in(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
    invocation: Option<&str>,
) -> Result<Value> {
    call_bound(hub, bot, name, args, context, context.and_then(|c| c.turn.execution.as_ref()), invocation).await
}
async fn call_bound(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
    invocation: Option<&str>,
) -> Result<Value> {
    crate::fork::sessoes::bindings::recheck_optional(hub, execution)?;
    if name == "send_file" {
        return send_file(hub, bot, args, context, execution).await;
    }
    if name == crate::chat::reactions::TOOL {
        let (emoji, to) =
            (args["emoji"].as_str().unwrap_or_default().to_owned(), args["to"].as_str().map(str::to_owned));
        deliver(hub, bot, context, |reply| Cmd::React { execution: execution.cloned(), emoji, to, reply }).await?;
        return Ok(json!({"reacted": true}));
    }
    if name == crate::fork::genui::flow::TOOL {
        return show_choice(hub, bot, args, context, invocation).await;
    }
    if name == crate::fork::owner_question::TOOL {
        let text = crate::fork::owner_question::question(args)?;
        deliver(hub, bot, context, |reply| Cmd::AskOwner { execution: execution.cloned(), text, reply }).await?;
        return Ok(json!({"asked": true, "status": "waitingForOwner"}));
    }
    if name != "send_message" {
        bail!("unknown chat tool: {name}");
    }
    let text = args["text"].as_str().unwrap_or_default().trim();
    if text.is_empty() || text.len() > MAX_MESSAGE_BYTES {
        bail!("text must be nonempty and at most {MAX_MESSAGE_BYTES} bytes");
    }
    deliver(hub, bot, context, |reply| Cmd::SendToUser { execution: execution.cloned(), text: text.to_owned(), reply })
        .await?;
    Ok(json!({"sent": true}))
}

/// G1b: cartão de escolha só na conversa capturada, com invocação do envelope da ponte.
async fn show_choice(
    hub: &Hub,
    bot: &str,
    args: &Value,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
    invocation: Option<&str>,
) -> Result<Value> {
    let context = context.ok_or_else(|| anyhow!(crate::fork::genui::flow::MAIN_REFUSAL))?;
    let invocation = invocation.ok_or_else(|| anyhow!("genui_invocation_required"))?.to_owned();
    // Parse sem eco; o objeto validado não carrega escopo nem identidade.
    let payload = crate::fork::genui::contract::ValidatedChoice::parse(&args.to_string())?;
    context.check(hub)?;
    let (reply, delivered) = oneshot::channel();
    let command = Cmd::ShowChoice { invocation, payload: Box::new(payload), reply };
    hub.send_cmd(bot, context.command(command))?;
    tokio::time::timeout(DELIVERY_TIMEOUT, delivered)
        .await
        .map_err(|_| anyhow!("the bot didn't take the message in time"))?
        .map_err(|_| anyhow!("the bot shut down"))?
}

/// Hands the bot's actor a command for its running turn and waits for its answer.
async fn deliver(
    hub: &Hub,
    bot: &str,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
    cmd: impl FnOnce(oneshot::Sender<Result<()>>) -> Cmd,
) -> Result<()> {
    let (reply, delivered) = oneshot::channel();
    if let Some(context) = context {
        context.check(hub)?;
    }
    let command = cmd(reply);
    hub.send_cmd(
        bot,
        match context {
            Some(c) => c.command(command),
            None => command,
        },
    )?;
    tokio::time::timeout(DELIVERY_TIMEOUT, delivered)
        .await
        .map_err(|_| anyhow!("the bot didn't take the message in time"))?
        .map_err(|_| anyhow!("the bot shut down"))?
}

struct CancelCopy(Arc<std::sync::atomic::AtomicBool>);
impl Drop for CancelCopy {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

async fn send_file(
    hub: &Arc<Hub>,
    bot: &str,
    args: &Value,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<Value> {
    let path = args["path"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("path is required"))?;
    let name = match args.get("name") {
        Some(value) => Some(crate::chat::files::name(value.as_str().ok_or_else(|| anyhow!("name must be a string"))?)?),
        None => None,
    };
    let guard = CancelCopy(Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let (reply, delivered) = oneshot::channel();
    if let Some(context) = context {
        context.check(hub)?;
    }
    let command =
        Cmd::SendFile { execution: execution.cloned(), path: path.to_owned(), name, cancelled: guard.0.clone(), reply };
    hub.send_cmd(
        bot,
        match context {
            Some(c) => c.command(command),
            None => command,
        },
    )?;
    let result = tokio::time::timeout(Duration::from_secs(60), delivered).await;
    let delivered = result.map_err(|_| anyhow!("file sharing timed out; check the conversation before retrying"))?;
    delivered.map_err(|_| anyhow!("file sharing interrupted"))??;
    Ok(json!({"sent": true}))
}
