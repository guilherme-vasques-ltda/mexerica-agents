// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Small, host-local bot delegation. The recipient uses its own ACP session;
//! native subagents remain the harness's responsibility.

use crate::LockExt;
use crate::agent::bot::Cmd;
use crate::fork::cadeias::equipe::Entrada;
use crate::fork::delegacao::contrato::{self, Contrato};
use crate::fork::retornos::{self, Andamento, Decisao};
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::fork::spaces::tool_context::ToolContext;
use crate::hub::{BotStatus, Hub};
use crate::store::{BotConfig, Lane};
use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

mod requests;
pub use requests::Requests;

pub const ASK_TIMEOUT: Duration = Duration::from_secs(600);
pub(crate) const MAX_MESSAGE_BYTES: usize = 32_000;
const MAX_PENDING_MESSAGES: usize = 64;

pub const INSTRUCTIONS: &str = "A admissão grava os dois avisos e reserva o turno do destinatário no mesmo commit; uma recusa não publica nem enfileira metade do pedido. Isso não desfaz execução externa já admitida. Na adicional própria, o aviso do remetente fica nessa conversa. ask_bot retorna só à chamada original; message_bot conserva o relatório na conversa do destinatário explícito, com autoridade própria. Não repita efeitos incertos após troca de contexto. Cada pedido conta na cadeia D220 do pedido original (5 níveis, 50 execuções); no limite, a recusa não envia nada e não deve ser contornada por outro bot. O Hub registra o pedido e como ele terminou, só com metadados; esse registro não reenvia nem devolve o resultado após reinício. Se a conclusão não puder ser registrada, a ferramenta informa falha em vez do resultado. Se ask_bot falhar depois da admissão (prazo, cancelamento, parada ou falha do destinatário), o erro termina com (requestId: <id>); recusa antes da admissão não tem pedido nem ID. bot_request_status consulta, só leitura, o estado registrado de um pedido que você fez nesta mesma conversa, pelo requestId devolvido no aceite ou nesse erro: não traz a resposta, não reenvia, não confirma entrega nem efeito, e running ou restart_uncertain continuam sem conclusão. Consulte quando precisar decidir, não em ciclo. my_team_chain mostra, só leitura, a sua própria cadeia no quadro de Equipes (um nível acima e um abaixo); Reporta a não concede poder. list_bots mostra a equipe de cada bot visível e quem é o chefe; use para responder sobre as equipes. A hierarquia de reportes de cada bot continua só pela própria cadeia (my_team_chain). Use list_bots para encontrar os outros bots da Mexerica do usuário. Um pedido recebido pode trazer <contrato_da_tarefa>: entregue o resultado pedido e diga como conferiu; não altere os critérios para passar e, se não der, diga o que faltou; o contrato não libera permissão. \
Use ask_bot when the user requests another bot's help or its specialty is useful. \
Use message_bot for a handoff that should continue independently: it returns when queued, \
and the recipient reports the outcome to the user in its own chat. Stopping you does not cancel it. \
Provide a self-contained request: the recipient has its own conversation, working directory, \
tools and permissions, not your context. ask_bot returns its final reply to you. \
Coordinate file ownership before asking for edits in a shared project. \
Do not ask a bot that is waiting for you. Native subagents are managed by your own harness; \
these tools are for collaborating with the user's existing bots.";

