//! Seam privado para o broker futuro, sem Caller/DTO como autoridade.
//! Usa exclusivamente a Store original; não emite `ResolvedScope` nem oferece SQL
//! genérico. Nenhum produtor positivo chama estes métodos neste recorte.
use super::{
    lifecycle::{LifecycleChange, transition_installation},
    manifest::{Builtin, Capability, Effect},
    model::{
        InstallationState, PluginAccountRef, PluginError, PluginInstallationRef, PluginResourceKind,
        PluginResourceOwner, PluginResourceRef,
    },
    policy,
};
use crate::{
    LockExt,
    fork::escopo::{ContextRef, Id, Revision, Version},
    store::Store,
};
use rusqlite::{Connection, OptionalExtension, params};

type Result<T> = std::result::Result<T, PluginError>;

pub(super) fn db_error(error: rusqlite::Error) -> PluginError {
    let rusqlite::Error::SqliteFailure(code, message) = error else {
        return PluginError::Storage;
    };
    // Consome e descarta a mensagem privada; só o código estático pode sair.
    drop(message);
    if code.code == rusqlite::ErrorCode::ConstraintViolation { PluginError::Constraint } else { PluginError::Storage }
}

fn revision(value: i64) -> Result<Revision> {
    u64::try_from(value).ok().and_then(|v| Revision::try_from(v).ok()).ok_or(PluginError::InvalidDescriptor)
}

pub(super) fn identifier(value: String) -> Result<Id> {
    value.try_into().map_err(|_| PluginError::InvalidDescriptor)
}

pub(super) fn first() -> Result<Revision> {
    revision(1)
}

fn counter(value: Revision) -> Result<i64> {
    i64::try_from(u64::from(value)).map_err(|_| PluginError::CounterExhausted)
}

fn state(value: InstallationState) -> &'static str {
    match value {
        InstallationState::Disabled => "disabled",
        InstallationState::Enabled => "enabled",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct GrantSnapshot {
    pub id: Id,
    pub generation: Revision,
    pub revision: Revision,
    pub revoked: bool,
}

pub(super) struct Registry<'a> {
    store: &'a Store,
}

impl<'a> Registry<'a> {
    pub(super) fn new(store: &'a Store) -> Self {
        Self { store }
    }

    // Exclusivamente configuração interna. Ativação inicial/N2 não é boot DDL.
    pub(super) fn register(&self, hub: &Id, id: &Id, builtin: Builtin) -> Result<PluginInstallationRef> {
        insert_installation(&self.store.db.locked(), hub, id, builtin)
    }

    pub(super) fn installation(&self, id: &Id) -> Result<PluginInstallationRef> {
        installation(&self.store.db.locked(), id).map(|(snapshot, _)| snapshot)
    }

    pub(super) fn change(
        &self,
        expected: &PluginInstallationRef,
        change: LifecycleChange,
    ) -> Result<PluginInstallationRef> {
        let mut c = self.store.db.locked();
        let tx = c.transaction().map_err(db_error)?;
        let next = apply_change(&tx, expected, change)?;
        tx.commit().map_err(db_error)?;
        Ok(next)
    }

    pub(super) fn add_account(&self, expected: &PluginInstallationRef, id: &Id) -> Result<PluginAccountRef> {
        let mut c = self.store.db.locked();
        let tx = c.transaction().map_err(db_error)?;
        let account = add_account_in(&tx, expected, id)?;
        tx.commit().map_err(db_error)?;
        Ok(account)
    }

    pub(super) fn add_resource(&self, resource: &PluginResourceRef) -> Result<()> {
        let mut c = self.store.db.locked();
        let tx = c.transaction().map_err(db_error)?;
        add_resource_in(&tx, resource)?;
        tx.commit().map_err(db_error)
    }

    /// Configuração por recurso exato/contexto privado, sem seletor ou herança.
    /// Não é admissão: `ContextRef` e Effect seguem descritivos e não consomem cartão.
    pub(super) fn grant(
        &self,
        id: &Id,
        expected: &PluginResourceRef,
        context: &ContextRef,
        capability: Capability,
    ) -> Result<GrantSnapshot> {
        let mut c = self.store.db.locked();
        let tx = c.transaction().map_err(db_error)?;
        let snapshot = grant_in(&tx, id, expected, context, capability)?;
        tx.commit().map_err(db_error)?;
        Ok(snapshot)
    }

