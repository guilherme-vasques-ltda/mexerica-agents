//! Conversa do Motor Mexerica guardada pelo Hub. O processo do motor
//! é descartável (cancelar o turno, trocar de provedor ou uma queda o matam, e
//! trocar de modelo no app fecha o adaptador inteiro); a conversa fica aqui, por
//! sessão, e o processo novo a recebe por `restore_history`.
//!
//! O que o Hub retém é uma cópia ENXUTA, nunca o objeto do motor:
//! - só usuário, assistente e resultado de ferramenta; pensamento, detalhes,
//!   erros e diagnósticos não entram, e mensagem de assistente com erro ou
//!   abortada tampouco;
//! - só texto e chamadas de ferramenta, com os campos que o pi-ai precisa para
//!   reenviar a conversa a qualquer provedor. Os metadados são campos PERMITIDOS
//!   (rótulos de api/provedor/modelo, motivo de parada e os números de uso e custo
//!   por nome), e cada rótulo e cada id passam pela mesma redação do Guard; o que
//!   a redação alteraria não é guardado;
//! - toda cadeia de texto (texto, argumentos, ids, nomes) passa pela redação do
//!   Guard (`guard_hub::mcp::redigir_json`): as chaves do cofre, os formatos
//!   comuns de credencial, cookies e campos de credencial por nome;
//! - limites RÍGIDOS, todos na mesma unidade do motor (bytes UTF-8 do JSON): por
//!   mensagem, e por conversa o ARRAY serializado (colchetes e vírgulas inclusos),
//!   além de mensagens. Estourou, sai a volta mais antiga inteira; numa volta
//!   sozinha acima do teto ficam o pedido e os grupos mais novos que couberem
//!   (chamada de ferramenta e resultados saem juntos, nunca um sem o outro), e o
//!   resto é descartado.
//!
//! A cópia vive só na memória do Hub, nunca é gravada nem registrada, e só é
//! reaberta pelo mesmo bot, no mesmo Space e com a mesma conta e revisão
//! (`session/load`). Invalidar uma sessão apaga a cópia DELA e só dela. Cada cópia
//! também lembra a geração da credencial de cada provedor que a recebeu: se a
//! credencial de um provedor muda, a conversa que já passou por ele recomeça sem
//! cópia (não dá para provar que a nova credencial é da mesma conta).

use super::OWNER_SEPARATOR;
use crate::guard_hub::mcp::redigir_json;
use anyhow::{Result, ensure};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Uma mensagem, depois de limpa, nunca passa disto (o JSON inteiro, envelope incluso).
pub(super) const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Teto do ARRAY JSON da conversa guardada e do comando `restore_history` (bem abaixo do limite do motor).
pub(super) const MAX_HISTORY_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_HISTORY_MESSAGES: usize = 2_000;
const MAX_BLOCKS: usize = 64;
/// Ponto de partida da reserva do envelope (papel, ids, rótulos, uso) dentro do teto de uma mensagem.
const ENVELOPE_BYTES: usize = 8 * 1024;
const MAX_LABEL: usize = 128;
/// Conversas mantidas na memória do Hub (as de sessões fechadas ou de conexões
/// encerradas, mais as vivas): teto de verdade, e por quanto tempo sem atividade.
const MAX_SESSIONS: usize = 64;
const IDLE_TTL: Duration = Duration::from_secs(12 * 3600);
const TRUNCATED: &str = "\n[truncado pelo Hub]";

fn redact_text(text: &str, known: &[String]) -> String {
    match redigir_json(&Value::String(text.to_owned()), known) {
        Value::String(text) => text,
        _ => String::new(),
    }
}

/// Texto limpo e dentro do orçamento (`budget` em bytes, consumido).
fn clean_text(text: &str, known: &[String], budget: &mut usize) -> Option<String> {
    if *budget == 0 {
        return None;
    }
    let mut text = redact_text(text, known);
    if text.len() > *budget {
        let mut end = *budget;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(TRUNCATED);
    }
    *budget = budget.saturating_sub(text.len());
    Some(text)
}

/// Id ou nome curto que a redação não altera; senão a mensagem inteira não é guardada.
fn identifier(value: &Value, known: &[String]) -> Option<String> {
    let text = value.as_str()?;
    (text.len() <= MAX_LABEL && redact_text(text, known) == text).then(|| text.to_owned())
}

