//! MEX-1355: espera crescente nas falhas de seleção de "Organizar memória".
//!
//! Sem conta ou modelo utilizável, cada bot tentava de novo a cada 5 s e cada
//! tentativa virava uma linha na Atividade. Agora a mesma falha espera 5 s,
//! 10 s, 20 s, 40 s e para após 5 seguidas. A saída da parada é por evento:
//! toda gravação do cadastro de contas (conectar, reconectar, catálogo novo,
//! inclusive contas gerenciadas) e todo salvar da função chamam [`changed`], que
//! zera a espera quando o resumo da seleção mudou. Como reserva, parado, o
//! sonhador sonda a conta escolhida no máximo a cada 10 min, para enxergar uma
//! reconexão ou um catálogo corrigido fora do Hub. O estado é global (a seleção é global) e
//! fica no Store, então sobrevive a reinícios do Hub. Decisão, contagem e
//! associação da linha da Atividade passam pela mesma seção serializada; falhas
//! simultâneas de bots diferentes na mesma janela contam uma vez só.
use crate::{
    LockExt,
    fork::{
        background_notice::Problem,
        modelos_funcao::{
            catalogo::FunctionId,
            config::{self, Selection},
            resolver,
        },
    },
    store::{Store, now_ms},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const SLOT: &str = "fork.background-selection-backoff.v1";
pub(crate) const FIRST_MS: i64 = 5_000;
pub(crate) const STOP_AFTER: u32 = 5;
/// Enquanto espera ou parado, o sonhador só confere a configuração neste ritmo.
const POLL_MS: i64 = 60_000;
/// Parado, a sondagem do login local roda no máximo neste ritmo (todos os bots juntos).
pub(crate) const PROBE_MS: i64 = 600_000;

/// Serializa leitura e gravação do estado entre os sonhadores e as vagas do executor.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    fingerprint: String,
    reason: String,
    failures: u32,
    next_at: i64,
    history_id: Option<String>,
    #[serde(default)]
    probed_at: i64,
}

/// Falhas de configuração da seleção (conta/modelo), que só mudam quando o dono
/// mexe ou a conta reconecta. Os demais problemas seguem o caminho comum.
pub(crate) fn selection_reason(problem: &Problem) -> Option<&'static str> {
    Some(match problem {
        Problem::Model => "model",
        Problem::AccountDisconnected { .. } => "account_disconnected",
        Problem::NoClaude => "no_claude",
        Problem::PersonalSubscription => "personal_subscription_chat_only",
        Problem::Codex => "codex",
        Problem::Api => "api",
        _ => return None,
    })
}

/// Resumo do que decide a seleção: documento da função e estado das contas.
/// Campos voláteis (horário de verificação) ficam de fora para não zerar à toa.
fn fingerprint(store: &Store) -> String {
    let selection = config::load(store)
        .ok()
        .and_then(|document| document.functions.get(&FunctionId::MemoryConsolidation).cloned())
        .map(|selection: Selection| serde_json::to_string(&selection).unwrap_or_default());
    let accounts = resolver::accounts(store).ok().map(|accounts| {
        accounts
            .iter()
            .map(|a| {
                let models: Vec<String> =
                    a.models.iter().map(|m| serde_json::to_string(m).unwrap_or_else(|_| m.id.clone())).collect();
                format!(
                    "{}|{}|{}|{}|{}|{}",
                    a.id,
                    a.backend,
                    a.billing,
                    a.state,
                    a.subscription_verified,
                    models.join(",")
                )
            })
            .collect::<Vec<_>>()
    });
    crate::agent::registry::sha256_hex(format!("{selection:?}\n{accounts:?}").as_bytes())
}

fn read(store: &Store) -> Option<State> {
    store.kv_read(SLOT).ok().flatten().and_then(|raw| serde_json::from_str(&raw).ok())
}

fn delay_ms(failures: u32) -> i64 {
    FIRST_MS.saturating_mul(1_i64 << failures.saturating_sub(1).min(STOP_AFTER - 1))
}

fn wait_in(store: &Store, now: i64) -> Option<i64> {
    let state = read(store)?;
    if state.fingerprint != fingerprint(store) {
        delete(store);
        return None;
    }
    if state.failures >= STOP_AFTER {
        return Some(i64::MAX);
    }
    (now < state.next_at).then(|| state.next_at - now)
}