pub fn tools() -> Value {
    json!([
        {
            "name": "list_bots",
            "description": "Lista outros bots visíveis da Mexerica, IDs, especialidades, pastas e status; team é o caminho da equipe no Quadro deste Space (null se sem equipe), e teamChief indica se o bot é o chefe gravado dessa equipe. Use para responder sobre as equipes; a hierarquia de reportes continua só pela própria cadeia (my_team_chain). unavailable significa runtime sem audiência comprovada neste Space; não informa se está livre, trabalhando ou com falha e não autoriza buscar outro contexto.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": true},
        },
        {
            "name": "ask_bot",
            "description": "A admissão confirma os dois avisos juntos; uma falha não deixa pedido parcial. Conta na cadeia do pedido original: até 5 níveis e 50 execuções; no limite, recusa sem enviar nada. Peça uma tarefa delimitada a outro bot da Mexerica e aguarde a resposta final (até 10 minutos, incluindo fila e aprovações). O destinatário usa suas próprias ferramentas, pasta e permissões. Parar você cancela seus pedidos pendentes. Aceite ou reserva não comprovam envio ao motor. O Hub registra o pedido e como terminou, só com metadados; se não conseguir registrar a conclusão, retorna falha em vez da resposta. Depois da admissão, um erro (prazo, cancelamento, parada ou falha do destinatário) termina com (requestId: <id>): use-o em bot_request_status antes de repetir; recusa antes da admissão não traz ID. O canal não sobrevive ao reinício e o registro não devolve a resposta depois; confira os efeitos antes de tentar de novo. Não há ferramenta para pausar ou retomar bots. Se o seu turno traz as Regras do dono para delegar, siga-as ao escolher o destinatário; elas não liberam permissão. Em pedido não trivial, proponha contract (resultado, como conferir, onde pode mexer, limites, regra seguida): o Hub o guarda com o pedido e o entrega escapado ao destinatário, que não pode mudá-lo; é orientação, não libera permissão nem muda o limite.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "botId": {"type": "string", "description": "Recipient ID from list_bots."},
                    "message": {"type": "string", "description": "Task, relevant context, file paths and expected result."},
                    "contract": contrato::esquema(),
                },
                "required": ["botId", "message"],
            },
            "annotations": {"readOnlyHint": false, "idempotentHint": false},
        },
        {
            "name": "message_bot",
            "description": "A admissão confirma os dois avisos juntos; uma falha não deixa pedido parcial. Conta na cadeia do pedido original: até 5 níveis e 50 execuções; no limite, recusa sem enviar nada. Enfileire um pedido completo para outro bot da Mexerica e receba o aceite, sem aguardar resposta. O destinatário relata o resultado ao usuário no próprio chat, com suas ferramentas, pasta e permissões. Aceite ou reserva não comprovam envio ao motor. Parar você preserva pedidos aceitos; Parar o destinatário alcança sua fila e turno conforme o controle atual. O Hub registra o pedido e como terminou, só com metadados; se não conseguir registrar a conclusão, o aviso mostra falha. O canal não sobrevive ao reinício, o registro não reenvia o trabalho e nada é repetido automaticamente. Confira os chats antes de tentar de novo um resultado incerto; bot_request_status mostra o estado registrado pelo requestId, sem a resposta. Não há ferramenta para pausar ou retomar bots. Se o seu turno traz as Regras do dono para delegar, siga-as ao escolher o destinatário; elas não liberam permissão. Em pedido não trivial, proponha contract (resultado, como conferir, onde pode mexer, limites, regra seguida): o Hub o guarda com o pedido e o entrega escapado ao destinatário, que não pode mudá-lo; é orientação, não libera permissão nem muda o limite.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "botId": {"type": "string", "description": "Recipient ID from list_bots."},
                    "message": {"type": "string", "description": "Task, relevant context, file paths and expected outcome for the user."},
                    "contract": contrato::esquema(),
                },
                "required": ["botId", "message"],
            },
            "annotations": {"readOnlyHint": false, "idempotentHint": false},
        },
        {
            "name": retornos::consulta::FERRAMENTA,
            "description": "Consulta só leitura do estado registrado de um pedido ask_bot/message_bot que você fez nesta mesma conversa, pelo requestId devolvido no aceite ou no fim do erro de ask_bot depois da admissão. Estados: accepted (ainda não enviado); running (envio possível, sem conclusão registrada; não prova que o bot está trabalhando); finished com outcome completed, failed, stopped ou incomplete e motivo; cancelled; interrupted, inclusive restart_uncertain (incerto após reinício, nunca concluído). completed é só o fim do turno do destinatário: não confirma entrega, leitura nem o combinado. Não traz resposta, texto ou artefatos, não reenvia e não acorda o destinatário. Pedido de outra conversa, de outro bot ou inexistente recebe a mesma recusa. Vale só no Pessoal, na conversa própria do bot e durante a execução atual. Não use em ciclo: consulte quando precisar decidir, por exemplo antes de repetir um pedido incerto.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "requestId": {"type": "string", "description": "requestId devolvido por ask_bot ou message_bot nesta conversa."},
                },
                "required": ["requestId"],
                "additionalProperties": false,
            },
            "annotations": {"readOnlyHint": true, "idempotentHint": true},
        },
        crate::fork::times::cadeia::tool()
    ])
}

/// Sent through the recipient's normal actor queue, but never merged with user
/// messages or another request. Its completion mode owns reply delivery or the
/// independent message's notice lifetime.
pub struct BotRequest {
    pub id: String,
    pub entry_id: String,
    /// MX-77: o aviso do remetente (par do `entry_id`); o despacho marca os dois `sent`.
    origin_entry_id: Option<String>,
    pub prompt: String,
    /// MX-77: o "Editar" da bandeja remonta o prompt com o nome do remetente e o contrato congelado.
    source_name: String,
    contract: Option<Contrato>,
    /// Bound the whole chain, including independent handoffs after their sender finishes.
    hops: u8,
    /// Recibo D220 deste repasse; o turno do destinatário continua a mesma cadeia.
    chain: Option<crate::fork::cadeias::modelo::Recibo>,
    /// Registro R1: existe só quando a admissão trouxe `ExecutionRef`.
    retorno: Andamento,
    /// D6: a pessoa comprovada da conversa do remetente, carimbada pelo Hub na admissão — nunca do corpo do pedido.
    /// Sem ela, o destinatário não entrega a nota "Sobre você" de ninguém.
    pub person: Option<String>,
    completion: Completion,
}

enum Completion {
    ReplyToSender(oneshot::Sender<Result<String>>),
    ReportInRecipientChat(Box<MessageLifetime>),
}

impl BotRequest {
    /// Pedido `ask_bot` mínimo para testes do ator (MB-1b).
    #[cfg(test)]
    pub(crate) fn for_test(id: &str) -> (Self, oneshot::Receiver<Result<String>>) {
        let (reply, answer) = oneshot::channel();
        let request = Self {
            id: id.into(),
            entry_id: id.into(),
            origin_entry_id: None,
            prompt: String::new(),
            source_name: String::new(),
            contract: None,
            hops: 0,
            chain: None,
            retorno: Andamento::SemRegistro,
            person: None,
            completion: Completion::ReplyToSender(reply),
        };
        (request, answer)
    }

