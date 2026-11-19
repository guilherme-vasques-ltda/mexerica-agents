//! Donnabot (D384, sono 2b1): a linhagem fato/nota → mensagem, só de acréscimo.
//!
//! Cada mensagem que já sustentou um fato ou uma nota (a evidência de qualquer versão e as fontes da origem, que
//! trazem a procedência herdada e as fontes de um conflito) ganha uma linha em `donna_memory_lineage`. Quem grava
//! são gatilhos do SQLite, na mesma transação da escrita: cobrem toda rota de escrita de hoje e as futuras, sem
//! código por rota. A tabela não tem chave estrangeira e recusa UPDATE e DELETE (D369): a linha fica mesmo depois
//! que o item é apagado fisicamente (Desfazer de criação, esquecer, limpar), para a limpeza C7 reter a mensagem.
//!
//! O gatilho nunca faz uma escrita de memória falhar: evidência ou origem malformadas só não geram linha
//! (`json_valid` antes de abrir qualquer JSON).
//!
//! Só identificadores opacos (D426): `kind` = `fact` (item = id do fato em texto) ou `note` (item = o código
//! aleatório da nota, `memory_file_versions.note_key`). Nada derivado do nome da nota entra aqui: o vínculo
//! código ↔ (camada, nome) mora só nas versões da nota e some com elas (`forget_layer`, limpar, excluir o bot).
//! Nota apagada e recriada com o mesmo nome ganha código novo. `rev`: a revisão global da primeira versão em que a
//! mensagem apareceu.

use rusqlite::Connection;

/// A evidência é uma lista separada por vírgula: vira array JSON escapando `\` e `"`.
macro_rules! quoted {
    ($col:literal) => {
        concat!("'[\"'||replace(replace(replace(coalesce(", $col, ",''),'\\','\\\\'),'\"','\\\"'),',','\",\"')||'\"]'")
    };
}

/// O array da evidência, ou `[]` se ainda assim não for JSON válido (caractere de controle, por exemplo).
macro_rules! evidence_array {
    ($col:literal) => {
        concat!("(CASE WHEN json_valid(", quoted!($col), ") THEN ", quoted!($col), " ELSE '[]' END)")
    };
}

/// As fontes `sources[].messageId` de uma origem válida; origem malformada não dá fonte.
macro_rules! origin_sources {
    ($col:literal) => {
        concat!(
            "json_tree(CASE WHEN json_valid(coalesce(",
            $col,
            ",'')) THEN ",
            $col,
            " ELSE '{}' END)",
            " WHERE key='messageId' AND type='text' AND atom<>''",
            " AND path LIKE '$.sources[%]' AND path NOT LIKE '$.sources[%].%'"
        )
    };
}

macro_rules! fact_rows {
    ($from:literal, $id:literal, $evidence:literal, $origin:literal, $rev:literal) => {
        concat!(
            "SELECT 'fact' AS kind, CAST(",
            $id,
            " AS TEXT) AS item, value AS message, ",
            $rev,
            " AS rev FROM ",
            $from,
            "json_each(",
            evidence_array!($evidence),
            ") WHERE type='text' AND value<>''",
            " UNION ALL SELECT 'fact', CAST(",
            $id,
            " AS TEXT), atom, ",
            $rev,
            " FROM ",
            $from,
            origin_sources!($origin)
        )
    };
}

macro_rules! table {
    () => {
        "CREATE TABLE IF NOT EXISTS donna_memory_lineage(
            kind TEXT NOT NULL,item TEXT NOT NULL,message TEXT NOT NULL,rev INTEGER NOT NULL,
            PRIMARY KEY(kind,item,message)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS donna_memory_lineage_message ON donna_memory_lineage(message);
         "
    };
}

/// O código da versão anterior da mesma nota, se ela ainda existia (conteúdo não nulo); senão, nada (código novo).
macro_rules! previous_key {
    ($v:literal) => {
        previous!("note_key", $v)
    };
}

/// Uma coluna da versão imediatamente anterior da mesma nota, se ela ainda existia (conteúdo não nulo).
macro_rules! previous {
    ($col:literal, $v:literal) => {
        concat!(
            "(SELECT p.",
            $col,
            " FROM memory_file_versions p WHERE p.content IS NOT NULL AND p.id=(SELECT MAX(q.id)
              FROM memory_file_versions q WHERE q.bot=",
            $v,
            ".bot AND q.name=",
            $v,
            ".name AND q.id<",
            $v,
            ".id))"
        )
    };
}