/// `Some(ms)` = ainda esperando (ou parado) pela mesma falha com a mesma
/// configuração; `None` = pode tentar. Configuração nova apaga o estado.
pub(crate) fn wait_ms(store: &Store, now: i64) -> Option<i64> {
    let _serial = LOCK.locked();
    wait_in(store, now)
}

/// Próxima conferência do sonhador depois de um `Later`: nunca antes de 5 s,
/// nunca depois de 60 s, para retomar sozinho quando a configuração mudar.
pub(crate) fn poll_after_ms(store: &Store) -> i64 {
    wait_ms(store, now_ms()).map_or(FIRST_MS, |ms| ms.clamp(FIRST_MS, POLL_MS))
}

/// Evento: o cadastro de contas ou a escolha da função foi gravado. Se o resumo
/// mudou, a função sai da espera (ou da parada) na hora, sem esperar a sondagem;
/// regravação sem mudança (só o horário de verificação) não zera nada.
pub(crate) fn changed(store: &Store) {
    let _serial = LOCK.locked();
    if read(store).is_some_and(|state| state.fingerprint != fingerprint(store)) {
        delete(store);
    }
}

/// Parado e sem sondagem recente: reserva a vez de sondar o login local (um bot só).
pub(crate) fn claim_probe(store: &Store, now: i64) -> bool {
    let _serial = LOCK.locked();
    let Some(mut state) = read(store) else { return false };
    if state.failures < STOP_AFTER || now.saturating_sub(state.probed_at) < PROBE_MS {
        return false;
    }
    state.probed_at = now;
    write(store, &state).is_ok()
}

/// Conta a falha; devolve a linha da Atividade a reaproveitar quando o motivo e
/// a configuração são os mesmos. Dentro da janela de espera de uma falha já
/// contada (outro bot falhou ao mesmo tempo), não conta de novo.
fn failed_in(store: &Store, reason: &str, now: i64) -> Option<String> {
    let fingerprint = fingerprint(store);
    let previous = read(store).filter(|s| s.reason == reason && s.fingerprint == fingerprint);
    if let Some(state) = &previous
        && now < state.next_at
    {
        return state.history_id.clone();
    }
    let failures = previous.as_ref().map_or(1, |s| s.failures.saturating_add(1));
    let history_id = previous.as_ref().and_then(|s| s.history_id.clone());
    let state = State {
        fingerprint,
        reason: reason.into(),
        failures,
        next_at: now.saturating_add(delay_ms(failures)),
        history_id: history_id.clone(),
        probed_at: previous.map_or(0, |s| s.probed_at),
    };
    if let Err(error) = write(store, &state) {
        tracing::warn!(%error, "espera da seleção de fundo não gravada");
    }
    history_id
}

fn remember_in(store: &Store, id: &str) {
    if let Some(mut state) = read(store) {
        state.history_id = Some(id.into());
        if let Err(error) = write(store, &state) {
            tracing::warn!(%error, "espera da seleção de fundo não gravada");
        }
    }
}

/// Registra uma falha de seleção numa seção só: conta a falha e atualiza a
/// linha anterior da Atividade (`touch`) ou cria outra (`record`) e a associa.
pub(crate) fn record_failure(
    store: &Store,
    reason: &str,
    touch: impl FnOnce(&str) -> bool,
    record: impl FnOnce() -> Option<String>,
) {
    let _serial = LOCK.locked();
    if failed_in(store, reason, now_ms()).is_some_and(|id| touch(&id)) {
        return;
    }
    if let Some(id) = record() {
        remember_in(store, &id);
    }
}

/// A seleção funcionou: a próxima falha começa do zero, numa linha nova.
pub(crate) fn clear(store: &Store) {
    let _serial = LOCK.locked();
    delete(store);
}

fn delete(store: &Store) {
    if let Err(error) = store.kv_delete(SLOT) {
        tracing::warn!(%error, "espera da seleção de fundo não apagada");
    }
}

fn write(store: &Store, state: &State) -> Result<()> {
    store.kv_set(SLOT, &serde_json::to_string(state)?)
}

#[cfg(test)]
pub(crate) fn failures(store: &Store) -> u32 {
    read(store).map_or(0, |s| s.failures)
}