    /// MX-77: pedido de teste com o id da cadeia e o do aviso distintos, como na admissão real.
    #[cfg(test)]
    pub(crate) fn for_test_named(id: &str, entry_id: &str, source: &str) -> (Self, oneshot::Receiver<Result<String>>) {
        let (reply, answer) = oneshot::channel();
        let request = Self {
            id: id.into(),
            entry_id: entry_id.into(),
            origin_entry_id: None,
            prompt: String::new(),
            source_name: source.into(),
            contract: None,
            hops: 0,
            chain: None,
            retorno: Andamento::SemRegistro,
            person: None,
            completion: Completion::ReplyToSender(reply),
        };
        (request, answer)
    }

    /// MX-77: `message_bot` de teste — o Drop do [`MessageLifetime`] encerra os avisos `cancelled`.
    /// Reserva a cota do remetente como a admissão real, para o `release_message` do Drop fechar a conta.
    #[cfg(test)]
    pub(crate) fn for_test_message(id: &str, entry_id: &str, hub: Arc<Hub>, entries: Vec<String>) -> Self {
        hub.team.reserve_message_in("a").expect("reserva de teste");
        let lifetime = MessageLifetime {
            hub,
            id: id.into(),
            sender: "a".into(),
            entries,
            target_name: "b".into(),
            started: false,
            finished: false,
            rejected_by_owner: false,
        };
        Self {
            id: id.into(),
            entry_id: entry_id.into(),
            origin_entry_id: None,
            prompt: String::new(),
            source_name: String::new(),
            contract: None,
            hops: 0,
            chain: None,
            retorno: Andamento::SemRegistro,
            person: None,
            completion: Completion::ReportInRecipientChat(Box::new(lifetime)),
        }
    }

    pub fn expects_reply(&self) -> bool {
        matches!(self.completion, Completion::ReplyToSender(_))
    }

    pub fn reply_closed(&self) -> bool {
        matches!(&self.completion, Completion::ReplyToSender(reply) if reply.is_closed())
    }

    /// Antes do permit: `false` quando o pedido já foi cancelado ou interrompido e
    /// não deve ser enviado. Leitura só; a transição para `running` é do permit.
    pub(crate) fn still_open(&self, store: &crate::store::Store) -> Result<bool> {
        if self.retorno == Andamento::Aceito { retornos::aceito(store, &self.id) } else { Ok(true) }
    }

    /// MX-145: o registro ainda `accepted` que o permit leva a `running` na MESMA Tx
    /// em que o teto passa (`retornos::despachar_na`). A mesma tentativa (retomada
    /// após limite) não grava de novo.
    pub(crate) fn unrecorded_id(&self) -> Option<String> {
        (self.retorno == Andamento::Aceito).then(|| self.id.clone())
    }

    pub(crate) fn permission_checkpoint(&self) -> Result<crate::fork::delegacao::pausa::Pedido<'_>> {
        match self.retorno {
            Andamento::EmExecucao(fence) => Ok(crate::fork::delegacao::pausa::Pedido { id: &self.id, fence }),
            _ => bail!("delegated_permission_unrecorded"),
        }
    }

    /// A cerca gravada pelo permit; sem ela não há envio.
    pub(crate) fn dispatch_recorded(&mut self, fence: i64) {
        self.retorno = Andamento::EmExecucao(fence);
    }

    /// Pedido de teste com registro R1 `accepted` (MX-145): exercita a cerca do permit.
    #[cfg(test)]
    pub(crate) fn for_test_recorded(id: &str) -> (Self, oneshot::Receiver<Result<String>>) {
        let (mut request, answer) = Self::for_test(id);
        request.retorno = Andamento::Aceito;
        (request, answer)
    }

    /// Grava o término antes do canal em memória e dos avisos finais.
    pub(crate) fn record_end(&self, store: &crate::store::Store, bot: &str, fim: retornos::Fim) -> retornos::Registro {
        retornos::registrar_fim(store, &self.id, bot, self.retorno, fim)
    }

    /// Permissão concedida e prompt a caminho: o pedido saiu da fila. Para `message_bot` os dois
    /// avisos viram `sent` com "Running…"; para `ask_bot` (MX-77) os dois avisos também saem de
    /// `queued`, para a bandeja do destinatário esvaziar — o texto e o fim continuam os de sempre.
    pub fn mark_started(&mut self, hub: &Hub) {
        match &mut self.completion {
            Completion::ReportInRecipientChat(message) => {
                message.started = true;
                for id in &message.entries {
                    if let Some(mut e) = message.hub.store.entry(id) {
                        let heading = e.data["heading"].as_str().unwrap_or("Bot request");
                        e.data["text"] = format!("{heading}\nRunning in {}'s chat…", message.target_name).into();
                        e.data["status"] = json!(RequestStatus::Sent);
                        message.hub.set_entry(id, &e.data);
                    }
                }
            }
            Completion::ReplyToSender(_) => {
                for id in [Some(self.entry_id.as_str()), self.origin_entry_id.as_deref()].into_iter().flatten() {
                    if let Some(mut e) = hub.store.entry(id)
                        && e.data["status"] == "queued"
                    {
                        e.data["status"] = json!(RequestStatus::Sent);
                        hub.set_entry(id, &e.data);
                    }
                }
            }
        }
    }

    /// MX-77: "Excluir" na bandeja do destinatário — o pedido sai da fila sem rodar e o remetente é
    /// avisado: `ask_bot` recebe o erro tipado [`OwnerRejected`]; `message_bot` tem os avisos
    /// encerrados `cancelled` com o motivo no Drop do [`MessageLifetime`].
    pub(crate) fn reject_by_owner(self) {
        match self.completion {
            Completion::ReplyToSender(reply) => {
                let _ = reply.send(Err(OwnerRejected.into()));
            }
            Completion::ReportInRecipientChat(mut message) => {
                message.rejected_by_owner = true;
            }
        }
    }

    /// MX-77: "Editar" na bandeja — reescreve só o texto do pedido; o contrato proposto pelo bot fica
    /// congelado e o prompt é remontado com a nota de que o dono editou antes do envio.
    pub(crate) fn edit_text(&mut self, text: &str) {
        let pedido = contrato::com_mensagem(text, self.contract.as_ref());
        self.prompt = prompt_request(self.expects_reply(), &self.source_name, &pedido, true);
    }

    pub fn complete(self, result: Result<String>, cancelled: bool) {
        match self.completion {
            Completion::ReplyToSender(reply) => {
                let _ = reply.send(result);
            }
            Completion::ReportInRecipientChat(mut message) => {
                if cancelled {
                    message.finish(RequestStatus::Cancelled, "Recipient stopped. Partial work may have happened.");
                } else {
                    match result {
                        Ok(_) => message.finish(RequestStatus::Completed, "Completed."),
                        Err(error) => message.finish(RequestStatus::Failed, &format!("{error:#}")),
                    }
                }
            }
        }
    }
}