    pub(super) fn revoke(&self, expected: &GrantSnapshot) -> Result<GrantSnapshot> {
        if expected.revoked {
            return Err(PluginError::InvalidTransition);
        }
        let next_generation =
            revision(counter(expected.generation)?.checked_add(1).ok_or(PluginError::CounterExhausted)?)
                .map_err(|_| PluginError::CounterExhausted)?;
        let next_revision = revision(counter(expected.revision)?.checked_add(1).ok_or(PluginError::CounterExhausted)?)
            .map_err(|_| PluginError::CounterExhausted)?;
        let c = self.store.db.locked();
        let changed = c.execute(
            "UPDATE donna_plugin_grants SET state='revoked',generation=?1,revision=?2 WHERE id=?3 AND generation=?4 AND revision=?5 AND state='active'",
            params![counter(next_generation)?, counter(next_revision)?, expected.id.as_str(), counter(expected.generation)?, counter(expected.revision)?],
        ).map_err(db_error)?;
        if changed != 1 {
            return Err(PluginError::SnapshotConflict);
        }
        Ok(GrantSnapshot {
            id: expected.id.clone(),
            generation: next_generation,
            revision: next_revision,
            revoked: true,
        })
    }

    /// Consulta privada de configuração. Não retorna autorização ou conteúdo.
    pub(super) fn configured_effect(
        &self,
        expected: &PluginResourceRef,
        context: &ContextRef,
        capability: Capability,
    ) -> Result<Option<Effect>> {
        let mut db = self.store.db.locked();
        let c = db.transaction().map_err(db_error)?;
        let builtin = current_installation(&c, expected.installation_ref())?;
        if resource(&c, expected.installation_ref(), expected.resource_id())? != *expected {
            return Err(PluginError::SnapshotConflict);
        }
        bot_context(&c, context)?;
        let effect = policy::grant_effect(builtin, expected, context, capability)?;
        let context_json = serde_json::to_string(context).map_err(|_| PluginError::InvalidDescriptor)?;
        let (ag, ar) = match expected.owner() {
            PluginResourceOwner::Native { .. } => (None, None),
            PluginResourceOwner::External { account_ref } => {
                (Some(counter(account_ref.generation)?), Some(counter(account_ref.revision)?))
            }
        };
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM donna_plugin_grants g JOIN bots b ON b.id=g.bot_id AND b.deleted=0
             WHERE g.resource_id=?1 AND g.context=?2 AND g.capability=?3 AND g.effect=?4 AND g.state='active'
             AND g.installation_generation=?5 AND g.resource_generation=?6 AND g.resource_revision=?7
             AND g.account_generation IS ?8 AND g.account_revision IS ?9)",
                params![
                    expected.resource_id().as_str(),
                    context_json,
                    capability.key(),
                    effect.key(),
                    counter(expected.installation_ref().generation)?,
                    counter(expected.generation())?,
                    counter(expected.revision())?,
                    ag,
                    ar
                ],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        c.commit().map_err(db_error)?;
        Ok(exists.then_some(effect))
    }
}

pub(super) fn add_account_in(c: &Connection, expected: &PluginInstallationRef, id: &Id) -> Result<PluginAccountRef> {
    current_installation(c, expected)?;
    c.execute(
        "INSERT INTO donna_plugin_accounts(id,installation_id,generation,revision) VALUES(?1,?2,1,1)",
        params![id.as_str(), expected.installation_id.as_str()],
    )
    .map_err(db_error)?;
    Ok(PluginAccountRef {
        version: Version,
        installation_ref: expected.clone(),
        account_id: id.clone(),
        generation: first()?,
        revision: first()?,
    })
}

pub(super) fn add_resource_in(c: &Connection, resource: &PluginResourceRef) -> Result<()> {
    if resource.generation() != first()? || resource.revision() != first()? {
        return Err(PluginError::InvalidDescriptor);
    }
    let builtin = current_installation(c, resource.installation_ref())?;
    policy::resource_shape(builtin, resource)?;
    let (owner, hub, space, account) = match resource.owner() {
        PluginResourceOwner::Native { hub_id, space_id } => {
            ("native", Some(hub_id.as_str()), Some(space_id.as_str()), None)
        }
        PluginResourceOwner::External { account_ref } => {
            if account(c, resource.installation_ref(), &account_ref.account_id)? != *account_ref {
                return Err(PluginError::SnapshotConflict);
            }
            ("external", None, None, Some(account_ref.account_id.as_str()))
        }
    };
    c.execute(
        "INSERT INTO donna_plugin_resources(id,installation_id,owner,hub_id,space_id,account_id,kind,generation,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,1,1)",
        params![resource.resource_id().as_str(), resource.installation_ref().installation_id.as_str(), owner, hub, space, account, kind_key(resource.kind())],
    ).map_err(db_error)?;
    Ok(())
}

fn kind_key(kind: PluginResourceKind) -> &'static str {
    match kind {
        PluginResourceKind::Calendar => "calendar",
        PluginResourceKind::WhatsappNumber => "whatsapp_number",
    }
}

