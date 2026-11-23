//! Fronteira Rust interna, sem consumidor no binário. O produtor futuro deve autenticar
//! o contexto e as evidências fora desta crate; construir os tipos não concede autoridade.

use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const READONLY: &str = "https://www.googleapis.com/auth/gmail.readonly";
const TIMEOUT: Duration = Duration::from_secs(5);
const REFERENCE_TTL: Duration = Duration::from_secs(300);
const MAX_REFERENCES: usize = 512;
const PAGE_SIZE: usize = 25;
const MAX_MESSAGES: usize = 20;
const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct Context {
    pub person: Uuid,
    pub device: Uuid,
    pub workspace: Uuid,
    pub account: Uuid,
    pub installation: Uuid,
    pub generation: Uuid,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Definition {
    pub protocol_version: String,
    pub schema_digest: [u8; 32],
}

pub struct VerifiedState {
    pub context: Context,
    pub can_view: bool,
    pub definition: Definition,
    pub effective_scopes: Option<Vec<String>>,
}

pub struct ProviderRow {
    pub id: String,
    pub subject: Option<String>,
    pub last_message_at: DateTime<Utc>,
}

pub struct ProviderPage {
    pub threads: Vec<ProviderRow>,
    pub next_page_token: Option<String>,
}

pub struct ProviderMessage {
    pub sent_at: DateTime<Utc>,
    pub body_text: String,
}

pub struct ProviderThread {
    pub subject: Option<String>,
    pub messages: Vec<ProviderMessage>,
}

pub struct TransportFailure;

pub trait ReadTransport {
    fn list(
        &self,
        page_token: Option<&str>,
        limit: usize,
        cancel: CancellationToken,
    ) -> impl Future<Output = Result<ProviderPage, TransportFailure>> + Send;

    fn open(
        &self,
        thread_id: &str,
        cancel: CancellationToken,
    ) -> impl Future<Output = Result<ProviderThread, TransportFailure>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Denied,
    CursorInvalid,
    Unavailable,
}

pub struct Row {
    pub thread_id: Uuid,
    pub subject: Option<String>,
    pub last_message_at: DateTime<Utc>,
}

pub struct Page {
    pub threads: Vec<Row>,
    pub next_cursor: Option<Uuid>,
}

pub struct Message {
    pub message_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub body_text: String,
}

pub struct Thread {
    pub thread_id: Uuid,
    pub subject: Option<String>,
    pub messages: Vec<Message>,
    pub truncated: bool,
}

enum Reference {
    Thread(String),
    Cursor(String),
}

#[derive(Default)]
struct State {
    verified: Option<VerifiedState>,
    generation: Arc<CancellationToken>,
    references: HashMap<Uuid, (Instant, Reference)>,
}

/// Instância privada de uma instalação curada. Não autentica usuários nem cria transporte.
pub struct Reader<T> {
    account: Uuid,
    installation: Uuid,
    approved: Definition,
    transport: T,
    state: Mutex<State>,
}

impl<T: ReadTransport> Reader<T> {
    pub fn new(account: Uuid, installation: Uuid, approved: Definition, transport: T) -> Self {
        Self { account, installation, approved, transport, state: Mutex::default() }
    }

    /// Apenas o produtor confiável do Hub pode alimentar este estado. Toda troca invalida referências e pedidos.
    pub fn replace_verified_state(&self, verified: Option<VerifiedState>) -> Result<(), ReadError> {
        let mut state = self.state.lock().map_err(|_| ReadError::Unavailable)?;
        let old = std::mem::replace(&mut *state, State { verified, ..State::default() });
        drop(state);
        old.generation.cancel();
        Ok(())
    }

    pub async fn list(&self, context: &Context, cursor: Option<Uuid>) -> Result<Page, ReadError> {
        let (generation, page_token) = self.admit(context, cursor.map(Lookup::Cursor))?;
        let cancel = generation.child_token();
        let _cancel_on_exit = cancel.clone().drop_guard();
        let provider =
            run(&generation, async { self.transport.list(page_token.as_deref(), PAGE_SIZE, cancel).await }).await?;
        let (rows, next_token) = project_page(provider)?;
        self.release(context, &generation, |state| {
            let needed = rows.len() + usize::from(next_token.is_some());
            state.references.retain(|_, (expires, _)| *expires > Instant::now());
            if state.references.len() + needed > MAX_REFERENCES {
                return Err(ReadError::Unavailable);
            }
            let threads = rows
                .into_iter()
                .map(|row| Row {
                    thread_id: state.insert(Reference::Thread(row.id)),
                    subject: row.subject,
                    last_message_at: row.last_message_at,
                })
                .collect();
            let next_cursor = next_token.map(|token| state.insert(Reference::Cursor(token)));
            Ok(Page { threads, next_cursor })
        })
    }

    pub async fn open(&self, context: &Context, thread_id: Uuid) -> Result<Thread, ReadError> {
        let (generation, id) = self.admit(context, Some(Lookup::Thread(thread_id)))?;
        let id = id.ok_or(ReadError::Denied)?;
        let cancel = generation.child_token();
        let _cancel_on_exit = cancel.clone().drop_guard();
        let provider = run(&generation, async { self.transport.open(&id, cancel).await }).await?;
        let thread = project_thread(thread_id, provider)?;
        self.release(context, &generation, |_| Ok(thread))
    }

    fn check(&self, state: &State, context: &Context) -> Result<(), ReadError> {
        let verified = state.verified.as_ref().ok_or(ReadError::Denied)?;
        if &verified.context != context
            || context.account != self.account
            || context.installation != self.installation
            || !verified.can_view
            || !verified.effective_scopes.as_deref().is_some_and(|scopes| scopes == [READONLY])
        {
            return Err(ReadError::Denied);
        }
        if verified.definition != self.approved || self.approved.protocol_version.is_empty() {
            return Err(ReadError::Unavailable);
        }
        Ok(())
    }

    fn admit(
        &self,
        context: &Context,
        lookup: Option<Lookup>,
    ) -> Result<(Arc<CancellationToken>, Option<String>), ReadError> {
        let state = self.state.lock().map_err(|_| ReadError::Unavailable)?;
        self.check(&state, context)?;
        let argument = match lookup {
            None => None,
            Some(Lookup::Cursor(id)) => match state.lookup(id) {
                Some(Reference::Cursor(token)) => Some(token.clone()),
                _ => return Err(ReadError::CursorInvalid),
            },
            Some(Lookup::Thread(id)) => match state.lookup(id) {
                Some(Reference::Thread(id)) => Some(id.clone()),
                _ => return Err(ReadError::Denied),
            },
        };
        Ok((state.generation.clone(), argument))
    }

    fn release<R>(
        &self,
        context: &Context,
        generation: &Arc<CancellationToken>,
        commit: impl FnOnce(&mut State) -> Result<R, ReadError>,
    ) -> Result<R, ReadError> {
        let mut state = self.state.lock().map_err(|_| ReadError::Unavailable)?;
        self.check(&state, context)?;
        if !Arc::ptr_eq(&state.generation, generation) {
            return Err(ReadError::Denied);
        }
        commit(&mut state)
    }
}

enum Lookup {
    Cursor(Uuid),
    Thread(Uuid),
}

impl State {
    fn lookup(&self, id: Uuid) -> Option<&Reference> {
        self.references.get(&id).filter(|(expires, _)| *expires > Instant::now()).map(|(_, value)| value)
    }

    fn insert(&mut self, value: Reference) -> Uuid {
        loop {
            let id = Uuid::new_v4();
            if let std::collections::hash_map::Entry::Vacant(slot) = self.references.entry(id) {
                slot.insert((Instant::now() + REFERENCE_TTL, value));
                return id;
            }
        }
    }
}

async fn run<R>(
    generation: &CancellationToken,
    call: impl Future<Output = Result<R, TransportFailure>>,
) -> Result<R, ReadError> {
    if generation.is_cancelled() {
        return Err(ReadError::Denied);
    }
    tokio::select! {
        biased;
        () = generation.cancelled() => Err(ReadError::Denied),
        result = tokio::time::timeout(TIMEOUT, call) => result.ok().and_then(Result::ok).ok_or(ReadError::Unavailable),
    }
}

fn project_page(mut page: ProviderPage) -> Result<(Vec<ProviderRow>, Option<String>), ReadError> {
    let valid_reference = |value: &String| !value.is_empty() && value.len() <= 1024;
    if page.threads.len() > PAGE_SIZE || page.next_page_token.as_ref().is_some_and(|token| !valid_reference(token)) {
        return Err(ReadError::Unavailable);
    }
    for row in &mut page.threads {
        if !valid_reference(&row.id) || row.subject.as_ref().is_some_and(|s| s.len() > MAX_TEXT_BYTES) {
            return Err(ReadError::Unavailable);
        }
        row.subject = row.subject.take().map(|s| text(&s, 512, false).0);
    }
    page.threads.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at).then_with(|| a.id.cmp(&b.id)));
    Ok((page.threads, page.next_page_token))
}

