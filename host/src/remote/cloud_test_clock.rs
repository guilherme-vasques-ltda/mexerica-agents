//! Relógio só de teste, isolado na tarefa que executa o pull; sem estado global compartilhado.
use std::cell::Cell;
use std::future::Future;

tokio::task_local! {
    static NOW: Cell<i64>;
}

pub(super) fn now_ms() -> i64 {
    NOW.try_with(Cell::get).unwrap_or_else(|_| crate::store::now_ms())
}

pub(in crate::remote) async fn at<T>(now: i64, work: impl Future<Output = T>) -> T {
    NOW.scope(Cell::new(now), work).await
}

pub(in crate::remote) fn set(now: i64) {
    NOW.with(|clock| clock.set(now));
}
