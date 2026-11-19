//! C6: relatório do dono. Só SELECT, PRAGMAs de leitura e metadados de arquivos.
use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) const METHOD: &str = "donnaStorageReport";

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    call_with_hook(hub, caller, body, || {}).await
}

async fn call_with_hook(
    hub: &Arc<Hub>,
    caller: &Caller,
    body: Value,
    after_measure: impl FnOnce() + Send + 'static,
) -> Result<Value> {
    authorize(caller, &body)?;
    let space = match body.get("spaceId") {
        Some(value) if !value.is_null() => value.as_str().ok_or_else(|| anyhow::anyhow!("invalid_space"))?,
        _ => crate::fork::spaces::PERSONAL,
    }
    .to_owned();
    hub.store.require_space(&space)?;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || {
        // Só a descoberta do caminho usa a conexão compartilhada. Todo o trabalho
        // pesado usa outra conexão READ_ONLY, sem Store::open nem migrações.
        let path = hub.store.db.locked().path().map(PathBuf::from);
        let result = report(path.as_deref(), &space);
        after_measure();
        // Descarte a medição se o Space desapareceu durante a leitura dos arquivos.
        finish_report(&hub, &space, result)
    })
    .await?
}

fn finish_report(hub: &Hub, space: &str, result: Report) -> Result<Value> {
    hub.store.require_space(space)?;
    Ok(serde_json::to_value(result)?)
}

