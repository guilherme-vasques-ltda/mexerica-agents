//! D389: o que os bots aprendem sozinhos sobre o dono. Uma escolha do Hub, para todos os bots
//! (inclusive o Berga): `auto` grava com Desfazer (D246, padrão), `suggest` guarda cada mudança
//! como sugestão pendente até o dono aprovar e `off` não extrai nem propõe nada.
//! Vale para a extração do bot de fundo e para `remember`/`revise_memory`/`archive_memory`/
//! `edit_memory`; a edição manual do dono no app não muda em nenhum modo.
//!
//! `memoryLearning {}` devolve `{mode, pending}`; `saveMemoryLearning {mode}` grava e devolve o
//! mesmo formato. O dono no próprio computador (Local) ou no app pareado com Control (Device): a autoridade
//! original é capturada antes de qualquer espera e conferida na Tx que grava (R5/SEG-491-01). Ponte e bot não.
use crate::{api::devices::Caller, fork::autoridade::Autoridade, hub::Hub, store::Store};
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const KEY: &str = "memory.learning";
pub(crate) const METHODS: [&str; 2] = ["memoryLearning", "saveMemoryLearning"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    Auto,
    Suggest,
    Off,
}

fn parse(raw: Option<String>) -> Mode {
    // Só esta API grava a chave, já validada; ausente é o comportamento de antes (D246).
    raw.and_then(|v| serde_json::from_value(Value::String(v)).ok()).unwrap_or_default()
}

/// Leitura na mesma Connection/Tx do chamador, sem reentrar no Store.
pub(crate) fn mode_in(db: &Connection) -> Result<Mode> {
    Ok(parse(db.query_row("SELECT v FROM kv WHERE k=?1", [KEY], |r| r.get(0)).optional()?))
}

pub(crate) fn mode(store: &Store) -> Result<Mode> {
    Ok(parse(store.kv_read(KEY)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    mode: Mode,
}

fn state(store: &Store) -> Result<Value> {
    Ok(json!({"mode": mode(store)?, "pending": crate::fork::memory_auto::suggestions::pending(store)?}))
}

pub(crate) fn call(hub: &Hub, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    let autoridade = Autoridade::capturar(hub, caller)?;
    if method == "saveMemoryLearning" {
        let Save { mode } =
            serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: escolha de memória inválida."))?;
        let raw = serde_json::to_value(mode)?;
        let raw = raw.as_str().ok_or_else(|| anyhow!("invalid_field"))?;
        autoridade.with_db(hub, None, |db| {
            db.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [KEY, raw])?;
            Ok(())
        })?;
    } else {
        anyhow::ensure!(body.as_object().is_none_or(serde_json::Map::is_empty), "invalid_field: consulta inválida");
    }
    state(&hub.store)
}
