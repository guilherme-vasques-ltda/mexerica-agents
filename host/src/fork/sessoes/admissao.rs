//! Consumidor de Sessões no encaixe `ClaimHooks` da VG-03 (`fork::bot_admission::permit_with`). Roda na MESMA
//! `Connection` da transação IMMEDIATE do permit, antes do claim `queued` → `sent`: síncrono, falível, sem Store, Hub,
//! rede, await ou transação aninhada. Falha desfaz classificação, recibo, escolha, claim e permissão juntos.
//!
//! - **Dispatch** (primeiro despacho humano): a classificação é a canônica do Roteamento (`admission_class`), com a
//!   conta e a revisão capturadas no setup e a seleção que de fato executa. `Routed` ⇒ captura com as revisões
//!   canônicas (`revisions`, lidas aqui, logo antes do gravador) e o carimbo bruto da fonte certa e `record_dispatch`
//!   (pré-imagem). `NotRouted` ⇒ nenhum recibo; só na principal.
//! - **Resume** (retomada D72): `classify_resume` com a classe gravada na espera; `Routed` confere entradas, seleção e
//!   escolha originais; `Legacy` segue o caminho atual da VG, sem Roteamento; todo erro falha fechado.
//!
//! D372: cada conversa do Pessoal (principal e extras) tem seleção própria, EFETIVA (a que os turnos executam) e,
//! se o dono trocou, uma PRÓXIMA (`engine_choice`). A próxima só vira efetiva no início de um turno novo
//! ([`start_choice`]); durante o turno, a espera e a retomada a efetiva não muda. Por isso as guardas exigem a
//! execução igual à efetiva (Dispatch) e a efetiva igual à admitida (Resume). Principal de canal/Space segue a fonte
//! canônica deles. A extra é sempre roteada, com carimbo da própria sessão (`SessionAccountStampV1`) e recibo. Fatos
//! sem produtor uniforme ficam `RequiredFactNotAttested`.
//!
//! Instante do carimbo: `ensure_session` (setup, antes do permit) grava o carimbo da execução aberta; o encaixe lê o
//! valor bruto na Tx do permit, antes de qualquer escrita dela. Edição do bot depois do setup não muda o turno
//! admitido (o snapshot do setup é o que executa e o que se grava); conta revogada falha fechado.
use super::engine_choice::{self, SessionEngine};
use super::{bindings::BindingRef, store};
use crate::fork::roteamento::admission::{
    AdmissionCapture, CapturedBinding, CapturedText, CheckpointKey, DecidedBy, Destination, EntryEvidence,
    EntryProvenance, ExecutionSeal, Intent, Reason, RequiredFact, RoutingSnapshot, SelectionSnapshot, StampCapture,
    TurnProvenance, UnattestedFacts,
};
use crate::fork::roteamento::receipt::{self, ResumeRoute, RoutingClass};
use crate::fork::spaces::EngineSelection;
use crate::store::{BotConfig, Lane};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};

/// O turno humano original que o ator leva ao permit; nada aqui é recapturado do Hub.
pub(crate) struct Turn<'a> {
    /// Configuração que de fato executa (com a escolha congelada da extra já aplicada).
    pub cfg: &'a BotConfig,
    pub lane: &'a Lane,
    pub entries: &'a [String],
    /// Token original do Hub (`Ticket.execution_token`), o mesmo do claim e da ativação.
    pub token: &'a str,
    /// Revisão da conta gerenciada com que o setup abriu a execução (`ensure_session`); nunca a do cadastro atual.
    pub account_revision: Option<u64>,
}

/// A seleção que executa este turno, sem completar nada: o modelo pode ser o padrão do motor (`None`).
fn running(cfg: &BotConfig) -> EngineSelection {
    EngineSelection { backend: cfg.backend.clone(), model: cfg.model.clone(), model_options: cfg.model_options.clone() }
}

fn same(a: &SelectionSnapshot, b: &SelectionSnapshot) -> bool {
    a.account_id == b.account_id
        && a.selection.backend == b.selection.backend
        && a.selection.model == b.selection.model
        && a.selection.model_options == b.selection.model_options
}

