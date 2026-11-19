//! Curadoria pura; o efeito retornado é requisito, nunca cartão consumido.
use super::{
    manifest::{Builtin, Capability, Effect},
    model::{PluginError, PluginResourceKind, PluginResourceOwner, PluginResourceRef},
};
use crate::fork::escopo::{Audience, ContextRef};

pub(super) fn resource_shape(builtin: Builtin, resource: &PluginResourceRef) -> Result<(), PluginError> {
    match (builtin, resource.kind(), resource.owner()) {
        (Builtin::Scatty, PluginResourceKind::Calendar, _)
        | (Builtin::Whatsapp, PluginResourceKind::WhatsappNumber, PluginResourceOwner::External { .. }) => Ok(()),
        _ => Err(PluginError::PolicyDenied),
    }
}

pub(super) fn grant_effect(
    builtin: Builtin,
    resource: &PluginResourceRef,
    context: &ContextRef,
    capability: Capability,
) -> Result<Effect, PluginError> {
    resource_shape(builtin, resource)?;
    if context.space_id().as_str() != crate::fork::spaces::PERSONAL
        || context.hub_id() != &resource.installation_ref().hub_id
        || context.team_id().is_some()
        || !matches!(context.audience(), Audience::Principal {} | Audience::Additional {})
    {
        return Err(PluginError::PolicyDenied);
    }
    if let PluginResourceOwner::Native { space_id, .. } = resource.owner()
        && space_id != context.space_id()
    {
        return Err(PluginError::OwnerMismatch);
    }
    match (builtin, resource.owner(), capability) {
        (
            Builtin::Scatty,
            PluginResourceOwner::Native { .. },
            Capability::CalendarAvailability | Capability::CalendarRead,
        )
        | (
            Builtin::Whatsapp,
            PluginResourceOwner::External { .. },
            Capability::WhatsappReadChats | Capability::WhatsappReadGroups,
        ) => Ok(Effect::Read),
        (
            Builtin::Scatty,
            PluginResourceOwner::Native { .. },
            Capability::CalendarCreate | Capability::CalendarUpdate,
        ) => Ok(Effect::NativeWriteWithCard),
        (
            Builtin::Scatty,
            PluginResourceOwner::External { .. },
            Capability::CalendarAvailability | Capability::CalendarRead,
        ) => Ok(Effect::ExternalReadWithCard),
        (
            Builtin::Whatsapp,
            PluginResourceOwner::External { .. },
            Capability::WhatsappReplyChats | Capability::WhatsappReplyGroups,
        ) => Ok(Effect::ExternalWriteWithCard),
        _ => Err(PluginError::PolicyDenied),
    }
}