fn authorize(caller: &Caller, body: &Value) -> Result<()> {
    super::bot_editor::authorization::require_owner(caller)?;
    ensure!(
        body.as_object().is_some_and(|o| o.keys().all(|k| k == "spaceId")),
        "invalid_field: consulta de armazenamento inválida."
    );
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    space_id: String,
    measured_at: i64,
    total: Total,
    bots: Option<Vec<Bot>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Total {
    context: &'static str,
    database_bytes: Option<u64>,
    wal_bytes: Option<u64>,
    database_free_bytes: Option<u64>,
    disk_free_bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bot {
    id: String,
    name: String,
    messages: Messages,
    attachments_bytes: Option<u64>,
    memory: Memory,
    whatsapp: Whatsapp,
    bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Messages {
    main: Option<MessagePart>,
    extras: Option<MessagePart>,
    bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessagePart {
    bytes: u64,
    count: u64,
    oldest_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Memory {
    rows_bytes: Option<u64>,
    files_bytes: Option<u64>,
    bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Whatsapp {
    hub_bytes: Option<u64>,
    wacli_bytes: Option<u64>,
    bytes: Option<u64>,
}

fn available<T>(part: &str, result: Result<T>) -> Option<T> {
    result.map_err(|error| tracing::warn!(part, %error, "storage measurement unavailable")).ok()
}

fn report(path: Option<&Path>, space: &str) -> Report {
    let db = path.and_then(|p| available("database", open(p)));
    let total = Total {
        context: "global",
        database_bytes: path.and_then(|p| available("database file", file_bytes(p, false))),
        wal_bytes: path.and_then(|p| available("database WAL", file_bytes(&suffix(p, "-wal"), true))),
        database_free_bytes: db.as_ref().and_then(|c| available("freelist", free_bytes(c))),
        disk_free_bytes: path
            .and_then(Path::parent)
            .and_then(|p| available("disk", fs2::available_space(p).map_err(Into::into))),
    };
    let bots = db.as_ref().and_then(|c| {
        available("bots", roster(c, space))
            .map(|rows| rows.into_iter().map(|(id, config)| bot(c, id, &config)).collect())
    });
    Report { space_id: space.to_owned(), measured_at: crate::store::now_ms(), total, bots }
}

fn open(path: &Path) -> Result<Connection> {
    ensure!(!path.as_os_str().is_empty(), "database has no file");
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    c.busy_timeout(std::time::Duration::from_secs(1))?;
    Ok(c)
}

fn bytes_column(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Integer, Box::new(e)))
}

fn free_bytes(c: &Connection) -> Result<u64> {
    let pages = c.query_row("PRAGMA freelist_count", [], |r| bytes_column(r, 0))?;
    let size = c.query_row("PRAGMA page_size", [], |r| bytes_column(r, 0))?;
    Ok(pages * size)
}

fn roster(c: &Connection, space: &str) -> Result<Vec<(String, String)>> {
    Ok(c.prepare(
        "SELECT id, config FROM bots WHERE deleted=0 AND CASE WHEN json_valid(config)
        THEN COALESCE(json_extract(config,'$.spaceId'),?2)=?1 ELSE ?1=?2 END ORDER BY id",
    )?
    .query_map(rusqlite::params![space, crate::fork::spaces::PERSONAL], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect::<rusqlite::Result<_>>()?)
}

fn bot(c: &Connection, id: String, config: &str) -> Bot {
    let cfg = available("bot config", serde_json::from_str::<crate::store::BotConfig>(config).map_err(Into::into));
    let name = if cfg.is_none() {
        format!("{id} (inválido)")
    } else {
        serde_json::from_str::<Value>(config)
            .ok()
            .and_then(|v| v["name"].as_str().map(str::to_owned))
            .unwrap_or_else(|| id.clone())
    };
    let main = available("main messages", messages(c, &id, false));
    let extras = available("extra messages", messages(c, &id, true));
    let message_bytes = sum([main.as_ref().map(|p| p.bytes), extras.as_ref().map(|p| p.bytes)]);
    let attachments_bytes =
        cfg.as_ref().and_then(|cfg| available("uploads", directory_bytes(&crate::chat::uploads::root(cfg))));
    let rows_bytes = available("memory rows", memory_rows(c, &id));
    let files_bytes = available("memory files", directory_bytes(&crate::chat::memory::files::folder(Some(&id))));
    let memory_bytes = sum([rows_bytes, files_bytes]);
    let hub_bytes = available("WhatsApp inbox", whatsapp_rows(c, &id));
    let wacli_bytes = available("wacli files", super::whatsapp::storage_bytes(&id));
    let whatsapp_bytes = sum([hub_bytes, wacli_bytes]);
    Bot {
        id,
        name,
        messages: Messages { main, extras, bytes: message_bytes },
        attachments_bytes,
        memory: Memory { rows_bytes, files_bytes, bytes: memory_bytes },
        whatsapp: Whatsapp { hub_bytes, wacli_bytes, bytes: whatsapp_bytes },
        bytes: sum([message_bytes, attachments_bytes, memory_bytes, whatsapp_bytes]),
    }
}

fn messages(c: &Connection, bot: &str, extras: bool) -> Result<MessagePart> {
    Ok(c.query_row(
        "SELECT COALESCE(SUM(length(CAST(data AS BLOB))),0),
            COUNT(CASE WHEN kind IN ('user','agent') THEN 1 END),
            MIN(CASE WHEN kind IN ('user','agent') THEN created_at END)
         FROM entries WHERE bot_id=?1 AND (session IS NOT NULL)=?2",
        rusqlite::params![bot, extras],
        |r| Ok(MessagePart { bytes: bytes_column(r, 0)?, count: bytes_column(r, 1)?, oldest_at: r.get(2)? }),
    )?)
}

fn memory_rows(c: &Connection, bot: &str) -> Result<u64> {
    // Conteúdo persistido atual + versões. Não rateia índices/FTS nem memória compartilhada.
    let mut bytes = 0;
    for sql in [
        "SELECT COALESCE(SUM(length(CAST(text AS BLOB))+length(CAST(evidence AS BLOB))+length(CAST(origin AS BLOB))+length(CAST(COALESCE(conflict_text,'') AS BLOB))),0) FROM memory_entries WHERE bot=?1",
        "SELECT COALESCE(SUM(length(CAST(v.text AS BLOB))+length(CAST(v.evidence AS BLOB))+length(CAST(v.origin AS BLOB))+length(CAST(COALESCE(v.conflict_text,'') AS BLOB))),0) FROM donna_memory_versions v JOIN memory_entries e ON e.id=v.entry WHERE e.bot=?1",
        "SELECT COALESCE(SUM(length(CAST(COALESCE(content,'') AS BLOB))+length(CAST(origin AS BLOB))+length(CAST(COALESCE(conflict_text,'') AS BLOB))),0) FROM memory_file_versions WHERE bot=?1",
        "SELECT COALESCE(SUM(length(CAST(name AS BLOB))+length(CAST(hash AS BLOB))+length(CAST(COALESCE(description,'') AS BLOB))),0) FROM memory_files WHERE bot=?1",
    ] {
        bytes += c.query_row(sql, [bot], |r| bytes_column(r, 0))?;
    }
    Ok(bytes)
}

fn whatsapp_rows(c: &Connection, bot: &str) -> Result<u64> {
    Ok(c.query_row(
        "SELECT COALESCE(SUM(length(CAST(body AS BLOB))+length(CAST(COALESCE(media,'') AS BLOB))+
            length(CAST(COALESCE(chat_name,'') AS BLOB))+length(CAST(sender_name AS BLOB))+
            length(CAST(COALESCE(sender_phone,'') AS BLOB))),0)
         FROM donna_whatsapp_messages WHERE bot_id=?1",
        [bot],
        |r| bytes_column(r, 0),
    )?)
}

fn sum<const N: usize>(parts: [Option<u64>; N]) -> Option<u64> {
    parts.into_iter().try_fold(0_u64, |a, p| a.checked_add(p?))
}

fn suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

pub(super) fn file_bytes(path: &Path, missing_is_zero: bool) -> Result<u64> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(meta.is_file(), "not a regular file");
            Ok(meta.len())
        }
        Err(e) if missing_is_zero && e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e.into()),
    }
}

fn directory_bytes(path: &Path) -> Result<u64> {
    let mut pending = vec![path.to_path_buf()];
    let mut bytes = 0_u64;
    while let Some(dir) = pending.pop() {
        let meta = match std::fs::symlink_metadata(&dir) {
            Ok(meta) => meta,
            Err(e) if dir == path && e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        ensure!(meta.is_dir(), "not a regular directory");
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                // Não segue links nem informa um total parcial como se fosse completo.
                ensure!(kind.is_file(), "unsupported file type");
                bytes = bytes.checked_add(entry.metadata()?.len()).ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            }
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) async fn test_call_with_hook(
    hub: &Arc<Hub>,
    body: Value,
    hook: impl FnOnce() + Send + 'static,
) -> Result<Value> {
    call_with_hook(hub, &Caller::Local, body, hook).await
}
