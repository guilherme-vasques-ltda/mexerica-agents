//! R3: mensagens recebidas no número do bot vão para a caixa própria do `WhatsApp`
//! (`donna_whatsapp_messages`, ver `messages.rs`), como conteúdo de fora (D125). Nunca para
//! o chat do bot, as Sessões, o motor ou o histórico consultável; sem resposta.
//!
//! - O webhook do wacli é só um aviso ("chegou algo"): o Hub escuta em `127.0.0.1`, porta
//!   efêmera, com segredo HMAC novo a cada início do `sync` do bot. O corpo do aviso nunca é
//!   lido como dado; aviso de fora do computador ou com assinatura errada é ignorado.
//! - A fonte da verdade é o `wacli.db`, aberto só para leitura, com cursor por `rowid`
//!   guardado por bot. Uma leitura de segurança roda a cada [`POLL`] se um aviso se perder.
//! - A checagem "este bot pode ver" é só a do registro de plugins (`whatsapp_grants`).
use crate::{
    LockExt,
    fork::plugins::whatsapp_grants::{self, Read},
    hub::Hub,
};
use anyhow::{Context, Result};
use axum::{
    Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use hmac::{KeyInit as _, Mac as _};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::Sha256;
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Notify, watch};

/// Leitura de segurança: pega o que um aviso perdido não trouxe.
const POLL: Duration = Duration::from_secs(60);
/// Limpeza do prazo de guarda das caixas.
const SWEEP: Duration = Duration::from_secs(3600);
/// Mensagem lida com mais idade que isto chegou com o computador ou o wacli fora.
const OFFLINE_AFTER_MS: i64 = 3 * 60 * 1000;
const BATCH: i64 = 200;
/// Teto de linhas de chamada já vistas relidas por passada.
const RECONCILE_MAX: i64 = 500;
const MAX_TEXT: usize = 4000;
const MAX_BODY: usize = 1 << 20;

/// O que o receptor de avisos e os leitores dividem. Sem `Hub`: o receptor só acorda.
pub(super) struct Shared {
    endpoint: tokio::sync::Mutex<Option<(u16, tokio::task::JoinHandle<()>)>>,
    bots: Mutex<HashMap<String, Waker>>,
    poll: Mutex<Duration>,
    #[cfg(all(test, unix))]
    fetch_pause: Mutex<Option<FetchPause>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            endpoint: tokio::sync::Mutex::default(),
            bots: Mutex::default(),
            poll: Mutex::new(POLL),
            #[cfg(all(test, unix))]
            fetch_pause: Mutex::default(),
        }
    }
}

#[cfg(all(test, unix))]
struct FetchPause {
    bot: String,
    reached: Arc<Notify>,
    resume: std::sync::mpsc::Receiver<()>,
    timeout: Duration,
}

struct Waker {
    secret: String,
    notify: Arc<Notify>,
    /// R3b: acorda a fila de transcrição do bot quando chega voz.
    voice: Arc<Notify>,
}

/// Endereço e segredo que o `sync` do bot recebe por flag. O segredo fica visível na lista
/// de processos do próprio usuário (o wacli só aceita por flag): risco aceito no computador
/// do usuário; ele muda a cada início do `sync`.
pub(super) struct Hook {
    pub url: String,
    pub secret: String,
}

impl Shared {
    #[cfg(test)]
    pub(super) fn set_poll(&self, poll: Duration) {
        *self.poll.locked() = poll;
    }

    #[cfg(all(test, unix))]
    pub(super) fn pause_after_fetch(&self, bot: &str) -> (Arc<Notify>, std::sync::mpsc::Sender<()>) {
        self.pause_after_fetch_for(bot, Duration::from_secs(5))
    }

    #[cfg(all(test, unix))]
    pub(super) fn pause_after_fetch_for(
        &self,
        bot: &str,
        timeout: Duration,
    ) -> (Arc<Notify>, std::sync::mpsc::Sender<()>) {
        let reached = Arc::new(Notify::new());
        let (resume, receiver) = std::sync::mpsc::channel();
        *self.fetch_pause.locked() =
            Some(FetchPause { bot: bot.to_owned(), reached: reached.clone(), resume: receiver, timeout });
        (reached, resume)
    }