fn project_thread(thread_id: Uuid, mut thread: ProviderThread) -> Result<Thread, ReadError> {
    if thread.messages.len() > 100
        || thread.messages.iter().any(|m| m.body_text.len() > 4 * MAX_TEXT_BYTES)
        || thread.subject.as_ref().is_some_and(|s| s.len() > MAX_TEXT_BYTES)
    {
        return Err(ReadError::Unavailable);
    }
    let mut truncated = thread.messages.len() > MAX_MESSAGES;
    let mut remaining = MAX_TEXT_BYTES;
    thread.messages.sort_by_key(|m| m.sent_at);
    let messages = thread
        .messages
        .into_iter()
        .take(MAX_MESSAGES)
        .map(|m| {
            let (body_text, cut) = text(&m.body_text, remaining, true);
            remaining -= body_text.len();
            truncated |= cut;
            Message { message_id: Uuid::new_v4(), sent_at: m.sent_at, body_text }
        })
        .collect();
    let subject = thread.subject.map(|s| {
        let (subject, cut) = text(&s, 512, false);
        truncated |= cut;
        subject
    });
    Ok(Thread { thread_id, subject, messages, truncated })
}

fn text(source: &str, max: usize, multiline: bool) -> (String, bool) {
    let mut out = String::new();
    for c in source.chars().filter(|&c| {
        !matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            && (!c.is_control() || multiline && matches!(c, '\n' | '\t'))
    }) {
        if out.len() + c.len_utf8() > max {
            return (out, true);
        }
        out.push(c);
    }
    (out, false)
}

#[cfg(test)]
mod tests;
