//! Leituras de controle da consolidação que não confundem falha com ausência.
use crate::store::Store;
use anyhow::{Context, Result, ensure};

pub(crate) fn bookmark_key(bot: &str) -> String {
    format!("memory.dream.{bot}")
}

pub(crate) fn reviewed_key(bot: &str) -> String {
    format!("memory.dream.reviewed.{bot}")
}

pub(crate) fn read_bookmark(store: &Store, bot: &str) -> Result<i64> {
    let Some(value) = store.kv_read(&bookmark_key(bot)).context("leitura do bookmark de memória")? else {
        return Ok(0);
    };
    let seq: i64 = value.parse().context("bookmark de memória inválido")?;
    ensure!(seq >= 0, "bookmark de memória negativo");
    Ok(seq)
}

pub(crate) fn read_reviewed(store: &Store, bot: &str) -> Result<Option<String>> {
    store.kv_read(&reviewed_key(bot)).context("leitura da revisão de memória")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    use std::path::Path;

    #[test]
    fn leitura_distingue_ausencia_valor_invalido_e_erro() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        assert_eq!(read_bookmark(&store, "b").unwrap(), 0);
        assert_eq!(read_reviewed(&store, "b").unwrap(), None);
        store.kv_set("memory.dream.b", "42").unwrap();
        assert_eq!(read_bookmark(&store, "b").unwrap(), 42);
        for value in ["-1", "invalido"] {
            store.kv_set("memory.dream.b", value).unwrap();
            assert!(read_bookmark(&store, "b").is_err());
        }
        store.db.locked().execute_batch("DROP TABLE kv").unwrap();
        assert!(read_bookmark(&store, "b").is_err());
        assert!(read_reviewed(&store, "b").is_err());
    }
}
