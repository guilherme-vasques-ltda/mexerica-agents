use super::{
    catalogo::FunctionId,
    config::{self, Binding, Selection},
};
use crate::{api::devices::Caller, store::Store};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    UnknownFunction,
    ContextDenied,
    RevisionChanged,
    ConfigurationUnavailable,
    AccountsUnavailable,
    AccountMissing,
    AccountChoiceRequired,
    AccountDisconnected,
    BillingUnknown,
    ApiNotEnabled,
    SubscriptionUnverified,
    QuotaUnknown,
    QuotaExhausted,
    ModelMissing,
    OptionUnsupported,
    CapabilitiesUnconfirmed,
    ModelIncompatible,
    DefaultModelUnknown,
    ManagedInVoice,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub values: Vec<String>,
    pub default: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub options: Vec<ModelOption>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing)]
    pub origin: String,
    pub backend: String,
    pub billing: String,
    pub state: String,
    pub models: Vec<Model>,
    #[serde(skip_serializing)]
    pub quota_identity: Option<String>,
    #[serde(skip_serializing)]
    pub subscription_verified: bool,
}

pub fn accounts(store: &Store) -> Result<Vec<Account>> {
    #[derive(Deserialize)]
    struct Snapshot {
        accounts: Vec<Account>,
    }
    let snapshot: Snapshot = serde_json::from_value(crate::fork::engine_accounts::read_for_functions(store)?)?;
    let mut ids = std::collections::BTreeSet::new();
    if snapshot.accounts.iter().any(|a| !ids.insert(&a.id)) {
        bail!("accounts_unavailable: IDs de conta duplicados; seleção preservada.");
    }
    Ok(snapshot.accounts)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub input: Vec<Modality>,
    pub output: Vec<Modality>,
    pub tools_disabled: bool,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Cota por conta será comprovada no 20.2a; a consulta local só tem Unknown.")
)]
pub enum Quota {
    Unknown,
    Available,
    Exhausted,
}

/// Evidência do adaptador/Hub, nunca desserializada de payload da API ou do modelo.
/// A identidade precisa corresponder à cota da conta escolhida, não ao backend do bot.
pub struct AccountEvidence {
    pub account_id: String,
    pub quota_identity: String,
    pub quota: Quota,
    pub models: Vec<(String, Capabilities)>,
}

/// Construído pelo transporte/consumidor confiável; não implementa Deserialize.
/// Uso de jobs dependerá da autorização/procedência da 13/22 e do executor 20.2a.
pub struct TrustedContext<'a> {
    pub caller: &'a Caller,
    pub owner: &'a str,
    pub authorized: bool,
    pub external: bool,
    pub bot_id: Option<&'a str>,
    pub lane: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub entry_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub revision: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub evidence: &'a [AccountEvidence],
}

impl<'a> TrustedContext<'a> {
    pub fn administration(caller: &'a Caller) -> Self {
        Self {
            caller,
            owner: "owner",
            authorized: true,
            external: false,
            bot_id: None,
            lane: None,
            turn_id: None,
            entry_id: None,
            run_id: None,
            revision: None,
            input_tokens: 0,
            output_tokens: 0,
            evidence: &[],
        }
    }