/// Rótulo do motor (api, provedor, modelo): só caracteres de identificador e nada que a redação alteraria.
fn label(value: &Value, known: &[String]) -> String {
    let text = value.as_str().unwrap_or_default();
    let plain = text.chars().all(|c| c.is_ascii_alphanumeric() || "._:/+-".contains(c));
    if plain && identifier(value, known).is_some() { text.to_owned() } else { String::new() }
}

/// Motivo de parada: só os que o pi-ai produz e que viram conversa (erro e abortado já saíram antes).
fn stop_reason(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "stop" => Some("stop"),
        "length" => Some("length"),
        "toolUse" => Some("toolUse"),
        _ => None,
    }
}

/// Um contador de uso: número finito e não negativo (inteiro quando é inteiro, como o JSON do motor), senão 0.
fn count(value: &Value) -> Value {
    if let Some(n) = value.as_u64() {
        return n.into();
    }
    match value.as_f64() {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(f) if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 9.0e15 => (f as u64).into(),
        Some(f) if f.is_finite() && f >= 0.0 => json!(f),
        _ => 0.into(),
    }
}

/// Uso e custo: SÓ os campos permitidos, sempre os mesmos, sempre números (a forma que o receptor do motor também devolve).
fn usage(raw: &Value) -> Value {
    let cost = &raw["cost"];
    json!({
        "input": count(&raw["input"]),
        "output": count(&raw["output"]),
        "cacheRead": count(&raw["cacheRead"]),
        "cacheWrite": count(&raw["cacheWrite"]),
        "totalTokens": count(&raw["totalTokens"]),
        "cost": {
            "input": count(&cost["input"]),
            "output": count(&cost["output"]),
            "cacheRead": count(&cost["cacheRead"]),
            "cacheWrite": count(&cost["cacheWrite"]),
            "total": count(&cost["total"]),
        },
    })
}

/// Blocos de texto de `content` (string ou lista), limpos e dentro do orçamento.
fn text_blocks(content: &Value, known: &[String], budget: &mut usize) -> Vec<Value> {
    match content {
        Value::String(text) => {
            clean_text(text, known, budget).map(|t| json!({"type": "text", "text": t})).into_iter().collect()
        }
        Value::Array(blocks) => blocks
            .iter()
            .take(MAX_BLOCKS)
            .filter(|block| block["type"] == "text")
            .filter_map(|block| clean_text(block["text"].as_str()?, known, budget))
            .map(|t| json!({"type": "text", "text": t}))
            .collect(),
        _ => Vec::new(),
    }
}

/// A cópia enxuta de uma mensagem com o texto dentro de `budget` bytes brutos, ou `None`.
fn build(message: &Value, known: &[String], mut budget: usize) -> Option<Value> {
    let timestamp = message["timestamp"].as_i64().unwrap_or(0);
    match message["role"].as_str()? {
        "user" => {
            let content = text_blocks(&message["content"], known, &mut budget);
            (!content.is_empty()).then(|| json!({"role": "user", "content": content, "timestamp": timestamp}))
        }
        "assistant" => {
            let stop = stop_reason(&message["stopReason"])?;
            let mut content = Vec::new();
            let mut call_ids: Vec<String> = Vec::new();
            let mut texts = 0;
            for block in message["content"].as_array()? {
                match block["type"].as_str() {
                    Some("text") => {
                        texts += 1;
                        if texts <= MAX_BLOCKS {
                            content.extend(
                                clean_text(block["text"].as_str()?, known, &mut budget)
                                    .map(|t| json!({"type": "text", "text": t})),
                            );
                        }
                    }
                    Some("toolCall") => {
                        // Cada chamada precisa do resultado dela: passar do teto de chamadas descarta a mensagem.
                        if call_ids.len() >= MAX_BLOCKS {
                            return None;
                        }
                        let mut arguments = redigir_json(&block["arguments"], known);
                        if !arguments.is_object() || arguments.to_string().len() > budget {
                            arguments = json!({});
                        }
                        budget = budget.saturating_sub(arguments.to_string().len());
                        let id = identifier(&block["id"], known)?;
                        if call_ids.contains(&id) {
                            return None;
                        }
                        content.push(json!({
                            "type": "toolCall",
                            "id": id,
                            "name": identifier(&block["name"], known)?,
                            "arguments": arguments,
                        }));
                        call_ids.push(id);
                    }
                    _ => {}
                }
            }
            if content.is_empty() {
                return None;
            }
            let mut out = Map::new();
            out.insert("role".into(), "assistant".into());
            out.insert("content".into(), Value::Array(content));
            for key in ["api", "provider", "model"] {
                out.insert(key.into(), label(&message[key], known).into());
            }
            out.insert("stopReason".into(), stop.into());
            out.insert("usage".into(), usage(&message["usage"]));
            out.insert("timestamp".into(), timestamp.into());
            Some(Value::Object(out))
        }
        "toolResult" => Some(json!({
            "role": "toolResult",
            "toolCallId": identifier(&message["toolCallId"], known)?,
            "toolName": identifier(&message["toolName"], known)?,
            "content": text_blocks(&message["content"], known, &mut budget),
            "isError": message["isError"] == true,
            "timestamp": timestamp,
        })),
        _ => None,
    }
}

