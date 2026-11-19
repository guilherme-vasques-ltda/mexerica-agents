//! Observa somente o contador persistido para asserções dos testes do canal.
use rusqlite::Connection;

pub(super) fn geracao_persistida(db: &Connection, chave: &str) -> i64 {
    let valor: i64 =
        db.query_row("SELECT geracao FROM donna_device_concessao WHERE chave=?1", [chave], |r| r.get(0)).unwrap();
    assert!(valor > 0 && valor < i64::MAX, "contador persistido inválido ou esgotado");
    valor
}