/// A linha com o código da versão, para o FROM; sem código, nenhuma linha (a escrita passa).
macro_rules! note_key {
    ($v:literal) => {
        concat!("(SELECT note_key FROM memory_file_versions WHERE id=", $v, ".id AND note_key IS NOT NULL) k, ")
    };
}

/// A tabela, a recusa de UPDATE/DELETE e os gatilhos. Idempotente: roda em todo início, porque uma migração do
/// Codync que reconstrói `memory_entries` leva os gatilhos junto (como o índice de busca).
const ENSURE: &str = concat!(
    table!(),
    "CREATE TRIGGER IF NOT EXISTS donna_memory_lineage_no_update BEFORE UPDATE ON donna_memory_lineage BEGIN
        SELECT RAISE(ABORT,'donna_memory_lineage: só acréscimo');
     END;
     CREATE TRIGGER IF NOT EXISTS donna_memory_lineage_no_delete BEFORE DELETE ON donna_memory_lineage BEGIN
        SELECT RAISE(ABORT,'donna_memory_lineage: só acréscimo');
     END;
     CREATE TRIGGER IF NOT EXISTS donna_memory_lineage_fact_insert AFTER INSERT ON memory_entries BEGIN
        INSERT OR IGNORE INTO donna_memory_lineage(kind,item,message,rev) ",
    fact_rows!("", "new.id", "new.evidence", "new.origin", "new.rev"),
    ";
     END;
     CREATE TRIGGER IF NOT EXISTS donna_memory_lineage_fact_update
        AFTER UPDATE OF evidence,origin,rev ON memory_entries BEGIN
        INSERT OR IGNORE INTO donna_memory_lineage(kind,item,message,rev) ",
    fact_rows!("", "new.id", "new.evidence", "new.origin", "new.rev"),
    ";
     END;
     CREATE TRIGGER IF NOT EXISTS donna_memory_lineage_note_insert AFTER INSERT ON memory_file_versions BEGIN
        UPDATE memory_file_versions SET note_key=COALESCE(",
    previous_key!("new"),
    ",lower(hex(randomblob(16)))) WHERE id=new.id AND note_key IS NULL;
        INSERT OR IGNORE INTO donna_memory_lineage(kind,item,message,rev)
        SELECT 'note', k.note_key, atom, new.rev FROM ",
    note_key!("new"),
    origin_sources!("new.origin"),
    ";
     END;
     CREATE TRIGGER IF NOT EXISTS donna_memory_lineage_note_update AFTER UPDATE OF origin ON memory_file_versions BEGIN
        INSERT OR IGNORE INTO donna_memory_lineage(kind,item,message,rev)
        SELECT 'note', k.note_key, atom, new.rev FROM ",
    note_key!("new"),
    origin_sources!("new.origin"),
    ";
     END;"
);

/// Migração única (`DONNABOT` em `schema.rs`): dá um código a cada nota existente (um por vida da nota: uma versão
/// depois de uma exclusão começa outra) e traz a linhagem do que já existe, cada mensagem com a MENOR revisão em
/// que apareceu, juntando evidência e origem de todas as versões guardadas (R1-02).
pub(crate) const BACKFILL: &str = concat!(
    table!(),
    "ALTER TABLE memory_file_versions ADD COLUMN note_key TEXT;
     UPDATE memory_file_versions SET note_key=lower(hex(randomblob(16))) WHERE ",
    previous!("id", "memory_file_versions"),
    " IS NULL;
     UPDATE memory_file_versions SET note_key=(SELECT s.note_key FROM memory_file_versions s
        WHERE s.bot=memory_file_versions.bot AND s.name=memory_file_versions.name AND s.id<memory_file_versions.id
        AND s.note_key IS NOT NULL ORDER BY s.id DESC LIMIT 1) WHERE note_key IS NULL;
     INSERT OR IGNORE INTO donna_memory_lineage(kind,item,message,rev)
     SELECT kind,item,message,MIN(rev) FROM (",
    fact_rows!("donna_memory_versions t, ", "t.entry", "t.evidence", "t.origin", "t.rev"),
    " UNION ALL ",
    fact_rows!("memory_entries t, ", "t.id", "t.evidence", "t.origin", "t.rev"),
    " UNION ALL SELECT 'note' AS kind, t.note_key AS item, atom AS message, t.rev AS rev
       FROM memory_file_versions t, ",
    origin_sources!("t.origin"),
    " AND t.note_key IS NOT NULL) GROUP BY kind,item,message;"
);

/// Cria a tabela e os gatilhos que faltarem. Roda em todo início, depois das migrações.
pub(crate) fn ensure(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(ENSURE)
}

#[cfg(test)]
mod tests {
    use crate::files::{self, Author as FileAuthor, Cause, Change};
    use crate::nature::{self, Hint, Revision};
    use crate::origin::{self, Kind, Origin, Source};
    use crate::store::{self, New, Writer};
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&conn).unwrap();
        conn
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// As mensagens da linhagem de um item, em ordem.
    fn lineage(conn: &Connection, kind: &str, item: &str) -> Vec<(String, i64)> {
        let mut stmt = conn
            .prepare("SELECT message,rev FROM donna_memory_lineage WHERE kind=?1 AND item=?2 ORDER BY message")
            .unwrap();
        stmt.query_map([kind, item], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    }

    fn messages(conn: &Connection, id: i64) -> Vec<String> {
        lineage(conn, "fact", &id.to_string()).into_iter().map(|(m, _)| m).collect()
    }

    fn count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM donna_memory_lineage", [], |r| r.get(0)).unwrap()
    }

    fn sourced(list: &[&str]) -> Origin {
        Origin {
            schema: 1,
            kind: Kind::Conversation,
            sources: list
                .iter()
                .map(|m| Source { message_id: (*m).to_owned(), kind: Kind::User, detail: None })
                .collect(),
            external: false,
        }
    }

    fn fact(conn: &Connection, bot: Option<&str>, text: &str, evidence: &[&str]) -> i64 {
        let evidence = ids(evidence);
        store::add(conn, Writer::Bot("b1"), New { bot, text, room: None, evidence: &evidence }, 1, 10).unwrap().id
    }

    /// O código opaco gravado na versão da nota.
    fn key(conn: &Connection, version: i64) -> String {
        conn.query_row("SELECT note_key FROM memory_file_versions WHERE id=?1", [version], |r| r.get(0)).unwrap()
    }

    fn note(conn: &Connection, bot: Option<&str>, content: Option<&str>, rev: i64) -> i64 {
        let change = Change {
            bot,
            name: "viagem.md",
            content,
            author: FileAuthor::Bot,
            author_bot: Some("b1"),
            cause: if content.is_some() { Cause::Edit } else { Cause::Delete },
            mtime: 1,
            size: 1,
        };
        files::record(conn, &change, 1, rev).unwrap().id
    }

    #[test]
    fn creation_records_evidence_with_its_rev() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m1", "m2"]);
        assert_eq!(lineage(&conn, "fact", &id.to_string()), [("m1".into(), 10), ("m2".into(), 10)]);
    }

    #[test]
    fn origin_sources_join_the_lineage() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m1"]);
        origin::set(&conn, id, &sourced(&["m0", "m1"])).unwrap();
        assert_eq!(messages(&conn, id), ["m0", "m1"]);
    }

    /// R3-01/D409: o conflito troca a evidência para N e conserva o texto de M; as duas ficam na linhagem.
    #[test]
    fn conflict_with_only_new_evidence_keeps_the_old_message() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m"]);
        let revision = Revision {
            entry: id,
            text: "Mora em Curitiba",
            literal_owner: false,
            hint: Some(Hint::Conflict),
            bot: "b1",
        };
        nature::revise(&conn, Writer::Bot("b1"), revision, 2, 20).unwrap();
        conn.execute("UPDATE memory_entries SET evidence='n' WHERE id=?1", [id]).unwrap();
        assert_eq!(lineage(&conn, "fact", &id.to_string()), [("m".into(), 10), ("n".into(), 20)]);
    }

    #[test]
    fn owner_edit_pin_archive_activate_and_restore_keep_every_message() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m"]);
        store::revise(&conn, Writer::User, id, "Mora em Olinda", 2, 11).unwrap();
        store::pin(&conn, id, true, 3, 12).unwrap();
        store::archive(&conn, Writer::User, id, None, 4, 13).unwrap();
        store::activate(&conn, id, 5, 14).unwrap();
        store::archive(&conn, Writer::User, id, None, 6, 15).unwrap();
        store::bring_back(&conn, id, 7, 16).unwrap();
        conn.execute("UPDATE memory_entries SET evidence='n' WHERE id=?1", [id]).unwrap();
        nature::restore(&conn, id, 10, 8, 17).unwrap();
        assert_eq!(messages(&conn, id), ["m", "n"], "nada sai; o rev do fato sobe e invalida a checagem (D409)");
        let current: i64 = conn.query_row("SELECT rev FROM memory_entries WHERE id=?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(current, 17);
    }

    /// D369/D412: o DELETE físico (Desfazer de criação, esquecer, retirar, limpar, excluir bot) conserva a linhagem.
    #[test]
    fn physical_deletes_keep_the_lineage() {
        let conn = db();
        let forgotten = fact(&conn, Some("b1"), "Gosta de café", &["m1"]);
        let withdrawn = fact(&conn, Some("b1"), "Gosta de chá", &["m2"]);
        let yours = fact(&conn, None, "Tem um gato", &["m3"]);
        let dropped = fact(&conn, Some("b1"), "Usa Linux", &["m4"]);
        store::forget(&conn, forgotten).unwrap();
        store::withdraw(&conn, withdrawn).unwrap();
        store::forget_layer(&conn, None).unwrap();
        store::drop_bot(&conn, "b1").unwrap();
        conn.execute("DELETE FROM memory_entries", []).unwrap();
        for (id, m) in [(forgotten, "m1"), (withdrawn, "m2"), (yours, "m3"), (dropped, "m4")] {
            assert_eq!(messages(&conn, id), [m]);
        }
    }

    #[test]
    fn notes_record_their_sources_under_an_opaque_key_and_survive_forget_layer() {
        let conn = db();
        let version = note(&conn, Some("b1"), Some("# Viagem"), 30);
        origin::set_file(&conn, version, &sourced(&["m1"])).unwrap();
        let first = key(&conn, version);
        assert_eq!(first.len(), 32, "128 bits aleatórios em hexadecimal");
        // A versão seguinte da mesma nota viva (e a sua exclusão) guarda o mesmo código e herda a origem.
        let edit = note(&conn, Some("b1"), Some("# Viagem 2"), 31);
        let deleted = note(&conn, Some("b1"), None, 32);
        assert_eq!((key(&conn, edit), key(&conn, deleted)), (first.clone(), first.clone()));
        let shared = note(&conn, None, Some("# Casa"), 33);
        origin::set_file(&conn, shared, &sourced(&["m2"])).unwrap();
        assert_ne!(key(&conn, shared), first, "outra camada, outra nota");
        let shared_key = key(&conn, shared);
        files::forget_layer(&conn, Some("b1")).unwrap();
        files::forget_layer(&conn, None).unwrap();
        assert_eq!(lineage(&conn, "note", &first), [("m1".into(), 30)]);
        assert_eq!(lineage(&conn, "note", &shared_key), [("m2".into(), 33)]);
    }

    /// SEG-497-01/R1-01 (D426): a nota apagada e recriada com o mesmo nome ganha código novo e não reúne a
    /// linhagem antiga; o mesmo depois de limpar a camada.
    #[test]
    fn a_recreated_note_gets_a_new_key() {
        let conn = db();
        let old = note(&conn, Some("b1"), Some("# Viagem"), 30);
        origin::set_file(&conn, old, &sourced(&["m1"])).unwrap();
        note(&conn, Some("b1"), None, 31);
        let new = note(&conn, Some("b1"), Some("# Outra viagem"), 32);
        origin::set_file(&conn, new, &sourced(&["m2"])).unwrap();
        assert_ne!(key(&conn, new), key(&conn, old));
        // A linhagem do código antigo não ganha nada da nota nova. A nova lista a origem que `files::record`
        // herdou da versão anterior (m1), porque é o que a versão afirma; o código antigo não é reusado.
        assert_eq!(lineage(&conn, "note", &key(&conn, old)), [("m1".into(), 30)]);
        assert_eq!(lineage(&conn, "note", &key(&conn, new)), [("m1".into(), 32), ("m2".into(), 32)]);
        let before = key(&conn, new);
        files::forget_layer(&conn, Some("b1")).unwrap();
        let again = note(&conn, Some("b1"), Some("# Viagem"), 33);
        assert_ne!(key(&conn, again), before);
    }

    /// SEG-497-01/R1-01 (D426): depois de esquecer, limpar e excluir o bot, nenhum valor de nenhuma coluna de
    /// nenhuma tabela contém o nome da nota, e a linhagem continua lá.
    #[test]
    fn no_table_keeps_the_note_name_after_forget_clear_and_bot_deletion() {
        let conn = db();
        let name = "saude-exame-hiv.md";
        for (bot, rev) in [(Some("b1"), 30), (None, 31), (Some("b2"), 32)] {
            let change = Change {
                bot,
                name,
                content: Some("# Nota"),
                author: FileAuthor::Bot,
                author_bot: Some("b1"),
                cause: Cause::Edit,
                mtime: 1,
                size: 1,
            };
            let version = files::record(&conn, &change, 1, rev).unwrap().id;
            origin::set_file(&conn, version, &sourced(&["uuid-message-1"])).unwrap();
        }
        files::forget_layer(&conn, Some("b1")).unwrap();
        store::forget_layer(&conn, None).unwrap();
        files::forget_layer(&conn, None).unwrap();
        store::drop_bot(&conn, "b2").unwrap();
        assert_eq!(count(&conn), 3, "a linhagem fica (D412)");
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(tables.contains(&"donna_memory_lineage".to_owned()));
        for table in tables {
            let columns: Vec<String> = conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            for column in columns {
                let found: i64 = conn
                    .query_row(
                        &format!("SELECT COUNT(*) FROM \"{table}\" WHERE CAST(\"{column}\" AS TEXT) LIKE '%saude%'"),
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(found, 0, "{table}.{column} guarda o nome da nota");
            }
        }
    }

    /// O gatilho nunca faz a escrita falhar: evidência ou origem malformadas só não geram linha.
    #[test]
    fn malformed_evidence_or_origin_never_fails_the_write() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &[]);
        assert_eq!(messages(&conn, id), [""; 0], "sem evidência, sem linha");
        conn.execute("UPDATE memory_entries SET origin='{não é json',rev=11 WHERE id=?1", [id]).unwrap();
        conn.execute(r#"UPDATE memory_entries SET origin='{"sources":"x"}',rev=12 WHERE id=?1"#, [id]).unwrap();
        conn.execute(r#"UPDATE memory_entries SET origin='{"sources":[1,"m",{"messageId":7}]}' WHERE id=?1"#, [id])
            .unwrap();
        conn.execute("UPDATE memory_entries SET evidence=char(1)||',a' WHERE id=?1", [id]).unwrap();
        assert_eq!(messages(&conn, id), [""; 0]);
        // Aspas e barra na evidência não quebram o JSON montado.
        conn.execute(r#"UPDATE memory_entries SET evidence='a"b,c\d' WHERE id=?1"#, [id]).unwrap();
        assert_eq!(messages(&conn, id), [r#"a"b"#, r"c\d"]);
        let version = note(&conn, Some("b1"), Some("# Nota"), 40);
        conn.execute("UPDATE memory_file_versions SET origin='[' WHERE id=?1", [version]).unwrap();
        assert_eq!(lineage(&conn, "note", &key(&conn, version)), []);
        // Versão sem código (como uma linha escrita por fora do gatilho): a escrita da origem passa, sem linha.
        conn.execute("UPDATE memory_file_versions SET note_key=NULL WHERE id=?1", [version]).unwrap();
        origin::set_file(&conn, version, &sourced(&["m9"])).unwrap();
        assert_eq!(count(&conn), 2);
    }

    #[test]
    fn the_lineage_refuses_update_and_delete() {
        let conn = db();
        fact(&conn, Some("b1"), "Mora em Recife", &["m"]);
        assert!(conn.execute("UPDATE donna_memory_lineage SET rev=0", []).is_err());
        assert!(conn.execute("DELETE FROM donna_memory_lineage", []).is_err());
        assert_eq!(count(&conn), 1);
    }

    /// Escrita em lote (undoDream, restaurar várias): uma tentativa de INSERT por fonte de cada linha alterada,
    /// e nenhuma linha nova quando as fontes já estão lá.
    #[test]
    fn batch_rewrites_add_no_rows_for_known_sources() {
        let conn = db();
        for n in 0..200 {
            fact(&conn, Some("b1"), &format!("Fato {n}"), &[&format!("m{n}a"), &format!("m{n}b")]);
        }
        assert_eq!(count(&conn), 400);
        conn.execute("UPDATE memory_entries SET rev=rev+1", []).unwrap();
        conn.execute("UPDATE memory_entries SET state='archived',rev=rev+1", []).unwrap();
        assert_eq!(count(&conn), 400);
        conn.execute("DELETE FROM memory_entries", []).unwrap();
        assert_eq!(count(&conn), 400);
    }

    /// Banco de antes da migração 4: sem a tabela, os gatilhos nem o código das notas.
    fn before_migration_4(conn: &Connection) {
        conn.execute_batch(
            "DROP TRIGGER donna_memory_lineage_no_delete; DROP TRIGGER donna_memory_lineage_no_update;
             DROP TRIGGER donna_memory_lineage_fact_insert; DROP TRIGGER donna_memory_lineage_fact_update;
             DROP TRIGGER donna_memory_lineage_note_insert; DROP TRIGGER donna_memory_lineage_note_update;
             DROP TABLE donna_memory_lineage;
             ALTER TABLE memory_file_versions DROP COLUMN note_key;
             UPDATE memory_meta SET value=3 WHERE key='donnabot';",
        )
        .unwrap();
    }

    /// A migração traz a linhagem do que já existia: todas as versões guardadas, o fato atual e as notas, estas
    /// com um código por vida da nota (a recriada depois de apagada ganha outro).
    #[test]
    fn migration_backfills_existing_versions_and_notes() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m"]);
        conn.execute("UPDATE memory_entries SET evidence='n',rev=11 WHERE id=?1", [id]).unwrap();
        let first = note(&conn, Some("b1"), Some("# Viagem"), 30);
        origin::set_file(&conn, first, &sourced(&["k"])).unwrap();
        let edit = note(&conn, Some("b1"), Some("# Viagem 2"), 31);
        note(&conn, Some("b1"), None, 32);
        let again = note(&conn, Some("b1"), Some("# Viagem"), 33);
        origin::set_file(&conn, again, &sourced(&["j"])).unwrap();
        before_migration_4(&conn);
        crate::schema::migrate(&conn).unwrap();
        assert_eq!(lineage(&conn, "fact", &id.to_string()), [("m".into(), 10), ("n".into(), 11)]);
        assert_eq!(key(&conn, first), key(&conn, edit));
        assert_ne!(key(&conn, first), key(&conn, again));
        assert_eq!(lineage(&conn, "note", &key(&conn, first)), [("k".into(), 30)]);
        assert_eq!(lineage(&conn, "note", &key(&conn, again)), [("j".into(), 33)]);
    }

    /// R1-02: a fonte que entrou pela origem antes de virar evidência fica com a revisão em que apareceu primeiro.
    #[test]
    fn migration_keeps_the_first_rev_when_the_origin_came_before_the_evidence() {
        let conn = db();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m"]);
        // Arquivado por N na rev 20: a origem passa a incluir N; revisado por N na rev 30 (evidência só N).
        conn.execute("UPDATE memory_entries SET state='archived',rev=20 WHERE id=?1", [id]).unwrap();
        origin::set(&conn, id, &sourced(&["m", "n"])).unwrap();
        conn.execute("UPDATE memory_entries SET state='active',evidence='n',rev=30 WHERE id=?1", [id]).unwrap();
        before_migration_4(&conn);
        crate::schema::migrate(&conn).unwrap();
        assert_eq!(lineage(&conn, "fact", &id.to_string()), [("m".into(), 10), ("n".into(), 20)]);
    }

    /// Uma migração do Codync que reconstrói `memory_entries` leva os gatilhos; o início seguinte os recria.
    #[test]
    fn triggers_come_back_after_a_table_rebuild() {
        let conn = db();
        conn.execute_batch("DROP TRIGGER donna_memory_lineage_fact_insert").unwrap();
        crate::schema::migrate(&conn).unwrap();
        let id = fact(&conn, Some("b1"), "Mora em Recife", &["m"]);
        assert_eq!(messages(&conn, id), ["m"]);
    }
}