/// A cópia enxuta de uma mensagem do motor, ou `None` se ela não deve ser guardada. O texto é cortado em bytes
/// brutos e o JSON escapa aspas e controles, então o resultado é MEDIDO e o orçamento cai até o JSON caber no teto.
pub(super) fn sanitize(message: &Value, known: &[String]) -> Option<Value> {
    let mut budget = MAX_MESSAGE_BYTES - ENVELOPE_BYTES;
    loop {
        let clean = build(message, known, budget)?;
        let bytes = size(&clean);
        if bytes <= MAX_MESSAGE_BYTES {
            return Some(clean);
        }
        if budget == 0 {
            return None;
        }
        // O orçamento cai na proporção do que passou do teto (poucas voltas, cada uma refaz a redação), e sempre
        // pelo menos um byte.
        budget = (budget * MAX_MESSAGE_BYTES / bytes).min(budget - 1);
    }
}

/// Bytes UTF-8 do JSON de uma mensagem: a unidade de todos os tetos (a mesma do motor).
fn size(message: &Value) -> usize {
    message.to_string().len()
}

/// Bytes UTF-8 do ARRAY JSON de `count` mensagens que somam `sum` bytes: colchetes e vírgulas inclusos.
fn array_bytes(sum: usize, count: usize) -> usize {
    2 + sum + count.saturating_sub(1)
}

fn role(message: &Value) -> &str {
    message["role"].as_str().unwrap_or_default()
}

/// Onde acaba o grupo que começa em `start`: a mensagem do assistente e os resultados das
/// chamadas dele, que saem juntos; qualquer outra mensagem é um grupo de uma só.
fn group_end(messages: &[Value], start: usize) -> usize {
    let ids: Vec<&str> = messages[start]["content"]
        .as_array()
        .map(|blocks| blocks.iter().filter(|b| b["type"] == "toolCall").filter_map(|b| b["id"].as_str()).collect())
        .unwrap_or_default();
    let mut end = start + 1;
    while role(&messages[start]) == "assistant"
        && end < messages.len()
        && role(&messages[end]) == "toolResult"
        && messages[end]["toolCallId"].as_str().is_some_and(|id| ids.contains(&id))
    {
        end += 1;
    }
    end
}

/// Tira a unidade mais antiga que pode sair e devolve o que saiu: a volta mais antiga inteira quando há mais
/// de uma; numa volta só, o grupo mais antigo depois do pedido. `None` quando só resta o pedido.
fn evict(messages: &mut Vec<Value>) -> Option<Vec<Value>> {
    let next_turn = messages.iter().skip(1).position(|m| role(m) == "user").map(|i| i + 1);
    let range = if let Some(end) = next_turn {
        0..end
    } else {
        let start = usize::from(role(messages.first()?) == "user");
        (start < messages.len() && messages.len() > 1).then(|| start..group_end(messages, start))?
    };
    Some(messages.drain(range).collect())
}