#[cfg(test)]
pub(crate) fn expire(store: &Store) {
    if let Some(mut state) = read(store) {
        state.next_at = 0;
        let _ = write(store, &state);
    }
}

/// Simula a parada já com a última sondagem vencida.
#[cfg(test)]
pub(crate) fn expire_probe(store: &Store) {
    if let Some(mut state) = read(store) {
        state.probed_at = 0;
        let _ = write(store, &state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::Path};

    /// Falha com uma Atividade de mentira: devolve a linha usada (tocada ou nova).
    fn fail(store: &Store, reason: &str) -> String {
        let used = RefCell::new(String::new());
        record_failure(
            store,
            reason,
            |id| {
                id.clone_into(&mut used.borrow_mut());
                true
            },
            || {
                let id = uuid::Uuid::new_v4().to_string();
                id.clone_into(&mut used.borrow_mut());
                Some(id)
            },
        );
        used.into_inner()
    }

    #[test]
    fn espera_dobra_e_para_na_quinta() {
        let esperas: Vec<i64> = (1..=STOP_AFTER).map(delay_ms).collect();
        assert_eq!(esperas, [5_000, 10_000, 20_000, 40_000, 80_000]);
        assert_eq!(delay_ms(u32::MAX), 80_000);
    }

    #[test]
    fn mesma_falha_espera_para_e_motivo_novo_recomeca() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        assert_eq!(wait_ms(&store, now_ms()), None);
        let line = fail(&store, "model");
        assert!(wait_ms(&store, now_ms()).is_some_and(|ms| ms > 0 && ms <= FIRST_MS));
        assert_eq!(fail(&store, "model"), line, "outra falha na mesma janela: mesma linha");
        assert_eq!(failures(&store), 1, "e não conta de novo");
        for n in 2..=STOP_AFTER {
            expire(&store);
            assert_eq!(wait_ms(&store, now_ms()), None, "a espera venceu");
            assert_eq!(fail(&store, "model"), line, "mesma linha na Atividade");
            assert_eq!(failures(&store), n);
        }
        expire(&store);
        assert_eq!(wait_ms(&store, now_ms()), Some(i64::MAX), "parado após 5 falhas iguais");
        assert_eq!(poll_after_ms(&store), POLL_MS, "parado, só confere a configuração");
        let now = now_ms();
        assert!(claim_probe(&store, now), "parado: sonda o login local");
        assert!(!claim_probe(&store, now + 1), "uma sondagem por vez");
        assert!(claim_probe(&store, now + PROBE_MS), "de novo só depois de 10 min");
        assert_ne!(fail(&store, "account_disconnected"), line, "motivo novo, linha nova");
        assert_eq!(failures(&store), 1);
        assert!(!claim_probe(&store, now + 2 * PROBE_MS), "só parado sonda");
        clear(&store);
        assert_eq!(wait_ms(&store, now_ms()), None);
        assert_eq!(poll_after_ms(&store), FIRST_MS);
    }

    #[test]
    fn falhas_simultaneas_contam_uma_vez_numa_linha_so() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let shared = &store;
        let lines: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4).map(|_| scope.spawn(move || fail(shared, "model"))).collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).collect()
        });
        assert!(lines.iter().all(|line| *line == lines[0]), "uma linha só: {lines:?}");
        assert_eq!(failures(&store), 1);
    }

    #[test]
    fn configuracao_nova_retoma_sozinha() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for _ in 0..STOP_AFTER {
            expire(&store);
            fail(&store, "model");
        }
        assert_eq!(wait_ms(&store, now_ms()), Some(i64::MAX));
        // Uma conta conectada ou modelo escolhido muda o cadastro/documento.
        let id = format!("ac_{}", "a".repeat(64));
        let registry = serde_json::json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
            "id":id, "backend":"claude", "billing":"subscription", "state":"connected",
            "launch":{"kind":"cli_profile", "executable":"/bin/true", "directory":"/tmp"},
            "scope":"owner", "quotaIdentity":id, "verifiedAt":1,
            "subscriptionProof":{"method":"claude_auth_status", "plan":"max"},
            "models":[{"id":"haiku", "options":[]}]
        }]});
        store.kv_set("fork.engine-accounts.v1", &registry.to_string()).unwrap();
        assert_eq!(wait_ms(&store, now_ms()), None);
        assert_eq!(failures(&store), 0);
    }
}
