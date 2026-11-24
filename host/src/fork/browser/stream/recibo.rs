//! Tabela e efeito exclusivos da prova, sem migração ou consumidor de produção.
use super::canal::EntradaEncerrada;
use rusqlite::Connection;
use uuid::Uuid;

pub(super) fn gravar_no(db: &Connection, id: Uuid) -> rusqlite::Result<Result<(), EntradaEncerrada>> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_fixture_recibos(id BLOB NOT NULL PRIMARY KEY CHECK(length(id) = 16))",
    )?;
    let gravados = db.execute(
        "INSERT INTO browser_fixture_recibos(id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        [id.as_bytes().as_slice()],
    )?;
    Ok(if gravados == 1 { Ok(()) } else { Err(EntradaEncerrada::Repetida) })
}

#[cfg(test)]
mod testes {
    use super::*;
    use rusqlite::TransactionBehavior;

    #[test]
    fn consumo_duravel_recusa_o_mesmo_id_apos_reabrir() {
        let pasta = tempfile::tempdir().unwrap();
        let caminho = pasta.path().join("recibos.sqlite");
        let id = Uuid::new_v4();
        {
            let mut db = Connection::open(&caminho).unwrap();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
            assert_eq!(gravar_no(&tx, id).unwrap(), Ok(()));
            tx.commit().unwrap();
            // Nenhum efeito foi confirmado; o recibo ainda impede repetição.
        }
        let mut db = Connection::open(&caminho).unwrap();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        assert_eq!(gravar_no(&tx, id).unwrap(), Err(EntradaEncerrada::Repetida));
        tx.rollback().unwrap();
    }

    #[test]
    fn rollback_nao_publica_o_recibo() {
        let pasta = tempfile::tempdir().unwrap();
        let caminho = pasta.path().join("recibos.sqlite");
        let id = Uuid::new_v4();
        {
            let mut db = Connection::open(&caminho).unwrap();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
            assert_eq!(gravar_no(&tx, Uuid::new_v4()).unwrap(), Ok(()));
            tx.commit().unwrap();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
            assert_eq!(gravar_no(&tx, id).unwrap(), Ok(()));
            tx.rollback().unwrap();
        }
        let db = Connection::open(&caminho).unwrap();
        let total: i64 = db.query_row("SELECT count(*) FROM browser_fixture_recibos", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 1);
    }
}
