//! Login codes (2FA) submitted through a `kind: "code"` connection card. They live
//! only in Hub memory — never in the vault, the store, an entry, a log or a trace —
//! bound to the bot, the login, the card and the session binding that asked for it,
//! valid for five minutes and single use. A restarted Hub simply has none.
//!
//! Retenção: a varredura ativa (thread ociosa por `Codes`) descarta o que passa de
//! `TTL` mesmo sem `take`; `put` também varre, limita as entradas e descarta o
//! mais antigo. Todo descarte zera o buffer (`Zeroizing`).
use crate::LockExt;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const TTL: Duration = Duration::from_secs(5 * 60);
/// Teto de códigos vivos; passando dele, `put` descarta o mais antigo.
const MAX_CODES: usize = 16;
/// Intervalo máximo da thread de varredura; ela dorme até a próxima expiração.
const SWEEP_MAX: Duration = Duration::from_secs(60);

/// O que `take` encontrou para bot + login + origem.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Take {
    /// Um único código pronto e válido da origem; já consumido.
    Ready(String),
    /// Nenhum código pronto e válido: recusa.
    Missing,
    /// Mais de um código pronto da mesma origem: ambíguo, nada é consumido.
    Several,
}

pub(crate) struct Codes {
    inner: Arc<Mutex<HashMap<String, Code>>>,
    sweep_started: AtomicBool,
    /// Testes injetam a falha de criação do varredor nesta instância.
    #[cfg(test)]
    sweeper_denied: AtomicBool,
}

impl Default for Codes {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            sweep_started: AtomicBool::new(false),
            #[cfg(test)]
            sweeper_denied: AtomicBool::new(false),
        }
    }
}

struct Code {
    bot: String,
    login: String,
    /// Vínculo de sessão (`acp_binding`) da execução que pediu o cartão.
    origin: String,
    value: Zeroizing<String>,
    at: Instant,
}

impl Codes {
    /// Descarta os vencidos; chamado por `put`, `take` e pela thread de varredura.
    fn sweep(inner: &Mutex<HashMap<String, Code>>) {
        inner.locked().retain(|_, c| c.at.elapsed() < TTL);
    }

    /// Sobe a thread de expiração ativa uma única vez; ela morre com o `Codes`.
    /// `false` quando não foi possível garantir a expiração ativa: sem ela o
    /// segredo não pode ficar guardado, então `put` falha fechado.
    fn ensure_sweeper(&self) -> bool {
        if self.sweep_started.swap(true, Ordering::AcqRel) {
            return true;
        }
        if !self.spawn_sweeper() {
            self.sweep_started.store(false, Ordering::Release);
            return false;
        }
        true
    }

    fn spawn_sweeper(&self) -> bool {
        let weak = Arc::downgrade(&self.inner);
        #[cfg(test)]
        if self.sweeper_denied.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::Builder::new()
            .name("login-codes-sweep".into())
            .spawn(move || {
                loop {
                    let Some(map) = weak.upgrade() else { return };
                    let wait = {
                        let mut inner = map.locked();
                        inner.retain(|_, c| c.at.elapsed() < TTL);
                        inner
                            .values()
                            .map(|c| TTL.saturating_sub(c.at.elapsed()))
                            .min()
                            .unwrap_or(SWEEP_MAX)
                            .min(SWEEP_MAX)
                    };
                    drop(map);
                    std::thread::sleep(wait.max(Duration::from_millis(100)));
                }
            })
            .is_ok()
    }

    /// Keeps `value` for `bot` + `login` under card `entry`, bound to the session
    /// `origin` that asked for it, replacing any earlier one for that card.
    /// Falha fechado quando a expiração ativa não pôde ser garantida: o código
    /// não é guardado e o erro é fixo, sem o valor.
    pub(crate) fn put(&self, entry: &str, bot: &str, login: &str, origin: &str, value: String) -> Result<()> {
        if !self.ensure_sweeper() {
            return Err(anyhow!("Não foi possível guardar o código com segurança agora. Peça outro cartão de código."));
        }
        let mut inner = self.inner.locked();
        inner.retain(|_, c| c.at.elapsed() < TTL);
        if inner.len() >= MAX_CODES
            && let Some(oldest) = inner.iter().min_by_key(|(_, c)| c.at).map(|(id, _)| id.clone())
        {
            inner.remove(&oldest);
        }
        inner.insert(
            entry.to_owned(),
            Code {
                bot: bot.to_owned(),
                login: login.to_owned(),
                origin: origin.to_owned(),
                value: Zeroizing::new(value),
                at: Instant::now(),
            },
        );
        Ok(())
    }