/// Aplica o teto de mensagens e o de bytes do array JSON (`sum`: soma dos bytes das mensagens); `true` se algo saiu.
fn fit(messages: &mut Vec<Value>, sum: &mut usize) -> bool {
    let mut dropped = false;
    while array_bytes(*sum, messages.len()) > MAX_HISTORY_BYTES || messages.len() > MAX_HISTORY_MESSAGES {
        let Some(removed) = evict(messages) else { break };
        *sum -= removed.iter().map(size).sum::<usize>();
        dropped = true;
    }
    dropped
}

/// Mensagens que o motor terminou de emitir NESTE turno, já limpas e já dentro do
/// teto: a coleta nunca cresce além dele, mesmo num turno gigante.
pub(super) struct TurnLog {
    /// `None`: o Hub não conseguiu ler o que a proteção precisa; nada do turno é guardado.
    known: Option<Vec<String>>,
    messages: Vec<Value>,
    bytes: usize,
    dropped: bool,
}

impl TurnLog {
    pub(super) fn new(known: Vec<String>) -> Self {
        Self { known: Some(known), messages: Vec::new(), bytes: 0, dropped: false }
    }

    /// Sem a leitura que a proteção exige (cofre indisponível, chave curta demais): o turno não é guardado.
    pub(super) fn unprotected() -> Self {
        Self { known: None, messages: Vec::new(), bytes: 0, dropped: false }
    }

    /// `message_end` do motor. Passou do teto: saem os grupos mais antigos do turno (o pedido fica);
    /// resultado de uma chamada que já saiu também não entra.
    pub(super) fn push(&mut self, raw: &Value) {
        let Some(known) = &self.known else { return };
        let Some(message) = sanitize(raw, known) else { return };
        if role(&message) == "toolResult" && !self.answers_open_call(&message) {
            self.dropped = true;
            return;
        }
        if role(&message) != "toolResult" {
            // Uma mensagem nova fecha a volta anterior: a chamada que ficou sem resultado sai agora, não enterrada.
            self.drop_open_calls();
        }
        self.bytes += size(&message);
        self.messages.push(message);
        self.dropped |= fit(&mut self.messages, &mut self.bytes);
    }

    /// O resultado responde a uma chamada ainda aberta da última mensagem do assistente: mesmo id e mesmo nome da
    /// ferramenta, e cada chamada recebe um resultado só (a mesma regra do receptor do motor).
    fn answers_open_call(&self, result: &Value) -> bool {
        let mut answered = self.messages.iter().rev().take_while(|m| role(m) == "toolResult");
        let Some(assistant) = self.messages.iter().rev().find(|m| role(m) != "toolResult") else { return false };
        role(assistant) == "assistant"
            && !answered.any(|m| m["toolCallId"] == result["toolCallId"])
            && assistant["content"].as_array().is_some_and(|blocks| {
                blocks.iter().any(|b| {
                    b["type"] == "toolCall" && b["id"] == result["toolCallId"] && b["name"] == result["toolName"]
                })
            })
    }

    /// Uma volta de ferramentas que não terminou (turno cancelado ou processo morto no meio): a mensagem do assistente
    /// e os resultados que já chegaram saem juntos. A conversa guardada nunca tem chamada sem o resultado dela.
    fn drop_open_calls(&mut self) {
        let Some(start) = self.messages.iter().rposition(|m| role(m) != "toolResult") else { return };
        let calls = self.messages[start]["content"]
            .as_array()
            .map_or(0, |blocks| blocks.iter().filter(|b| b["type"] == "toolCall").count());
        let answered = self.messages.len() - start - 1;
        if role(&self.messages[start]) == "assistant" && calls > answered {
            self.messages.truncate(start);
            self.bytes = self.messages.iter().map(size).sum();
            self.dropped = true;
        }
    }

    /// O que o turno guardou e se algo foi descartado (o processo vivo ainda tem esse algo).
    pub(super) fn take(&mut self) -> (Vec<Value>, bool) {
        self.drop_open_calls();
        self.bytes = 0;
        (std::mem::take(&mut self.messages), std::mem::take(&mut self.dropped))
    }
}

struct State {
    messages: Vec<Value>,
    /// Soma dos bytes das mensagens (o array JSON soma colchetes e vírgulas: ver `array_bytes`).
    bytes: usize,
    /// O recorte tirou algo que o processo vivo do motor ainda tem: o próximo turno reenvia a cópia.
    resync: bool,
    /// Geração da credencial de cada provedor que já recebeu esta conversa.
    credentials: HashMap<String, String>,
    /// Última atividade: um turno guardado, uma reabertura ou o fechamento da sessão.
    active: Instant,
}

