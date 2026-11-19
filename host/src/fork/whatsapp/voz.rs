//! R3b: transcreve as mensagens de voz recebidas e guarda o TEXTO na caixa do `WhatsApp`
//! (conteúdo de fora, D125; mesmo prazo de 30 dias). O bot só a lê pela ferramenta de leitura
//! do `WhatsApp`, quando pedido (R4a); fora dela, nunca vai ao motor, ao histórico nem ao log
//! (nem texto, nem áudio).
//!
//! Uma transcrição por vez por bot, numa tarefa própria: a entrada de texto não espera. O
//! áudio é baixado só pelo wacli oficial (`wacli::download`, só leitura) para uma pasta
//! temporária dentro da pasta `0700` do bot e apagado logo depois, com sucesso ou falha.
//! Custo: cobrado na conta do provedor do dono (`voice::transcribe`, provedor e modelo de
//! transcrição do computador, R1).
use super::{
    messages::{self, Voice},
    wacli,
};
use crate::{
    LockExt,
    fork::plugins::whatsapp_grants::{self, Read},
    hub::Hub,
    voice::Provider,
};
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rusqlite::{Connection, OptionalExtension as _};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Notify, watch};

/// Limites dos provedores (os mesmos de `voice::check_size`): `OpenAI` 25 MiB de arquivo;
/// Gemini 20 MiB em base64, ou seja, até 15 MiB de arquivo.
const OPENAI_MAX: i64 = 25 * 1024 * 1024;
const GEMINI_MAX: i64 = 15 * 1024 * 1024;
const POLL: Duration = Duration::from_secs(60);
/// Mensagens de voz por passada (uma de cada vez).
const BATCH: i64 = 20;
const MAX_TEXT: usize = 4000;

fn max_bytes(p: Provider) -> i64 {
    match p {
        Provider::OpenAi => OPENAI_MAX,
        Provider::Gemini => GEMINI_MAX,
    }
}

fn provider_key(p: Provider) -> &'static str {
    match p {
        Provider::OpenAi => "openai",
        Provider::Gemini => "gemini",
    }
}

/// Transcreve ao acordar (voz nova), a cada minuto e uma vez ao começar; sai com `stop`.
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
        if pass(&hub, &bot, &dir).await.is_err() {
            // Sem conteúdo no log: só que a passada falhou.
            tracing::warn!("whatsapp: a transcrição de mensagens de voz falhou");
        }
        tokio::select! {
            () = wake.notified() => {}
            () = tokio::time::sleep(POLL) => {}
            changed = stop.changed() => if changed.is_err() || *stop.borrow() { break },
        }
    }
}

/// O que fazer com uma voz antes de baixar qualquer coisa.
enum Plan {
    /// Não transcrever mais (opção desligada ou "Ver" revogado depois): fica só "Mensagem de voz".
    Skip,
    Final(&'static str),
    Go(Provider, String),
}

/// A voz ainda pode ser transcrita agora: plugin ligado, a conta dela ainda é a do número,
/// "Ver" (`can_read`, a concessão do registro) para essa conta e "Transcrever" ligado.
/// Conferido antes de baixar, de novo antes de mandar ao provedor e antes de gravar.
fn allowed(c: &Connection, host_id: &str, bot: &str, voice: &Voice) -> Result<bool> {
    let current: Option<String> = c
        .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [bot], |r| {
            r.get(0)
        })
        .optional()?;
    let read = if voice.group { Read::Groups } else { Read::Chats };
    Ok(current.as_deref() == Some(voice.account.as_str())
        && super::store::plugin_on(c, host_id)?
        && whatsapp_grants::can_read(c, host_id, &voice.account, bot, read)?
        && super::store::transcribe_audio(c, bot)?)
}

fn plan(hub: &Hub, bot: &str, voice: &Voice, provider: Option<&(Provider, String)>) -> Result<Plan> {
    if !allowed(&hub.store.db.locked(), &hub.host_id, bot, voice)? {
        return Ok(Plan::Skip);
    }
    let Some((p, model)) = provider else { return Ok(Plan::Final("no_key")) };
    if voice.file_length.is_some_and(|len| len > max_bytes(*p)) {
        return Ok(Plan::Final("too_large"));
    }
    if !voice.mime.as_deref().is_some_and(|m| m.starts_with("audio/ogg")) {
        return Ok(Plan::Final("unsupported"));
    }
    Ok(Plan::Go(*p, model.clone()))
}

