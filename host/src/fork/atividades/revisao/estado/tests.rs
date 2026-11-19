//! Apenas SQLite sintético; nenhum app, banco do dono ou produtor positivo.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    LockExt,
    fork::atividades::store::{StoreTx, with_store_tx},
    store::Store,
};

// Função livre preserva o HRTB de StoreTx; método como fn pointer fixa o lifetime.
fn blocked(tx: &mut StoreTx<'_>) -> Result<bool, crate::fork::atividades::modelo::CoreError> {
    tx.review_work_blocked()
}

#[test]
fn d502_persistencia_aborto_e_entrega_tardia_mesma_instancia() {
    let dir = std::env::temp_dir().join(format!("d502-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("synthetic.db");
    {
        let store = Store::open(&path).unwrap();
        assert!(!with_store_tx(&store, blocked).unwrap());
        let db = store.db.locked();
        fixture::seed(&db, "never-delivered", "holding");
        fixture::seed(&db, "uncertain-delivery", "delivery_admitted");
        migrate(&db).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert!(with_store_tx(&store, blocked).unwrap());
    assert!(with_store_tx(&store, |tx| tx.review_abort_undelivered("never-delivered")).unwrap());
    assert!(!with_store_tx(&store, |tx| tx.review_abort_undelivered("uncertain-delivery")).unwrap());
    assert!(!with_store_tx(&store, |tx| tx.review_abort_undelivered("new-boot-or-missing-window")).unwrap());
    assert!(with_store_tx(&store, blocked).unwrap(), "outra instância conserva Q4");
    let db = store.db.locked();
    let (phase, gesture, delivery): (String, bool, bool) = db
        .query_row(
            "SELECT phase,gesture_eligible,delivery_eligible FROM donna_reviews WHERE instance_ref='never-delivered'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((phase.as_str(), gesture, delivery), ("aborted", false, false));
    // Uma entrega tardia não pode readmitir o gesto/DTO invalidado.
    let changed = db.execute("UPDATE donna_reviews SET phase='delivery_admitted' WHERE instance_ref='never-delivered' AND phase='holding' AND delivery_eligible=1", []).unwrap();
    assert_eq!(changed, 0);
    db.execute("UPDATE donna_reviews SET original_deadline=0,display_deadline=0", []).unwrap();
    assert!(work_allowed_on(&db).is_err(), "prazo vencido não libera");
    drop(db);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn d502_falha_sql_estado_desconhecido_e_rollback_nao_abrem() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    fixture::seed(&store.db.locked(), "original", "holding");
    let aborted: Result<(), _> = with_store_tx(&store, |tx| {
        assert!(tx.review_abort_undelivered("original")?);
        Err(crate::fork::atividades::modelo::CoreError::Storage)
    });
    assert!(aborted.is_err());
    let db = store.db.locked();
    assert!(work_allowed_on(&db).is_err());
    db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE donna_reviews SET phase='future-unknown'").unwrap();
    assert!(work_allowed_on(&db).is_err());
    db.execute_batch("DROP TABLE donna_reviews").unwrap();
    assert!(work_allowed_on(&db).is_err());
}

#[test]
fn d502_indice_parcial_preserva_recusa_e_falha_fechada() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    {
        let db = store.db.locked();
        fixture::seed(&db, "original", "holding");
        db.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
        for (phase, gesture, delivery, expected) in [
            ("waiting", 1, 1, false),
            ("aborted", 0, 0, false),
            ("holding", 0, 0, true),
            ("delivery_admitted", 0, 0, true),
            ("future-unknown", 0, 0, true),
            ("aborted", 1, 0, true),
            ("aborted", 0, 1, true),
            ("aborted", 1, 1, true),
            ("aborted", 2, 0, true),
        ] {
            db.execute(
                "UPDATE donna_reviews SET phase=?1,gesture_eligible=?2,delivery_eligible=?3",
                rusqlite::params![phase, gesture, delivery],
            )
            .unwrap();
            assert_eq!(db.query_row(BLOCKED, [], |r| r.get::<_, bool>(0)).unwrap(), expected);
            assert_eq!(work_allowed_on(&db).is_err(), expected);
        }
        // A contenção do Store não se transforma em ausência de bloqueio.
        assert!(with_store_tx(&store, blocked).is_err());
    }
    assert!(with_store_tx(&store, blocked).unwrap());
    store.db.locked().execute_batch("DELETE FROM donna_reviews; DROP INDEX donna_reviews_blocked").unwrap();
    assert!(work_allowed_on(&store.db.locked()).is_err(), "índice ausente não cai em scan");
    assert!(with_store_tx(&store, blocked).is_err());
    store
        .db
        .locked()
        .execute_batch("CREATE INDEX donna_reviews_blocked ON donna_reviews(request_ref) WHERE phase='holding'")
        .unwrap();
    assert!(work_allowed_on(&store.db.locked()).is_err(), "índice incompatível recusa");
    assert!(with_store_tx(&store, blocked).is_err());
    store.db.locked().execute_batch("DROP INDEX donna_reviews_blocked").unwrap();
    migrate(&store.db.locked()).unwrap();
    assert!(!with_store_tx(&store, blocked).unwrap());
    store.db.locked().execute_batch("DROP TABLE donna_reviews").unwrap();
    assert!(work_allowed_on(&store.db.locked()).is_err());
    assert!(with_store_tx(&store, blocked).is_err());
}

#[test]
fn d502_consulta_indexada_custo_independente_do_historico() {
    use rusqlite::StatementStatus;
    use std::io::Write as _;

    fn measure(db: &Connection, sql: &str) -> (bool, i32, i32) {
        let mut stmt = db.prepare(sql).unwrap();
        let result = stmt.query_row([], |r| r.get::<_, bool>(0)).unwrap();
        (result, stmt.get_status(StatementStatus::VmStep), stmt.get_status(StatementStatus::FullscanStep))
    }

    // Consulta original do head 91d73d8d, antes do índice (PK/UNIQUE apenas).
    const ORIGINAL: &str = "SELECT EXISTS(SELECT 1 FROM donna_reviews WHERE NOT (
        phase IS 'waiting' OR (phase IS 'aborted' AND gesture_eligible IS 0 AND delivery_eligible IS 0)))";
    let mut samples = Vec::new();
    for size in [32, 32_768] {
        let db = Connection::open_in_memory().unwrap();
        migrate(&db).unwrap();
        db.execute(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<?1)
            INSERT INTO donna_reviews SELECT printf('history-%d',i),printf('instance-%d',i),'{}',90000,90000,
            CASE WHEN i%2=0 THEN 'waiting' ELSE 'aborted' END,
            CASE WHEN i%2=0 THEN 1 ELSE 0 END,CASE WHEN i%2=0 THEN 1 ELSE 0 END FROM n",
            [size],
        )
        .unwrap();
        db.execute_batch("DROP INDEX donna_reviews_blocked").unwrap();
        let old_clear = measure(&db, ORIGINAL);
        fixture::seed(&db, "last-in-table", "holding");
        let old_hold = measure(&db, ORIGINAL);
        // O custo de construir o índice é DDL de abertura, fora da leitura medida.
        migrate(&db).unwrap();
        let plan = db
            .prepare(&format!("EXPLAIN QUERY PLAN {BLOCKED}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(plan.iter().any(|line| line.contains("donna_reviews_blocked")));
        let indexed_hold = measure(&db, BLOCKED);
        assert!(work_allowed_on(&db).is_err());
        db.execute("DELETE FROM donna_reviews WHERE instance_ref='last-in-table'", []).unwrap();
        let indexed_clear = measure(&db, BLOCKED);
        work_allowed_on(&db).unwrap();
        assert!(!old_clear.0 && old_hold.0 && !indexed_clear.0 && indexed_hold.0);
        assert_eq!(indexed_clear.2, 0);
        assert_eq!(indexed_hold.2, 0);
        assert!(indexed_clear.1 <= 32 && indexed_hold.1 <= 32);
        assert!(old_clear.2 >= size - 1 && old_hold.2 >= size - 1);
        writeln!(std::io::stdout().lock(), "D502_COST {}", serde_json::json!({
            "sqlite":rusqlite::version(),"history":size,"blockingRows":{"clear":0,"hold":1},"columns":["blocked","vmSteps","fullscanSteps"],"plan":plan,
            "originalClear":old_clear,"originalHold":old_hold,
            "indexedClear":indexed_clear,"indexedHold":indexed_hold,
            "scope":"SELECT only; DDL/population excluded; synthetic in-memory database"
        })).unwrap();
        samples.push((old_clear, old_hold, indexed_clear, indexed_hold));
    }
    assert_eq!(samples[0].2, samples[1].2, "passos sem bloqueio independem do histórico não bloqueante");
    assert_eq!(samples[0].3, samples[1].3, "primeiro bloqueio não percorre histórico não bloqueante");
    assert!(samples[1].0.1 > samples[0].0.1 * 100);
    assert!(samples[1].1.1 > samples[0].1.1 * 100);
}