/// A conversa de uma sessão, compartilhada entre o adaptador e o arquivo do Hub.
pub(super) struct History {
    state: Mutex<State>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                messages: Vec::new(),
                bytes: 0,
                resync: false,
                credentials: HashMap::new(),
                active: Instant::now(),
            }),
        }
    }
}

impl History {
    /// `dropped`: a coleta do turno já descartou algo (o recorte daqui também marca).
    pub(super) fn remember(&self, (new, dropped): (Vec<Value>, bool)) {
        let Ok(mut state) = self.state.lock() else { return };
        state.active = Instant::now();
        state.bytes += new.iter().map(size).sum::<usize>();
        state.messages.extend(new);
        let state = &mut *state;
        state.resync |= dropped | fit(&mut state.messages, &mut state.bytes);
    }

    pub(super) fn snapshot(&self) -> Vec<Value> {
        self.state.lock().map(|state| state.messages.clone()).unwrap_or_default()
    }

    /// `true` uma vez depois de um recorte: quem tem processo vivo reenvia a cópia.
    pub(super) fn take_resync(&self) -> bool {
        self.state.lock().is_ok_and(|mut state| std::mem::take(&mut state.resync))
    }

    /// Marca atividade (reabertura ou fechamento da sessão): o prazo sem uso conta daqui.
    pub(super) fn touch(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.active = Instant::now();
        }
    }

    /// Provas: finge que a ultima atividade foi ha mais tempo, sem esperar.
    #[cfg(test)]
    pub(super) fn backdate(&self, by: Duration) {
        if let Ok(mut state) = self.state.lock() {
            state.active = Instant::now().checked_sub(by).unwrap_or(state.active);
        }
    }

    fn idle(&self) -> Duration {
        self.state.lock().map_or(Duration::MAX, |state| state.active.elapsed())
    }

    /// A credencial de `provider` (a geração dela) vai receber esta conversa. Se o provedor já a recebeu com
    /// OUTRA geração, não há como provar que é a mesma conta: a cópia é apagada, o processo vivo recebe a lista
    /// vazia e a conversa recomeça. `true` quando a conversa segue. Geração vazia = sem credencial (nada a vincular).
    pub(super) fn bind_credential(&self, provider: &str, generation: &str) -> bool {
        if generation.is_empty() {
            return true;
        }
        let Ok(mut state) = self.state.lock() else { return false };
        if state.credentials.get(provider).is_some_and(|known| known != generation) {
            state.messages.clear();
            state.bytes = 0;
            state.resync = true;
            state.credentials.clear();
            state.credentials.insert(provider.to_owned(), generation.to_owned());
            return false;
        }
        state.credentials.insert(provider.to_owned(), generation.to_owned());
        true
    }

    /// O Hub não conseguiu proteger o que seria guardado (cofre ilegível, chave curta demais): nada fica guardado.
    pub(super) fn drop_unprotected(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.messages.clear();
            state.bytes = 0;
        }
    }
}

type Archive = HashMap<(String, String), Arc<History>>;

/// Conversas por (dono, sessão) que sobrevivem ao adaptador: trocar de modelo no
/// app fecha a conexão e a nova pede `session/load` da mesma sessão. O dono é o bot
/// mais a identidade efetiva dele (Space, conta e revisão): ver `conversation_owner`.
static ARCHIVE: LazyLock<Mutex<Archive>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Tira o que ficou sem atividade além do prazo e abre `room` vaga(s) tirando as mais antigas sem uso; as vivas ficam.
fn purge(archive: &mut Archive, room: usize) {
    purge_with(archive, room, IDLE_TTL);
}

fn purge_with(archive: &mut Archive, room: usize, ttl: Duration) {
    archive.retain(|_, history| history.idle() < ttl || Arc::strong_count(history) > 1);
    while archive.len() + room > MAX_SESSIONS {
        let oldest = archive
            .iter()
            .filter(|(_, history)| Arc::strong_count(history) == 1)
            .max_by_key(|(_, history)| history.idle())
            .map(|(key, _)| key.clone());
        let Some(key) = oldest else { break };
        archive.remove(&key);
    }
}

