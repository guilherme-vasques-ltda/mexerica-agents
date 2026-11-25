//! Registro durável das recusas da porta de permissões do Motor no macOS.
//! Cada linha carrega só ids do escopo, a série do pedido, o motivo tipado e o
//! instante: sem segredo, sem entrada bruta e sem conteúdo da ação.

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};

use super::porta::Pedido;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Motivo {
    Negada,
    Expirada,
    Fechada,
}

impl Motivo {
    fn codigo(self) -> &'static str {
        match self {
            Motivo::Negada => "negada",
            Motivo::Expirada => "expirada",
            Motivo::Fechada => "fechada",
        }
    }
}

/// Cria a tabela de recusas no banco do Hub; idempotente e sem dados de exemplo.
pub(super) fn preparar(db: &Connection) -> Result<()> {
    exigir_durabilidade(db)?;
    db.execute(
        "CREATE TABLE IF NOT EXISTS recusas_motor_macos (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             titular INTEGER NOT NULL CHECK (titular > 0),
             dispositivo INTEGER NOT NULL CHECK (dispositivo > 0),
             bot INTEGER NOT NULL CHECK (bot > 0),
             sessao INTEGER NOT NULL CHECK (sessao > 0),
             geracao INTEGER NOT NULL CHECK (geracao > 0),
             serie INTEGER NOT NULL CHECK (serie > 0),
             motivo TEXT NOT NULL CHECK (motivo IN ('negada', 'expirada', 'fechada')),
             instante_ms INTEGER NOT NULL CHECK (instante_ms >= 0)
         ) STRICT",
        [],
    )
    .context("criar tabela de recusas do Motor no macOS")?;
    Ok(())
}

/// Grava a recusa uma única vez, sem repetição: o erro propaga e o chamador
/// nega sem efeito. Só aceita o pedido tipado e o motivo fechado.
pub(super) fn registrar(db: &Connection, pedido: Pedido, motivo: Motivo, instante_ms: u64) -> Result<()> {
    exigir_durabilidade(db)?;
    db.execute(
        "INSERT INTO recusas_motor_macos
             (titular, dispositivo, bot, sessao, geracao, serie, motivo, instante_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            i64_sql(pedido.escopo.titular.get(), "titular")?,
            i64_sql(pedido.escopo.dispositivo.get(), "dispositivo")?,
            i64_sql(pedido.escopo.bot.get(), "bot")?,
            i64_sql(pedido.escopo.sessao.get(), "sessão")?,
            i64_sql(pedido.escopo.geracao.get(), "geração")?,
            i64_sql(pedido.serie, "série")?,
            motivo.codigo(),
            i64_sql(instante_ms, "instante")?,
        ],
    )
    .context("persistir recusa")?;
    Ok(())
}

fn i64_sql(valor: u64, campo: &'static str) -> Result<i64> {
    i64::try_from(valor).with_context(|| format!("{campo} fora do intervalo do registro de recusas"))
}

fn exigir_durabilidade(db: &Connection) -> Result<()> {
    let sincrono: i64 =
        db.pragma_query_value(None, "synchronous", |row| row.get(0)).context("consultar sincronização das recusas")?;
    ensure!(sincrono == 2, "sincronização das recusas não é FULL; gravação durável exigida");
    db.pragma_update(None, "fullfsync", true).context("configurar flush das recusas")?;
    Ok(())
}