    fn valid(&self) -> bool {
        let ids = [self.bot_id, self.lane, self.turn_id, self.entry_id, self.run_id];
        self.authorized
            && self.owner == "owner"
            && matches!(self.caller, Caller::Local)
            && (ids.iter().all(Option::is_none) || ids.iter().all(|id| id.is_some_and(|id| !id.is_empty())))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionUnavailable {
    pub function_id: String,
    pub reason: Reason,
    pub revision: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFunction {
    pub function_id: FunctionId,
    pub requested: Selection,
    pub effective: Binding,
    pub owner: String,
    pub billing: String,
    pub revision: u64,
    pub defaults_revision: u32,
    pub capabilities: Capabilities,
    pub deadline_ms: u64,
    pub declared_alternative: Option<Binding>,
    pub alternative_unavailable: Option<Reason>,
    pub execution: &'static str,
}

fn binding_capabilities(
    id: FunctionId,
    binding: &Binding,
    accounts: &[Account],
    context: &TrustedContext<'_>,
) -> Result<Capabilities, Reason> {
    let account = accounts.iter().find(|a| a.id == binding.account_id).ok_or(Reason::AccountMissing)?;
    // O cadastro atual é exclusivo do dono (D118), validado pelo contexto acima.
    // Unverified é um estado de autenticação/plano, não uma desconexão.
    if !matches!(account.state.as_str(), "connected" | "unverified") {
        return Err(Reason::AccountDisconnected);
    }
    match account.billing.as_str() {
        "paid_api" | "api" => return Err(Reason::ApiNotEnabled),
        "subscription" => {}
        _ => return Err(Reason::BillingUnknown),
    }
    if !account.subscription_verified || account.state != "connected" {
        return Err(Reason::SubscriptionUnverified);
    }
    let evidence = context
        .evidence
        .iter()
        .find(|e| e.account_id == account.id && Some(e.quota_identity.as_str()) == account.quota_identity.as_deref());
    match evidence.map_or(Quota::Unknown, |e| e.quota) {
        Quota::Unknown => return Err(Reason::QuotaUnknown),
        Quota::Exhausted => return Err(Reason::QuotaExhausted),
        Quota::Available => {}
    }
    let model = account.models.iter().find(|m| m.id == binding.model_id).ok_or(Reason::ModelMissing)?;
    for (key, value) in &binding.model_options {
        if !model.options.iter().any(|o| &o.id == key && o.values.contains(value)) {
            return Err(Reason::OptionUnsupported);
        }
    }
    let capabilities = evidence
        .and_then(|e| e.models.iter().find(|(m, _)| *m == model.id))
        .map(|(_, capabilities)| capabilities.clone())
        .ok_or(Reason::CapabilitiesUnconfirmed)?;
    if !capabilities.input.contains(&Modality::Text)
        || !capabilities.output.contains(&Modality::Text)
        || !capabilities.tools_disabled
        || (id == FunctionId::Vision && !capabilities.input.contains(&Modality::Image))
        || capabilities.max_input_tokens < context.input_tokens
        || capabilities.max_output_tokens < context.output_tokens
    {
        return Err(Reason::ModelIncompatible);
    }
    Ok(capabilities)
}

pub fn resolve_function(
    store: &Store,
    function_id: &str,
    context: &TrustedContext<'_>,
) -> Result<ResolvedFunction, FunctionUnavailable> {
    let mut revision = None;
    let mut resolve = || -> Result<ResolvedFunction, Reason> {
        let id: FunctionId = serde_json::from_value(function_id.into()).map_err(|_| Reason::UnknownFunction)?;
        if !context.valid() || (id == FunctionId::MessageTriage && context.external) {
            return Err(Reason::ContextDenied);
        }
        let doc = config::load(store).map_err(|_| {
            if accounts(store).is_err() { Reason::AccountsUnavailable } else { Reason::ConfigurationUnavailable }
        })?;
        revision = Some(doc.revision);
        if context.revision.is_some_and(|expected| expected != doc.revision) {
            return Err(Reason::RevisionChanged);
        }
        let requested = doc.functions[&id].clone();
        let (binding, alternative, declared_alternative_issue) = match &requested {
            Selection::ManagedInVoice => return Err(Reason::ManagedInVoice),
            Selection::Custom { binding } => (binding, None, None),
            Selection::Automatic { account_id, binding, alternative, alternative_issue, issue, .. } => {
                let Some(binding) = binding else {
                    // O motivo gravado pode ser antigo: a conta escolhida desconectada
                    // é `account_disconnected`, nunca `default_model_unknown`.
                    let live = account_id
                        .as_deref()
                        .and_then(|id| accounts(store).ok()?.into_iter().find(|a| a.id == id))
                        .and_then(|account| config::usable(&account).err());
                    return Err(live.or(*issue).unwrap_or(Reason::DefaultModelUnknown));
                };
                (binding, alternative.clone(), *alternative_issue)
            }
        };
        let accounts = accounts(store).map_err(|_| Reason::AccountsUnavailable)?;
        let capabilities = binding_capabilities(id, binding, &accounts, context)?;
        let alternative_unavailable = alternative
            .as_ref()
            .and_then(|b| binding_capabilities(id, b, &accounts, context).err())
            .or(declared_alternative_issue);
        Ok(ResolvedFunction {
            function_id: id,
            effective: binding.clone(),
            requested,
            owner: context.owner.into(),
            billing: "subscription".into(),
            revision: doc.revision,
            defaults_revision: doc.defaults_revision,
            capabilities,
            deadline_ms: id.definition().deadline_ms,
            declared_alternative: alternative,
            alternative_unavailable,
            execution: "not_implemented",
        })
    };
    resolve().map_err(|reason| FunctionUnavailable { function_id: function_id.into(), reason, revision })
}
