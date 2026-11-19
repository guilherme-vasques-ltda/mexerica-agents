//! D368 (revisão de segurança, rodadas 3 e 5): a autoridade ORIGINAL de um pedido acompanha a tarefa
//! do pedido inteira. É capturada na entrada do `dispatch`, antes do cofre, de `team_lock`, filas e
//! verificações externas. Os pontos únicos de gravação dos recursos do dono (cofre, registro de
//! conectores, catálogo de skills, `WhatsApp`, Agenda, rotinas) gravam por `gravar`: com o app do dono
//! pareado, dentro de `Autoridade::with_db`, que a reconfere na mesma transação; com aparelho sem a
//! autoridade (sem Control ou sem o retrato do transporte), recusa sem gravar (fail-closed).
//! `Local` (o próprio computador do Hub) e as tarefas internas do Hub, fora de pedido, gravam numa
//! transação comum.
use crate::LockExt;
use crate::api::devices::Caller;
use crate::fork::autoridade::Autoridade;
use crate::hub::Hub;
use crate::store::{Scope, Store};
use anyhow::{Result, bail};
use rusqlite::Connection;
use std::future::Future;
use std::sync::Arc;

pub(crate) const SEM_AUTORIDADE: &str = "not_allowed: este aparelho não tem a autorização do dono para esta mudança.";

/// De onde vem o pedido em curso.
#[derive(Clone)]
pub(crate) enum Origem {
    /// O próprio computador do Hub.
    Local,
    /// O app do dono pareado, com a autoridade original do transporte.
    Dono(Arc<Hub>, Autoridade),
    /// Aparelho sem Control ou sem a autoridade do transporte.
    Recusada,
}

tokio::task_local! {
    static PEDIDO: Origem;
}

/// Roda o pedido com a origem dele.
pub(crate) async fn com_pedido<T>(
    hub: &Arc<Hub>,
    caller: &Caller,
    pedido: impl Future<Output = Result<T>>,
) -> Result<T> {
    let origem = match caller {
        Caller::Local => Origem::Local,
        // Todo aparelho real chega com a autoridade do transporte (`autoridade::autenticar`).
        Caller::Device { scopes, autoridade: Some(_), .. } if scopes.contains(&Scope::Control) => {
            Origem::Dono(hub.clone(), Autoridade::capturar(hub, caller)?)
        }
        Caller::Device { .. } => Origem::Recusada,
        // Ponte de ferramentas, pareamento e bootstrap não chegam aos métodos do dono (`permit`);
        // o que gravam é efeito interno do Hub (tokens, cartões), sem marca de dono.
        Caller::Ponte { .. } | Caller::Pairing { .. } | Caller::Bootstrap { .. } => return pedido.await,
    };
    PEDIDO.scope(origem, pedido).await
}

/// A origem do pedido em curso; `None` fora de pedido (tarefas internas do Hub).
pub(crate) fn atual() -> Option<Origem> {
    PEDIDO.try_with(Clone::clone).ok()
}

thread_local! {
    /// Origem transportada explicitamente para trabalho fora da tarefa do pedido:
    /// `spawn_blocking` não herda o task-local `PEDIDO`. Escopo da thread; quem
    /// define limpa ao sair (guarda RAII em `com_origem`).
    static ORIGEM_EXPLICITA: std::cell::RefCell<Option<Origem>> = const { std::cell::RefCell::new(None) };
}

/// Executa `f` com a origem do pedido transportada explicitamente. As gravações
/// internas (`gravar`) passam a reconferir a autoridade do dono na própria
/// transação do efeito, em vez de cair em "operação interna".
pub(crate) fn com_origem<T>(origem: Origem, f: impl FnOnce() -> T) -> T {
    struct Limpa;
    impl Drop for Limpa {
        fn drop(&mut self) {
            ORIGEM_EXPLICITA.with(|c| *c.borrow_mut() = None);
        }
    }
    ORIGEM_EXPLICITA.with(|c| *c.borrow_mut() = Some(origem));
    let _limpa = Limpa;
    f()
}

/// A origem em vigor nesta thread: a do pedido (task-local) ou a explícita
/// transportada por `com_origem`.
pub(crate) fn origem_efetiva() -> Option<Origem> {
    atual().or_else(|| ORIGEM_EXPLICITA.with(|c| c.borrow().clone()))
}

/// Grava conforme a origem do pedido em curso.
pub(crate) fn gravar<T>(store: &Store, write: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    gravar_com(store, origem_efetiva().as_ref(), write)
}

/// Grava conforme uma origem guardada antes (tarefa que continua depois do pedido, como o pareamento).
pub(crate) fn gravar_com<T>(
    store: &Store,
    origem: Option<&Origem>,
    write: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    match origem {
        Some(Origem::Dono(hub, original)) => {
            antes_de_gravar(store);
            original.with_db(hub, None, write)
        }
        Some(Origem::Recusada) => bail!(SEM_AUTORIDADE),
        Some(Origem::Local) | None => {
            let mut db = store.db.locked();
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let value = write(&tx)?;
            tx.commit()?;
            Ok(value)
        }
    }
}

/// Reconfere a origem guardada antes de agir (ator que recebe o pedido depois de uma fila).
pub(crate) fn conferir(origem: Option<&Origem>) -> Result<()> {
    match origem {
        Some(Origem::Dono(hub, original)) => original.with_db(hub, None, |_| Ok(())),
        Some(Origem::Recusada) => bail!(SEM_AUTORIDADE),
        Some(Origem::Local) | None => Ok(()),
    }
}

/// Teste-porta: revoga o aparelho depois da captura e antes da gravação do dono.
#[cfg(test)]
pub(crate) fn antes_de_gravar(store: &Store) {
    let before = store.before_owner_write.locked().take();
    if let Some(before) = before {
        before();
    }
}

#[cfg(not(test))]
#[inline]
pub(crate) fn antes_de_gravar(_store: &Store) {}