/// A combinação da configuração que executa, comparável ao que foi admitido; comando próprio nunca é comparável.
fn executing(cfg: &BotConfig) -> Option<SelectionSnapshot> {
    if cfg.command.is_some() {
        return None;
    }
    Some(SelectionSnapshot { account_id: cfg.fork_editor.engine_account_id.clone()?, selection: running(cfg) })
}

/// Qual seleção ler: a efetiva (execução em curso, retomada, reconfiguração), a configurada (prever o próximo turno)
/// ou a de um turno novo (promove a próxima antes de ler).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Momento {
    Efetiva,
    Configurada,
    TurnoNovo,
}

/// A seleção própria da conversa do turno. Extra: obrigatória (NULL ou ilegível falham fechado, nunca caem no padrão
/// do bot nem na principal). Principal: a do próprio chat do bot, se houver; canal, Space ou principal ainda sem
/// linha → `None` (fonte canônica). Ilegível falha fechado. Devolve também se promoveu a próxima.
fn lane_engine(
    c: &Connection,
    lane: &Lane,
    cfg: &BotConfig,
    momento: Momento,
) -> Result<Option<(SessionEngine, bool)>> {
    let session = match lane.session.as_deref() {
        Some(session) => session,
        None if cfg.context.is_none() && lane.chat == cfg.id && store::get(c, &lane.chat)?.is_some() => &lane.chat,
        None => return Ok(None),
    };
    let promoted = momento == Momento::TurnoNovo && engine_choice::promote(c, session)?;
    let engine = match momento {
        Momento::Configurada => engine_choice::configured(c, session)?,
        Momento::Efetiva | Momento::TurnoNovo => engine_choice::read(c, session)?,
    };
    match engine {
        Some(engine) => Ok(Some((engine, promoted))),
        None if lane.session.is_some() => Err(anyhow!("engine_unavailable")),
        None => Ok(None),
    }
}

fn apply(c: &Connection, lane: &Lane, cfg: &mut BotConfig, momento: Momento) -> Result<Option<bool>> {
    let Some((engine, promoted)) = lane_engine(c, lane, cfg, momento)? else { return Ok(None) };
    engine.apply(cfg);
    Ok(Some(promoted))
}

/// A conversa cuja seleção própria o turno desta lane executa (a mesma regra de `lane_engine`); `None` fora do recorte
/// (canal, Space, principal ainda sem linha).
pub(crate) fn engine_session(c: &Connection, lane: &Lane, cfg: &BotConfig) -> Result<Option<String>> {
    Ok(match lane.session.as_deref() {
        Some(session) => Some(session.to_owned()),
        None if cfg.context.is_none() && lane.chat == cfg.id && store::get(c, &lane.chat)?.is_some() => {
            Some(lane.chat.clone())
        }
        None => None,
    })
}

/// Aplica a seleção EFETIVA da conversa (D372) à configuração de execução, sem promover nada: execução em curso,
/// retomada e reconfiguração. Devolve se havia seleção própria.
pub(crate) fn apply_choice(c: &Connection, lane: &Lane, cfg: &mut BotConfig) -> Result<bool> {
    Ok(apply(c, lane, cfg, Momento::Efetiva)?.is_some())
}

/// Aplica a seleção CONFIGURADA (próxima ?? efetiva), só para prever o próximo turno (pré-checagem do envio).
pub(crate) fn apply_configured(c: &Connection, lane: &Lane, cfg: &mut BotConfig) -> Result<bool> {
    Ok(apply(c, lane, cfg, Momento::Configurada)?.is_some())
}

/// Início de um turno NOVO (`start_prepared_turn` sem espera): promove a próxima e aplica a efetiva. Devolve se
/// promoveu (o dono trocou a seleção desde o turno anterior).
pub(crate) fn start_choice(c: &Connection, lane: &Lane, cfg: &mut BotConfig) -> Result<bool> {
    Ok(apply(c, lane, cfg, Momento::TurnoNovo)?.unwrap_or(false))
}

/// D372 (decisão A de Modelos): a principal do Pessoal com seleção EFETIVA própria tem o carimbo no domínio da sessão
/// (`account.stamp.<bot>/session:<bot>`) e grava `SessionAccountStampV1`; sem seleção (fora do recorte), segue o do
/// bot. Ilegível falha fechado.
pub(crate) fn principal_owns_stamp(c: &Connection, cfg: &BotConfig) -> Result<bool> {
    if cfg.context.is_some() || store::get(c, &cfg.id)?.is_none() {
        return Ok(false);
    }
    Ok(engine_choice::read(c, &cfg.id)?.is_some())
}

