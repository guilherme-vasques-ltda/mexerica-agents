//! Descendência de Rotinas: procedência interna, orçamento D220 ancestral e SQL restrito.
//! Validação recua estritamente por nível (máximo cinco): até três SELECTs por
//! camada, mais fallback Team/cadastro/procedência e admissão/CAS. O seam reserva
//! 32 instruções por admissão/despacho descendente, sem ampliar o teto de 64.
use super::modelo::Recibo;
use serde::{Deserialize, Serialize};

/// Só o Hub constrói a procedência. A cópia privada acompanha edições e reinícios.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoutineOrigin {
    version: u8,
    creator: Recibo,
    space_id: String,
    chat_id: String,
    session_id: String,
    execution_digest: String,
}

use super::{
    admissao,
    modelo::{ContextoPersistido, Escopo, Origem, Pedido, Recusa, Tipo},
    store,
};
use crate::{
    fork::{
        bot_admission::{self, Origin, Phase},
        sessoes::{ExecutionRef, bindings},
    },
    store::BotConfig,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

impl RoutineOrigin {
    fn validate(&self, bot: &str) -> Result<()> {
        ensure!(
            self.version == 1
                && self.creator.bot_id == bot
                && !self.space_id.is_empty()
                && !self.chat_id.is_empty()
                && !self.session_id.is_empty()
                && self.execution_digest.len() == 64
                && self.execution_digest.bytes().all(|c| c.is_ascii_hexdigit())
                && (1..=5).contains(&self.creator.level)
                && (1..=50).contains(&self.creator.ordinal)
                && !self.creator.chain_id.is_empty()
                && !self.creator.admission_id.is_empty()
                && !self.creator.run_id.is_empty(),
            Recusa::ReciboInvalido
        );
        Ok(())
    }

    pub(crate) fn scope(&self, bot: &str, routine: &str) -> Escopo {
        Escopo::RotinaDescendente {
            bot_id: bot.into(),
            space_id: self.space_id.clone(),
            rotina_id: routine.into(),
            criacao: self.clone(),
        }
    }
}

fn bot_space_on(c: &Connection, bot: &str) -> Result<String> {
    let config: String = c
        .query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0))
        .optional()?
        .ok_or(Recusa::ReciboInvalido)?;
    let config: BotConfig = serde_json::from_str(&config).map_err(|_| Recusa::ReciboInvalido)?;
    ensure!(config.id == bot && !config.is_group(), Recusa::ReciboInvalido);
    Ok(config.space_id)
}

fn same_space(scope: &Escopo, bot: &str, space: &str) -> bool {
    match scope {
        Escopo::ConversaDoBot { bot_id, space_id }
        | Escopo::TurnoMcp { bot_id, space_id, .. }
        | Escopo::PedidoEntreBots { bot_id, space_id, .. } => bot_id == bot && space_id.as_deref() == Some(space),
        Escopo::RotinaDescendente { bot_id, space_id, .. } => bot_id == bot && space_id == space,
    }
}

/// Contexto já admitido do criador: Rotinas/raiz ou repasse Team exato. Não autoriza
/// um pedido recebido por mera posse do recibo; a criação também confere inflight/R1.
fn creator_context(c: &Connection, r: &Recibo) -> Result<Option<ContextoPersistido>> {
    if let Some(context) = store::contexto(c, r)? {
        return Ok(Some(context));
    }
    let scope: Option<String> = c.query_row(
        "SELECT a.scope FROM donna_chain_admissions a JOIN donna_chain_roots root ON root.id=a.chain_id AND root.version=1
         WHERE a.id=?1 AND a.chain_id=?2 AND a.run_id=?3 AND a.bot_id=?4 AND a.level=?5 AND a.ordinal=?6
           AND a.kind='delegation' AND a.routine_id IS NULL AND a.source=root.source
           AND json_extract(a.scope,'$.kind')='pedido_entre_bots' AND json_extract(a.scope,'$.bot_id')=a.bot_id",
        params![r.admission_id, r.chain_id, r.run_id, r.bot_id, r.level, r.ordinal], |row| row.get(0)).optional()?;
    Ok(scope
        .map(|scope| serde_json::from_str(&scope).map(|escopo| ContextoPersistido { escopo, rotina: None }))
        .transpose()
        .map_err(|_| Recusa::ReciboInvalido)?)
}

