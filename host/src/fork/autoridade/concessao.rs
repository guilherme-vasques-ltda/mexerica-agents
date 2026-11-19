//! Um só contador persistente para todos os escritores de devices, inclusive SQL.
use anyhow::{Result, ensure};
use rusqlite::Connection;

#[derive(Clone, Copy)]
enum Registro {
    Novo,
    Existente,
}

/// A mesma instrução é usada pelos gatilhos dentro da transação do escritor.
/// CHECK rejeita tipo inválido e overflow; nunca CAST/coerção/reparo de contador.
fn nova_concessao(chave: &str, registro: Registro) -> String {
    let exigir = match registro {
        Registro::Novo => String::new(),
        Registro::Existente => format!(
            "SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM donna_device_concessao WHERE chave={chave})
             THEN RAISE(ABORT,'integridade: geração ausente') END;"
        ),
    };
    format!(
        "{exigir} SELECT CASE WHEN EXISTS(SELECT 1 FROM donna_device_concessao WHERE chave={chave}
        AND (typeof(geracao)<>'integer' OR geracao<=0 OR geracao=9223372036854775807))
        THEN RAISE(ABORT,'integridade: contador inválido ou esgotado') END;
        INSERT INTO donna_device_concessao(chave,geracao) VALUES({chave},1)
        ON CONFLICT(chave) DO UPDATE SET geracao=geracao+1;"
    )
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    let existe: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_device_concessao')",
        [],
        |r| r.get(0),
    )?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_device_concessao(
        chave TEXT PRIMARY KEY NOT NULL,
        geracao INTEGER NOT NULL CHECK(typeof(geracao)='integer' AND geracao>0));",
    )?;
    if !existe {
        tx.execute("INSERT INTO donna_device_concessao SELECT key,1 FROM devices", [])?;
    }
    // Não preencher silenciosamente uma linha perdida em migração posterior.
    let faltam: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM devices d LEFT JOIN donna_device_concessao c ON c.chave=d.key WHERE c.chave IS NULL)",
        [],
        |r| r.get(0),
    )?;
    ensure!(!faltam, "integridade: geração de aparelho ausente");
    tx.execute_batch(&format!(
        "CREATE TRIGGER IF NOT EXISTS donna_concessao_insert AFTER INSERT ON devices BEGIN {} END;
        CREATE TRIGGER IF NOT EXISTS donna_concessao_delete AFTER DELETE ON devices BEGIN {} END;
        CREATE TRIGGER IF NOT EXISTS donna_concessao_update AFTER UPDATE OF key,source,grant_id,scopes ON devices
        BEGIN {} END;
        CREATE TRIGGER IF NOT EXISTS donna_concessao_nova_chave AFTER UPDATE OF key ON devices
        WHEN OLD.key IS NOT NEW.key BEGIN {} END;
        CREATE TRIGGER IF NOT EXISTS donna_concessao_prazo AFTER UPDATE OF lease_until ON devices
        WHEN OLD.lease_until IS NOT NEW.lease_until AND NOT (
            OLD.source='account' AND NEW.source='account' AND OLD.lease_until IS NOT NULL
            AND NEW.lease_until IS NOT NULL AND NEW.lease_until>=OLD.lease_until)
        BEGIN {} END;
        CREATE TRIGGER IF NOT EXISTS donna_concessao_preservar BEFORE DELETE ON donna_device_concessao
        BEGIN SELECT RAISE(ABORT,'integridade: geração persistente'); END;
        CREATE TRIGGER IF NOT EXISTS donna_concessao_monotona BEFORE UPDATE ON donna_device_concessao
        WHEN NEW.chave IS NOT OLD.chave OR typeof(NEW.geracao)<>'integer'
          OR NEW.geracao<=OLD.geracao
        BEGIN SELECT RAISE(ABORT,'integridade: geração inválida'); END;",
        nova_concessao("NEW.key", Registro::Novo),
        nova_concessao("OLD.key", Registro::Existente),
        nova_concessao("OLD.key", Registro::Existente),
        // Mudança de chave encerra a identidade antiga e abre a nova.
        nova_concessao("NEW.key", Registro::Novo),
        nova_concessao("OLD.key", Registro::Existente)
    ))?;
    tx.commit()?;
    Ok(())
}

pub(super) fn geracao(db: &Connection, chave: &str) -> Result<i64> {
    let valor: i64 =
        db.query_row("SELECT geracao FROM donna_device_concessao WHERE chave=?1", [chave], |r| r.get(0))?;
    ensure!(valor > 0 && valor < i64::MAX, "integridade: geração inválida ou esgotada");
    Ok(valor)
}