    /// The card was cancelled or failed: its code, if any, is gone.
    pub(crate) fn drop(&self, entry: &str) {
        self.inner.locked().remove(entry);
    }

    /// The login was removed: every code waiting for it is gone.
    pub(crate) fn drop_login(&self, login: &str) {
        self.inner.locked().retain(|_, c| c.login != login);
    }

    /// O código mais novo e válido que `bot` tem para `login` NA origem `origin`,
    /// consumido no ato. Origem diferente, cartão ambíguo ou vencido não consomem.
    pub(crate) fn take(&self, bot: &str, login: &str, origin: &str) -> Take {
        Self::sweep(&self.inner);
        let mut inner = self.inner.locked();
        let matches: Vec<String> = inner
            .iter()
            .filter(|(_, c)| c.bot == bot && c.login == login && c.origin == origin)
            .map(|(id, _)| id.clone())
            .collect();
        match matches.as_slice() {
            [] => Take::Missing,
            [one] => inner.remove(one).map_or(Take::Missing, |c| Take::Ready(c.value.to_string())),
            _ => Take::Several,
        }
    }

    /// Rewinds a code's clock, to test expiry without waiting.
    #[cfg(test)]
    pub(crate) fn backdate(&self, entry: &str, by: Duration) {
        if let Some(c) = self.inner.locked().get_mut(entry) {
            c.at = Instant::now().checked_sub(by).unwrap();
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.locked().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backdate(codes: &Codes, entry: &str, by: Duration) {
        let mut inner = codes.inner.locked();
        let c = inner.get_mut(entry).unwrap();
        c.at = Instant::now().checked_sub(by).unwrap();
    }

    fn deny_sweeper(codes: &Codes) {
        codes.sweeper_denied.store(true, Ordering::Relaxed);
    }

    #[test]
    fn codes_are_bound_single_use_and_expire() {
        let codes = Codes::default();
        codes.put("card-a", "bot", "login-1", "sess-a", "123456".into()).unwrap();
        codes.put("card-b", "bot", "login-1", "sess-a", "654321".into()).unwrap();
        // Dois cartões prontos da mesma origem: ambíguo, nada é consumido.
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Several);
        codes.drop("card-b");
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Ready("123456".into()));
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Missing, "single use");
        codes.put("card-c", "bot", "login-1", "sess-a", "111111".into()).unwrap();
        assert_eq!(codes.take("other-bot", "login-1", "sess-a"), Take::Missing);
        assert_eq!(codes.take("bot", "login-2", "sess-a"), Take::Missing);
        // Outra sessão do mesmo bot+login não consome o código desta.
        assert_eq!(codes.take("bot", "login-1", "sess-b"), Take::Missing, "origem diferente");
        codes.drop("card-c");
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Missing, "cancel discards");
        codes.put("card-d", "bot", "login-1", "sess-a", "222222".into()).unwrap();
        backdate(&codes, "card-d", TTL + Duration::from_secs(1));
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Missing, "expired");
    }

    #[test]
    fn expiry_and_cap_discard_without_take() {
        let codes = Codes::default();
        codes.put("old", "bot", "login-1", "sess-a", "000000".into()).unwrap();
        backdate(&codes, "old", TTL + Duration::from_secs(1));
        // A varredura (a mesma da thread) descarta sem nenhum take.
        Codes::sweep(&codes.inner);
        assert_eq!(codes.len(), 0, "vencido some sem take");
        for i in 0..MAX_CODES + 2 {
            codes.put(&format!("card-{i}"), "bot", "login-1", "sess-a", "111111".into()).unwrap();
        }
        assert_eq!(codes.len(), MAX_CODES, "teto de entradas");
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Several, "mais de um pronto é ambíguo");
        codes.drop_login("login-1");
        assert_eq!(codes.len(), 0, "remoção do login descarta tudo");
    }

    #[test]
    fn sweeper_spawn_failure_refuses_to_keep_the_code() {
        let codes = Codes::default();
        deny_sweeper(&codes);
        let err = codes.put("card-x", "bot", "login-1", "sess-a", "123456".into()).unwrap_err();
        assert!(!err.to_string().contains("123456"), "erro fixo, sem o valor");
        assert_eq!(codes.len(), 0, "nenhum segredo fica guardado");
        assert_eq!(codes.take("bot", "login-1", "sess-a"), Take::Missing);
    }
}
