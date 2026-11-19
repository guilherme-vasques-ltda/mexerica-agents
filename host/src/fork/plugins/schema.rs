//! Migração 1, apenas estrutura vazia, na conexão da Store existente.
use super::model::PluginError;
use rusqlite::{Connection, OptionalExtension};

// Gatilhos que citam `donna_plugin_installations`; o schema.sql e o agenda.sql
// (que `plugins::migrate` roda depois) os recriam.
const TRIGGERS: [&str; 6] = [
    "donna_scatty_events_parent",
    "donna_plugin_accounts_parent",
    "donna_plugin_resources_parent",
    "donna_plugin_grants_parent",
    "donna_plugin_installations_identity",
    "donna_plugin_installations_delete",
];

pub(super) fn migrate(c: &Connection) -> Result<(), PluginError> {
    let widen = needs_widen(c)?;
    let whatsapp = needs_whatsapp_kinds(c)?;
    // O SQLite embutido liga FKs por padrão, e o DROP da reconstrução apagaria os pais
    // de contas/recursos. O PRAGMA não muda dentro de transação: desliga antes e restaura.
    let fk: bool = if widen || whatsapp {
        c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).map_err(super::store::db_error)?
    } else {
        false
    };
    if fk {
        c.execute_batch("PRAGMA foreign_keys=OFF").map_err(super::store::db_error)?;
    }
    let result = (|| {
        let tx = c.unchecked_transaction().map_err(super::store::db_error)?;
        if widen {
            widen_builtins(&tx)?;
        }
        if whatsapp {
            widen_whatsapp_kinds(&tx)?;
        }
        tx.execute_batch(include_str!("schema.sql")).map_err(super::store::db_error)?;
        tx.commit().map_err(super::store::db_error)
    })();
    if fk {
        c.execute_batch("PRAGMA foreign_keys=ON").map_err(super::store::db_error)?;
    }
    result
}

fn needs_widen(c: &Connection) -> Result<bool, PluginError> {
    let sql: Option<String> = c
        .query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='donna_plugin_installations'", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(super::store::db_error)?;
    Ok(sql.is_some_and(|sql| !sql.contains("'whatsapp'")))
}

/// SQLite não altera CHECK: reconstrói a tabela antiga, preservando as linhas.
/// Contas, recursos e grants não mudam; seus FKs voltam a apontar para o mesmo nome.
// D537: manter o valor legado email no CHECK durante upgrades do WhatsApp.
// Não é registro de builtin em runtime; ver docs/archive/d537-email/schema-legado.md.
fn widen_builtins(c: &Connection) -> Result<(), PluginError> {
    for trigger in TRIGGERS {
        c.execute_batch(&format!("DROP TRIGGER IF EXISTS {trigger};")).map_err(super::store::db_error)?;
    }
    c.execute_batch(
        "CREATE TABLE donna_plugin_installations_v2(
            id TEXT PRIMARY KEY NOT NULL,
            version INTEGER NOT NULL CHECK(version=1),
            hub_id TEXT NOT NULL,
            builtin TEXT NOT NULL CHECK(builtin IN ('scatty','email','whatsapp')),
            state TEXT NOT NULL CHECK(state IN ('disabled','enabled')),
            generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 9007199254740991),
            revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
            UNIQUE(hub_id,builtin)
        ) STRICT;
        INSERT INTO donna_plugin_installations_v2(id,version,hub_id,builtin,state,generation,revision)
            SELECT id,version,hub_id,builtin,state,generation,revision FROM donna_plugin_installations;
        DROP TABLE donna_plugin_installations;
        ALTER TABLE donna_plugin_installations_v2 RENAME TO donna_plugin_installations;",
    )
    .map_err(super::store::db_error)
}

/// R3/R4c: recursos e grants antigos não aceitam o número do `WhatsApp` nem suas capacidades
/// (ver no R3, responder no R4c). A reconstrução usa o `schema.sql` atual, que tem as duas.
fn needs_whatsapp_kinds(c: &Connection) -> Result<bool, PluginError> {
    let sql: Option<String> = c
        .query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='donna_plugin_grants'", [], |r| r.get(0))
        .optional()
        .map_err(super::store::db_error)?;
    Ok(sql.is_some_and(|sql| !sql.contains("'whatsapp_reply_chats'")))
}

/// Reconstrói recursos e grants com a definição atual do `schema.sql` (fonte única),
/// preservando as linhas. Gatilhos que citam as duas tabelas saem antes e voltam pelo
/// `schema.sql`/`agenda.sql` logo depois; os índices de grants também.
fn widen_whatsapp_kinds(c: &Connection) -> Result<(), PluginError> {
    let triggers: Vec<String> = {
        let mut stmt = c
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='trigger'
                 AND (sql LIKE '%donna_plugin_resources%' OR sql LIKE '%donna_plugin_grants%')",
            )
            .map_err(super::store::db_error)?;
        stmt.query_map([], |r| r.get(0)).and_then(Iterator::collect).map_err(super::store::db_error)?
    };
    for trigger in triggers {
        c.execute_batch(&format!("DROP TRIGGER IF EXISTS \"{}\";", trigger.replace('"', "")))
            .map_err(super::store::db_error)?;
    }
    for table in ["donna_plugin_resources", "donna_plugin_grants"] {
        let ddl = table_ddl(table)?.replacen(table, &format!("{table}_v2"), 1);
        c.execute_batch(&format!(
            "{ddl}
            INSERT INTO {table}_v2 SELECT * FROM {table};
            DROP TABLE {table};
            ALTER TABLE {table}_v2 RENAME TO {table};"
        ))
        .map_err(super::store::db_error)?;
    }
    Ok(())
}

/// O `CREATE TABLE` de uma tabela do `schema.sql`, até o `) STRICT;` dela.
fn table_ddl(table: &str) -> Result<String, PluginError> {
    let sql = include_str!("schema.sql");
    let start = sql.find(&format!("CREATE TABLE IF NOT EXISTS {table}(")).ok_or(PluginError::Storage)?;
    let end = sql[start..].find(") STRICT;").ok_or(PluginError::Storage)? + start + ") STRICT;".len();
    Ok(sql[start..end].to_owned())
}