/// Chamada exclusivamente na Tx de criação. Nunca lê recibos dos argumentos MCP.
/// O inflight do produtor liga a origem ao token da `ExecutionRef`; mensagem humana
/// reconferida permite apenas a compatibilidade independente quando falta inflight.
pub(crate) fn capture_creation_on(
    c: &Connection,
    bot: &str,
    space: &str,
    execution: &ExecutionRef,
    human_message: Option<&str>,
) -> Result<Option<RoutineOrigin>> {
    let session = bindings::assert_execution_live(c, execution)?;
    let binding = bindings::resolve(c, execution.acp_binding().context("chamada vencida")?, bot)?;
    ensure!(binding.session_ref == session && bot_space_on(c, bot)? == space, Recusa::ReciboInvalido);
    if let Some(id) = human_message {
        let entry = crate::store::entry_on(c, id)?.context("pedido humano ausente")?;
        ensure!(
            entry.bot_id == bot && entry.session_id == session.id && entry.is_user_authored(),
            Recusa::ReciboInvalido
        );
    }
    let Some(dispatch) = bot_admission::current_on(c, bot)? else {
        ensure!(human_message.is_some(), "Não foi possível conferir a origem da criação desta rotina.");
        // Um slot de rotina nunca ganha independência por mensagem humana anexada.
        ensure!(!binding.slot.as_deref().is_some_and(|s| s.starts_with("routine-")), Recusa::ReciboInvalido);
        return Ok(None);
    };
    ensure!(
        dispatch.phase == Phase::Permitted
            && !dispatch.stop_requested
            && dispatch.execution_token == execution.turn_token()
            && dispatch.conversation_id == session.chat_id,
        "chamada vencida: a execução criadora não está em andamento"
    );
    let creator = match dispatch.origin {
        Origin::Routine { id } => {
            ensure!(
                human_message.is_none() && binding.slot.as_deref() == Some(crate::routines::session_slot(&id).as_str()),
                Recusa::ReciboInvalido
            );
            let acp = binding.acp_session.as_deref().context("sessão da rotina ausente")?;
            let slot_key = crate::store::run_session_key(bot, &crate::routines::session_slot(&id));
            let saved: String = c.query_row("SELECT v FROM kv WHERE k=?1", [&slot_key], |r| r.get(0))?;
            ensure!(saved == acp, Recusa::ReciboInvalido);
            // Mesmo ACP em outro slot/principal torna a atribuição ambígua (I-023).
            let prefix = crate::store::run_session_key(bot, "");
            let other: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM kv WHERE substr(k,1,length(?1))=?1 AND k<>?2 AND v=?3) OR EXISTS(SELECT 1 FROM bots WHERE id=?4 AND session_id=?3)",
                params![prefix, slot_key, acp, bot],
                |r| r.get(0),
            )?;
            ensure!(!other, Recusa::ReciboInvalido);
            let raw: String = c.query_row("SELECT value FROM kv,json_each(kv.v,'$.runs') WHERE kv.k='routines.v1' AND json_extract(value,'$.id')=?1 AND json_extract(value,'$.botId')=?2 AND json_extract(value,'$.status') IN ('running','recovering')", params![id,bot], |r|r.get(0))?;
            let run: Value = serde_json::from_str(&raw)?;
            let receipt: Recibo = serde_json::from_value(run["_chain"].clone())?;
            ensure!(
                receipt.run_id == id
                    && receipt.bot_id == bot
                    && store::phase(c, &receipt)? == super::modelo::Fase::EnvioPossivel,
                Recusa::ReciboInvalido
            );
            receipt
        }
        Origin::Ask | Origin::Message => {
            ensure!(
                human_message.is_none() && !binding.slot.as_deref().is_some_and(|s| s.starts_with("routine-")),
                Recusa::ReciboInvalido
            );
            let mode = if dispatch.origin == Origin::Ask { "wait" } else { "async" };
            let ids = serde_json::to_string(&dispatch.entry_ids)?;
            let mut stmt = c.prepare("SELECT r.id,r.chain_id,r.admission_id FROM donna_session_requests r WHERE r.schema_version=1 AND r.kind='inter_bot' AND r.state='running' AND r.mode=?1 AND r.target_bot_id=?2 AND r.target_chat_id=?3 AND r.target_session_id=?4 AND r.attempt=1 AND r.fence>=2 AND r.target_entry_id IN (SELECT value FROM json_each(?5))")?;
            let requests = stmt
                .query_map(params![mode, bot, session.chat_id, session.id, ids], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ensure!(requests.len() == 1, Recusa::ReciboInvalido);
            let (id, chain, admission) = &requests[0];
            let receipt = store::receipt(c, admission)?.context("admissão do pedido ausente")?;
            ensure!(
                receipt.run_id == *id && receipt.chain_id == *chain && receipt.bot_id == bot,
                Recusa::ReciboInvalido
            );
            let context = creator_context(c, &receipt)?.context("contexto do pedido ausente")?;
            ensure!(matches!(context.escopo, Escopo::PedidoEntreBots { .. }), Recusa::ReciboInvalido);
            receipt
        }
        Origin::Human => {
            ensure!(!binding.slot.as_deref().is_some_and(|s| s.starts_with("routine-")), Recusa::ReciboInvalido);
            return Ok(None);
        }
        _ => anyhow::bail!("Não foi possível conferir a origem da criação desta rotina."),
    };
    let context = creator_context(c, &creator)?.context("contexto criador ausente")?;
    ensure!(same_space(&context.escopo, bot, space), Recusa::ReciboInvalido);
    Ok(Some(RoutineOrigin {
        version: 1,
        creator,
        space_id: space.into(),
        chat_id: session.chat_id,
        session_id: session.id,
        execution_digest: admissao::digest(&("routine-creation", execution.acp_binding(), execution.turn_token()))?,
    }))
}