    /// Segredo novo e sinal de acordar do bot. Sem receptor, só a leitura de segurança vale.
    pub(super) async fn register(self: &Arc<Self>, bot: &str) -> (Option<Hook>, Arc<Notify>, Arc<Notify>) {
        let notify = Arc::new(Notify::new());
        let voice = Arc::new(Notify::new());
        let mut bytes = [0_u8; 32];
        let secret = getrandom::fill(&mut bytes).ok().map(|()| hex(&bytes));
        let port = match self.port().await {
            Ok(port) => Some(port),
            Err(error) => {
                tracing::warn!(error = %error, "whatsapp: receptor de avisos indisponível; só a leitura periódica");
                None
            }
        };
        let hook = secret
            .zip(port)
            .map(|(secret, port)| Hook { url: format!("http://127.0.0.1:{port}/whatsapp/{bot}"), secret });
        let secret = hook.as_ref().map_or_else(String::new, |h| h.secret.clone());
        self.bots.locked().insert(bot.to_owned(), Waker { secret, notify: notify.clone(), voice: voice.clone() });
        (hook, notify, voice)
    }

    fn wake_voice(&self, bot: &str) {
        if let Some(waker) = self.bots.locked().get(bot) {
            waker.voice.notify_one();
        }
    }

    pub(super) fn unregister(&self, bot: &str) {
        self.bots.locked().remove(bot);
    }

    async fn port(self: &Arc<Self>) -> Result<u16> {
        let mut endpoint = self.endpoint.lock().await;
        if let Some((port, task)) = endpoint.as_ref()
            && !task.is_finished()
        {
            return Ok(*port);
        }
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.context("whatsapp_hook_bind")?;
        let port = listener.local_addr()?.port();
        let app = Router::new()
            .route("/whatsapp/{bot}", post(receive))
            .layer(DefaultBodyLimit::max(MAX_BODY))
            .with_state(self.clone())
            .into_make_service_with_connect_info::<SocketAddr>();
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                tracing::warn!(error = %error, "whatsapp: receptor de avisos parou");
            }
        });
        *endpoint = Some((port, task));
        Ok(port)
    }

    pub(super) async fn shutdown(&self) {
        if let Some((_, task)) = self.endpoint.lock().await.take() {
            task.abort();
        }
    }

    /// Aviso aceito só do próprio computador, para um bot com `sync` de pé e com a
    /// assinatura `sha256=<HMAC do corpo>` do segredo atual. O corpo não é usado.
    pub(super) fn accept(&self, peer: SocketAddr, bot: &str, signature: Option<&str>, body: &[u8]) -> bool {
        if !peer.ip().is_loopback() {
            return false;
        }
        let bots = self.bots.locked();
        let Some(waker) = bots.get(bot).filter(|w| !w.secret.is_empty()) else { return false };
        let Some(given) = signature.and_then(|s| s.strip_prefix("sha256=")).and_then(unhex) else { return false };
        let Ok(mut mac) = hmac::Hmac::<Sha256>::new_from_slice(waker.secret.as_bytes()) else { return false };
        mac.update(body);
        if mac.verify_slice(&given).is_err() {
            return false;
        }
        waker.notify.notify_one();
        true
    }
}

