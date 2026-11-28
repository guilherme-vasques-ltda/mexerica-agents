use crate::api::devices::Forbidden;
use crate::store::now_ms;
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

pub(super) fn migrate_na(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_session_privacy(
           request_id TEXT PRIMARY KEY NOT NULL REFERENCES donna_session_requests(id),
           privacy_class TEXT NOT NULL CHECK(privacy_class IN ('public_in_space','restricted_private','unknown_taint')),
           body_disposition TEXT NOT NULL CHECK(body_disposition='body_retained'),
           owner_principal_id TEXT NOT NULL CHECK(length(trim(owner_principal_id))>0),
           owner_epoch INTEGER NOT NULL CHECK(typeof(owner_epoch)='integer' AND owner_epoch>0),
           policy_revision INTEGER NOT NULL CHECK(typeof(policy_revision)='integer' AND policy_revision>0),
           hub_generation INTEGER NOT NULL CHECK(typeof(hub_generation)='integer' AND hub_generation>0),
           delivery_state TEXT NOT NULL CHECK(delivery_state IN ('retained','revoked_purged')),
           revoked_at INTEGER NOT NULL CHECK(typeof(revoked_at)='integer' AND revoked_at>=0),
           audience_json TEXT NOT NULL CHECK(CASE WHEN json_valid(audience_json) THEN COALESCE(
             json_type(audience_json)='object'
             AND json_type(audience_json,'$.space_id')='text'
             AND length(trim(json_extract(audience_json,'$.space_id')))>0
             AND json_type(audience_json,'$.owner_epoch')='integer'
             AND json_extract(audience_json,'$.owner_epoch')=owner_epoch
             AND CASE json_extract(audience_json,'$.kind')
               WHEN 'owner_only' THEN json_type(audience_json,'$.owner_principal_id')='text'
                 AND json_extract(audience_json,'$.owner_principal_id')=owner_principal_id
               WHEN 'explicit_principals' THEN json_type(audience_json,'$.allowed_principal_ids')='array'
                 AND json_type(audience_json,'$.allowed_bot_ids')='array'
                 AND json_type(audience_json,'$.policy_revision')='integer'
                 AND json_extract(audience_json,'$.policy_revision')=policy_revision
               ELSE 0 END,0) ELSE 0 END),
           CHECK((delivery_state='revoked_purged') = (revoked_at>0)));
         DROP TRIGGER IF EXISTS donna_session_privacy_no_downgrade;
         CREATE TRIGGER IF NOT EXISTS donna_session_privacy_no_downgrade BEFORE UPDATE ON donna_session_privacy
         WHEN (OLD.privacy_class IN ('restricted_private','unknown_taint') AND NEW.privacy_class='public_in_space')
           OR (OLD.privacy_class='unknown_taint' AND NEW.privacy_class<>'unknown_taint')
           OR NEW.body_disposition IS NOT 'body_retained'
         BEGIN SELECT RAISE(ABORT, 'privacy_class_downgrade_forbidden'); END;
         CREATE TRIGGER IF NOT EXISTS donna_session_privacy_grant_immutable BEFORE UPDATE ON donna_session_privacy
         WHEN NEW.request_id IS NOT OLD.request_id OR NEW.audience_json IS NOT OLD.audience_json
           OR NEW.owner_principal_id IS NOT OLD.owner_principal_id OR NEW.owner_epoch IS NOT OLD.owner_epoch
           OR NEW.policy_revision IS NOT OLD.policy_revision OR NEW.hub_generation IS NOT OLD.hub_generation
         BEGIN SELECT RAISE(ABORT, 'privacy_authority_change_forbidden'); END;
         CREATE TRIGGER IF NOT EXISTS donna_session_privacy_no_unrevoke BEFORE UPDATE ON donna_session_privacy
         WHEN OLD.delivery_state='revoked_purged' AND NEW.revoked_at IS NOT OLD.revoked_at
         BEGIN SELECT RAISE(ABORT, 'privacy_revocation_final'); END;
         CREATE TRIGGER IF NOT EXISTS donna_session_privacy_no_delete BEFORE DELETE ON donna_session_privacy
         BEGIN SELECT RAISE(ABORT, 'privacy_row_delete_forbidden'); END;",
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivacyClass {
    PublicInSpace,
    RestrictedPrivate,
    UnknownTaint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyDisposition {
    BodyRetained,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AudienceGrant {
    OwnerOnly {
        space_id: String,
        owner_principal_id: String,
        owner_epoch: i64,
    },
    ExplicitPrincipals {
        space_id: String,
        allowed_principal_ids: Vec<String>,
        allowed_bot_ids: Vec<String>,
        owner_epoch: i64,
        policy_revision: i64,
    },
}

impl AudienceGrant {
    fn space(&self) -> &str {
        match self {
            Self::OwnerOnly { space_id, .. } | Self::ExplicitPrincipals { space_id, .. } => space_id,
        }
    }

    fn matches_authority(&self, owner: &str, epoch: i64, revision: i64) -> bool {
        !self.space().trim().is_empty()
            && match self {
                Self::OwnerOnly { owner_principal_id, owner_epoch, .. } => {
                    owner_principal_id == owner && *owner_epoch == epoch
                }
                Self::ExplicitPrincipals {
                    allowed_principal_ids,
                    allowed_bot_ids,
                    owner_epoch,
                    policy_revision,
                    ..
                } => {
                    *owner_epoch == epoch
                        && *policy_revision == revision
                        && allowed_principal_ids.iter().chain(allowed_bot_ids).all(|id| !id.trim().is_empty())
                }
            }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BodyReaderAllowlist {
    #[allow(dead_code, reason = "a rota de leitura direta ainda não está integrada")]
    OwnerDirectView,
    MissionBoardProjection,
    RequesterBotStatus,
    #[allow(dead_code, reason = "superfície expressamente recusada; sem integração")]
    BroadcastEventBus,
    #[allow(dead_code, reason = "superfície expressamente recusada; sem integração")]
    RoutineOrWebhookConsumer,
    #[allow(dead_code, reason = "superfície expressamente recusada; sem integração")]
    PeerBotDelivery,
}

pub(crate) struct ReadAuthority<'a> {
    pub space_id: &'a str,
    pub principal_id: Option<&'a str>,
    pub bot_id: Option<&'a str>,
    pub owner_epoch: i64,
    pub policy_revision: i64,
    pub control: ControlAuthority,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlAuthority {
    #[allow(dead_code, reason = "a rota de leitura direta ainda não está integrada")]
    Validated,
    #[allow(dead_code, reason = "a rota de leitura autenticada ainda não está integrada")]
    Unavailable,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadAccess {
    MetadataOnly,
    OwnerBody,
}

pub(super) struct Qualification {
    audience: AudienceGrant,
    owner: String,
    epoch: i64,
    revision: i64,
    revoked: bool,
    requester_bot: String,
}

#[derive(Clone, Copy)]
pub(super) enum MetadataProjection {
    MissionBoard,
    RequesterStatus,
}

#[cfg(test)]
pub(crate) fn resultado_so_existe_em_memoria(c: &Connection, request_id: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT content='unreferenced' AND final_entry_id IS NULL FROM donna_session_results WHERE request_id=?1",
        [request_id],
        |row| row.get(0),
    )
}

impl Qualification {
    pub(super) fn read_authorized(
        &self,
        reader: BodyReaderAllowlist,
        caller: Option<&ReadAuthority<'_>>,
    ) -> Result<ReadAccess, Forbidden> {
        use BodyReaderAllowlist::{
            BroadcastEventBus, OwnerDirectView, PeerBotDelivery, RequesterBotStatus, RoutineOrWebhookConsumer,
        };
        if self.revoked || matches!(reader, BroadcastEventBus | RoutineOrWebhookConsumer | PeerBotDelivery) {
            return Err(Forbidden);
        }
        let caller = caller.ok_or(Forbidden)?;
        if caller.control != ControlAuthority::Validated
            || caller.space_id != self.audience.space()
            || caller.owner_epoch != self.epoch
            || caller.policy_revision != self.revision
        {
            return Err(Forbidden);
        }
        if matches!(reader, OwnerDirectView) {
            return if caller.principal_id == Some(self.owner.as_str()) && caller.bot_id.is_none() {
                Ok(ReadAccess::OwnerBody)
            } else {
                Err(Forbidden)
            };
        }
        if matches!(reader, RequesterBotStatus) && caller.bot_id != Some(self.requester_bot.as_str()) {
            return Err(Forbidden);
        }
        let allowed = match (&self.audience, caller.principal_id) {
            (_, None) => false,
            (AudienceGrant::OwnerOnly { owner_principal_id, .. }, Some(id)) => {
                caller.bot_id.is_none() && id == owner_principal_id
            }
            (AudienceGrant::ExplicitPrincipals { allowed_principal_ids, allowed_bot_ids, .. }, Some(id)) => {
                allowed_principal_ids.iter().any(|allowed| allowed == id)
                    && caller.bot_id.is_none_or(|bot| allowed_bot_ids.iter().any(|allowed| allowed == bot))
            }
        };
        if allowed { Ok(ReadAccess::MetadataOnly) } else { Err(Forbidden) }
    }
}

pub(super) fn load(c: &Connection, request_id: &str) -> Result<Option<Qualification>> {
    let row = c
        .query_row(
            "SELECT p.audience_json,p.owner_principal_id,p.owner_epoch,p.policy_revision,
           p.delivery_state,r.requester_bot_id
         FROM donna_session_privacy p JOIN donna_session_requests r ON r.id=p.request_id WHERE p.request_id=?1",
            [request_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, String>(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    row.map(|(audience, owner, epoch, revision, delivery, requester_bot)| {
        let audience: AudienceGrant =
            serde_json::from_str(&audience).map_err(|_| anyhow!("retorno_privacidade_invalida"))?;
        ensure!(audience.matches_authority(&owner, epoch, revision), "retorno_privacidade_invalida");
        Ok(Qualification { audience, owner, epoch, revision, revoked: delivery != "retained", requester_bot })
    })
    .transpose()
}

pub(super) fn metadata_visible_na(
    c: &Connection,
    id: &str,
    projection: MetadataProjection,
    caller: Option<&ReadAuthority<'_>>,
) -> Result<bool> {
    Ok(match load(c, id)? {
        Some(qualification) => {
            let reader = match projection {
                MetadataProjection::MissionBoard => BodyReaderAllowlist::MissionBoardProjection,
                MetadataProjection::RequesterStatus => BodyReaderAllowlist::RequesterBotStatus,
            };
            matches!(qualification.read_authorized(reader, caller), Ok(ReadAccess::MetadataOnly))
        }
        None => matches!(projection, MetadataProjection::RequesterStatus),
    })
}

#[allow(dead_code, reason = "a captura de leituras e os executores de efeitos ainda não estão integrados")]
mod execution {
    use super::{AudienceGrant, BodyDisposition, PrivacyClass, Result, Transaction, ensure, params};

    #[derive(Clone, Copy)]
    pub(crate) enum ReadProvenance {
        AttestedInSpace,
        PrivateRead,
        UnattestedRead,
    }

    pub(crate) struct ExecutionTaint {
        class: PrivacyClass,
        disposition: BodyDisposition,
        owner: String,
        audience: AudienceGrant,
    }

    impl ExecutionTaint {
        pub(crate) fn new(owner: String, audience: AudienceGrant) -> Self {
            Self { class: PrivacyClass::PublicInSpace, disposition: BodyDisposition::BodyRetained, owner, audience }
        }

        pub(crate) fn observe(&mut self, read: ReadProvenance) {
            self.class = match (self.class, read) {
                (_, ReadProvenance::UnattestedRead) | (PrivacyClass::UnknownTaint, _) => PrivacyClass::UnknownTaint,
                (_, ReadProvenance::PrivateRead) => PrivacyClass::RestrictedPrivate,
                (class, ReadProvenance::AttestedInSpace) => class,
            };
        }

        pub(crate) fn class(&self) -> PrivacyClass {
            self.class
        }

        pub(crate) fn disposition(&self) -> BodyDisposition {
            self.disposition
        }

        pub(crate) fn audience(&self) -> &AudienceGrant {
            &self.audience
        }

        pub(crate) fn qualify_na(
            &self,
            tx: &Transaction<'_>,
            request_id: &str,
            authority: EffectAuthority,
        ) -> Result<()> {
            ensure!(
                self.audience.matches_authority(&self.owner, authority.owner_epoch, authority.policy_revision),
                "retorno_privacidade_invalida"
            );
            let class = match self.class {
                PrivacyClass::PublicInSpace => "public_in_space",
                PrivacyClass::RestrictedPrivate => "restricted_private",
                PrivacyClass::UnknownTaint => "unknown_taint",
            };
            let changed = tx.execute(
                "INSERT INTO donna_session_privacy(request_id,privacy_class,body_disposition,audience_json,
                   owner_principal_id,owner_epoch,policy_revision,hub_generation,delivery_state,revoked_at)
                 SELECT id,?2,'body_retained',?3,?4,?5,?6,?7,'retained',0 FROM donna_session_requests
                 WHERE id=?1 AND schema_version=1 AND state IN ('accepted','running')
                 ON CONFLICT(request_id) DO UPDATE SET privacy_class=CASE
                   WHEN donna_session_privacy.privacy_class='unknown_taint' THEN 'unknown_taint'
                   WHEN excluded.privacy_class='public_in_space' THEN donna_session_privacy.privacy_class
                   ELSE excluded.privacy_class END
                 WHERE donna_session_privacy.delivery_state='retained'
                   AND donna_session_privacy.audience_json=excluded.audience_json
                   AND donna_session_privacy.owner_principal_id=excluded.owner_principal_id
                   AND donna_session_privacy.owner_epoch=excluded.owner_epoch
                   AND donna_session_privacy.policy_revision=excluded.policy_revision
                   AND donna_session_privacy.hub_generation=excluded.hub_generation",
                params![
                    request_id,
                    class,
                    serde_json::to_string(&self.audience)?,
                    self.owner,
                    authority.owner_epoch,
                    authority.policy_revision,
                    authority.hub_generation
                ],
            )?;
            ensure!(changed == 1, "retorno_privacidade_vencida");
            Ok(())
        }
    }

    pub(crate) enum SinkGuard<'a> {
        Qualified { class: PrivacyClass, owner_principal_id: &'a str, audience: &'a AudienceGrant },
        Unqualified,
    }

    impl SinkGuard<'_> {
        pub(crate) fn authorize(&self, taint: &ExecutionTaint) -> Result<ExecutionTaint> {
            let Self::Qualified { class, owner_principal_id, audience } = self else {
                anyhow::bail!("taint_sink_denied");
            };
            ensure!(
                *owner_principal_id == taint.owner
                    && *audience == &taint.audience
                    && (*class != PrivacyClass::PublicInSpace || taint.class == PrivacyClass::PublicInSpace),
                "taint_sink_denied"
            );
            Ok(ExecutionTaint {
                class: match (taint.class, class) {
                    (PrivacyClass::UnknownTaint, _) | (_, PrivacyClass::UnknownTaint) => PrivacyClass::UnknownTaint,
                    (PrivacyClass::RestrictedPrivate, _) | (_, PrivacyClass::RestrictedPrivate) => {
                        PrivacyClass::RestrictedPrivate
                    }
                    _ => PrivacyClass::PublicInSpace,
                },
                owner: taint.owner.clone(),
                disposition: taint.disposition(),
                audience: taint.audience.clone(),
            })
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) struct EffectAuthority {
        pub hub_generation: i64,
        pub owner_epoch: i64,
        pub policy_revision: i64,
    }

    pub(crate) struct EffectFenceCheck<'a> {
        pub request_id: &'a str,
        pub fence: i64,
        pub authority: EffectAuthority,
    }

    impl EffectFenceCheck<'_> {
        pub(crate) fn revalidate_na(&self, tx: &Transaction<'_>, current: Option<EffectAuthority>) -> Result<()> {
            ensure!(current == Some(self.authority), "retorno_efeito_vencido");
            let valid: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM donna_session_requests r JOIN donna_session_privacy p ON p.request_id=r.id
                 WHERE r.id=?1 AND r.schema_version=1 AND r.state='running' AND r.fence=?2 AND p.hub_generation=?3
                   AND p.owner_epoch=?4 AND p.policy_revision=?5 AND p.delivery_state='retained')",
                params![
                    self.request_id,
                    self.fence,
                    self.authority.hub_generation,
                    self.authority.owner_epoch,
                    self.authority.policy_revision
                ],
                |r| r.get(0),
            )?;
            ensure!(valid, "retorno_efeito_vencido");
            Ok(())
        }
    }
}

#[allow(unused_imports, reason = "contrato dos executores pendente de integração")]
pub(crate) use execution::{EffectAuthority, EffectFenceCheck, ExecutionTaint, ReadProvenance, SinkGuard};

#[allow(dead_code, reason = "os produtores de revogação de autoridade ainda não estão integrados")]
#[derive(Clone, Copy)]
pub(crate) enum RevokeScope<'a> {
    Request(&'a str),
    Bot(&'a str),
    Space(&'a str),
    OwnerEpoch { owner_principal_id: &'a str, owner_epoch: i64 },
}

#[allow(dead_code, reason = "os produtores de revogação de autoridade ainda não estão integrados")]
/// Executa na transação IMMEDIATE de quem altera a autoridade, sem commit próprio.
pub(crate) fn revoke_matching_na(tx: &Transaction<'_>, scope: RevokeScope<'_>) -> Result<usize> {
    let (kind, id, epoch) = match scope {
        RevokeScope::Request(id) => ("request", id, 0),
        RevokeScope::Bot(id) => ("bot", id, 0),
        RevokeScope::Space(id) => ("space", id, 0),
        RevokeScope::OwnerEpoch { owner_principal_id, owner_epoch } => ("owner_epoch", owner_principal_id, owner_epoch),
    };
    let now = now_ms();
    let revoked = {
        let mut st = tx.prepare(
            "UPDATE donna_session_privacy AS p SET delivery_state='revoked_purged',revoked_at=?4
             WHERE p.delivery_state='retained' AND CASE ?1
               WHEN 'request' THEN p.request_id=?2
               WHEN 'bot' THEN EXISTS(SELECT 1 FROM donna_session_requests r WHERE r.id=p.request_id
                 AND (r.requester_bot_id=?2 OR r.target_bot_id=?2))
               WHEN 'space' THEN json_extract(p.audience_json,'$.space_id')=?2
               WHEN 'owner_epoch' THEN p.owner_principal_id=?2 AND p.owner_epoch=?3
               ELSE 0 END RETURNING request_id",
        )?;
        st.query_map(params![kind, id, epoch, now], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in &revoked {
        super::encerrar_na(tx, id, super::Decisao::Cancelado)?;
    }
    let legacy = tx.execute(
        "UPDATE donna_session_requests AS r SET state='cancelled',end_reason='cancelled',fence=fence+1,updated_at=?3
         WHERE r.schema_version=1 AND r.state IN ('accepted','running')
           AND CASE ?1 WHEN 'request' THEN r.id=?2 WHEN 'bot' THEN r.requester_bot_id=?2 OR r.target_bot_id=?2 ELSE 0 END
           AND NOT EXISTS(SELECT 1 FROM donna_session_privacy p WHERE p.request_id=r.id)",
        params![kind, id, now],
    )?;
    Ok(revoked.len() + legacy)
}
