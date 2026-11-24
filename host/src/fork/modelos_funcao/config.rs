use super::{
    catalogo::{FUNCTIONS, FunctionId},
    resolver::{Account, Reason, accounts},
};
use crate::store::{Store, now_ms};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SLOT: &str = "fork.function-models.v1";
pub const DEFAULTS_REVISION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub account_id: String,
    pub model_id: String,
    pub model_options: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum Selection {
    Automatic {
        #[serde(rename = "accountId")]
        account_id: Option<String>,
        binding: Option<Binding>,
        alternative: Option<Binding>,
        #[serde(rename = "alternativeAccountId")]
        alternative_account_id: Option<String>,
        #[serde(rename = "alternativeIssue")]
        alternative_issue: Option<Reason>,
        issue: Option<Reason>,
    },
    Custom {
        #[serde(flatten)]
        binding: Binding,
    },
    ManagedInVoice,
}

/// Intenção humana, distinta do binding automático calculado pelo servidor.
#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum SelectionInput {
    Automatic {
        #[serde(default, rename = "accountId")]
        account_id: Option<String>,
        #[serde(default, rename = "alternativeAccountId")]
        alternative_account_id: Option<String>,
    },
    Custom {
        #[serde(flatten)]
        binding: Binding,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub revision: u64,
    pub defaults_revision: u32,
    pub updated_by: String,
    pub updated_at: i64,
    pub functions: BTreeMap<FunctionId, Selection>,
}

fn invalid() -> anyhow::Error {
    anyhow!("configuration_invalid: configuração de funções inválida; seleção preservada.")
}

fn valid_account_id(id: &str) -> bool {
    id.len() == 67 && id.starts_with("ac_") && id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_catalog_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !["sk-", "ghp_", "github_pat_"].iter().any(|prefix| text.starts_with(prefix))
}

fn valid_binding(binding: &Binding) -> bool {
    valid_account_id(&binding.account_id)
        && valid_catalog_id(&binding.model_id)
        && binding.model_options.len() <= 16
        && binding.model_options.iter().all(|(key, value)| valid_catalog_id(key) && valid_catalog_id(value))
}

fn decode(raw: &str) -> Result<Document> {
    let doc: Document = serde_json::from_str(raw).map_err(|_| invalid())?;
    if doc.schema_version != 1
        || doc.defaults_revision != DEFAULTS_REVISION
        || doc.revision == 0
        || !matches!(doc.updated_by.as_str(), "owner" | "migration")
        || doc.updated_at < 0
        || doc.functions.len() != FUNCTIONS.len()
    {
        return Err(invalid());
    }
    for id in FUNCTIONS {
        let selection = doc.functions.get(&id).ok_or_else(invalid)?;
        let valid = match selection {
            Selection::ManagedInVoice => id.definition().managed_in_voice,
            Selection::Custom { binding } => !id.definition().managed_in_voice && valid_binding(binding),
            Selection::Automatic {
                account_id,
                binding,
                alternative,
                alternative_account_id,
                alternative_issue,
                issue,
            } => {
                !id.definition().managed_in_voice
                    && account_id.as_deref().is_none_or(valid_account_id)
                    && binding.as_ref().is_none_or(|b| valid_binding(b) && account_id.as_ref() == Some(&b.account_id))
                    && alternative_account_id
                        .as_deref()
                        .is_none_or(|a| id == FunctionId::BotDraft && valid_account_id(a))
                    && alternative.as_ref().is_none_or(|b| {
                        id == FunctionId::BotDraft
                            && valid_binding(b)
                            && alternative_account_id.as_ref() == Some(&b.account_id)
                    })
                    && (id == FunctionId::BotDraft || alternative_issue.is_none())
                    && (id != FunctionId::BotDraft || (alternative.is_some() != alternative_issue.is_some()))
                    && (binding.is_some() != issue.is_some())
            }
        };
        if !valid {
            return Err(invalid());
        }
    }
    Ok(doc)
}

pub fn load(store: &Store) -> Result<Document> {
    let raw = store.kv_read(SLOT)?.ok_or_else(|| anyhow!("configuration_missing: configuração não inicializada."))?;
    decode(&raw)
}

/// Conta utilizável pelo trabalhador: conectada, assinatura comprovada e sem API paga.
/// "Não confirmada" vira `subscription_unverified`; desconectada ou indisponível,
/// `account_disconnected` (o motivo honesto, não `default_model_unknown`).
pub(crate) fn usable(account: &Account) -> Result<(), Reason> {
    match account.state.as_str() {
        "connected" => {}
        "unverified" => return Err(Reason::SubscriptionUnverified),
        _ => return Err(Reason::AccountDisconnected),
    }
    match account.billing.as_str() {
        "subscription" => {}
        "paid_api" | "api" => return Err(Reason::ApiNotEnabled),
        _ => return Err(Reason::BillingUnknown),
    }
    if !account.subscription_verified {
        return Err(Reason::SubscriptionUnverified);
    }
    Ok(())
}