/// O arquivo tem teto de verdade: com todas as vagas em uso por sessões vivas, recusa a conversa nova.
fn insert(archive: &mut Archive, owner: &str, sid: &str) -> Result<Arc<History>> {
    purge(archive, 1);
    ensure!(archive.len() < MAX_SESSIONS, "sessões demais abertas no Motor Mexerica");
    let history = Arc::new(History::default());
    archive.insert((owner.to_owned(), sid.to_owned()), history.clone());
    Ok(history)
}

fn lookup(archive: &mut Archive, owner: &str, sid: &str) -> Option<Arc<History>> {
    purge(archive, 0);
    let history = archive.get(&(owner.to_owned(), sid.to_owned()))?;
    history.touch();
    Some(history.clone())
}

/// Apaga a conversa da sessão `sid` do bot (qualquer identidade) e esvazia a cópia, mesmo que um adaptador ainda a segure.
fn forget(archive: &mut Archive, bot: &str, sid: &str) {
    let prefix = format!("{bot}{OWNER_SEPARATOR}");
    archive.retain(|(owner, id), history| {
        let gone = id == sid && owner.starts_with(&prefix);
        if gone {
            history.drop_unprotected();
        }
        !gone
    });
}

fn archive() -> Result<std::sync::MutexGuard<'static, Archive>> {
    ARCHIVE.lock().map_err(|_| anyhow::anyhow!("arquivo de conversas indisponível"))
}

/// Conversa nova de uma sessão nova.
pub(super) fn create(owner: &str, sid: &str) -> Result<Arc<History>> {
    ensure_sweeper();
    let mut archive = archive()?;
    insert(&mut archive, owner, sid)
}

/// A conversa guardada de uma sessão DESTE dono, ou `None` (outro dono, expirada ou desconhecida).
pub(super) fn reopen(owner: &str, sid: &str) -> Option<Arc<History>> {
    ensure_sweeper();
    let mut archive = archive().ok()?;
    lookup(&mut archive, owner, sid)
}

/// Aplica o prazo sem atividade agora (o arquivo também o aplica a cada abertura e reabertura).
pub(super) fn sweep() {
    if let Ok(mut archive) = archive() {
        purge(&mut archive, 0);
    }
}

/// De quanto em quanto tempo a tarefa de limpeza apaga, de fato, o que passou do prazo sem atividade.
const SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);

static SWEEPER: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

/// Garante a tarefa que apaga fisicamente as conversas vencidas: sem ela, a cópia só sairia da memória na próxima
/// operação do arquivo. Sessões ainda abertas num adaptador não são tocadas. Sem runtime (provas síncronas), nada roda.
fn ensure_sweeper() {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let Ok(mut sweeper) = SWEEPER.lock() else { return };
    if sweeper.as_ref().is_some_and(|task| !task.is_finished()) {
        return;
    }
    *sweeper = Some(runtime.spawn(async {
        let mut tick = tokio::time::interval(SWEEP_EVERY);
        loop {
            tick.tick().await;
            sweep();
        }
    }));
}

/// A tarefa de limpeza está viva (provas).
#[cfg(test)]
pub(super) fn sweeper_running() -> bool {
    SWEEPER.lock().is_ok_and(|sweeper| sweeper.as_ref().is_some_and(|task| !task.is_finished()))
}

/// Apaga a conversa guardada de UMA sessão do bot (a sessão foi invalidada); as demais ficam.
pub(crate) fn forget_session(bot: &str, sid: &str) {
    if let Ok(mut archive) = archive() {
        forget(&mut archive, bot, sid);
    }
}

/// Guarda uma conversa de OUTRA sessão do mesmo bot (como a de uma rotina) para as provas do ator.
#[cfg(test)]
pub(crate) fn seed(bot: &str, sid: &str) {
    drop(create(&super::conversation_owner(bot, "rotina"), sid));
}

/// Quantas conversas do bot estão guardadas (provas do ator).
#[cfg(test)]
pub(crate) fn stored_for(bot: &str) -> usize {
    let prefix = format!("{bot}{OWNER_SEPARATOR}");
    archive().map_or(0, |archive| archive.keys().filter(|(owner, _)| owner.starts_with(&prefix)).count())
}

#[cfg(test)]
mod tests;