/// Reconfiguração no meio de um turno ou fora dele (edição do bot): a conversa com seleção própria conserva a do
/// turno capturado (`running`), nunca a do padrão novo; a troca da conversa vale no próximo `start_turn`.
pub(crate) fn keep_turn_engine(running: &BotConfig, cfg: &mut BotConfig) {
    cfg.backend.clone_from(&running.backend);
    cfg.model.clone_from(&running.model);
    cfg.model_options.clone_from(&running.model_options);
    cfg.fast_mode = running.fast_mode;
    cfg.fork_editor.engine_account_id.clone_from(&running.fork_editor.engine_account_id);
}

/// Retomada D72 depois de reiniciar: a configuração volta à seleção ADMITIDA no despacho original (recibo do
/// Roteamento), não à seleção atual da conversa nem ao padrão do bot. Velocidade (`fastMode`) não faz parte da
/// seleção admitida e segue a gravada na conversa. Sem recibo (principal legada `NotRouted`), só retoma se a
/// configuração atual ainda for a do turno (`backend`/`model` da espera); senão falha fechado (pausa visível). As
/// guardas do permit (`resume`) continuam valendo depois.
pub(crate) fn apply_resume(
    c: &Connection,
    lane: &Lane,
    cfg: &mut BotConfig,
    token: Option<&str>,
    routed: bool,
    turn: (&str, Option<&str>),
) -> Result<()> {
    if cfg.context.is_some() {
        // Conversa de Space: a retomada já restaurou a seleção capturada pela fonte própria (`context_binding`).
        return Ok(());
    }
    apply_choice(c, lane, cfg)?;
    if !routed {
        ensure!(cfg.backend == turn.0 && cfg.model.as_deref() == turn.1, "engine_choice_execution_mismatch");
        return Ok(());
    }
    let token = token.filter(|t| !t.is_empty()).ok_or_else(|| anyhow!("admission_legacy_execution_missing"))?;
    let original = receipt::read_checkpoint(c, &key_for(c, &cfg.id, lane, token)?)?
        .ok_or_else(|| anyhow!("routing_resume_checkpoint_missing"))?;
    let admitted = &original.captured().routing.admitted;
    ensure!(cfg.command.is_none(), "engine_choice_custom_command");
    cfg.backend.clone_from(&admitted.selection.backend);
    cfg.model.clone_from(&admitted.selection.model);
    cfg.model_options.clone_from(&admitted.selection.model_options);
    cfg.fork_editor.engine_account_id = Some(admitted.account_id.clone());
    Ok(())
}

fn evidence(c: &Connection, id: &str) -> Result<EntryEvidence> {
    let (source, run): (Option<String>, Option<String>) = c.query_row(
        "SELECT json_extract(data,'$.sourceBotId'), json_extract(data,'$.runId') FROM entries WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(match (source, run) {
        (Some(source_bot_id), _) => EntryEvidence::HubEntrySourceBotV1 { source_bot_id },
        (None, Some(run_id)) => EntryEvidence::HubEntryRunV1 { run_id },
        (None, None) => EntryEvidence::RequiredFactNotAttested,
    })
}

fn key(c: &Connection, turn: &Turn<'_>) -> Result<CheckpointKey> {
    key_for(c, &turn.cfg.id, turn.lane, turn.token)
}

fn key_for(c: &Connection, bot: &str, lane: &Lane, token: &str) -> Result<CheckpointKey> {
    let space: String = c
        .query_row("SELECT space_id FROM donna_space_conversations WHERE id=?1", [&lane.chat], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("session_destination_missing"))?;
    Ok(CheckpointKey {
        destination: Destination {
            bot_id: bot.to_owned(),
            space_id: space,
            session: store::SessionRef { id: lane.session_id().to_owned(), chat_id: lane.chat.clone() },
        },
        execution_token: token.to_owned(),
    })
}

