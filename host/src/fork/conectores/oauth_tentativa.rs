//! Marca de cancelado por tentativa de login.
//!
//! O app dá a cada tentativa um identificador próprio, antes de abrir qualquer página. Cancelar grava a marca aqui, sem
//! depender de a tentativa já estar registrada nem de descartar nada; o Hub confere a marca imediatamente antes de
//! gravar a credencial. Uma tentativa marcada nunca conclui, nem se o callback chegou antes do cancelamento.
use crate::LockExt;
use anyhow::{Result, bail};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub(crate) const CANCELADA: &str = "Este login foi cancelado. Inicie o login de novo.";
const RECUSA: &str = "O identificador da tentativa de login é inválido.";
const VALE_POR: Duration = Duration::from_secs(30 * 60);

type Marcas = Option<HashMap<String, Instant>>;

/// A trava da lista de marcas, segurada de ponto a ponto por quem vai gravar a credencial.
pub(crate) type Trava = RefCell<Option<MutexGuard<'static, Marcas>>>;

static CANCELADAS: Mutex<Marcas> = Mutex::new(None);

/// `tentativa` do pedido: ausente ou `null` é ausência; texto curto de letras, números, hífen e sublinhado vale; o resto recusa.
pub(crate) fn do_pedido(b: &Value) -> Result<Option<String>> {
    match b.get("tentativa") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t))
            if (16..=64).contains(&t.len())
                && t.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') =>
        {
            Ok(Some(t.clone()))
        }
        _ => bail!(RECUSA),
    }
}

pub(crate) fn cancelar(tentativa: &str) {
    let mut marcas = CANCELADAS.locked();
    let mapa = marcas.get_or_insert_with(HashMap::new);
    mapa.retain(|_, desde| desde.elapsed() < VALE_POR);
    mapa.insert(tentativa.to_owned(), Instant::now());
}

fn conferir(marcas: &Marcas, tentativa: &str) -> Result<()> {
    let cancelada = marcas.as_ref().and_then(|m| m.get(tentativa)).is_some_and(|desde| desde.elapsed() < VALE_POR);
    anyhow::ensure!(!cancelada, CANCELADA);
    Ok(())
}

pub(crate) fn viva(tentativa: Option<&str>) -> Result<()> {
    let Some(tentativa) = tentativa else { return Ok(()) };
    conferir(&CANCELADAS.locked(), tentativa)
}

/// Confere a marca e a mantém travada em `trava`: quem cancela espera quem grava terminar, então a gravação ou
/// vem antes do cancelamento ou nunca acontece. A trava cai quando `trava` sai de escopo.
pub(crate) fn viva_travando(tentativa: Option<&str>, trava: &Trava) -> Result<()> {
    let Some(tentativa) = tentativa else { return Ok(()) };
    let mut guarda = trava.borrow_mut();
    let marcas = guarda.get_or_insert_with(|| CANCELADAS.locked());
    conferir(marcas, tentativa)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn so_identificador_curto_e_simples_passa() {
        assert_eq!(do_pedido(&json!({})).unwrap(), None);
        assert_eq!(do_pedido(&json!({"tentativa": null})).unwrap(), None);
        assert!(do_pedido(&json!({"tentativa": "a1b2c3d4-e5f6-4789-8abc-def012345678"})).unwrap().is_some());
        for ruim in
            [json!(""), json!("curto"), json!("x".repeat(65)), json!("a b c d e f g h i j k l"), json!(7), json!(["a"])]
        {
            assert_eq!(do_pedido(&json!({"tentativa": ruim})).unwrap_err().to_string(), RECUSA);
        }
    }

    #[test]
    fn marcada_nao_vive_mais() {
        assert!(viva(None).is_ok());
        assert!(viva(Some("nunca-marcada-0000000")).is_ok());
        cancelar("marcada-unitaria-0001");
        assert_eq!(viva(Some("marcada-unitaria-0001")).unwrap_err().to_string(), CANCELADA);
    }
}
