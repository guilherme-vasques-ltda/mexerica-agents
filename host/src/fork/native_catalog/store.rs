//! Vínculo e recibos próprios; callbacks usam somente a Connection recebida.
use super::model::{Add, Added, Intent, Link, LinkState};
use crate::store::BotConfig;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_native_slots(
            space_id TEXT NOT NULL, catalog_key TEXT NOT NULL,
            generation INTEGER NOT NULL CHECK(generation>0),
            revision INTEGER NOT NULL CHECK(revision>0),
            bot_id TEXT NOT NULL UNIQUE REFERENCES bots(id),
            profile_version INTEGER NOT NULL CHECK(profile_version>0),
            state TEXT NOT NULL CHECK(state IN ('active','removed')),
            base_generation INTEGER NOT NULL CHECK(base_generation>=0),
            base_revision INTEGER NOT NULL CHECK(base_revision>=0),
            operation TEXT NOT NULL CHECK(operation IN ('add','readd')),
            PRIMARY KEY(space_id,catalog_key));
        CREATE TABLE IF NOT EXISTS donna_native_receipts(
            space_id TEXT NOT NULL, catalog_key TEXT NOT NULL, client_request_key TEXT NOT NULL,
            digest TEXT NOT NULL, operation TEXT NOT NULL CHECK(operation IN ('add','readd')),
            generation INTEGER NOT NULL CHECK(generation>0),
            bot_id TEXT NOT NULL REFERENCES bots(id),
            PRIMARY KEY(space_id,catalog_key,client_request_key));
        CREATE TRIGGER IF NOT EXISTS donna_native_receipts_immutable_update
            BEFORE UPDATE ON donna_native_receipts BEGIN SELECT RAISE(ABORT,'native_receipt_immutable'); END;
        CREATE TRIGGER IF NOT EXISTS donna_native_receipts_immutable_delete
            BEFORE DELETE ON donna_native_receipts BEGIN SELECT RAISE(ABORT,'native_receipt_immutable'); END;",
    )?;
    Ok(())
}

struct Slot {
    generation: i64,
    revision: i64,
    bot_id: String,
    profile_version: u32,
    state: String,
    base_generation: i64,
    base_revision: i64,
    operation: String,
}
fn slot(c: &Connection, space: &str, key: &str) -> Result<Option<Slot>> {
    Ok(c.query_row(
        "SELECT generation,revision,bot_id,profile_version,state,base_generation,base_revision,operation
         FROM donna_native_slots WHERE space_id=?1 AND catalog_key=?2",
        [space, key],
        |r| {
            Ok(Slot {
                generation: r.get(0)?,
                revision: r.get(1)?,
                bot_id: r.get(2)?,
                profile_version: r.get(3)?,
                state: r.get(4)?,
                base_generation: r.get(5)?,
                base_revision: r.get(6)?,
                operation: r.get(7)?,
            })
        },
    )
    .optional()?)
}

pub(super) fn link(c: &Connection, space: &str, key: &str) -> Result<Link> {
    let Some(s) = slot(c, space, key)? else {
        return Ok(Link {
            catalog_key: key.into(),
            generation: 0,
            revision: 0,
            bot_id: None,
            profile_version: None,
            state: LinkState::Available,
        });
    };
    // A projeção não lê o perfil editado, nem procura homônimos ou outro Space.
    let state = audience(c, space, &s.bot_id)?;
    ensure!(matches!(state, LinkState::Removed) == (s.state == "removed"), "native_link_unavailable");
    Ok(Link {
        catalog_key: key.into(),
        generation: s.generation,
        revision: s.revision,
        bot_id: Some(s.bot_id),
        profile_version: Some(s.profile_version),
        state,
    })
}

fn audience(c: &Connection, space: &str, bot: &str) -> Result<LinkState> {
    let deleted: Option<bool> = c
        .query_row(
            "SELECT deleted FROM bots WHERE id=?1 AND COALESCE(json_extract(config,'$.spaceId'),?3)=?2",
            params![bot, space, crate::fork::spaces::PERSONAL],
            |r| r.get(0),
        )
        .optional()?;
    match deleted {
        Some(false) => Ok(LinkState::Active),
        Some(true) => Ok(LinkState::Removed),
        None => anyhow::bail!("native_link_unavailable"),
    }
}

fn answer(c: &Connection, hub: &str, request: &Add, generation: i64, bot: String) -> Result<Added> {
    Ok(Added {
        hub_id: hub.into(),
        space_id: request.space_id.clone(),
        catalog_key: request.catalog_key.clone(),
        client_request_key: request.client_request_key.clone(),
        generation,
        state: audience(c, &request.space_id, &bot)?,
        bot_id: bot,
    })
}
fn conflict() -> anyhow::Error {
    crate::remote::cloud::Conflict("native_catalog_conflict: releia a prévia antes de um novo gesto.").into()
}