/// Bytes brutos do carimbo na fonte certa, lidos antes de qualquer escrita desta transação: conversa de Space, sessão
/// extra ou principal com seleção própria (carimbo da sessão, `account.stamp.<bot>/session:<id>`) ou principal sem
/// seleção (carimbo do bot). O do bot nunca representa uma sessão com seleção própria.
fn stamp(c: &Connection, turn: &Turn<'_>) -> Result<StampCapture> {
    let read = |key: String| -> Result<CapturedText> {
        let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [&key], |r| r.get(0)).optional()?;
        Ok(CapturedText::from_captured(raw))
    };
    Ok(if turn.cfg.context.is_some() {
        StampCapture::SpaceEngineStampV1 { capture: read(format!("donna.space.engine.{}", turn.lane.chat))? }
    } else if let Some(session) = &turn.lane.session {
        StampCapture::SessionAccountStampV1 {
            capture: read(format!("account.stamp.{}/session:{session}", turn.cfg.id))?,
        }
    } else if principal_owns_stamp(c, turn.cfg)? {
        StampCapture::SessionAccountStampV1 {
            capture: read(format!("account.stamp.{}/session:{}", turn.cfg.id, turn.lane.chat))?,
        }
    } else {
        StampCapture::BotAccountStampV1 { capture: read(format!("account.stamp.{}", turn.cfg.id))? }
    })
}

/// F6-B, regra (k): sob uma troca de conta, os vínculos da reserva são conferidos NESTA transação (despacho e retomada),
/// no mesmo instante do permit: revogar entre a prova do ator e o permit recusa aqui, sem recibo nem envio.
pub(crate) fn conferir_troca_de_conta(c: &Connection, session: &str, cfg: &BotConfig) -> Result<()> {
    if let Some(over) = crate::fork::fallback_contas::estado::sobreposicao(c, session)?
        && cfg.fork_editor.engine_account_id.as_deref() == Some(over.atual.as_str())
    {
        crate::fork::fallback_contas::prova::conferir_vinculos_on(c, cfg, &over.atual)
            .map_err(|falha| anyhow!("fallback_no_proof: {}.", falha.motivo()))?;
    }
    Ok(())
}

/// Primeiro despacho humano. Devolve a classe canônica que o permit grava no registro de admissão.
pub(crate) fn dispatch(c: &Connection, turn: &Turn<'_>, binding: &BindingRef) -> Result<RoutingClass> {
    ensure!(
        binding.session_ref.chat_id == turn.lane.chat && binding.session_ref.id == turn.lane.session_id(),
        "admission_binding_changed"
    );
    let session = store::get(c, turn.lane.session_id())?
        .filter(|s| s.chat_id == turn.lane.chat)
        .ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(session.state == "open", "session_closed");
    ensure!(session.principal == turn.lane.session.is_none(), "unknown_session");
    // Seleção da conversa ilegível falha fechado; a extra sem seleção nunca executa o padrão do bot. A efetiva não
    // muda durante o turno (a troca do dono fica na próxima): o turno executa exatamente ela.
    let engine = engine_choice::read(c, &session.id)?;
    ensure!(session.principal || engine.is_some(), "engine_unavailable");
    if let Some(engine) = &engine {
        ensure!(engine.runs_in(turn.cfg), "engine_choice_execution_mismatch");
    }
    conferir_troca_de_conta(c, &session.id, turn.cfg)?;
    let selection = running(turn.cfg);
    let account = match turn.cfg.fork_editor.engine_account_id.as_deref() {
        None => None,
        // Conta gerenciada sem a revisão capturada no setup não é provada: falha fechado, nunca vira legado.
        Some(id) => Some((id, turn.account_revision.ok_or_else(|| anyhow!("routing_selection_unavailable"))?)),
    };
    let class = receipt::admission_class(c, turn.cfg, true, session.principal, account, &selection)?;
    if !matches!(class, RoutingClass::Routed {}) {
        // Extras são sempre roteadas; só a principal pode seguir sem recibo, pela classe canônica.
        ensure!(session.principal, "routing_selection_unavailable");
        return Ok(class);
    }
    let admitted = executing(turn.cfg).ok_or_else(|| anyhow!("routing_selection_unavailable"))?;
    let (account_id, account_revision) = account.ok_or_else(|| anyhow!("routing_selection_unavailable"))?;
    ensure!(admitted.account_id == account_id, "routing_selection_unavailable");
    let key = key(c, turn)?;
    // Pré-imagem bruta do carimbo: lida antes de qualquer escrita desta transação.
    let private_stamp = stamp(c, turn)?;
    let hub_turn: i64 = c.query_row(
        "SELECT turn FROM entries WHERE id=?1",
        [turn.entries.last().ok_or_else(|| anyhow!("admission_input_missing"))?],
        |r| r.get(0),
    )?;
    let provenance = turn
        .entries
        .iter()
        .map(|id| Ok(EntryProvenance { entry_id: id.clone(), evidence: evidence(c, id)? }))
        .collect::<Result<Vec<_>>>()?;
    // Revisões do instante da admissão (Fixo, sem decisão Auto), na mesma Connection, logo antes do gravador.
    let revisions = receipt::revisions(c, &key.destination.space_id, &turn.cfg.id)?;
    let capture = AdmissionCapture {
        schema_version: 1,
        key,
        intent: Intent { entry_ids: turn.entries.to_vec(), hub_turn, expected_session_revision: session.revision },
        binding: CapturedBinding { binding_ref: binding.id.as_str().to_owned() },
        account_revision,
        private_stamp,
        execution_seal: ExecutionSeal::NotCaptured,
        routing: RoutingSnapshot {
            requested: admitted.clone(),
            admitted: admitted.clone(),
            decided_by: DecidedBy::Fixo,
            reason: if engine.is_some() { Reason::FrozenSession } else { Reason::Fixed },
            policy_revision: revisions.policy,
            general_guide_revision: revisions.general_guide,
            // O guia do bot vive no mesmo documento da política (rota-03a).
            bot_guide_revision: revisions.policy,
            session_origin: session.origin.clone(),
            turn_provenance: TurnProvenance {
                entries: provenance,
                authority_origin: RequiredFact::RequiredFactNotAttested,
            },
            required_facts: UnattestedFacts {
                coverage_before: RequiredFact::RequiredFactNotAttested,
                coverage_after: RequiredFact::RequiredFactNotAttested,
                account_identity_generation_catalog: RequiredFact::RequiredFactNotAttested,
            },
        },
    };
    let recorded = receipt::record_dispatch(c, &capture)?;
    // Recibo idêntico já existente não autoriza outra seleção.
    ensure!(same(&recorded.captured().routing.admitted, &admitted), "routing_checkpoint_conflict");
    Ok(class)
}

