//! Gravação vinculada ao bot na mesma Tx; callbacks internos só usam a Connection recebida.
//! MCP conserva execução e mensagem humana originais até a mesma Tx; não emite Authority.
use crate::store::{BotConfig, Store};
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};

impl Store {
    pub(crate) fn guard_routines_commit_for_bot<'a>(
        &self,
        bot: &str,
        raw: &str,
        live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<()> {
        self.guard_routines_commit_bound(Some(bot), raw, live_runs)
    }

    pub(super) fn guard_routines_commit_bound<'a>(
        &self,
        bot: Option<&str>,
        raw: &str,
        live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<()> {
        self.guard_routines_commit_in_origin(bot, raw, live_runs, None)
    }

    pub(crate) fn guard_routines_commit_in_origin<'a>(
        &self,
        bot: Option<&str>,
        raw: &str,
        live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
        origin: Option<&crate::routines::tool_origin::ToolOrigin>,
    ) -> Result<()> {
        let live_runs = live_runs.into_iter().map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
        self.guard_routines_change_in_origin(bot, origin, |_| Ok(((), raw.to_owned(), live_runs)))
    }

    /// Admissão MCP e JSON/fila compartilham a guarda e a mesma IMMEDIATE.
    /// Callback síncrono: não chama Store/Hub, não abre Tx nem publica efeitos.
    pub(crate) fn guard_routines_change_in_origin<T>(
        &self,
        bot: Option<&str>,
        origin: Option<&crate::routines::tool_origin::ToolOrigin>,
        change: impl FnOnce(&Connection) -> Result<(T, String, Vec<(String, String)>)>,
    ) -> Result<T> {
        // D544: todos os bots, inclusive o Berga, podem agendar o próprio trabalho.
        // A autoridade original e o vínculo de sessão continuam sendo reconferidos na mesma Tx.
        let (result, removed, namespace) =
            crate::fork::donna_fixa::pedido::gravar(self, |tx| routines_change_on(tx, bot, origin, change))?;
        if let Some(namespace) = namespace {
            crate::guard_hub::mcp::runtime::forget_contexts(&namespace, &removed);
        }
        Ok(result)
    }
}

#[allow(clippy::type_complexity, reason = "Resultado, contextos removidos e namespace da mesma Tx.")]
fn routines_change_on<T>(
    tx: &Connection,
    bot: Option<&str>,
    origin: Option<&crate::routines::tool_origin::ToolOrigin>,
    change: impl FnOnce(&Connection) -> Result<(T, String, Vec<(String, String)>)>,
) -> Result<(T, Vec<String>, Option<String>)> {
    if let Some(origin) = origin {
        origin.check(tx, bot.context("routine requires its originating bot")?)?;
    }
    if let Some(bot) = bot {
        let config: String = tx
            .query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |row| row.get(0))
            .optional()?
            .context("routine requires an existing agent bot")?;
        let config: BotConfig =
            serde_json::from_str(&config).map_err(|_| anyhow!("routine requires an existing agent bot"))?;
        ensure!(config.id == bot && !config.is_group(), "routine requires an existing agent bot");
    }
    let namespace: Option<String> =
        tx.query_row("SELECT v FROM kv WHERE k='donna.guard.runtime.namespace'", [], |row| row.get(0)).optional()?;
    let (result, raw, live_runs) = change(tx)?;
    let removed = super::routines_commit_in(tx, &raw, live_runs.iter().map(|(bot, run)| (bot.as_str(), run.as_str())))?;
    Ok((result, removed, namespace))
}
