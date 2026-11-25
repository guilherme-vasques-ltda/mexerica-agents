//! Registro durável da porta de permissões do Motor no macOS: recusas e a
//! intenção registrada antes do efeito, na mesma sequência append-only — a
//! intenção fica ordenada com qualquer revogação seguinte. Cada linha carrega
//! só ids do escopo, a série do pedido, o motivo tipado, o caminho e o
//! instante: sem segredo, sem entrada bruta e sem conteúdo da ação.

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use std::num::NonZeroU64;
use std::path::Path;

use super::porta::Pedido;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Motivo {
    Negada,
    Expirada,
    Fechada,
    /// Intenção aprovada, registrada antes do efeito na mesma sequência das
    /// recusas — a revogação seguinte sempre fica depois dela.
    Intencao,
}

impl Motivo {
    fn codigo(self) -> &'static str {
        match self {
            Motivo::Negada => "negada",
            Motivo::Expirada => "expirada",
            Motivo::Fechada => "fechada",
            Motivo::Intencao => "intencao",
        }
    }
}

/// Caminho da porta que recusou: pedir, resolver, efetuar ou parar. Cada
/// linha do registro diz onde a recusa nasceu, sem entrada bruta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Caminho {
    Pedir,
    Resolver,
    Efetuar,
    Parar,
}

impl Caminho {
    fn codigo(self) -> &'static str {
        match self {
            Caminho::Pedir => "pedir",
            Caminho::Resolver => "resolver",
            Caminho::Efetuar => "efetuar",
            Caminho::Parar => "parar",
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
             motivo TEXT NOT NULL CHECK (motivo IN ('negada', 'expirada', 'fechada', 'intencao')),
             caminho TEXT NOT NULL CHECK (caminho IN ('pedir', 'resolver', 'efetuar', 'parar')),
             instancia INTEGER NOT NULL CHECK (instancia > 0),
             instante_ms INTEGER NOT NULL CHECK (instante_ms >= 0)
         ) STRICT",
        [],
    )
    .context("criar tabela de recusas do Motor no macOS")?;
    Ok(())
}

/// Grava a linha uma única vez, sem repetição: o erro propaga e o chamador
/// nega sem efeito. Vale para recusa e para intenção registrada. Só aceita o pedido opaco, o caminho, o motivo fechado e a
/// identidade da instância; o registro é append-only, sem deduplicação (a
/// unicidade de série entre portas ainda não está provada — a coluna
/// `instancia` identifica quem gravou, sem valer unicidade).
pub(super) fn registrar(
    db: &Connection,
    pedido: Pedido,
    caminho: Caminho,
    motivo: Motivo,
    instancia: NonZeroU64,
    instante_ms: u64,
) -> Result<()> {
    exigir_autocommit(db)?;
    exigir_durabilidade(db)?;
    let escopo = pedido.escopo();
    db.execute(
        "INSERT INTO recusas_motor_macos
             (titular, dispositivo, bot, sessao, geracao, serie, motivo, caminho, instancia, instante_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            i64_sql(escopo.titular.get(), "titular")?,
            i64_sql(escopo.dispositivo.get(), "dispositivo")?,
            i64_sql(escopo.bot.get(), "bot")?,
            i64_sql(escopo.sessao.get(), "sessão")?,
            i64_sql(escopo.geracao.get(), "geração")?,
            i64_sql(pedido.serie(), "série")?,
            motivo.codigo(),
            caminho.codigo(),
            i64_sql(instancia.get(), "instância")?,
            i64_sql(instante_ms, "instante")?,
        ],
    )
    .context("persistir recusa")?;
    Ok(())
}

/// Conexão durável do registro de recusas: aberta em WAL com
/// `synchronous=FULL` e `fullfsync` — o regime que o Store legado (aberto em
/// WAL+NORMAL) não fornece. A gravação de recusa recusa qualquer conexão
/// fora desse regime; esta é a única forma de apontar o registro ao banco
/// do Hub.
pub(super) struct Registro {
    db: Connection,
}

impl Registro {
    /// Abre a conexão do registro no banco indicado, já no regime durável, e
    /// prepara a tabela de recusas. Falha aqui significa recusa: sem retry
    /// e sem gravação por outra conexão.
    pub(super) fn abrir(caminho: &Path) -> Result<Self> {
        let db = Connection::open(caminho).context("abrir banco do registro de recusas")?;
        let journal: String = db
            .pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))
            .context("ligar WAL do registro de recusas")?;
        ensure!(journal == "wal", "journal do registro não ficou em WAL");
        db.pragma_update(None, "synchronous", "FULL").context("configurar sincronização do registro de recusas")?;
        db.pragma_update(None, "fullfsync", true).context("configurar flush do registro de recusas")?;
        exigir_durabilidade(&db)?;
        preparar(&db)?;
        Ok(Self { db })
    }

    /// Gravação única da recusa no regime durável; o erro propaga sem retry.
    pub(super) fn registrar(
        &self,
        pedido: Pedido,
        caminho: Caminho,
        motivo: Motivo,
        instancia: NonZeroU64,
        instante_ms: u64,
    ) -> Result<()> {
        registrar(&self.db, pedido, caminho, motivo, instancia, instante_ms)
    }
}

fn i64_sql(valor: u64, campo: &'static str) -> Result<i64> {
    i64::try_from(valor).with_context(|| format!("{campo} fora do intervalo do registro de recusas"))
}

fn exigir_autocommit(db: &Connection) -> Result<()> {
    ensure!(db.is_autocommit(), "transação externa aberta: recusa não pode ficar sujeita a rollback");
    Ok(())
}

fn exigir_durabilidade(db: &Connection) -> Result<()> {
    let journal: String =
        db.pragma_query_value(None, "journal_mode", |row| row.get(0)).context("consultar journal das recusas")?;
    ensure!(journal == "wal", "journal das recusas não é WAL; durabilidade confirmada só em WAL com synchronous FULL");
    let sincrono: i64 =
        db.pragma_query_value(None, "synchronous", |row| row.get(0)).context("consultar sincronização das recusas")?;
    ensure!(sincrono == 2, "sincronização das recusas não é FULL; gravação durável exigida");
    db.pragma_update(None, "fullfsync", true).context("configurar flush das recusas")?;
    Ok(())
}