/// Configuração por recurso exato/contexto privado, sem seletor ou herança.
/// Não é admissão: `ContextRef` e Effect seguem descritivos e não consomem cartão.
pub(super) fn grant_in(
    c: &Connection,
    id: &Id,
    expected: &PluginResourceRef,
    context: &ContextRef,
    capability: Capability,
) -> Result<GrantSnapshot> {
    let builtin = current_installation(c, expected.installation_ref())?;
    if resource(c, expected.installation_ref(), expected.resource_id())? != *expected {
        return Err(PluginError::SnapshotConflict);
    }
    bot_context(c, context)?;
    let effect = policy::grant_effect(builtin, expected, context, capability)?;
    let (account_generation, account_revision) = match expected.owner() {
        PluginResourceOwner::Native { .. } => (None, None),
        PluginResourceOwner::External { account_ref } => {
            (Some(counter(account_ref.generation)?), Some(counter(account_ref.revision)?))
        }
    };
    let context_json = serde_json::to_string(context).map_err(|_| PluginError::InvalidDescriptor)?;
    c.execute(
        "INSERT INTO donna_plugin_grants(id,installation_id,resource_id,bot_id,context,capability,effect,installation_generation,resource_generation,resource_revision,account_generation,account_revision,generation,revision,state)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,1,1,'active')",
        params![id.as_str(), expected.installation_ref().installation_id.as_str(), expected.resource_id().as_str(), context.bot_id().as_str(), context_json, capability.key(), effect.key(), counter(expected.installation_ref().generation)?, counter(expected.generation())?, counter(expected.revision())?, account_generation, account_revision],
    ).map_err(db_error)?;
    Ok(GrantSnapshot { id: id.clone(), generation: first()?, revision: first()?, revoked: false })
}

fn insert_installation(c: &Connection, hub: &Id, id: &Id, builtin: Builtin) -> Result<PluginInstallationRef> {
    c.execute(
        "INSERT INTO donna_plugin_installations(id,version,hub_id,builtin,state,generation,revision) VALUES(?1,1,?2,?3,'disabled',1,1)",
        params![id.as_str(), hub.as_str(), builtin.key()],
    ).map_err(db_error)?;
    Ok(PluginInstallationRef {
        version: Version,
        hub_id: hub.clone(),
        installation_id: id.clone(),
        generation: first()?,
        revision: first()?,
        state: InstallationState::Disabled,
    })
}

fn apply_change(
    c: &Connection,
    expected: &PluginInstallationRef,
    change: LifecycleChange,
) -> Result<PluginInstallationRef> {
    let (current, _) = installation(c, &expected.installation_id)?;
    let next = transition_installation(&current, expected, change)?;
    let changed = c.execute(
        "UPDATE donna_plugin_installations SET state=?1,generation=?2,revision=?3 WHERE id=?4 AND generation=?5 AND revision=?6",
        params![state(next.state), counter(next.generation)?, counter(next.revision)?, expected.installation_id.as_str(), counter(expected.generation)?, counter(expected.revision)?],
    ).map_err(db_error)?;
    if changed != 1 {
        return Err(PluginError::SnapshotConflict);
    }
    // Até reativar invalida admissões anteriores; nunca muda revoked->active.
    c.execute(
        "UPDATE donna_plugin_grants SET state='revoked',generation=generation+1,revision=revision+1 WHERE installation_id=?1 AND state='active'",
        [expected.installation_id.as_str()],
    ).map_err(db_error)?;
    Ok(next)
}