fn choose<'a>(accounts: &'a [Account], backend: &str, requested: Option<&str>) -> Result<&'a Account, Reason> {
    if let Some(id) = requested {
        let account = accounts.iter().find(|a| a.id == id && a.backend == backend).ok_or(Reason::AccountMissing)?;
        usable(account)?;
        return Ok(account);
    }
    let mut eligible = accounts.iter().filter(|a| a.backend == backend && a.subscription_verified);
    let account = eligible.next().ok_or(Reason::AccountMissing)?;
    if eligible.next().is_some() {
        return Err(Reason::AccountChoiceRequired);
    }
    Ok(account)
}

/// O login local preserva o comportamento anterior. Entre contas equivalentes,
/// o ID persistido desempata sem depender da ordem da descoberta. No automático,
/// a conta pedida desconectada, não confirmada ou sem modelo comprovado é pulada
/// em favor de outra conta Claude com assinatura comprovada e modelo anunciado;
/// sem nenhuma, o motivo da conta pedida é preservado (`account_disconnected`).
/// A troca para outra conta fica presa ao Space do bot: fora do Pessoal só a
/// conta pedida vale.
fn choose_background<'a>(accounts: &'a [Account], requested: Option<&str>, space: &str) -> Result<&'a Account, Reason> {
    let requested_issue = match requested {
        None => None,
        Some(id) => {
            let account = accounts.iter().find(|a| a.id == id && a.backend == "claude").ok_or(Reason::AccountMissing);
            if account.as_ref().is_ok_and(|a| matches!(a.billing.as_str(), "paid_api" | "api")) {
                return Err(Reason::ApiNotEnabled);
            }
            match account.and_then(|a| usable(a).and_then(|()| recipe(a, false)).map(|_| a)) {
                Ok(account) => return Ok(account),
                Err(reason) => Some(reason),
            }
        }
    };
    if space != crate::fork::spaces::PERSONAL {
        return Err(requested_issue.unwrap_or(Reason::AccountMissing));
    }
    // TODO(F7, MEX-1322): filtrar `accounts` pelo Space do bot quando existir o
    // registro de contas por Space; hoje o cadastro é global e pertence ao Pessoal.
    let verified = || accounts.iter().filter(|a| a.backend == "claude" && usable(a).is_ok());
    verified().filter(|a| recipe(a, false).is_ok()).min_by_key(|a| (a.origin != "legacy", &a.id)).ok_or_else(|| {
        requested_issue.unwrap_or(if verified().next().is_some() {
            Reason::DefaultModelUnknown
        } else {
            Reason::AccountMissing
        })
    })
}

fn recipe(account: &Account, codex: bool) -> Result<Binding, Reason> {
    // AC ainda não anuncia isDefault; escolher o primeiro modelo inventaria o padrão Codex.
    if codex {
        return Err(Reason::DefaultModelUnknown);
    }
    let model = account.models.iter().find(|m| m.id == "haiku").ok_or(Reason::DefaultModelUnknown)?;
    let model_options = model
        .options
        .iter()
        .find(|o| matches!(o.id.as_str(), "reasoning_effort" | "effort") && o.values.iter().any(|v| v == "low"))
        .map(|o| (o.id.clone(), "low".into()))
        .into_iter()
        .collect();
    Ok(Binding { account_id: account.id.clone(), model_id: model.id.clone(), model_options })
}

fn automatic(id: FunctionId, accounts: &[Account], requested: Option<&str>, alternative_id: Option<&str>) -> Selection {
    let codex = id.codex_default();
    let account = if matches!(id, FunctionId::MemoryConsolidation | FunctionId::BotNaming) {
        // O documento de funções é a seleção global do Pessoal.
        choose_background(accounts, requested, crate::fork::spaces::PERSONAL)
    } else {
        choose(accounts, if codex { "codex" } else { "claude" }, requested)
    };
    let binding = account.as_ref().map_err(|reason| *reason).and_then(|a| recipe(a, codex));
    let alternative_account = (id == FunctionId::BotDraft).then(|| choose(accounts, "claude", alternative_id));
    let alternative_account_id = alternative_id
        .map(str::to_owned)
        .or_else(|| alternative_account.as_ref().and_then(|a| a.as_ref().ok()).map(|a| a.id.clone()));
    let alternative = alternative_account.map(|a| a.and_then(|a| recipe(a, false)));
    Selection::Automatic {
        account_id: account.ok().map(|a| a.id.clone()).or_else(|| requested.map(str::to_owned)),
        issue: binding.as_ref().err().copied(),
        binding: binding.ok(),
        alternative_account_id,
        alternative_issue: alternative.as_ref().and_then(|a| a.as_ref().err().copied()),
        alternative: alternative.and_then(Result::ok),
    }
}

