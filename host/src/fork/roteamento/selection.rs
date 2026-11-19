use super::config::{self, Reserve};
use crate::{
    fork::{
        engine_accounts::{Account, Registry, managed},
        spaces::PERSONAL,
    },
    store::BotConfig,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::Connection;
use serde_json::{Value, json};

fn atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
        && !value.contains("://")
        && !value.starts_with('/')
}
pub(super) fn shape(reserve: &Reserve) -> Result<()> {
    ensure!(reserve.selection.model.is_some(), "invalid_field: combinação de reserva inválida.");
    receipt_shape(reserve)
}

/// O recibo da extra também registra o padrão implícito, sem opções nem modelo inventado.
/// Reservas continuam exigindo modelo explícito em `shape`/`validate`.
pub(super) fn receipt_shape(reserve: &Reserve) -> Result<()> {
    ensure!(
        reserve.account_id.len() == 67
            && reserve.account_id.starts_with("ac_")
            && reserve.account_id[3..].bytes().all(|b| b.is_ascii_hexdigit())
            && reserve.account_revision > 0,
        "invalid_field: referência de conta inválida."
    );
    let selection = &reserve.selection;
    ensure!(
        matches!(selection.backend.as_str(), "codex" | "claude")
            && selection.model.as_deref().is_none_or(atom)
            && (selection.model.is_some() || selection.model_options.is_empty())
            && selection.model_options.len() <= 8
            && selection.model_options.iter().all(|(k, v)| atom(k) && atom(v)),
        "invalid_field: combinação de reserva inválida."
    );
    Ok(())
}
pub(super) fn validate(registry: &Registry, bot: &BotConfig, reserve: &Reserve) -> Result<()> {
    shape(reserve)?;
    validate_receipt(registry, bot, reserve)
}

pub(super) fn validate_receipt(registry: &Registry, bot: &BotConfig, reserve: &Reserve) -> Result<()> {
    receipt_shape(reserve)?;
    let account = registry.account(&reserve.account_id)?;
    ensure!(
        account.managed.as_ref().map_or(1, |m| m.revision) == reserve.account_revision,
        "account_conflict: a conta mudou; confira novamente."
    );
    let mut candidate = bot.clone();
    candidate.backend.clone_from(&reserve.selection.backend);
    candidate.model.clone_from(&reserve.selection.model);
    candidate.model_options.clone_from(&reserve.selection.model_options);
    candidate.fork_editor.engine_account_id = Some(reserve.account_id.clone());
    // Intenção não substitui a configuração nem libera motores por comando próprio.
    candidate.command = None;
    managed::validate_selection_in(registry, &candidate)
}

pub(super) fn policy(db: &Connection, bot: &BotConfig, before: &config::Policy, next: &config::Policy) -> Result<()> {
    config::guide(&next.guide)?;
    ensure!(next.reserves.len() <= 8, "invalid_field: use no máximo oito reservas.");
    for (index, reserve) in next.reserves.iter().enumerate() {
        shape(reserve)?;
        ensure!(
            !next.reserves[..index].iter().any(|prior| config::same(prior, reserve).unwrap_or(false)),
            "invalid_field: reserva repetida."
        );
    }
    if bot.space_id != PERSONAL {
        // Sem prova de associação, apenas preservar/remover referências já salvas.
        ensure!(
            next.reserves.iter().all(|r| before.reserves.iter().any(|p| config::same(p, r).unwrap_or(false))),
            "routing_scope_unavailable: vínculo das contas com este Space ainda não comprovado."
        );
        return Ok(());
    }
    if !next.reserves.is_empty() {
        let registry = Registry::load_db(db)?;
        for reserve in &next.reserves {
            validate(&registry, bot, reserve)?;
        }
    }
    Ok(())
}

/// Não abre cofre, não sonda CLI, não chama modelo e não declara elegibilidade.
pub(super) fn fixed(db: &Connection, bot: &BotConfig) -> Value {
    if bot.space_id != PERSONAL {
        return json!({"status":"unavailable","reason":"scope_unproven"});
    }
    let Some(id) = &bot.fork_editor.engine_account_id else {
        return json!({"status":"unavailable","reason":"legacy_preserved"});
    };
    let checked = (|| -> Result<()> {
        let registry = Registry::load_db(db)?;
        let revision = registry.account(id)?.managed.as_ref().map_or(1, |m| m.revision);
        validate(
            &registry,
            bot,
            &Reserve {
                account_id: id.clone(),
                account_revision: revision,
                selection: crate::fork::spaces::EngineSelection {
                    backend: bot.backend.clone(),
                    model: bot.model.clone(),
                    model_options: bot.model_options.clone(),
                },
            },
        )?;
        ensure!(bot.command.is_none(), "custom_command");
        Ok(())
    })();
    if checked.is_ok() {
        json!({"status":"checked","reason":"catalog_only"})
    } else {
        json!({"status":"unavailable","reason":"combination_unproven"})
    }
}
pub(super) fn accounts(db: &Connection, bot: &BotConfig) -> Result<Value> {
    if bot.space_id != PERSONAL {
        return Ok(json!([]));
    }
    let registry = Registry::load_db(db).map_err(|_| {
        anyhow!("accounts_unavailable: não foi possível ler o cadastro de contas; preferências preservadas.")
    })?;
    Ok(json!(registry.accounts.iter().map(Account::dto).collect::<Vec<_>>()))
}