/// Retomada D72 pela regra canônica (`classify_resume`) com a classe gravada na espera, sem recibo inicial, sorteio,
/// nova escolha ou outro token; as guardas da VG (token, política, binding) continuam no permit. `Legacy` segue o
/// caminho atual da VG, sem Roteamento, e só na principal. Todo erro falha fechado, nunca vira legado.
pub(crate) fn resume(c: &Connection, turn: &Turn<'_>, checkpoint: &str) -> Result<RoutingClass> {
    let class = crate::fork::bot_admission::checkpoint_routing(checkpoint)?;
    let session = store::get(c, turn.lane.session_id())?
        .filter(|s| s.chat_id == turn.lane.chat)
        .ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(session.state == "open", "session_closed");
    let engine = engine_choice::read(c, &session.id)?;
    ensure!(session.principal || engine.is_some(), "engine_unavailable");
    // A conversa na reserva que entra em espera D72 e retoma também é conferida de novo dentro do permit.
    conferir_troca_de_conta(c, &session.id, turn.cfg)?;
    match receipt::classify_resume(c, &key(c, turn)?, Some(&class))? {
        ResumeRoute::Legacy => {
            ensure!(session.principal, "routing_class_conflict");
        }
        ResumeRoute::Routed(original) => {
            let captured = original.captured();
            ensure!(captured.intent.entry_ids == turn.entries, "routing_checkpoint_conflict");
            // A seleção que executa precisa ser a admitida no despacho original; nada é recalculado. A efetiva da
            // conversa também (uma troca durante a espera fica na próxima e só vale no próximo turno).
            ensure!(
                executing(turn.cfg).is_some_and(|r| same(&r, &captured.routing.admitted)),
                "engine_choice_execution_mismatch"
            );
            if let Some(engine) = &engine {
                let effective = SelectionSnapshot {
                    account_id: engine.account_id.clone().ok_or_else(|| anyhow!("engine_choice_execution_mismatch"))?,
                    selection: engine.selection(),
                };
                ensure!(same(&effective, &captured.routing.admitted), "engine_choice_execution_mismatch");
            }
        }
    }
    Ok(class)
}