pub fn initialize(store: &Store) -> Result<()> {
    if let Some(raw) = store.kv_read(SLOT)? {
        let Ok(mut document) = decode(&raw) else { return Ok(()) };
        // Reavalia só as seleções automáticas do trabalhador: sem vínculo, ou com
        // vínculo numa conta que deixou de ser utilizável (desconectada, não
        // confirmada, sem o modelo). A conta pedida continua preferida; se não
        // servir, o automático pula para outra comprovada ou grava o motivo
        // honesto. Escolha personalizada nunca é trocada aqui.
        {
            let Ok(available) = accounts(store) else { return Ok(()) };
            let mut changed = false;
            for id in [FunctionId::MemoryConsolidation, FunctionId::BotNaming] {
                let Some(current) = document.functions.get(&id).cloned() else { continue };
                let Selection::Automatic { account_id, binding, .. } = &current else { continue };
                let stale = binding.as_ref().is_none_or(|b| {
                    available
                        .iter()
                        .find(|a| a.id == b.account_id)
                        .is_none_or(|a| usable(a).is_err() || !a.models.iter().any(|m| m.id == b.model_id))
                });
                if !stale {
                    continue;
                }
                let candidate = automatic(id, &available, account_id.as_deref(), None);
                if candidate != current {
                    document.functions.insert(id, candidate);
                    changed = true;
                }
            }
            if changed {
                document.revision = document.revision.checked_add(1).ok_or_else(invalid)?;
                document.updated_at = now_ms();
                store.kv_compare_exchange(SLOT, Some(&raw), &serde_json::to_string(&document)?)?;
            }
        }
        return Ok(());
    }
    let Ok(accounts) = accounts(store) else {
        // Não registrar erro/payload: o cadastro pode conter referências privadas.
        tracing::warn!("accounts_unavailable: inicialização de modelos por função adiada");
        return Ok(());
    };
    let functions = FUNCTIONS
        .into_iter()
        .map(|id| {
            (
                id,
                if id.definition().managed_in_voice {
                    Selection::ManagedInVoice
                } else {
                    automatic(id, &accounts, None, None)
                },
            )
        })
        .collect();
    let doc = Document {
        schema_version: 1,
        revision: 1,
        defaults_revision: DEFAULTS_REVISION,
        updated_by: "migration".into(),
        updated_at: now_ms(),
        functions,
    };
    store.kv_compare_exchange(SLOT, None, &serde_json::to_string(&doc)?)?;
    Ok(())
}

pub fn save(store: &Store, expected: u64, id: Option<FunctionId>, input: Option<SelectionInput>) -> Result<Document> {
    let raw = store.kv_read(SLOT)?.ok_or_else(invalid)?;
    let mut doc = decode(&raw)?;
    if doc.revision != expected {
        bail!("settings_conflict: esta configuração mudou em outra janela; recarregue antes de salvar.");
    }
    if let Some(id) = id {
        if id.definition().managed_in_voice {
            bail!("managed_in_voice: esta função é gerenciada em Voz.");
        }
        let selection = match input {
            Some(SelectionInput::Custom { binding }) => {
                if !valid_binding(&binding) {
                    bail!("invalid_field: seleção inválida.");
                }
                // D118: o cadastro contém somente contas do dono. Catálogo valida a intenção;
                // remoções posteriores continuam preservadas na leitura do documento.
                let accounts = accounts(store)?;
                let model = accounts
                    .iter()
                    .find(|a| a.id == binding.account_id)
                    .and_then(|a| a.models.iter().find(|m| m.id == binding.model_id));
                if model.is_none_or(|model| {
                    binding.model_options.iter().any(|(key, value)| {
                        !model.options.iter().any(|option| option.id == *key && option.values.contains(value))
                    })
                }) {
                    bail!("invalid_field: conta, modelo ou opção não disponível no catálogo.");
                }
                Selection::Custom { binding }
            }
            Some(SelectionInput::Automatic { account_id, alternative_account_id }) => {
                if account_id.as_deref().is_some_and(|id| !valid_account_id(id))
                    || alternative_account_id.as_deref().is_some_and(|id| !valid_account_id(id))
                    || (id != FunctionId::BotDraft && alternative_account_id.is_some())
                {
                    bail!("invalid_field: vínculo automático inválido.");
                }
                let accounts = accounts(store)?;
                if matches!(id, FunctionId::MemoryConsolidation | FunctionId::BotNaming)
                    && account_id.as_ref().is_some_and(|requested| {
                        accounts.iter().any(|a| &a.id == requested && matches!(a.billing.as_str(), "paid_api" | "api"))
                    })
                {
                    bail!("invalid_field: escolha um modelo explicitamente para usar a chave de API.");
                }
                // Só a existência no cadastro: escolher uma conta hoje desconectada é
                // permitido e aparece como `account_disconnected` na resolução.
                for (requested, backend) in [
                    (account_id.as_deref(), if id.codex_default() { "codex" } else { "claude" }),
                    (alternative_account_id.as_deref(), "claude"),
                ] {
                    if requested
                        .is_some_and(|requested| !accounts.iter().any(|a| a.id == requested && a.backend == backend))
                    {
                        bail!("invalid_field: conta automática não disponível no cadastro.");
                    }
                }
                automatic(id, &accounts, account_id.as_deref(), alternative_account_id.as_deref())
            }
            None => automatic(id, &accounts(store)?, None, None),
        };
        doc.functions.insert(id, selection);
    } else {
        let accounts = accounts(store)?;
        for id in FUNCTIONS.into_iter().filter(|id| !id.definition().managed_in_voice) {
            doc.functions.insert(id, automatic(id, &accounts, None, None));
        }
    }
    doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
    doc.updated_by = "owner".into();
    doc.updated_at = now_ms();
    if !store.kv_compare_exchange(SLOT, Some(&raw), &serde_json::to_string(&doc)?)? {
        bail!("settings_conflict: esta configuração mudou em outra janela; recarregue antes de salvar.");
    }
    // MEX-1355: trocar conta ou modelo da função tira a função da parada.
    crate::fork::background_selection::backoff::changed(store);
    Ok(doc)
}