/// Leitura idempotente: não depende da disponibilidade atual de perfil/backend.
/// Authority e transação pertencem ao chamador; audiência/digest são revalidados aqui.
pub(super) fn replay(c: &Connection, hub: &str, request: &Add, digest: &str) -> Result<Option<Added>> {
    crate::fork::spaces::require_space(c, &request.space_id)?;
    // Antes do slot e da versão corrente: replay nunca escolhe outro destino.
    let receipt: Option<(String, i64, String)> = c
        .query_row(
            "SELECT digest,generation,bot_id FROM donna_native_receipts
         WHERE space_id=?1 AND catalog_key=?2 AND client_request_key=?3",
            params![request.space_id, request.catalog_key, request.client_request_key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((saved, generation, bot)) = receipt {
        if saved != digest {
            return Err(conflict());
        }
        return answer(c, hub, request, generation, bot).map(Some);
    }
    Ok(None)
}

/// None solicita preparação externa pela fábrica comum. Some confirma efeito/recibo.
/// O chamador já abriu a transação sob a Authority ORIGINAL; nunca chamar Store aqui.
pub(super) fn apply(
    c: &Connection,
    hub: &str,
    request: &Add,
    digest: &str,
    prepared: Option<&BotConfig>,
) -> Result<Option<Added>> {
    if let Some(answer) = replay(c, hub, request, digest)? {
        return Ok(Some(answer));
    }
    ensure!(request.profile_version == super::catalog::VERSION, "native_profile_version_conflict: releia a prévia.");
    let current = slot(c, &request.space_id, &request.catalog_key)?;
    if let Some(s) = &current {
        if s.state == "active" {
            let same_active = request.intent == Intent::Add
                && request.expected_generation == s.generation
                && request.expected_revision == s.revision;
            let same_activation = request.intent.as_str() == s.operation
                && request.expected_generation == s.base_generation
                && request.expected_revision == s.base_revision;
            if !same_active && !same_activation {
                return Err(conflict());
            }
            // Nenhum save/reconfigure do bot dono: nem a versão atual do catálogo o sobrescreve.
            ensure!(matches!(audience(c, &request.space_id, &s.bot_id)?, LinkState::Active), "native_link_unavailable");
            return record(c, hub, request, digest, s.generation, &s.bot_id).map(Some);
        }
        if request.intent != Intent::Readd
            || request.expected_generation != s.generation
            || request.expected_revision != s.revision
        {
            return Err(conflict());
        }
        ensure!(matches!(audience(c, &request.space_id, &s.bot_id)?, LinkState::Removed), "native_link_unavailable");
    } else if request.intent != Intent::Add || request.expected_generation != 0 || request.expected_revision != 0 {
        return Err(conflict());
    }
    let Some(cfg) = prepared else { return Ok(None) };
    ensure!(cfg.space_id == request.space_id, "native_link_unavailable");
    let generation = current
        .as_ref()
        .map_or(Ok(1), |s| s.generation.checked_add(1).ok_or_else(|| anyhow::anyhow!("native_generation_exhausted")))?;
    // Produtor MAIN de cadastro/sessão principal. Bot, slot e recibo compartilham rollback.
    let revision = crate::store::bots::save_bot(c, cfg)?;
    if let Some(s) = current {
        let changed = c.execute(
            "UPDATE donna_native_slots SET generation=?3,revision=?4,bot_id=?5,profile_version=?6,state='active',
                base_generation=?7,base_revision=?8,operation=?9
             WHERE space_id=?1 AND catalog_key=?2 AND state='removed'
                AND generation=?7 AND revision=?8 AND bot_id=?10",
            params![
                request.space_id,
                request.catalog_key,
                generation,
                revision,
                cfg.id,
                request.profile_version,
                s.generation,
                s.revision,
                request.intent.as_str(),
                s.bot_id
            ],
        )?;
        ensure!(changed == 1, "native_catalog_conflict");
    } else {
        c.execute(
            "INSERT INTO donna_native_slots(space_id,catalog_key,generation,revision,bot_id,profile_version,state,
                base_generation,base_revision,operation) VALUES(?1,?2,?3,?4,?5,?6,'active',0,0,'add')",
            params![request.space_id, request.catalog_key, generation, revision, cfg.id, request.profile_version],
        )?;
    }
    record(c, hub, request, digest, generation, &cfg.id).map(Some)
}
fn record(c: &Connection, hub: &str, request: &Add, digest: &str, generation: i64, bot: &str) -> Result<Added> {
    c.execute(
        "INSERT INTO donna_native_receipts(space_id,catalog_key,client_request_key,digest,operation,generation,bot_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            request.space_id,
            request.catalog_key,
            request.client_request_key,
            digest,
            request.intent.as_str(),
            generation,
            bot
        ],
    )?;
    answer(c, hub, request, generation, bot.into())
}

/// Gancho da exclusão NORMAL, dentro da mesma transação. ID antigo nunca resolve o sucessor.
pub(crate) fn on_bot_deleted(c: &Connection, id: &str) -> Result<()> {
    let current: Option<(String, String, i64, i64)> = c.query_row(
        "SELECT space_id,catalog_key,generation,revision FROM donna_native_slots WHERE bot_id=?1 AND state='active'",
        [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).optional()?;
    if let Some((space, key, generation, revision)) = current {
        let changed = c.execute(
            "UPDATE donna_native_slots SET state='removed',revision=?6
             WHERE space_id=?1 AND catalog_key=?2 AND bot_id=?3 AND generation=?4 AND revision=?5 AND state='active'",
            params![space, key, id, generation, revision, crate::store::next_rev(c)?],
        )?;
        ensure!(changed == 1, "native_catalog_conflict");
    }
    Ok(())
}