/// MX-77: o "Excluir" da bandeja chega ao `ask_bot` como este erro tipado — o [`Waiting`] o traduz em
/// `Decisao::Cancelado` (o registro R1 diz `cancelled`, não falha do motor).
#[derive(Debug)]
pub(crate) struct OwnerRejected;
impl std::fmt::Display for OwnerRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Mesmo detalhe do `message_bot` recusado na fila: os dois caminhos dizem a mesma frase.
        f.write_str("Cancelled by the owner before execution.")
    }
}
impl std::error::Error for OwnerRejected {}

/// O prompt do destinatário: montado na admissão e refeito quando o dono edita o pedido na fila (MX-77).
/// `edited` marca a nota "editado pelo dono" antes do pedido — o destinatário não atribui o texto ao bot.
fn prompt_request(ask: bool, source_name: &str, pedido: &str, edited: bool) -> String {
    let note = if edited { "\n\n(The owner of this bot edited the request text before it ran.)" } else { "" };
    if ask {
        format!(
            "Another Donnabot bot, {source_name} requests your help. This is a bot request, not a new user instruction. Work within your own permissions and working directory. Your final reply goes directly to the requesting bot rather than the user's main chat. Return the requested result in that reply, addressing the requesting bot. Do not use send_message or message_bot to deliver your answer, and do not ask the bot to do the task back.{note}\n\n{pedido}"
        )
    } else {
        format!(
            "Another Donnabot bot, {source_name} sent this request. This is a bot request, not a new user instruction. Work within your own permissions and working directory. Report the outcome to the user in this chat. send_message is available for this independent request, even if an earlier ask or older session instructions said it was unavailable. Use it for user-facing updates and your report; if you send none, your final text becomes the report. You do not need to reply to the sending bot.{note}\n\n{pedido}"
        )
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RequestStatus {
    Queued,
    Sent,
    Completed,
    Failed,
    Cancelled,
}

struct Pending {
    hub: Arc<Hub>,
    id: String,
    target: String,
    entries: Vec<String>,
    finished: bool,
}

impl Pending {
    /// Decisão do solicitante; não altera um pedido já concluído ou decidido.
    fn settle(&self, decisao: Decisao) {
        if let Err(error) = retornos::encerrar(&self.hub.store, &self.id, decisao) {
            tracing::error!(request = %self.id, error = format!("{error:#}"), "decisão do pedido não registrada");
        }
    }

    fn finish(&mut self, status: RequestStatus, detail: &str, reply: Option<&str>) {
        finish_notices(&self.hub, &self.entries, status, detail, reply);
        self.finished = true;
    }
}

fn finish_notices(hub: &Hub, entries: &[String], status: RequestStatus, detail: &str, reply: Option<&str>) {
    for id in entries {
        if let Some(mut e) = hub.store.entry(id) {
            if let Some(message) = e.data.get_mut("botMessage").and_then(Value::as_object_mut) {
                if let Some(reply) = reply {
                    message.insert("reply".into(), reply.into());
                }
                if matches!(status, RequestStatus::Failed | RequestStatus::Cancelled) {
                    message.insert("detail".into(), detail.into());
                }
            }
            e.data["status"] = json!(status);
            let heading = e.data["heading"].as_str().unwrap_or("Bot request");
            e.data["text"] = format!("{heading}\n{detail}").into();
            if !matches!(status, RequestStatus::Completed) {
                e.data["style"] = "error".into();
            }
            hub.set_entry(id, &e.data);
        }
    }
}

/// Owned by the recipient's command/queue/turn after admission, never by a
/// waiting sender. Drop also covers commands discarded during actor shutdown.
struct MessageLifetime {
    hub: Arc<Hub>,
    id: String,
    sender: String,
    entries: Vec<String>,
    target_name: String,
    started: bool,
    finished: bool,
    /// MX-77: o dono excluiu o pedido na bandeja antes do envio.
    rejected_by_owner: bool,
}

impl MessageLifetime {
    fn finish(&mut self, status: RequestStatus, detail: &str) {
        finish_notices(&self.hub, &self.entries, status, detail, None);
        self.finished = true;
    }
}

impl Drop for MessageLifetime {
    fn drop(&mut self) {
        if !self.finished {
            // MX-77: a recusa do dono na fila é `cancelled` com motivo próprio, não "não iniciado".
            let decisao = if self.started {
                Decisao::DestinatarioParou
            } else if self.rejected_by_owner {
                Decisao::Cancelado
            } else {
                Decisao::NaoIniciado
            };
            if let Err(error) = retornos::encerrar(&self.hub.store, &self.id, decisao) {
                tracing::error!(request = %self.id, error = format!("{error:#}"), "decisão do pedido não registrada");
            }
            let detail = if self.started {
                "Recipient stopped. Partial work may have happened."
            } else if self.rejected_by_owner {
                "Cancelled by the owner before execution."
            } else {
                "Cancelled before execution."
            };
            self.finish(RequestStatus::Cancelled, detail);
        }
        self.hub.team.release_message(&self.sender);
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.finished {
            self.settle(Decisao::Cancelado);
            self.finish(RequestStatus::Cancelled, "Request cancelled. Partial work may have happened.", None);
        }
        self.hub.team.0.locked().pending.remove(&self.id);
        // Targeted cancellation never clears unrelated user messages or turns.
        let _ = self.hub.send_cmd(&self.target, Cmd::CancelAsk { id: self.id.clone() });
    }
}

/// D6: a pessoa comprovada da conversa do remetente, pela marca que o próprio Hub fixou na ACP do turno dele —
/// nunca do corpo do pedido nem da sessão do destinatário. `lane.session` é o ID VISÍVEL (extra/Space), não a
/// ACP sob a qual a marca foi gravada: o SID vem do contexto autenticado ou do runtime do turno, e a marca é
/// conferida no domínio da conversa dele (`lane.chat`). Sem marca, o pedido viaja sem pessoa.
fn request_person(hub: &Hub, source: &BotConfig, lane: &Lane, context: Option<&ToolContext>) -> Option<String> {
    let acp = context.map(ToolContext::session).map(str::to_owned).or_else(|| hub.runtime(&source.id).session);
    acp.and_then(|sid| crate::chat::memory::profile_person(&hub.store, &lane.chat, &sid))
}

fn visible_bot(hub: &Hub, id: &str) -> Result<BotConfig> {
    hub.store
        .bot(id)?
        .filter(|r| !r.deleted && !r.config.hidden && !r.config.is_group())
        .map(|r| r.config)
        .ok_or_else(|| anyhow!("unknown or hidden bot"))
}

#[cfg(test)]
pub async fn call(hub: &Arc<Hub>, from: &str, name: &str, args: &Value) -> Result<Value> {
    call_in_execution(hub, from, name, args, None).await
}
pub(crate) async fn call_in(
    hub: &Arc<Hub>,
    from: &str,
    name: &str,
    args: &Value,
    context: Option<&ToolContext>,
) -> Result<Value> {
    call_bound(hub, from, name, args, context, context.and_then(|c| c.turn.execution.as_ref())).await
}
pub(crate) async fn call_in_execution(
    hub: &Arc<Hub>,
    from: &str,
    name: &str,
    args: &Value,
    execution: Option<&ExecutionRef>,
) -> Result<Value> {
    call_bound(hub, from, name, args, None, execution).await
}
async fn call_bound(
    hub: &Arc<Hub>,
    from: &str,
    name: &str,
    args: &Value,
    context: Option<&ToolContext>,
    execution: Option<&ExecutionRef>,
) -> Result<Value> {
    bindings::recheck_optional(hub, execution)?;
    if let Some(context) = context {
        context.check(hub)?;
    }
    if let Some(context) = context.filter(|_| matches!(name, "ask_bot" | "message_bot")) {
        let (reply, admitted) = oneshot::channel();
        hub.send_cmd(
            from,
            Cmd::ContextTeam { context: Box::new(context.clone()), name: name.into(), args: args.clone(), reply },
        )?;
        let admitted = tokio::time::timeout(Duration::from_secs(10), admitted)
            .await
            .map_err(|_| anyhow!("team_admission_pending"))???;
        context.check(hub)?;
        return match admitted {
            Admission::Message(value) => Ok(value),
            Admission::Ask(waiting) => waiting.finish(hub, ASK_TIMEOUT, Some(context), execution).await,
        };
    }
    let source = visible_bot(hub, from)?;
    match name {
        "list_bots" => {
            let teams = crate::fork::times::listagem::consultar(&hub.store, &source.space_id)?;
            let bots: Vec<Value> = hub
                .store
                .bots()?
                .into_iter()
                .filter(|b| {
                    !b.deleted
                        && !b.config.hidden
                        && !b.config.is_group()
                        && b.config.id != from
                        && b.config.space_id == source.space_id
                })
                .map(|b| {
                    let (team, chief) = teams.get(&b.config.id).cloned().unwrap_or_default();
                    json!({
                        "id": b.config.id, "name": b.config.name, "description": b.config.description,
                        "backend": b.config.backend, "cwd": b.config.cwd,
                        "status": hub.bot_json(&b)["status"], "team": team, "teamChief": chief,
                    })
                })
                .collect();
            bindings::recheck_optional(hub, execution)?;
            Ok(json!({"bots": bots}))
        }
        "ask_bot" => {
            let to = args["botId"].as_str().ok_or_else(|| anyhow!("botId is required"))?;
            let message = args["message"].as_str().unwrap_or_default().trim();
            let contrato = contrato::ler(args)?;
            begin_ask(hub, &source, to, message, contrato.as_ref(), context, execution)?
                .finish(hub, ASK_TIMEOUT, context, execution)
                .await
        }
        "message_bot" => {
            let to = args["botId"].as_str().ok_or_else(|| anyhow!("botId is required"))?;
            let message = args["message"].as_str().unwrap_or_default().trim();
            let contrato = contrato::ler(args)?;
            message_bot(hub, &source, to, message, contrato.as_ref(), context, execution, None)
        }
        // R2a: só na conversa própria do Pessoal, sem contexto de Space; leitura e revalidação no serviço.
        retornos::consulta::FERRAMENTA => {
            anyhow::ensure!(context.is_none(), retornos::consulta::FORA_DO_ALCANCE);
            retornos::consulta::consultar(hub, &source.id, args, execution)
        }
        // D320: só a própria cadeia de Times, no Space do bot; leitura no módulo do fork.
        crate::fork::times::cadeia::FERRAMENTA => {
            let chain = crate::fork::times::cadeia::consultar(&hub.store, &source.id, &source.space_id, args)?;
            bindings::recheck_optional(hub, execution)?;
            Ok(chain)
        }
        _ => bail!("unknown team tool: {name}"),
    }
}

/// Referência estruturada de despacho interna (MX-142 R2-22): NÃO vem de
/// argumentos do modelo/tool — só o runtime a injeta via
/// `message_bot_task_dispatch`. Gravada em `botMessage.taskDispatch` dos dois
/// avisos, dentro da mesma tx de admissão, para o guard TOCTOU resolver o
/// claim sem parsing de texto livre. Coordenadas são referência, não
/// autoridade: a prova continua sendo o nonce -> agenda corrente.
#[derive(Clone)]
pub(crate) struct TaskDispatch {
    pub nonce: String,
    pub hub_id: String,
    pub space_id: String,
    pub project_id: String,
    pub task_id: String,
}

impl TaskDispatch {
    fn json(&self) -> Value {
        json!({
            "nonce": self.nonce, "hubId": self.hub_id, "spaceId": self.space_id,
            "projectId": self.project_id, "taskId": self.task_id,
        })
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Cadeia D220 existente + dispatch interno tipado; agrupar quebraria todos os callers."
)]
fn message_bot(
    hub: &Arc<Hub>,
    source: &BotConfig,
    to: &str,
    message: &str,
    contrato: Option<&Contrato>,
    context: Option<&ToolContext>,
    execution: Option<&ExecutionRef>,
    dispatch: Option<&TaskDispatch>,
) -> Result<Value> {
    if message.is_empty() || message.len() > MAX_MESSAGE_BYTES {
        bail!("message must be nonempty and at most {MAX_MESSAGE_BYTES} bytes");
    }
    if source.id == to {
        bail!("a bot cannot message itself");
    }
    let target = visible_bot(hub, to)?;
    anyhow::ensure!(target.space_id == source.space_id, "cross_space_reference");
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "team_commit");
    let _boundary = context.is_none().then(|| hub.store.memory_boundary.locked());
    let lane = source_lane(hub, source, context, execution)?;
    let (hops, chain) = hub.team.reserve_message_in(&source.id)?;
    let pedido = contrato::com_mensagem(message, contrato);
    let estruturado = contrato.map(Contrato::json);
    let id = uuid::Uuid::new_v4().to_string();
    let d220 = match Entrada::nova(&source.id, &target.id, &id, "message_bot", message, estruturado.as_ref(), chain) {
        Ok(d220) => d220,
        Err(error) => {
            hub.team.release_message(&source.id);
            return Err(error);
        }
    };
    let mut notices =
        [format!("Messaged {}: {}", target.name, message), format!("Message from {}: {}", source.name, message)].map(
            |heading| {
                json!({
                    "text": format!("{heading}\nQueued. Outcome will appear in {}'s chat.", target.name),
                    "heading": heading, "style": "info", "status": RequestStatus::Queued, "delegationId": id,
                    "sourceBotId": source.id, "targetBotId": target.id,
                    "botMessage": {"sourceBotId": source.id, "targetBotId": target.id, "text": message},
                })
            },
        );
    if let Some(contrato) = contrato {
        for n in &mut notices {
            n["botMessage"]["contract"] = contrato.json();
        }
    }
    if let Some(dispatch) = dispatch {
        for n in &mut notices {
            n["botMessage"]["taskDispatch"] = dispatch.json();
        }
    }
    crate::fork::delegacao::marcar_aviso(&hub.store, source, &lane, &mut notices[0]);
    // MX-77: a lane do remetente no aviso do destinatário — a bandeja mostra "De <bot> · <sessão/conversa>".
    notices[1]["sourceChat"] = lane.chat.clone().into();
    notices[1]["sourceSession"] = lane.session.clone().into();
    let turn = context.map(ToolContext::number);
    let (entries, chain) = match hub.add_team_notices(&lane, turn, &notices, execution, &d220) {
        Ok((entries, chain)) => (entries.map(|e| e.id).to_vec(), chain),
        Err(error) => {
            hub.team.release_message(&source.id);
            return Err(error);
        }
    };
    let lifetime = MessageLifetime {
        hub: hub.clone(),
        id: id.clone(),
        sender: source.id.clone(),
        entries,
        target_name: target.name.clone(),
        started: false,
        finished: false,
        rejected_by_owner: false,
    };
    hub.send_cmd(
        to,
        Cmd::BotRequest(BotRequest {
            id: id.clone(),
            entry_id: lifetime.entries[1].clone(),
            origin_entry_id: Some(lifetime.entries[0].clone()),
            prompt: prompt_request(false, &source.name, &pedido, false),
            source_name: source.name.clone(),
            contract: contrato.cloned(),
            hops,
            chain,
            retorno: if execution.is_some() { Andamento::Aceito } else { Andamento::SemRegistro },
            person: request_person(hub, source, &lane, context),
            completion: Completion::ReportInRecipientChat(Box::new(lifetime)),
        }),
    )?;
    Ok(json!({"requestId": id, "botId": target.id, "name": target.name, "status": RequestStatus::Queued}))
}

#[cfg(test)]
async fn ask(hub: &Arc<Hub>, source: &BotConfig, to: &str, message: &str, timeout: Duration) -> Result<Value> {
    begin_ask(hub, source, to, message, None, None, None)?.finish(hub, timeout, None, None).await
}

/// R2b: o mesmo `ask_bot` com execução do MCP, só com prazo curto para o teste.
#[cfg(test)]
pub(crate) async fn ask_in_execution(
    hub: &Arc<Hub>,
    from: &str,
    args: &Value,
    execution: &ExecutionRef,
    timeout: Duration,
) -> Result<Value> {
    let source = visible_bot(hub, from)?;
    let (to, message) = (args["botId"].as_str().unwrap_or_default(), args["message"].as_str().unwrap_or_default());
    let contrato = contrato::ler(args)?;
    begin_ask(hub, &source, to, message, contrato.as_ref(), None, Some(execution))?
        .finish(hub, timeout, None, Some(execution))
        .await
}

// O contexto do ator e a captura pessoal chegam do produtor original, nunca do runtime seguinte.
fn source_lane(
    hub: &Hub,
    source: &BotConfig,
    context: Option<&ToolContext>,
    execution: Option<&ExecutionRef>,
) -> Result<Lane> {
    if let Some(context) = context {
        context.check(hub)?;
        return Ok(context.lane());
    }
    if let Some(execution) = execution {
        let session = bindings::recheck(hub, execution)?;
        return Ok(Lane { session: (session.id != session.chat_id).then_some(session.id), chat: session.chat_id });
    }
    Ok(Lane::main(&source.id))
}

pub(crate) enum Admission {
    Message(Value),
    Ask(Box<Waiting>),
}

pub(crate) fn admit(hub: &Arc<Hub>, from: &str, name: &str, args: &Value, context: &ToolContext) -> Result<Admission> {
    let source = visible_bot(hub, from)?;
    let to = args["botId"].as_str().ok_or_else(|| anyhow!("botId is required"))?;
    let message = args["message"].as_str().unwrap_or_default().trim();
    let contrato = contrato::ler(args)?;
    match name {
        "message_bot" => message_bot(
            hub,
            &source,
            to,
            message,
            contrato.as_ref(),
            Some(context),
            context.turn.execution.as_ref(),
            None,
        )
        .map(Admission::Message),
        "ask_bot" => {
            begin_ask(hub, &source, to, message, contrato.as_ref(), Some(context), context.turn.execution.as_ref())
                .map(|w| Admission::Ask(Box::new(w)))
        }
        _ => bail!("unknown team admission: {name}"),
    }
}

pub(crate) struct Waiting {
    id: String,
    target: BotConfig,
    pending: Pending,
    cancelled: tokio::sync::watch::Receiver<bool>,
    result: oneshot::Receiver<Result<String>>,
}

fn begin_ask(
    hub: &Arc<Hub>,
    source: &BotConfig,
    to: &str,
    message: &str,
    contrato: Option<&Contrato>,
    context: Option<&ToolContext>,
    execution: Option<&ExecutionRef>,
) -> Result<Waiting> {
    if message.is_empty() || message.len() > MAX_MESSAGE_BYTES {
        bail!("message must be nonempty and at most {MAX_MESSAGE_BYTES} bytes");
    }
    if !matches!(hub.runtime(&source.id).status, BotStatus::Working | BotStatus::NeedsInput) {
        bail!("the requesting bot is no longer working");
    }
    let target = visible_bot(hub, to)?;
    anyhow::ensure!(target.space_id == source.space_id, "cross_space_reference");
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "team_commit");
    let _boundary = context.is_none().then(|| hub.store.memory_boundary.locked());
    let lane = source_lane(hub, source, context, execution)?;
    if !matches!(hub.runtime(&source.id).status, BotStatus::Working | BotStatus::NeedsInput) {
        bail!("the requesting bot is no longer working");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (cancelled, hops, chain) = hub.team.begin_in(&id, &source.id, to, &lane)?;
    let pedido = contrato::com_mensagem(message, contrato);
    let estruturado = contrato.map(Contrato::json);
    let d220 = match Entrada::nova(&source.id, &target.id, &id, "ask_bot", message, estruturado.as_ref(), chain) {
        Ok(d220) => d220,
        Err(error) => {
            hub.team.0.locked().pending.remove(&id);
            return Err(error);
        }
    };
    let mut notices =
        [format!("Asked {}: {}", target.name, message), format!("Request from {}: {}", source.name, message)].map(
            |heading| {
                json!({
                    "text": format!("{heading}\nWaiting for a reply…"), "heading": heading,
                    "style": "info", "status": RequestStatus::Queued, "delegationId": id,
                    "sourceBotId": source.id, "targetBotId": target.id,
                    "botMessage": {"sourceBotId": source.id, "targetBotId": target.id, "text": message},
                })
            },
        );
    if let Some(contrato) = contrato {
        for n in &mut notices {
            n["botMessage"]["contract"] = contrato.json();
        }
    }
    crate::fork::delegacao::marcar_aviso(&hub.store, source, &lane, &mut notices[0]);
    // MX-77: a lane do remetente no aviso do destinatário — a bandeja mostra "De <bot> · <sessão/conversa>".
    notices[1]["sourceChat"] = lane.chat.clone().into();
    notices[1]["sourceSession"] = lane.session.clone().into();
    let turn = context.map(ToolContext::number);
    let (entries, chain) = match hub.add_team_notices(&lane, turn, &notices, execution, &d220) {
        Ok((entries, chain)) => (entries.map(|e| e.id).to_vec(), chain),
        Err(error) => {
            hub.team.0.locked().pending.remove(&id);
            return Err(error);
        }
    };
    let pending = Pending { hub: hub.clone(), id: id.clone(), target: to.to_owned(), entries, finished: false };
    let (reply, result) = oneshot::channel();
    hub.send_cmd(
        to,
        Cmd::BotRequest(BotRequest {
            id: id.clone(),
            entry_id: pending.entries[1].clone(),
            origin_entry_id: Some(pending.entries[0].clone()),
            prompt: prompt_request(true, &source.name, &pedido, false),
            source_name: source.name.clone(),
            contract: contrato.cloned(),
            hops,
            chain,
            retorno: if execution.is_some() { Andamento::Aceito } else { Andamento::SemRegistro },
            person: request_person(hub, source, &lane, context),
            completion: Completion::ReplyToSender(reply),
        }),
    )?;
    Ok(Waiting { id, target, pending, cancelled, result })
}

/// Caminho interno autenticado (MX-142): mesmo `message_bot` — mesmos gates,
/// cadeia D220, Retornos e tx de admissão — com `TaskDispatch` tipado que o
/// modelo não pode forjar via argumentos. Uso restrito a código do Hub.
pub(crate) fn message_bot_task_dispatch(
    hub: &Arc<Hub>,
    from: &str,
    to: &str,
    message: &str,
    execution: Option<&ExecutionRef>,
    dispatch: &TaskDispatch,
) -> Result<Value> {
    bindings::recheck_optional(hub, execution)?;
    let source = visible_bot(hub, from)?;
    message_bot(hub, &source, to, message, None, None, execution, Some(dispatch))
}

impl Waiting {
    async fn finish(
        mut self,
        hub: &Arc<Hub>,
        timeout: Duration,
        context: Option<&ToolContext>,
        execution: Option<&ExecutionRef>,
    ) -> Result<Value> {
        let Self { id, target, pending, cancelled, result } = &mut self;
        let (result, decisao) = tokio::select! {
            r = &mut *result => match r {
                Ok(r) => {
                    // MX-77: o "Excluir" da bandeja chega como OwnerRejected — cancelamento do dono,
                    // não falha do motor; o registro R1 e os avisos dizem `cancelled`.
                    let decisao = r.as_ref().err().filter(|e| e.is::<OwnerRejected>()).map(|_| Decisao::Cancelado);
                    (r, decisao)
                }
                Err(_) => (Err(anyhow!("recipient stopped before replying")), Some(Decisao::DestinatarioParou)),
            },
            _ = cancelled.changed() => (Err(anyhow!("bot request cancelled")), Some(Decisao::Cancelado)),
            () = tokio::time::sleep(timeout) => {
                (Err(anyhow!("bot request timed out; partial work may have happened")), Some(Decisao::Prazo))
            }
        };
        // Antes do CancelAsk (no Drop de Pending): a decisão vence a conclusão tardia.
        if let Some(decisao) = decisao {
            pending.settle(decisao);
        }
        bindings::recheck_optional(hub, execution)?;
        if let Some(context) = context {
            context.check(hub)?;
        }
        match result {
            Ok(text) => {
                let reply = crate::agent::acp::truncate(&text, 4000);
                pending.finish(
                    RequestStatus::Completed,
                    &format!("Reply from {}:\n{reply}", target.name),
                    Some(&reply),
                );
                let mut out = json!({"requestId": id, "botId": target.id, "name": target.name, "reply": text});
                // MX-77: o dono editou o pedido na fila — o remetente não deve atribuir a ele o que não pediu.
                if pending
                    .entries
                    .iter()
                    .any(|entry| pending.hub.store.entry(entry).is_some_and(|e| e.data["botMessage"]["edited"] == true))
                {
                    out["edited"] = true.into();
                    out["note"] = "The owner of the receiving bot edited the request text before it ran; the reply refers to the edited text.".into();
                }
                Ok(out)
            }
            Err(error) => {
                // MX-77: a recusa do dono na bandeja encerra os avisos como `cancelled`, não `failed`.
                let rejected = error.is::<OwnerRejected>();
                pending.finish(
                    if rejected { RequestStatus::Cancelled } else { RequestStatus::Failed },
                    &format!("{error:#}"),
                    None,
                );
                // R2b: o pedido já está registrado; o ID permite consultar bot_request_status.
                Err(anyhow!("{error:#} (requestId: {id})"))
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod admission_tests;
#[cfg(test)]
mod tests;