#[cfg(test)]
mod background_defaults_tests {
    use super::*;
    use crate::fork::spaces::PERSONAL as P;

    fn account(id: &str, origin: &str) -> Account {
        Account {
            id: id.into(),
            label: "conta".into(),
            origin: origin.into(),
            backend: "claude".into(),
            billing: "subscription".into(),
            state: "connected".into(),
            models: vec![super::super::resolver::Model { id: "haiku".into(), options: vec![] }],
            quota_identity: None,
            subscription_verified: true,
        }
    }

    #[test]
    fn prefere_login_local_e_desempata_de_forma_estavel() {
        let managed = account("ac_b", "managed");
        let legacy = account("ac_a", "legacy");
        for accounts in [vec![managed.clone(), legacy.clone()], vec![legacy.clone(), managed.clone()]] {
            assert_eq!(choose_background(&accounts, None, P).unwrap().id, "ac_a");
            assert_eq!(choose_background(&accounts, Some("ac_b"), P).unwrap().id, "ac_b");
        }
    }

    #[test]
    fn automatico_pula_conta_pedida_que_nao_serve_e_guarda_o_motivo() {
        // Fora do Pessoal não há troca automática para outra conta.
        let mut off = account("ac_a", "legacy");
        off.state = "disconnected".into();
        off.subscription_verified = false;
        let ok = account("ac_b", "managed");
        let mut no_model = account("ac_c", "managed");
        no_model.models.clear();
        assert_eq!(choose_background(&[off.clone(), ok.clone()], Some("ac_a"), P).unwrap().id, "ac_b");
        assert_eq!(
            choose_background(&[off.clone(), ok.clone()], Some("ac_a"), "outro-space").unwrap_err(),
            Reason::AccountDisconnected
        );
        assert_eq!(
            choose_background(std::slice::from_ref(&ok), None, "outro-space").unwrap_err(),
            Reason::AccountMissing
        );
        assert_eq!(
            choose_background(&[off.clone(), no_model.clone()], Some("ac_a"), P).unwrap_err(),
            Reason::AccountDisconnected
        );
        assert_eq!(
            choose_background(std::slice::from_ref(&no_model), None, P).unwrap_err(),
            Reason::DefaultModelUnknown
        );
        assert_eq!(
            choose_background(&[no_model, ok], None, P).unwrap().id,
            "ac_b",
            "conta sem modelo comprovado é pulada"
        );
        assert_eq!(choose_background(&[], None, P).unwrap_err(), Reason::AccountMissing);
        let mut unverified = account("ac_d", "managed");
        unverified.state = "unverified".into();
        assert_eq!(usable(&unverified), Err(Reason::SubscriptionUnverified));
        let mut paid = account("ac_e", "managed");
        paid.billing = "paid_api".into();
        assert_eq!(usable(&paid), Err(Reason::ApiNotEnabled));
    }
}