/// Uma passada: transcreve, uma por vez, as vozes esperando. Devolve quantas terminaram.
/// Áudio que sobrou de uma queda do Hub entre o download e a leitura (o `Drop` de `Temp` não
/// rodou). A passada é a única que baixa áudio deste bot, então aqui não há download em curso.
/// `file_type` não segue links: um link é apagado, nunca o alvo. Se a própria pasta for um link,
/// não limpa nada (não apaga fora da pasta do bot).
fn clear_leftovers(dir: &Path) {
    let folder = dir.join("transcrever");
    if !std::fs::symlink_metadata(&folder).is_ok_and(|m| m.is_dir()) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| !t.is_dir()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub(super) async fn pass(hub: &Arc<Hub>, bot: &str, dir: &Path) -> Result<usize> {
    clear_leftovers(dir);
    let pending = messages::pending_voice(&hub.store.db.locked(), bot, BATCH)?;
    if pending.is_empty() {
        return Ok(0);
    }
    let provider = super::store::transcriber(&hub.store)?;
    let mut done = 0;
    for voice in &pending {
        let finished = match plan(hub, bot, voice, provider.as_ref())? {
            Plan::Skip => Some((None, None, None)),
            Plan::Final(state) => Some((Some(state), None, None)),
            Plan::Go(p, model) => match transcribe(hub, bot, dir, voice, p, &model).await {
                Ok(Outcome::Text(text)) => Some((Some("done"), Some(text), Some(provider_key(p)))),
                Ok(Outcome::TooLarge) => Some((Some("too_large"), None, None)),
                Ok(Outcome::Revoked) => Some((None, None, None)),
                Err(_) => None,
            },
        };
        {
            let c = hub.store.db.locked();
            // Mudou algo enquanto transcrevia (opção, "Ver", plugin ou número): não grava o texto.
            let finished = match finished {
                Some((Some("done"), ..)) if !allowed(&c, &hub.host_id, bot, voice)? => Some((None, None, None)),
                other => other,
            };
            match finished {
                Some((state, text, p)) => messages::finish_voice(&c, bot, voice, state, text.as_deref(), p)?,
                None => messages::fail_voice(&c, bot, voice)?,
            }
        }
        done += 1;
        hub.whatsapp.notify(&hub.events, bot);
    }
    Ok(done)
}

enum Outcome {
    Text(String),
    TooLarge,
    /// Deixou de valer entre o download e o provedor: o áudio não sai daqui.
    Revoked,
}

/// Arquivo temporário do áudio: apagado ao sair, com sucesso ou falha.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn transcribe(hub: &Arc<Hub>, bot: &str, dir: &Path, voice: &Voice, p: Provider, model: &str) -> Result<Outcome> {
    let binary = hub.whatsapp.binary().ok_or_else(|| anyhow::anyhow!("wacli_not_installed"))?;
    let folder = dir.join("transcrever");
    wacli::ensure_dir(&folder)?;
    let file = Temp(folder.join(format!("{}.ogg", uuid::Uuid::new_v4())));
    let bytes = wacli::download(&binary, dir, &voice.chat_id, &voice.message_id, &file.0).await?;
    if bytes > max_bytes(p) {
        return Ok(Outcome::TooLarge);
    }
    let len = i64::try_from(std::fs::metadata(&file.0)?.len()).unwrap_or(i64::MAX);
    if len > max_bytes(p) {
        return Ok(Outcome::TooLarge);
    }
    let audio = std::fs::read(&file.0)?;
    drop(file);
    #[cfg(test)]
    if let Some(hook) = hub.whatsapp.after_download() {
        hook(hub);
    }
    // O download leva até 180 s: confere de novo antes de mandar o áudio (e o custo) ao provedor.
    if !allowed(&hub.store.db.locked(), &hub.host_id, bot, voice)? {
        return Ok(Outcome::Revoked);
    }
    let text = provider_text(hub, p, model, &audio).await?;
    let mut text = text.trim().to_owned();
    if text.chars().count() > MAX_TEXT {
        text = text.chars().take(MAX_TEXT).collect::<String>() + "…";
    }
    Ok(Outcome::Text(text))
}

/// O provedor de transcrição do dono (`audio/ogg`, R0 #292). Nos testes, um provedor falso.
async fn provider_text(hub: &Arc<Hub>, p: Provider, model: &str, audio: &[u8]) -> Result<String> {
    #[cfg(test)]
    if let Some(fake) = hub.whatsapp.fake_transcriber() {
        return fake(hub, p, audio);
    }
    let value = crate::voice::transcribe(&hub.store, p, model, &STANDARD.encode(audio), Some("audio/ogg")).await?;
    Ok(value["text"].as_str().unwrap_or_default().to_owned())
}