/// IDs/digest preparados fora da Tx. A ocorrência é local à cadeia ancestral.
pub(crate) fn prepare(
    origin: &RoutineOrigin,
    bot: &str,
    routine: &str,
    run: &str,
    mut occurrence: String,
    intent: String,
) -> Pedido {
    let scope = origin.scope(bot, routine);
    // A cadeia compartilha event_key: duas filhas podem ter a mesma ocorrência
    // local sem serem o mesmo gatilho. Prefixo com tamanho evita ambiguidade.
    occurrence.insert_str(0, &format!("routine-child:{}:{routine}:", routine.len()));
    let mut p = admissao::pedido(Tipo::Delegacao, occurrence, intent, bot, run, Some(origin.creator.clone()));
    p.rotina = Some(routine.into());
    p.escopo = scope;
    p
}

/// Valida só a nova aresta, sem alterar as igualdades do consumidor genérico.
pub(super) fn predecessor_valido(c: &Connection, r: &Recibo, p: &Pedido) -> Result<bool> {
    let Escopo::RotinaDescendente { bot_id, space_id, rotina_id, criacao } = &p.escopo else { return Ok(false) };
    if criacao.validate(bot_id).is_err()
        || p.tipo != Tipo::Delegacao
        || p.origem != Origem::Interna
        || r != &criacao.creator
        || r.bot_id != *bot_id
        || p.candidato.bot_id != *bot_id
        || p.candidato.run_id == r.run_id
        || p.rotina.as_deref() != Some(rotina_id)
        || space_id != &criacao.space_id
    {
        return Ok(false);
    }
    let Some(context) = creator_context(c, r)? else { return Ok(false) };
    if !same_space(&context.escopo, bot_id, space_id) || bot_space_on(c, bot_id)? != *space_id {
        return Ok(false);
    }
    let raw: Option<String> = c.query_row("SELECT value FROM kv,json_each(kv.v,'$.routines') WHERE kv.k='routines.v1' AND json_extract(value,'$.id')=?1 AND json_extract(value,'$.botId')=?2 AND COALESCE(json_extract(value,'$.deleted'),0)=0",params![rotina_id,bot_id],|r|r.get(0)).optional()?;
    let Some(raw) = raw else { return Ok(false) };
    let value: Value = serde_json::from_str(&raw).map_err(|_| Recusa::ReciboInvalido)?;
    let Ok(stored) = serde_json::from_value::<RoutineOrigin>(value["_creationOrigin"].clone()) else {
        return Ok(false);
    };
    Ok(stored == *criacao)
}