/// Instalação única de um plugin próprio neste Hub, se já registrada.
pub(crate) fn builtin_installation(
    c: &Connection,
    hub: &Id,
    builtin: Builtin,
) -> Result<Option<PluginInstallationRef>> {
    let id: Option<String> = c
        .query_row(
            "SELECT id FROM donna_plugin_installations WHERE hub_id=?1 AND builtin=?2",
            params![hub.as_str(), builtin.key()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    id.map(|id| installation(c, &identifier(id)?).map(|(snapshot, _)| snapshot)).transpose()
}

/// Liga/desliga na transação do chamador (gestão do dono já validada por ele).
/// Registra desligada na primeira vez; cada troca sobe a geração e revoga grants.
pub(crate) fn set_builtin(
    c: &Connection,
    hub: &Id,
    builtin: Builtin,
    change: LifecycleChange,
) -> Result<PluginInstallationRef> {
    if c.is_autocommit() {
        return Err(PluginError::InvalidTransition);
    }
    let current = match builtin_installation(c, hub, builtin)? {
        Some(snapshot) => snapshot,
        None => insert_installation(c, hub, &identifier(uuid::Uuid::new_v4().to_string())?, builtin)?,
    };
    apply_change(c, &current, change)
}

// Consistência descritiva com o vínculo persistido, sem resolver autoridade.
fn bot_context(c: &Connection, context: &ContextRef) -> Result<()> {
    let raw: String = c
        .query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [context.bot_id().as_str()], |r| r.get(0))
        .optional()
        .map_err(db_error)?
        .ok_or(PluginError::PolicyDenied)?;
    let cfg: crate::store::BotConfig = serde_json::from_str(&raw).map_err(|_| PluginError::PolicyDenied)?;
    if cfg.id != context.bot_id().as_str() || cfg.space_id != crate::fork::spaces::PERSONAL || cfg.is_group() {
        return Err(PluginError::PolicyDenied);
    }
    Ok(())
}

fn installation(c: &Connection, id: &Id) -> Result<(PluginInstallationRef, Builtin)> {
    let (hub, builtin, generation, rev, status): (String, String, i64, i64, String) = c
        .query_row(
            "SELECT hub_id,builtin,generation,revision,state FROM donna_plugin_installations WHERE id=?1",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PluginError::NotFound)?;
    let state = match status.as_str() {
        "enabled" => InstallationState::Enabled,
        "disabled" => InstallationState::Disabled,
        _ => return Err(PluginError::InvalidDescriptor),
    };
    Ok((
        PluginInstallationRef {
            version: Version,
            hub_id: identifier(hub)?,
            installation_id: id.clone(),
            generation: revision(generation)?,
            revision: revision(rev)?,
            state,
        },
        Builtin::parse(&builtin)?,
    ))
}

pub(super) fn current_installation(c: &Connection, expected: &PluginInstallationRef) -> Result<Builtin> {
    let (current, builtin) = installation(c, &expected.installation_id)?;
    if &current != expected {
        return Err(PluginError::SnapshotConflict);
    }
    if current.state != InstallationState::Enabled {
        return Err(PluginError::InvalidTransition);
    }
    Ok(builtin)
}

pub(super) fn account(c: &Connection, installation: &PluginInstallationRef, id: &Id) -> Result<PluginAccountRef> {
    let (generation, rev): (i64, i64) = c
        .query_row(
            "SELECT generation,revision FROM donna_plugin_accounts WHERE id=?1 AND installation_id=?2",
            params![id.as_str(), installation.installation_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PluginError::NotFound)?;
    Ok(PluginAccountRef {
        version: Version,
        installation_ref: installation.clone(),
        account_id: id.clone(),
        generation: revision(generation)?,
        revision: revision(rev)?,
    })
}

pub(super) fn resource(c: &Connection, installation: &PluginInstallationRef, id: &Id) -> Result<PluginResourceRef> {
    // SQL fechado; nenhuma linha/consulta genérica sai deste módulo.
    let raw = c.query_row(
        "SELECT owner,hub_id,space_id,account_id,kind,generation,revision FROM donna_plugin_resources WHERE id=?1 AND installation_id=?2",
        params![id.as_str(), installation.installation_id.as_str()],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?)),
    ).optional().map_err(db_error)?.ok_or(PluginError::NotFound)?;
    let owner = match raw.0.as_str() {
        "native" => PluginResourceOwner::Native {
            hub_id: identifier(raw.1.ok_or(PluginError::InvalidDescriptor)?)?,
            space_id: identifier(raw.2.ok_or(PluginError::InvalidDescriptor)?)?,
        },
        "external" => PluginResourceOwner::External {
            account_ref: account(c, installation, &identifier(raw.3.ok_or(PluginError::InvalidDescriptor)?)?)?,
        },
        _ => return Err(PluginError::InvalidDescriptor),
    };
    let kind = match raw.4.as_str() {
        "calendar" => PluginResourceKind::Calendar,
        "whatsapp_number" => PluginResourceKind::WhatsappNumber,
        _ => return Err(PluginError::InvalidDescriptor),
    };
    PluginResourceRef::new(installation.clone(), owner, id.clone(), kind, revision(raw.5)?, revision(raw.6)?)
}

/// Restrição síncrona no hook existente, usando só a Connection da exclusão.
/// Não inicia transação, não relê Authority, não publica nem chama Store/Cloud.
pub(super) fn revoke_bot(c: &Connection, id: &str) -> Result<()> {
    if c.is_autocommit() {
        return Err(PluginError::InvalidTransition);
    }
    c.execute(
        "UPDATE donna_plugin_grants SET state='revoked',generation=generation+1,revision=revision+1 WHERE bot_id=?1 AND state='active'", [id],
    ).map_err(db_error)?;
    Ok(())
}