async fn receive(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    UrlPath(bot): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let signature = headers.get("x-wacli-signature").and_then(|v| v.to_str().ok());
    if shared.accept(peer, &bot, signature, &body) { StatusCode::NO_CONTENT } else { StatusCode::UNAUTHORIZED }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

// MARK: leitor

/// Lê ao acordar (aviso), a cada `poll` e uma vez ao começar; sai quando `stop` vira `true`.
pub(super) async fn follow(
    hub: Arc<Hub>,
    bot: String,
    dir: PathBuf,
    mut stop: watch::Receiver<bool>,
    wake: Arc<Notify>,
) {
    loop {
        if *stop.borrow() {
            break;
        }
        let (h, b, d) = (hub.clone(), bot.clone(), dir.clone());
        let pass = tokio::task::spawn_blocking(move || read(&h, &b, &d));
        match pass.await {
            // Chegou algo: a fila de transcrição confere se há voz nova (R3b).
            Ok(Ok(added)) if added > 0 => hub.whatsapp.inbox().wake_voice(&bot),
            // Sem conteúdo no log: só o erro técnico.
            Ok(Err(error)) => tracing::warn!(error = %error, "whatsapp: leitura de mensagens falhou"),
            _ => {}
        }
        let poll = *hub.whatsapp.inbox().poll.locked();
        tokio::select! {
            () = wake.notified() => {}
            () = tokio::time::sleep(poll) => {}
            changed = stop.changed() => if changed.is_err() || *stop.borrow() { break },
        }
    }
}

#[allow(clippy::struct_excessive_bools, reason = "Colunas independentes do wacli.db.")]
#[derive(Debug)]
struct Row {
    rowid: i64,
    chat: String,
    chat_name: Option<String>,
    id: String,
    sender: Option<String>,
    sender_name: Option<String>,
    sender_phone: Option<String>,
    ts: i64,
    from_me: bool,
    text: String,
    media: Option<String>,
    reaction: bool,
    deleted: bool,
    quoted_mine: bool,
    quoted_sender: Option<String>,
    /// Remetente salvo na agenda do número (nome da agenda, não só o nome do perfil).
    contact: bool,
    /// Registro de chamada (`call_log`): a chamada entra pela tabela de chamadas, não como texto.
    call: bool,
    /// Tamanho e tipo declarados do arquivo de mídia (`messages.file_length`/`mime_type`).
    file_length: Option<i64>,
    mime: Option<String>,
}

const SELECT: &str = "SELECT m.rowid, m.chat_jid, COALESCE(NULLIF(m.chat_name,''), NULLIF(c.name,'')), m.msg_id,
    m.sender_jid, COALESCE(NULLIF(m.sender_name,''), NULLIF(ct.full_name,''), NULLIF(ct.push_name,''),
    NULLIF(ct.business_name,''), NULLIF(ct.first_name,'')), ct.phone, m.ts, m.from_me,
    COALESCE(m.display_text, m.text, ''), NULLIF(m.media_type,''), m.reaction_to_id IS NOT NULL,
    m.deleted_at IS NOT NULL OR m.revoked=1,
    m.quoted_msg_id IS NOT NULL AND EXISTS(SELECT 1 FROM messages q
        WHERE q.chat_jid=m.chat_jid AND q.msg_id=m.quoted_msg_id AND q.from_me=1),
    NULLIF(m.quoted_sender_jid,''), {CONTACT},
    EXISTS(SELECT 1 FROM call_events ce WHERE ce.chat_jid=m.chat_jid AND ce.msg_id=m.msg_id),
    m.file_length, NULLIF(m.mime_type,'')
    FROM messages m LEFT JOIN chats c ON c.jid=m.chat_jid LEFT JOIN contacts ct ON ct.jid=m.sender_jid
    WHERE m.rowid>?1 ORDER BY m.rowid LIMIT ?2";

/// Nome vindo da agenda do celular (ação `contact` do app state): `full_name`/`first_name`
/// (wacli `internal/store/contacts.go:153-160`) ou `system_name`. Só `push_name` = não salvo.
const CONTACT: &str =
    "(COALESCE(NULLIF(ct.full_name,''),NULLIF(ct.first_name,''),NULLIF(ct.system_name,'')) IS NOT NULL)";

/// Chamadas (wacli `call_events`, `internal/store/schema.sql:158-177`): uma linha por evento
/// (`offer`, `terminate`, `call_log`…) de cada `call_id`; `ts` em segundos (`calls.go:37-40`).
const CALLS: &str = "SELECT ce.rowid, ce.chat_jid, COALESCE(NULLIF(ce.chat_name,''), NULLIF(c.name,'')), ce.call_id,
    ce.sender_jid, COALESCE(NULLIF(ce.sender_name,''), NULLIF(ct.full_name,''), NULLIF(ct.push_name,''),
    NULLIF(ct.business_name,''), NULLIF(ct.first_name,'')), ct.phone, ce.ts, COALESCE(ce.direction,''),
    NULLIF(ce.media,''), ce.duration_secs, {CONTACT}
    FROM call_events ce LEFT JOIN chats c ON c.jid=ce.chat_jid LEFT JOIN contacts ct ON ct.jid=ce.sender_jid
    WHERE {WHERE} LIMIT ?2";

/// Novas linhas de chamada, depois do cursor.
const NEW_CALLS: &str = "ce.rowid>?1 ORDER BY ce.rowid";
/// Linhas já vistas e recentes: o wacli atualiza o mesmo `rowid` (duração, direção, tipo,
/// hora; `internal/store/calls.go:57-64,120-140`), e o cursor sozinho não vê a mudança.
/// Percorre a janela em páginas por `(ts, rowid)` pelo índice oficial `idx_call_events_ts`
/// (`internal/store/schema.sql:180`): `?3` = início da página (nunca antes da janela),
/// `?4` = último rowid da página anterior no mesmo `ts`. O `+` em `+ce.rowid` impede o
/// SQLite de trocar o índice de tempo pela chave primária e varrer o histórico.
const SEEN_CALLS: &str = "+ce.rowid<=?1 AND ce.ts>=?3 AND NOT (ce.ts=?3 AND +ce.rowid<=?4) ORDER BY ce.ts, ce.rowid";
/// Janela da reconciliação das chamadas já vistas (bem menor que o prazo de guarda).
const CALL_RECONCILE_SECS: i64 = 7 * 24 * 3600;

/// Um evento de chamada lido do `wacli.db`.
struct CallRow {
    rowid: i64,
    chat: String,
    chat_name: Option<String>,
    call_id: String,
    sender: Option<String>,
    sender_name: Option<String>,
    sender_phone: Option<String>,
    ts: i64,
    direction: String,
    media: Option<String>,
    secs: i64,
    contact: bool,
}

/// O `wacli.db` do bot só para leitura; o Hub nunca escreve nele.
fn open(path: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .context("whatsapp_db_open")?;
    c.busy_timeout(Duration::from_secs(2))?;
    c.execute_batch("PRAGMA query_only=1")?;
    Ok(c)
}

fn fetch(c: &Connection, after: i64) -> Result<Vec<Row>> {
    let mut stmt = c.prepare(&SELECT.replace("{CONTACT}", CONTACT))?;
    anyhow::ensure!(stmt.readonly(), "whatsapp_db_write");
    let rows = stmt.query_map(params![after, BATCH], |r| {
        Ok(Row {
            rowid: r.get(0)?,
            chat: r.get(1)?,
            chat_name: r.get(2)?,
            id: r.get(3)?,
            sender: r.get(4)?,
            sender_name: r.get(5)?,
            sender_phone: r.get(6)?,
            ts: r.get(7)?,
            from_me: r.get(8)?,
            text: r.get(9)?,
            media: r.get(10)?,
            reaction: r.get(11)?,
            deleted: r.get(12)?,
            quoted_mine: r.get(13)?,
            quoted_sender: r.get(14)?,
            contact: r.get(15)?,
            call: r.get(16)?,
            file_length: r.get(17)?,
            mime: r.get(18)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// SQL da leitura de chamadas: novas (`seen = false`) ou já vistas da janela (`true`).
pub(super) fn calls_sql(seen: bool) -> String {
    CALLS.replace("{CONTACT}", CONTACT).replace("{WHERE}", if seen { SEEN_CALLS } else { NEW_CALLS })
}

/// `page = Some((ts, rowid))`: a página seguinte da janela das já vistas, a partir de `ts`.
fn fetch_calls(c: &Connection, after: i64, page: Option<(i64, i64)>) -> Result<Vec<CallRow>> {
    let mut stmt = c.prepare(&calls_sql(page.is_some()))?;
    anyhow::ensure!(stmt.readonly(), "whatsapp_db_write");
    let limit = if page.is_some() { RECONCILE_MAX } else { BATCH };
    let bind = |r: &rusqlite::Row<'_>| -> rusqlite::Result<CallRow> {
        Ok(CallRow {
            rowid: r.get(0)?,
            chat: r.get(1)?,
            chat_name: r.get(2)?,
            call_id: r.get(3)?,
            sender: r.get(4)?,
            sender_name: r.get(5)?,
            sender_phone: r.get(6)?,
            ts: r.get(7)?,
            direction: r.get(8)?,
            media: r.get(9)?,
            secs: r.get(10)?,
            contact: r.get(11)?,
        })
    };
    let rows = match page {
        Some((ts, rowid)) => {
            stmt.query_map(params![after, limit, ts, rowid], bind)?.collect::<rusqlite::Result<_>>()?
        }
        None => stmt.query_map(params![after, limit], bind)?.collect::<rusqlite::Result<_>>()?,
    };
    Ok(rows)
}

/// Conversas fixadas no número (`chats.pinned`, wacli `internal/store/chats.go:56,87`).
fn pinned(c: &Connection) -> Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT jid FROM chats WHERE pinned=1 LIMIT 500")?;
    anyhow::ensure!(stmt.readonly(), "whatsapp_db_write");
    Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

/// Os JIDs do próprio número (telefone e LID), sem o aparelho: de quem são as mensagens `from_me`.
fn own_jids(c: &Connection) -> Result<HashSet<String>> {
    let mut stmt =
        c.prepare("SELECT DISTINCT sender_jid FROM messages WHERE from_me=1 AND sender_jid IS NOT NULL LIMIT 20")?;
    let jids = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(jids.iter().map(|j| bare(j)).collect())
}

/// `5511…:12@s.whatsapp.net` → `5511…@s.whatsapp.net`.
fn bare(jid: &str) -> String {
    let (user, server) = jid.split_once('@').unwrap_or((jid, ""));
    let user = user.split(':').next().unwrap_or(user);
    format!("{user}@{server}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Chat {
    Individual,
    Group,
}

/// Status, canais e listas de transmissão não são conversas do bot.
fn chat_kind(jid: &str) -> Option<Chat> {
    if jid.ends_with("@g.us") {
        Some(Chat::Group)
    } else if jid.ends_with("@s.whatsapp.net") || jid.ends_with("@lid") {
        Some(Chat::Individual)
    } else {
        None
    }
}

fn phone(jid: Option<&str>, contact: Option<&str>) -> Option<String> {
    let from_jid = jid.filter(|j| j.ends_with("@s.whatsapp.net")).map(bare).and_then(|j| {
        let user = j.split('@').next().unwrap_or_default().to_owned();
        (!user.is_empty() && user.chars().all(|c| c.is_ascii_digit())).then_some(user)
    });
    let digits = from_jid.or_else(|| {
        let d: String = contact?.chars().filter(char::is_ascii_digit).collect();
        (!d.is_empty()).then_some(d)
    })?;
    Some(format!("+{digits}"))
}

/// Rótulo do tipo de mídia, só dos tipos que o wacli marca (`internal/wa/messages_media.go`:
/// image :15, video/gif :26-31,86-88, audio :41, document :57, sticker :71, location :110,
/// `live_location` :120). O conteúdo continua fora (R3b). O wacli não separa voz de arquivo de
/// áudio nem guarda a duração do áudio (`messages` sem coluna de duração), então sem "(0:14)".
fn placeholder(media: &str) -> &'static str {
    match media {
        "audio" => "Mensagem de voz",
        "image" => "Foto",
        "video" => "Vídeo",
        "gif" => "GIF",
        "document" => "Documento",
        "sticker" => "Figurinha",
        "location" => "Localização",
        "live_location" => "Localização em tempo real",
        _ => "Mensagem",
    }
}

/// Rótulo da chamada (`call_events.media`: `audio`/`video`, `internal/wa/calls.go:281-286`).
fn call_label(media: Option<&str>) -> &'static str {
    match media {
        Some("video") => "Chamada de vídeo",
        Some("audio") => "Chamada de voz",
        _ => "Chamada",
    }
}

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(MAX_TEXT).collect();
    out.push('…');
    out
}

/// Uma mensagem que o bot pode guardar: só o necessário (D302).
#[allow(clippy::struct_excessive_bools, reason = "Marcas independentes de uma mensagem.")]
pub(super) struct Item {
    pub chat_id: String,
    pub message_id: String,
    pub group: bool,
    pub chat_name: Option<String>,
    pub sender_name: String,
    pub sender_phone: Option<String>,
    pub sent_at: i64,
    pub body: String,
    pub media: Option<String>,
    /// Chegou com o computador/wacli fora: nunca motivo de resposta automática (R4).
    pub offline: bool,
    /// Remetente salvo na agenda do número; sem isso o app mostra "~Nome" (como o `WhatsApp`).
    pub contact: bool,
    /// Chamada recebida: `audio`/`video` (ausente = não é chamada ou o wacli não informou).
    pub call: Option<Call>,
    /// Mensagem de voz: tamanho e tipo que o wacli informou, para a transcrição (R3b).
    pub file_length: Option<i64>,
    pub mime: Option<String>,
    /// Guardar já como "transcrevendo" (voz, com "Transcrever áudios recebidos" ligado).
    pub transcribe: bool,
}

pub(super) struct Call {
    pub media: Option<String>,
    pub secs: i64,
}

/// O que entra na caixa, ou `None` (fica só no cursor). `linked_at` separa o histórico
/// do pareamento, que nunca entra.
fn classify(row: &Row, own: &HashSet<String>, linked_at: i64, now: i64) -> Option<(Read, Item)> {
    if row.from_me || row.reaction || row.deleted || row.call {
        return None;
    }
    let chat = chat_kind(&row.chat)?;
    let sent_at = row.ts.saturating_mul(1000);
    if sent_at < linked_at {
        return None;
    }
    // Fase 1: em grupo, só a mensagem que responde a uma mensagem do bot.
    let reply_to_bot = row.quoted_mine || row.quoted_sender.as_deref().is_some_and(|q| own.contains(&bare(q)));
    if chat == Chat::Group && !reply_to_bot {
        return None;
    }
    let body = match row.media.as_deref() {
        None if row.text.trim().is_empty() => return None,
        None => clip(&row.text),
        Some(media) => placeholder(media).to_owned(),
    };
    let sender = row.sender.as_deref().or((chat == Chat::Individual).then_some(row.chat.as_str()));
    let sender_phone = phone(sender, row.sender_phone.as_deref());
    let sender_name = row.sender_name.clone().or_else(|| sender_phone.clone()).unwrap_or_else(|| "Contato".into());
    let read = if chat == Chat::Group { Read::Groups } else { Read::Chats };
    Some((
        read,
        Item {
            chat_id: row.chat.clone(),
            message_id: row.id.clone(),
            group: chat == Chat::Group,
            chat_name: row.chat_name.clone(),
            sender_name,
            sender_phone,
            sent_at,
            body,
            media: row.media.clone(),
            offline: now - sent_at > OFFLINE_AFTER_MS,
            contact: row.contact,
            call: None,
            file_length: row.file_length,
            mime: row.mime.clone(),
            transcribe: false,
        },
    ))
}

/// Junta um evento mais novo da mesma chamada: tipo (e rótulo) e duração que ele trouxer.
fn merge_call(merged: &mut Item, newer: Item) {
    let (Some(old), Some(new)) = (merged.call.as_mut(), newer.call) else { return };
    if new.media.is_some() {
        merged.body = newer.body;
        old.media = new.media;
    }
    if new.secs > 0 {
        old.secs = new.secs;
    }
}

/// Chamada individual e nova cuja direção o wacli ainda não sabe (`""`/`unknown`,
/// `internal/wa/calls.go:252-265`): fica esperando a reconciliação.
fn direction_pending(row: &CallRow, linked_at: i64) -> bool {
    matches!(row.direction.as_str(), "" | "unknown")
        && chat_kind(&row.chat) == Some(Chat::Individual)
        && row.ts.saturating_mul(1000) >= linked_at
}

/// Chamada recebida numa conversa individual vira um item da conversa (um por `call_id`).
/// Chamadas feitas pelo número, de grupo (fase 1: grupo só com resposta ao bot) e do
/// histórico do pareamento não entram.
fn classify_call(row: &CallRow, linked_at: i64, now: i64) -> Option<Item> {
    if row.direction != "inbound" || chat_kind(&row.chat)? != Chat::Individual {
        return None;
    }
    let sent_at = row.ts.saturating_mul(1000);
    if sent_at < linked_at {
        return None;
    }
    let media = row.media.clone().filter(|m| m == "audio" || m == "video");
    let sender = row.sender.as_deref().unwrap_or(&row.chat);
    let sender_phone = phone(Some(sender), row.sender_phone.as_deref());
    let sender_name = row.sender_name.clone().or_else(|| sender_phone.clone()).unwrap_or_else(|| "Contato".into());
    Some(Item {
        chat_id: row.chat.clone(),
        message_id: format!("call:{}", row.call_id),
        group: false,
        chat_name: row.chat_name.clone(),
        sender_name,
        sender_phone,
        sent_at,
        body: call_label(media.as_deref()).to_owned(),
        media: None,
        offline: now - sent_at > OFFLINE_AFTER_MS,
        contact: row.contact,
        call: Some(Call { media, secs: row.secs.max(0) }),
        file_length: None,
        mime: None,
        transcribe: false,
    })
}

/// Uma passada: lê tudo o que é novo desde o cursor e guarda o que o bot pode ver.
pub(super) fn read(hub: &Hub, bot: &str, dir: &Path) -> Result<usize> {
    let number: Option<(String, i64, Option<i64>, Option<i64>)> = hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT n.account_id,n.linked_at,c.last_rowid,c.last_call_rowid FROM donna_whatsapp_numbers n
             LEFT JOIN donna_whatsapp_cursors c ON c.bot_id=n.bot_id AND c.account_id=n.account_id
             WHERE n.bot_id=?1 AND n.state='linked'",
            [bot],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((account, linked_at, cursor, call_cursor)) = number else { return Ok(0) };
    let path = dir.join("wacli.db");
    if !path.is_file() {
        return Ok(0);
    }
    let db = open(&path)?;
    let own = own_jids(&db)?;
    let mut cursor = cursor.unwrap_or(0);
    let mut added = 0;
    // R3b: com "Transcrever áudios recebidos" ligado, a voz já entra como "transcrevendo".
    let transcribe = super::store::transcribe_audio(&hub.store.db.locked(), bot)?;
    loop {
        let rows = fetch(&db, cursor)?;
        #[cfg(all(test, unix))]
        {
            let pause = {
                let mut slot = hub.whatsapp.inbox().fetch_pause.locked();
                if !rows.is_empty() && slot.as_ref().is_some_and(|p| p.bot == bot) { slot.take() } else { None }
            };
            if let Some(pause) = pause {
                pause.reached.notify_one();
                pause.resume.recv_timeout(pause.timeout).context("fixture: lote não liberado")?;
            }
        }
        let Some(last) = rows.last().map(|r| r.rowid) else { break };
        let now = crate::store::now_ms();
        let c = hub.store.db.locked();
        let tx = c.unchecked_transaction()?;
        for row in &rows {
            let Some((read, mut item)) = classify(row, &own, linked_at, now) else { continue };
            item.transcribe = transcribe && item.media.as_deref() == Some("audio");
            // A checagem "pode ver" é só a do registro, mensagem a mensagem.
            if whatsapp_grants::can_read(&tx, &hub.host_id, &account, bot, read)? {
                added += super::messages::insert(&tx, bot, &account, &item, now)?;
            }
        }
        cursor = last;
        // Só grava se o número ainda é o mesmo: desconectar no meio não ressuscita o cursor.
        tx.execute(
            "INSERT INTO donna_whatsapp_cursors(bot_id,account_id,last_rowid)
             SELECT ?1,?2,?3 WHERE EXISTS(SELECT 1 FROM donna_whatsapp_numbers WHERE bot_id=?1 AND account_id=?2)
             ON CONFLICT(bot_id) DO UPDATE SET last_rowid=excluded.last_rowid,
             last_call_rowid=CASE WHEN account_id=excluded.account_id THEN last_call_rowid ELSE 0 END,
             account_id=excluded.account_id",
            params![bot, account, cursor],
        )?;
        tx.commit()?;
        drop(c);
        if i64::try_from(rows.len()).unwrap_or(BATCH) < BATCH {
            break;
        }
    }
    added += read_calls(hub, bot, &account, linked_at, &db, call_cursor.unwrap_or(0))?;
    let pins_changed = super::messages::set_pins(&hub.store.db.locked(), bot, &pinned(&db)?)?;
    if added > 0 || pins_changed {
        hub.whatsapp.notify(&hub.events, bot);
    }
    Ok(added)
}

/// As chamadas novas desde o cursor próprio, com a mesma checagem "pode ver" do registro.
fn read_calls(hub: &Hub, bot: &str, account: &str, linked_at: i64, db: &Connection, mut cursor: i64) -> Result<usize> {
    let mut added = 0;
    // Primeiro reconcilia as linhas já vistas e recentes (o wacli as atualiza no mesmo rowid):
    // o upsert completa tipo/duração e uma chamada antes recusada (ex.: direção ainda vazia)
    // entra agora. Só leitura do wacli.db; o cursor não muda.
    // Página a página até cobrir a janela inteira (nunca só as primeiras linhas).
    let mut page = (crate::store::now_ms() / 1000 - CALL_RECONCILE_SECS, 0);
    loop {
        let seen = fetch_calls(db, cursor, Some(page))?;
        let Some(last) = seen.last() else { break };
        page = (last.ts, last.rowid);
        let full = i64::try_from(seen.len()).unwrap_or(RECONCILE_MAX) >= RECONCILE_MAX;
        let now = crate::store::now_ms();
        let c = hub.store.db.locked();
        let tx = c.unchecked_transaction()?;
        // Os eventos de uma mesma chamada, em ordem de hora, viram um estado só: o tipo e a
        // duração mais recentes que o wacli informou. Uma escrita por chamada, sem vaivém.
        let mut calls: Vec<((String, String), Item)> = Vec::new();
        for row in &seen {
            let Some(item) = classify_call(row, linked_at, now) else { continue };
            let key = (row.chat.clone(), row.call_id.clone());
            match calls.iter_mut().find(|(k, _)| *k == key) {
                Some((_, merged)) => merge_call(merged, item),
                None => calls.push((key, item)),
            }
        }
        for ((chat, call_id), item) in &calls {
            // Só completa chamada já guardada, ou entra a que ficou esperando a direção.
            // Recusada por falta de concessão não volta depois.
            let known =
                super::messages::call_known(&tx, bot, item)? || super::messages::take_pending(&tx, bot, chat, call_id)?;
            if known && whatsapp_grants::can_read(&tx, &hub.host_id, account, bot, Read::Chats)? {
                added += super::messages::insert_call(&tx, bot, account, item, now)?;
            }
        }
        tx.commit()?;
        drop(c);
        if !full {
            break;
        }
    }
    loop {
        let rows = fetch_calls(db, cursor, None)?;
        let Some(last) = rows.last().map(|r| r.rowid) else { break };
        let now = crate::store::now_ms();
        let c = hub.store.db.locked();
        let tx = c.unchecked_transaction()?;
        for row in &rows {
            let item = classify_call(row, linked_at, now);
            let waiting = item.is_none() && direction_pending(row, linked_at);
            if (item.is_some() || waiting) && whatsapp_grants::can_read(&tx, &hub.host_id, account, bot, Read::Chats)? {
                match item {
                    Some(item) => added += super::messages::insert_call(&tx, bot, account, &item, now)?,
                    None => super::messages::mark_pending(&tx, bot, &row.chat, &row.call_id)?,
                }
            }
        }
        cursor = last;
        tx.execute(
            "INSERT INTO donna_whatsapp_cursors(bot_id,account_id,last_rowid,last_call_rowid)
             SELECT ?1,?2,0,?3 WHERE EXISTS(SELECT 1 FROM donna_whatsapp_numbers WHERE bot_id=?1 AND account_id=?2)
             ON CONFLICT(bot_id) DO UPDATE SET last_call_rowid=excluded.last_call_rowid,
             last_rowid=CASE WHEN account_id=excluded.account_id THEN last_rowid ELSE 0 END,
             account_id=excluded.account_id",
            params![bot, account, cursor],
        )?;
        tx.commit()?;
        drop(c);
        if i64::try_from(rows.len()).unwrap_or(BATCH) < BATCH {
            break;
        }
    }
    Ok(added)
}

/// Prazo de guarda de todas as caixas: no início do Hub e de hora em hora, com ou sem
/// número conectado, plugin ligado ou bot pausado. As leituras já não entregam vencidas.
pub(super) async fn sweep(hub: Arc<Hub>) {
    loop {
        let h = hub.clone();
        let pass = tokio::task::spawn_blocking(move || {
            super::messages::expire_all(&h.store.db.locked(), crate::store::now_ms())
        });
        match pass.await {
            Ok(Ok(bots)) => {
                for bot in bots {
                    hub.whatsapp.notify(&hub.events, &bot);
                }
            }
            Ok(Err(error)) => tracing::warn!(error = %error, "whatsapp: limpeza do prazo de guarda falhou"),
            Err(_) => {}
        }
        if hub.whatsapp.closing() {
            break;
        }
        tokio::time::sleep(SWEEP).await;
    }
}