/// Só a âncora de delegação desta Run ou sua continuação exata. O pai pode já
/// ter terminado: a admissão confirmada conserva a procedência, não uma sessão viva.
pub(super) fn contexto_na(c: &Connection, r: &Recibo) -> Result<Option<ContextoPersistido>> {
    let raw: Option<(String,String,String)> = c.query_row("SELECT a.scope,a.routine_id,a.kind FROM donna_chain_admissions a JOIN donna_chain_roots root ON root.id=a.chain_id AND root.version=1 WHERE a.id=?1 AND a.chain_id=?2 AND a.run_id=?3 AND a.bot_id=?4 AND a.level=?5 AND a.ordinal=?6 AND a.source='internal' AND json_extract(a.scope,'$.kind')='rotina_descendente'",params![r.admission_id,r.chain_id,r.run_id,r.bot_id,r.level,r.ordinal],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    let Some((raw, routine, kind)) = raw else { return Ok(None) };
    let scope: Escopo = serde_json::from_str(&raw).map_err(|_| Recusa::ReciboInvalido)?;
    let Escopo::RotinaDescendente { bot_id, space_id, rotina_id, criacao } = &scope else { return Ok(None) };
    if criacao.validate(bot_id).is_err()
        || bot_id != &r.bot_id
        || rotina_id != &routine
        || criacao.creator.chain_id != r.chain_id
        || criacao.creator.level.checked_add(1) != Some(r.level)
        || r.level > 5
        || criacao.creator.ordinal >= r.ordinal
        || space_id != &criacao.space_id
    {
        return Ok(None);
    }
    let Some(parent) = creator_context(c, &criacao.creator)? else { return Ok(None) };
    if !same_space(&parent.escopo, bot_id, space_id) {
        return Ok(None);
    }
    // A âncora é a delegação única desta ocorrência; continuação não vira outra raiz.
    let valid: bool=c.query_row("SELECT EXISTS(SELECT 1 FROM donna_chain_admissions anchor JOIN donna_chain_admissions current ON current.id=?1 WHERE anchor.chain_id=?2 AND anchor.run_id=?3 AND anchor.bot_id=?4 AND anchor.routine_id=?5 AND anchor.scope=?6 AND anchor.kind='delegation' AND anchor.source='internal' AND anchor.predecessor=?7 AND anchor.level=?8 AND anchor.ordinal<=?9 AND ((current.id=anchor.id AND current.kind='delegation') OR (current.kind='continuation' AND EXISTS(SELECT 1 FROM donna_chain_admissions prev WHERE prev.id=current.predecessor AND prev.chain_id=current.chain_id AND prev.run_id=current.run_id AND prev.bot_id=current.bot_id AND prev.routine_id=current.routine_id AND prev.scope=current.scope AND prev.level=current.level AND prev.ordinal<current.ordinal AND prev.source='internal'))))",params![r.admission_id,r.chain_id,r.run_id,r.bot_id,routine,raw,criacao.creator.admission_id,r.level,r.ordinal],|r|r.get(0))?;
    if !valid || !matches!(kind.as_str(), "delegation" | "continuation") {
        return Ok(None);
    }
    Ok(Some(ContextoPersistido { escopo: scope, rotina: Some(routine) }))
}

/// Reencontra o ID canônico antes do replay estrito de `store::admit`.
pub(crate) fn admit_on(c: &Connection, p: &mut Pedido) -> Result<Result<Recibo, Recusa>> {
    // Nem uma chamada malformada deste adaptador pode virar raiz ou produzir
    // erro sem a identidade de recusa D220 esperada pelo consumidor.
    if p.tipo != Tipo::Delegacao || !matches!(p.escopo, Escopo::RotinaDescendente { .. }) {
        return Ok(Err(Recusa::ReciboInvalido));
    }
    let Some(parent) = p.anterior.as_ref() else {
        return Ok(Err(Recusa::ReciboInvalido));
    };
    let existing: Option<String> = c
        .query_row(
            "SELECT run_id FROM donna_chain_admissions WHERE chain_id=?1 AND event_key=?2",
            params![parent.chain_id, p.evento],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        p.candidato.run_id = id;
    }
    store::admit(c, p)
}

/// Construtor apenas de fixtures: produção só usa `capture_creation_on`.
#[cfg(test)]
pub(crate) fn fixture_origin(creator: Recibo, space: &str) -> RoutineOrigin {
    RoutineOrigin {
        version: 1,
        chat_id: creator.bot_id.clone(),
        session_id: creator.bot_id.clone(),
        creator,
        space_id: space.into(),
        execution_digest: "a".repeat(64),
    }
}

#[cfg(test)]
#[path = "descendencia_rotina_testes.rs"]
mod tests;
