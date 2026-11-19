//! Referências descritivas: validar forma não resolve ACL nem autentica pessoa.
pub(crate) mod fontes;
pub(crate) mod resolver;
#[cfg(test)]
mod testes_pessoal;
pub(crate) mod uso;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Version;
impl TryFrom<u8> for Version {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value == 1 { Ok(Self) } else { Err("unsupported_core_version") }
    }
}
impl From<Version> for u8 {
    fn from(_: Version) -> Self {
        1
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);
impl TryFrom<String> for Id {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let id = uuid::Uuid::parse_str(&value).map_err(|_| "invalid_core_id")?;
        if id.is_nil() || id.hyphenated().to_string() != value {
            return Err("invalid_core_id");
        }
        Ok(Self(value))
    }
}
impl Id {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}

/// Positiva e representável sem perda pelo consumidor TypeScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Revision(u64);
impl TryFrom<u64> for Revision {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value == 0 || value > 9_007_199_254_740_991 {
            return Err("invalid_core_revision");
        }
        Ok(Self(value))
    }
}
impl From<Revision> for u64 {
    fn from(value: Revision) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Audience {
    Principal {},
    Additional {},
    Group { group_id: Id },
    Activity { activity_id: Id },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextWire {
    version: Version,
    hub_id: Id,
    space_id: Id,
    conversation_id: Id,
    bot_id: Id,
    audience: Audience,
    team_id: Option<Id>,
    privacy_domain_id: Id,
    domain_revision: Revision,
    conversation_revision: Revision,
}

/// Somente descrição. O emissor futuro deve validar vínculo persistido + Caller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ContextWire", into = "ContextWire")]
pub struct ContextRef(ContextWire);
impl TryFrom<ContextWire> for ContextRef {
    type Error = &'static str;
    fn try_from(value: ContextWire) -> Result<Self, Self::Error> {
        let primary = value.conversation_id == value.bot_id;
        if (matches!(value.audience, Audience::Principal { .. }) && !primary)
            || (matches!(value.audience, Audience::Additional { .. } | Audience::Activity { .. }) && primary)
            || matches!(&value.audience, Audience::Group { group_id } if *group_id != value.conversation_id)
        {
            return Err("invalid_context_audience");
        }
        Ok(Self(value))
    }
}
impl ContextRef {
    pub(crate) fn hub_id(&self) -> &Id {
        &self.0.hub_id
    }
    pub(crate) fn space_id(&self) -> &Id {
        &self.0.space_id
    }
    pub(crate) fn bot_id(&self) -> &Id {
        &self.0.bot_id
    }
    pub(crate) fn team_id(&self) -> Option<&Id> {
        self.0.team_id.as_ref()
    }
    pub(crate) fn audience(&self) -> &Audience {
        &self.0.audience
    }
}
impl From<ContextRef> for ContextWire {
    fn from(value: ContextRef) -> Self {
        value.0
    }
}

/// Escopo pessoal positivo (05a). Sem Clone, construtor público ou Deserialize: só
/// `resolver::resolve_personal` o emite, conservando Caller, origem e fontes capturadas
/// para a revalidação no uso. Não é prova humana, grant de plugin nem autorização de escrita.
/// Nenhuma query de domínio pública aceita `ContextRef` como substituto desta prova.
pub struct ResolvedScope {
    expectation: fontes::PersonalScopeExpectation,
    caller: crate::api::devices::Caller,
    origin: fontes::PersonalOrigin,
    sources: crate::fork::atividades::escopo_store::PersonalSources,
}
