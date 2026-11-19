//! MEX-1294/F6-A: a leitura de uso da conta custa um processo do CLI (até 25 s). Aqui ela tem validade curta e
//! uma leitura por vez por conta: quem chega durante uma leitura espera a mesma resposta; quem chega depois dela,
//! dentro da validade, não sobe processo nenhum. Tela com várias linhas ou troca de conversa não empilha CLIs.
use crate::{LockExt, usage::Window};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

/// Validade de uma leitura que deu certo.
pub(super) const FRESH: Duration = Duration::from_secs(60);
/// Validade de uma leitura que falhou (CLI ausente, tempo esgotado, resposta inválida): evita martelar um CLI quebrado.
pub(super) const FAILED: Duration = Duration::from_secs(15);

/// Tempo máximo de uma leitura inteira (prova da conta + medição); no teste, curto para provar o corte.
pub(super) const READ_TIMEOUT: Duration = if cfg!(test) { Duration::from_secs(2) } else { Duration::from_secs(25) };

/// A leitura e a hora (ms) em que ela foi feita; `None` = sem leitura comprovada.
pub(super) type Reading = Option<(Vec<Window>, i64)>;
type Slot = Arc<tokio::sync::Mutex<Option<(Instant, Reading)>>>;

static SLOTS: LazyLock<Mutex<HashMap<String, Slot>>> = LazyLock::new(Default::default);

/// Devolve a leitura válida da chave ou executa `read` (uma por vez por chave) e guarda o resultado.
pub(super) async fn read_once<F, Fut>(key: &str, read: F) -> Reading
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<Vec<Window>>>,
{
    let slot = {
        let mut slots = SLOTS.locked();
        // Fatia vencida e sem ninguém esperando sai do mapa: o mapa não cresce com contas removidas.
        slots.retain(|_, s| {
            Arc::strong_count(s) > 1
                || s.try_lock().is_ok_and(|v| v.as_ref().is_some_and(|(at, r)| at.elapsed() < validity(r)))
        });
        slots.entry(key.to_owned()).or_default().clone()
    };
    let mut slot = slot.lock().await;
    if let Some((at, reading)) = slot.as_ref()
        && at.elapsed() < validity(reading)
    {
        return reading.clone();
    }
    let reading = read().await.map(|windows| (windows, crate::store::now_ms()));
    *slot = Some((Instant::now(), reading.clone()));
    reading
}

fn validity(reading: &Reading) -> Duration {
    if reading.is_some() { FRESH } else { FAILED }
}

/// Só para teste: esquece as leituras guardadas das chaves que começam com `prefix`.
#[cfg(test)]
pub(super) fn clear(prefix: &str) {
    SLOTS.locked().retain(|key, _| !key.starts_with(prefix));
}
