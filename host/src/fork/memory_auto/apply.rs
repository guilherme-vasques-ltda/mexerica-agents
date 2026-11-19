//! Memória + origem + recibo + conclusão no mesmo commit. Nenhum efeito precede sucesso.
use super::{
    queue::{Context, Reservation},
    worker::{ExtractionPlan, MemorySection, Operation},
};
use crate::{LockExt, hub::Hub, store::BotConfig};
use anyhow::{Context as _, Result, ensure};
use codync_mem::{
    files as mf,
    nature::{self, KindNature},
    origin::Origin,
    store::{self as mem, Writer},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::borrow::Cow;

/// Material privado do tombstone; a remoção pertence ao arquivo exato, nunca à chave vazia global.
fn note_fact<'a>(file: &str, content: Option<&'a str>) -> Cow<'a, str> {
    content.map_or_else(
        || Cow::Owned(format!("\0memory-auto/note-removal/v1/{}", super::queue::hash(file))),
        Cow::Borrowed,
    )
}

fn own_layer(c: &Context, shared: bool, cfg: &BotConfig) -> Result<Option<String>> {
    if !shared {
        return Ok(Some(c.memory_id.clone()));
    }
    ensure!(
        c.space_id == crate::fork::spaces::PERSONAL && crate::chat::memory::shared(cfg),
        "shared_memory_is_personal"
    );
    Ok(None)
}

#[cfg(test)]
pub(crate) fn stage(hub: &Hub, cfg: &BotConfig, turn: &crate::hub::Runtime, op: &Operation) -> Result<Value> {
    stage_in(hub, cfg, turn, op, None)
}

pub(crate) fn stage_in(
    hub: &Hub,
    cfg: &BotConfig,
    turn: &crate::hub::Runtime,
    op: &Operation,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
) -> Result<Value> {
    ensure!(!matches!(op, Operation::NotDurable { .. }), "memory_not_durable_requires_extractor");
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "memory_proposal_commit");
    let _boundary = hub.store.memory_boundary.locked();
    crate::fork::sessoes::bindings::recheck_optional(hub, turn.execution.as_ref())?;
    if let Some(context) = context {
        context.check(hub)?;
    }
    let current = hub.runtime(&cfg.id);
    ensure!(
        turn.memory_turn_token.is_some()
            && current.memory_turn_token == turn.memory_turn_token
            && current.message == turn.message
            && current.session == turn.session
            && current.lane == turn.lane
            && current.started_at == turn.started_at
            && !current.memory_exposure_error,
        "memory_turn_stale"
    );
    let id = super::queue::active_job(hub, &cfg.id)?
        .ok_or_else(|| anyhow::anyhow!("Memory proposals require an active turn the user started in your own chat."))?;
    let mut connection = hub.store.db.locked();
    let db = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    crate::fork::sessoes::bindings::check_optional(&db, turn.execution.as_ref())?;
    let raw: String =
        db.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1 AND outcome='running'", [&id], |r| {
            r.get(0)
        })?;
    let context: Context = serde_json::from_str(&raw)?;
    ensure!(context.backend != "codex", "codex_memory_mutation_refused");
    super::worker::validate_context(&db, &context)?;
    // Guardas de enquadramento antes do scanner: uma recusa do scanner nunca estaciona o que a
    // escrita normal já recusaria por evidência, escopo (camada, autor do dono) ou limite, e as
    // recusas do job contam no limite — um bot em laço não enche a conversa de avisos.
    ensure!(
        !op.evidence().is_empty()
            && op.evidence().iter().all(|id| context.messages.iter().any(|human| human.id == *id)),
        "memory_evidence_required"
    );
    match op {
        Operation::NotDurable { .. } => anyhow::bail!("memory_not_durable_requires_extractor"),
        Operation::Confirm { .. } | Operation::Reclassify { .. } => anyhow::bail!("memory_confirm_is_hub_only"),
        Operation::Revise { entry, expected_rev, .. } | Operation::Archive { entry, expected_rev, .. } => {
            let old = visible(&db, &context, cfg, *entry, *expected_rev)?;
            ensure!(old.author != mem::Author::User, "The user wrote this memory, so only the user can change it.");
        }
        Operation::Remember { shared, .. } | Operation::Note { shared, .. } => {
            own_layer(&context, *shared, cfg)?;
        }
    }
    if context.space_id != crate::fork::spaces::PERSONAL {
        crate::chat::memory::injecao_gate::secrets(op)?;
    }
    let raw = serde_json::to_string(op)?;
    let bytes: i64 = db.query_row(
        "SELECT COALESCE(SUM(length(CAST(operation AS BLOB))),0) FROM donna_memory_auto_proposals WHERE job=?1",
        [&id],
        |r| r.get(0),
    )?;
    ensure!(usize::try_from(bytes)?.saturating_add(raw.len()) <= 128_000, "memory_proposal_bytes_limit");
    let staged: i64 =
        db.query_row("SELECT COUNT(*) FROM donna_memory_auto_proposals WHERE job=?1", [&id], |r| r.get(0))?;
    ensure!(staged + super::recibo_injecao::recusas_do_job(&db, &id)? < 32, "memory_proposal_limit");
    // Segredo é recusa SEM pendência nem prévia: sai antes do scanner para o texto nunca chegar a
    // `donna_memory_injecao_pendentes`. O scanner de injeção (memória pessoal) confere o conteúdo
    // INTEGRAL na mesma transação que gravaria a proposta: veredito suspeito é recusa fail-closed,
    // com recibo sem texto nem hash e aviso ao dono.
    let refused = if context.space_id == crate::fork::spaces::PERSONAL
        && let Some(rule) =
            crate::chat::memory::injecao_gate::secrets(op).err().or_else(|| crate::chat::memory::injecao_gate::scan(op))
    {
        let turn_of = |message: Option<&String>| -> Result<i64> {
            Ok(message
                .map(|id| db.query_row("SELECT turn FROM entries WHERE id=?1", [id], |r| r.get(0)).optional())
                .transpose()?
                .flatten()
                .unwrap_or(1))
        };
        let (event, notice) = super::recibo_injecao::refuse(
            &db,
            super::recibo_injecao::Gate::Ferramenta,
            &super::recibo_injecao::Alvo { job: Some(&id), context: &context, cfg, op },
            rule,
            turn_of(turn.message.as_ref())?,
            crate::store::now_ms(),
        )?;
        Some((rule, event, notice))
    } else {
        None
    };
    if refused.is_none() {
        if let Operation::Note { content: Some(content), .. } = op {
            mf::check(content)?;
        } else if !op.text().is_empty() {
            codync_mem::check::text(op.text())?;
        }
        // D246: o vínculo da MESMA execução já conferida nesta Tx (`check_optional` acima), nunca dado da ferramenta.
        // Gravado uma vez: a mesma operação repetida no job conserva o vínculo original (`INSERT OR IGNORE`).
        let binding = turn
            .execution
            .as_ref()
            .and_then(crate::fork::sessoes::ExecutionRef::acp_binding)
            .ok_or_else(|| anyhow::anyhow!("chamada vencida: execução ausente"))?;
        db.execute(
            "INSERT OR IGNORE INTO donna_memory_auto_proposals(job,operation,binding) VALUES(?1,?2,?3)",
            params![id, raw, binding],
        )?;
    }
    db.commit()?;
    drop(connection);
    if let Some((rule, event, notice)) = refused {
        if let Some(entry) = hub.store.entry(&notice) {
            let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
        let _ = hub.events.send(event);
        return Err(rule.into());
    }
    Ok(json!({"status":"prepared","jobId":id}))
}

/// Fachada local (testes): seção própria, commit e publicação depois de soltá-la.
#[cfg(test)]
pub(crate) fn complete(
    hub: &Hub,
    r: &Reservation,
    plan: ExtractionPlan,
    authorize: impl Fn(&Connection, &Context) -> Result<()>,
) -> Result<Vec<String>> {
    commit(hub, r, plan, &authorize, true)
}

/// Guarda de seleção/audiência do coordenador é revalidada no commit, sem I/O nem nova autoridade. Sob o MB já
/// retido pela composição (D384); devolve só dados, e o coordenador publica depois de sair de todas as guardas.
pub(crate) fn complete_guarded(
    section: &MemorySection<'_>,
    r: &Reservation,
    plan: ExtractionPlan,
    authorize: impl Fn(&Connection, &Context) -> Result<()>,
) -> Result<Committed> {
    commit_guarded(section, r, plan, &authorize, true)
}

/// D389: aprovação do dono no app. Aplica a sugestão guardada como o modo automático faria (recibo e Desfazer),
/// pelo caminho local (seção própria, commit e publicação fora dela), sem reserva, extrator nem proposta D246:
/// a guarda confere o contexto atual e a sugestão pendente.
pub(crate) fn approve(
    hub: &Hub,
    r: &Reservation,
    op: Operation,
    suggestion: &str,
    autoridade: &crate::fork::autoridade::Autoridade,
) -> Result<Vec<String>> {
    let pending = |db: &Connection, c: &Context| super::suggestions::check_pending(db, suggestion, c);
    let plan = ExtractionPlan { operations: vec![op] };
    // SEG-491-01: MB → Owner (Cloud) → DB. A autoridade ORIGINAL do pedido é conferida antes e depois do efeito,
    // na mesma TxImmediate do commit (`with_db`); publica só depois de sair de todas as guardas.
    let committed = {
        let _section = MemorySection::enter(hub);
        autoridade.with_db(hub, None, |db| commit_in(db, r, plan, &pending, true, Some(suggestion)))?
    };
    Ok(committed.publish(hub))
}

/// D384: o que a transação confirmou, só dados: sem guarda, Connection, Tx ou referência à seção. Nasce depois de
/// `tx.commit()`; `publish` roda fora de MB/Cloud (e, no coordenador, também fora de Q/M/T).
#[must_use]
pub(crate) struct Committed {
    ids: Vec<String>,
    projections: Vec<(Option<String>, String)>,
    shared_changed: bool,
    audit_changed: bool,
    reinforcements: usize,
    parked: bool,
    /// Recusas do scanner: eventos a publicar e avisos ao dono inseridos na conversa.
    refusal_events: Vec<Value>,
    refusal_notices: Vec<String>,
    context: Context,
}
impl Committed {
    /// A cauda pós-commit, inalterada: projeção de notas (falha vira aviso e reconciliação), eventos das entradas e
    /// invalidação de memória/auditoria. Não reautoriza, não reaplica e não converte o commit em falha.
    pub(crate) fn publish(mut self, hub: &Hub) -> Vec<String> {
        self.project(hub);
        self.emit_entries(hub);
        self.finish(hub)
    }

    fn project(&mut self, hub: &Hub) {
        for (layer, name) in self.projections.drain(..) {
            // O diário é autoritativo; scan/restart também recompõem a projeção em caso de crash aqui.
            if crate::chat::memory::files::project_committed(hub, layer.as_deref(), &name).is_err() {
                tracing::warn!("projeção de nota de memória aguardando reconciliação");
            }
        }
    }

    fn emit_entries(&self, hub: &Hub) {
        for id in self.ids.iter().chain(&self.refusal_notices) {
            if let Some(entry) = hub.store.entry(id) {
                let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
            }
        }
        for event in &self.refusal_events {
            let _ = hub.events.send(event.clone());
        }
    }

    fn finish(self, hub: &Hub) -> Vec<String> {
        if !self.ids.is_empty() || self.reinforcements > 0 {
            crate::chat::memory::changed(hub, if self.shared_changed { None } else { Some(&self.context.memory_id) });
        } else if self.parked {
            // D389: só a tela relê (memoryRev); as sessões do bot não ouvem que aquilo virou memória.
            crate::chat::memory::screen_changed(hub, &self.context.bot_id);
        } else if self.audit_changed {
            super::audit::notify(hub, &self.context);
        }
        self.ids
    }
}

/// Recibo, cartão "Memória salva · Desfazer" e `memory.rev`, na Tx da escrita. Devolve o id e se mexeu em
/// "Sobre você". Também usado pelo sono (2b1c), com o trabalho de extração de onde veio a evidência.
fn receipt(
    tx: &Connection,
    (job, context): (&str, &Context),
    operation: &Operation,
    saved: &Saved,
    rev: i64,
    now: i64,
) -> Result<(String, bool)> {
    let evidence = operation.evidence();
    let key = super::queue::hash(&serde_json::to_string(operation)?);
    let id = format!("memory-auto-{job}-{key}");
    let current = if let Some(entry) = saved.entry {
        serde_json::to_value(mem::get(tx, entry)?.ok_or(mem::Error::NotFound)?)?
    } else {
        serde_json::to_value(
            mf::version(tx, saved.file_version.ok_or(mem::Error::NotFound)?)?.ok_or(mem::Error::NotFound)?,
        )?
    };
    let previous_origin = saved.previous.as_ref().map(|p| p["origin"].clone());
    tx.execute(
        "INSERT INTO donna_memory_auto_receipts(id,job,operation,entry,file_version,rev,previous,origin)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            id,
            job,
            serde_json::to_string(operation)?,
            saved.entry,
            saved.file_version,
            rev,
            saved.previous.as_ref().map(Value::to_string),
            serde_json::to_string(&saved.origin)?
        ],
    )?;
    tx.execute(
        "UPDATE donna_memory_auto_receipts SET nature=?2,nature_bot=?3,conflict_text=?4 WHERE id=?1",
        params![id, current["nature"].as_str(), current["natureBot"].as_str(), current["conflictText"].as_str()],
    )?;
    let mut memory = serde_json::to_value(operation)?;
    // Metadados do recibo substituem campos do plano apenas depois da gravação.
    memory["entry"] = json!(saved.entry);
    memory["shared"] = json!(current["bot"].is_null());
    memory["rev"] = json!(rev);
    memory["origin"] = json!(saved.origin);
    memory["nature"] = current["nature"].clone();
    memory["natureBot"] = current["natureBot"].clone();
    memory["conflictText"] = current["conflictText"].clone();
    if saved.entry.is_some() {
        memory["text"] = current["text"].clone();
    }
    memory["at"] = json!(now);
    for field in ["nature", "natureBot", "conflictText"] {
        let key = format!("previous{}{}", field[..1].to_uppercase(), &field[1..]);
        memory[key] = saved.previous.as_ref().map_or(Value::Null, |p| p[field].clone());
    }
    memory["previousAt"] =
        saved.previous.as_ref().map_or(Value::Null, |p| p.get("updatedAt").unwrap_or(&p["at"]).clone());
    memory["previousRev"] = saved.previous.as_ref().map_or(Value::Null, |p| p["rev"].clone());
    memory["previousOrigin"] = json!(previous_origin);
    memory["previous"] = saved.previous.as_ref().map_or(Value::Null, |p| p["text"].clone());
    memory["message"] = json!(evidence.first());
    memory["jobId"] = json!(job);
    memory["messageIds"] = json!(context.messages.iter().map(|m| &m.id).collect::<Vec<_>>());
    memory["responseIds"] = json!(context.responses.iter().map(|m| &m.id).collect::<Vec<_>>());
    memory["status"] = json!("done");
    memory["confirmation"] = json!(false);
    memory["version"] = json!(saved.file_version);
    let label = if current["nature"] == "conflict" { "Conflito na memória" } else { "Memória salva" };
    let turn = context.closing_turn.ok_or(super::worker::InvalidSnapshot::Evidence)?;
    memory["closingTurn"] = json!(turn);
    let data = json!({"text":label, "style":"info","author":context.bot_id,"memory":memory});
    let data = crate::fork::spaces::stamp_entry(tx, &context.chat_id, &data)?;
    tx.execute(
        "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at) VALUES(?1,?2,?3,'notice',?4,?5,?6,?6)",
        params![id, context.chat_id, rev, turn, data.to_string(), now],
    )?;
    tx.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [rev])?;
    Ok((id, current["bot"].is_null()))
}

/// Sono 2b1c, modo `auto`: aplica o atalho literal na Tx da rodada (já sob MB → Owner/Cloud → DB), com recibo e
/// cartão presos ao trabalho de extração de onde veio a evidência, para o Desfazer de sempre (`undo`). Devolve o id
/// do cartão e se mexeu em "Sobre você", ou `None` se o fato foi esquecido/bloqueado. Não publica: o chamador publica
/// depois das guardas.
pub(crate) fn confirm_in(
    tx: &Connection,
    (job, context): (&str, &Context),
    op: &Operation,
) -> Result<Option<(String, bool)>> {
    ensure!(matches!(op, Operation::Confirm { .. }), "memory_confirm_is_hub_only");
    let raw: String = tx.query_row("SELECT config FROM bots WHERE id=?1", [&context.bot_id], |row| row.get(0))?;
    let cfg: BotConfig = serde_json::from_str(&raw)?;
    let now = crate::store::now_ms();
    let origin = super::origin::derive(tx, context, op.evidence(), "")?;
    let rev = crate::store::next_rev(tx)?;
    match apply_one(tx, context, &cfg, op, &origin, now, rev) {
        Ok(Some(saved)) => Ok(Some(receipt(tx, (job, context), op, &saved, rev, now)?)),
        Ok(None) => Ok(None),
        Err(error) => match refusal(error) {
            Refused::Forgotten | Refused::Duplicate => Ok(None),
            Refused::Other(error) => Err(error),
        },
    }
}

impl Committed {
    /// O que a rodada do sono confirmou: os cartões do atalho literal, se algum foi em "Sobre você" (R1-04: avisa
    /// todos os bots que veem a camada, como a extração) e se sugestões novas foram guardadas.
    pub(crate) fn sleep(ids: Vec<String>, shared_changed: bool, parked: bool, context: Context) -> Self {
        Self {
            ids,
            projections: Vec::new(),
            shared_changed,
            audit_changed: false,
            reinforcements: 0,
            parked,
            refusal_events: Vec::new(),
            refusal_notices: Vec::new(),
            context,
        }
    }
}

#[derive(Debug)]
pub(crate) enum PreparedOutcome {
    Nothing,
    Applied(Vec<String>),
    Rejected,
}

/// Não recebe plano do chamador: apenas propostas autenticadas que o Hub guardou durante o turno original.
pub(crate) fn apply_prepared(hub: &Hub, r: &Reservation) -> Result<PreparedOutcome> {
    let count: i64 = hub.store.db.locked().query_row(
        "SELECT COUNT(*) FROM donna_memory_auto_proposals WHERE job=?1",
        [&r.id],
        |row| row.get(0),
    )?;
    if count == 0 {
        return Ok(PreparedOutcome::Nothing);
    }
    match commit(hub, r, ExtractionPlan::default(), &validate_prepared, false) {
        Ok(ids) => Ok(PreparedOutcome::Applied(ids)),
        Err(error) if super::worker::is_transient(&error) => Err(error),
        Err(_) => Ok(PreparedOutcome::Rejected),
    }
}

fn validate_prepared(db: &Connection, c: &Context) -> Result<()> {
    super::worker::validate_context(db, c)?;
    ensure!(!c.backend.is_empty() && c.backend != "codex", "memory_prepared_engine_refused");
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs WHERE bot=?1 AND chat=?2
        AND outcome='success' AND scope='allowed' AND json_extract(context,'$.token')=?3
        AND json_extract(context,'$.backend')=?4)",
        params![c.bot_id, c.chat_id, c.token, c.backend],
        |r| r.get(0),
    )?;
    ensure!(valid, "memory_prepared_proof_required");
    Ok(())
}

/// Recuperação/fechamento síncrono de propostas; sem envio e sem substituir a seleção do extrator.
pub(crate) fn finish_prepared(hub: &Hub, job: &str) -> Result<()> {
    let Some(r) = super::queue::reserve_job(&hub.store, job)? else { return Ok(()) };
    let outcome = apply_prepared(hub, &r)?;
    let disposition = match outcome {
        PreparedOutcome::Nothing => return super::worker::await_extractor(hub, &r),
        PreparedOutcome::Applied(ids) => {
            let _ = ids;
            return super::worker::await_extractor(hub, &r);
        }
        PreparedOutcome::Rejected => super::worker::Unavailable::Rejected,
    };
    super::worker::release(hub, &r, disposition)
}

/// Caminho local (`apply_prepared` e a fachada de testes): seção própria, commit, seção solta e só então publica.
fn commit(
    hub: &Hub,
    r: &Reservation,
    plan: ExtractionPlan,
    authorize: &dyn Fn(&Connection, &Context) -> Result<()>,
    finish: bool,
) -> Result<Vec<String>> {
    let committed = {
        let section = MemorySection::enter(hub);
        commit_guarded(&section, r, plan, authorize, finish)?
    };
    Ok(committed.publish(hub))
}

/// Toda a transação sob o MB recebido, sem adquiri-lo de novo e sem publicar nada.
fn commit_guarded(
    section: &MemorySection<'_>,
    r: &Reservation,
    plan: ExtractionPlan,
    authorize: &dyn Fn(&Connection, &Context) -> Result<()>,
    finish: bool,
) -> Result<Committed> {
    let mut db = section.hub().store.db.locked();
    let tx = db.transaction()?;
    let committed = commit_in(&tx, r, plan, authorize, finish, None)?;
    tx.commit()?;
    drop(db);
    Ok(committed)
}

/// O corpo da transação, só sobre a Connection recebida: sem lock, commit, Store ou Cloud. `approval` (D389) é a
/// sugestão que o dono aprova, sob a guarda da autoridade original que abriu esta Tx.
fn commit_in(
    tx: &Connection,
    r: &Reservation,
    mut plan: ExtractionPlan,
    authorize: &dyn Fn(&Connection, &Context) -> Result<()>,
    finish: bool,
    approval: Option<&str>,
) -> Result<Committed> {
    ensure!(plan.operations.len() <= 32 && serde_json::to_vec(&plan)?.len() <= 128_000, "memory_operation_limit");
    // A aprovação não tem reserva: a guarda dela confere o contexto atual e a sugestão pendente.
    if approval.is_none() {
        super::worker::validate(tx, r)?;
    }
    authorize(tx, &r.context)?;
    let learning = crate::fork::memory_learning::mode_in(tx)?;
    let raw: String = tx.query_row("SELECT config FROM bots WHERE id=?1", [&r.context.bot_id], |row| row.get(0))?;
    let cfg: BotConfig = serde_json::from_str(&raw)?;
    // A aprovação aplica só a sugestão do dono: nenhuma proposta D246 do job entra por este caminho.
    let prepared = if approval.is_some() {
        Vec::new()
    } else {
        tx.prepare("SELECT operation,binding FROM donna_memory_auto_proposals WHERE job=?1 ORDER BY rowid")?
            .query_map([&r.id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let bindings: Vec<Option<String>> = prepared.iter().map(|(_, binding)| binding.clone()).collect();
    // D246: o lote inteiro antes do primeiro efeito; uma proposta sem prova recusa todas, sem aplicação parcial.
    prepared_bindings_live(tx, &r.context, &bindings)?;
    let plan_len = plan.operations.len();
    for (raw, _) in prepared {
        plan.operations.push(serde_json::from_str(&raw)?);
    }
    ensure!(plan.operations.len() <= 64, "memory_operation_limit");
    let now = crate::store::now_ms();
    let mut ids = Vec::new();
    let mut projections = Vec::new();
    let mut shared_changed = false;
    let mut audit_changed = false;
    let mut reinforcements = 0;
    let mut parked = false;
    // SEG-507-02: operação ignorada, recusada ou sem efeito (confirmar vindo do extrator, duplicata, fato esquecido,
    // arquivar o já arquivado) tira a prova
    // `learningOn` do `noChange` e marca o job (`noChange` ou `applied`, lote misto: SEG-507-02 caso 2) como
    // `unproven`: as escritas válidas e os recibos ficam, mas o sono não toma o job como prova de leitura.
    let mut unproven = false;
    let mut refusal_events = Vec::new();
    let mut refusal_notices = Vec::new();
    for (i, operation) in plan.operations.iter().enumerate() {
        // Sono 2b1c: confirmar é do Hub (o sono, ou o dono aprovando a sugestão dele). Vinda do extrator, a operação
        // é ignorada, sem falhar o lote nem gravar nada (o MCP já a recusa no preparo).
        if approval.is_none() && matches!(operation, Operation::Confirm { .. } | Operation::Reclassify { .. }) {
            unproven = true;
            continue;
        }
        let evidence = operation.evidence();
        ensure!(
            !evidence.is_empty() && evidence.iter().all(|id| r.context.messages.iter().any(|human| human.id == *id)),
            "memory_evidence_required"
        );
        let secret = crate::chat::memory::injecao_gate::secrets(operation);
        if approval.is_some()
            || r.context.space_id != crate::fork::spaces::PERSONAL
            || matches!(operation, Operation::NotDurable { .. })
        {
            secret?;
        } else if matches!(learning, crate::fork::memory_learning::Mode::Off) && secret.is_err() {
            continue;
        }
        if matches!(operation, Operation::NotDurable { .. }) {
            ensure!(finish, "memory_not_durable_requires_extractor");
            audit_changed |=
                super::audit::record_job(tx, &r.id, super::audit::Reason::NotDurable, Some(evidence), now)?;
            continue;
        }
        // Segredo é recusa SEM pendência nem prévia: sai antes do scanner para o texto nunca chegar
        // a `donna_memory_injecao_pendentes`. O scanner de injeção (memória pessoal) confere o
        // conteúdo INTEGRAL na mesma transação da escrita: veredito suspeito é recusa fail-closed,
        // com recibo sem texto nem hash e aviso ao dono.
        if approval.is_none()
            && !matches!(learning, crate::fork::memory_learning::Mode::Off)
            && r.context.space_id == crate::fork::spaces::PERSONAL
            && let Some(rule) = secret.err().or_else(|| crate::chat::memory::injecao_gate::scan(operation))
        {
            let gate = if i < plan_len {
                super::recibo_injecao::Gate::Promocao
            } else {
                super::recibo_injecao::Gate::Ferramenta
            };
            let (event, notice) = super::recibo_injecao::refuse(
                tx,
                gate,
                &super::recibo_injecao::Alvo { job: Some(&r.id), context: &r.context, cfg: &cfg, op: operation },
                rule,
                r.context.closing_turn.unwrap_or(1),
                now,
            )?;
            refusal_events.push(event);
            refusal_notices.push(notice);
            unproven = true;
            continue;
        }
        let key = super::queue::hash(&serde_json::to_string(operation)?);
        let id = format!("memory-auto-{}-{key}", r.id);
        let existing: Option<String> = tx
            .query_row("SELECT status FROM donna_memory_auto_receipts WHERE id=?1", [&id], |row| row.get(0))
            .optional()?;
        if let Some(status) = existing {
            // O retry do próprio recibo desfeito não é uma nova duplicata.
            if status != "undone" {
                audit_changed |=
                    super::audit::record_job(tx, &r.id, super::audit::Reason::Duplicate, Some(evidence), now)?;
            }
            unproven = true;
            continue;
        }
        // D389: com a promoção desligada nada é gravado; o scanner não registra recusa do que nem tentou.
        if matches!((approval, &learning), (None, crate::fork::memory_learning::Mode::Off)) {
            continue;
        }
        // D389: fora do modo automático, só a aprovação do dono aplica.
        if let (None, crate::fork::memory_learning::Mode::Suggest) = (approval, learning) {
            // Mesma régua do automático (alvo, camada, revisão, compartilhamento, origem, bloqueios): o próprio
            // `apply_one` num SAVEPOINT desfeito. Só o que seria gravado vira sugestão; o resto, como no auto.
            let origin = super::origin::derive(tx, &r.context, evidence, operation.text())?;
            tx.execute_batch("SAVEPOINT d389_sugestao")?;
            let rev = crate::store::next_rev(tx);
            let trial = rev.and_then(|rev| apply_one(tx, &r.context, &cfg, operation, &origin, now, rev));
            tx.execute_batch("ROLLBACK TO d389_sugestao; RELEASE d389_sugestao")?;
            match trial.map_err(refusal) {
                Ok(Some(_)) => parked |= super::suggestions::park(tx, &r.id, &r.context, operation, now)?,
                Ok(None) | Err(Refused::Forgotten) => unproven = true,
                Err(Refused::Duplicate) => {
                    unproven = true;
                    audit_changed |=
                        super::audit::record_job(tx, &r.id, super::audit::Reason::Duplicate, Some(evidence), now)?;
                }
                Err(Refused::Other(error)) => return Err(error),
            }
            continue;
        }
        // R1-557-01: a linhagem da sugestão (a) é reconferida antes de ler a fonte, para uma mensagem apagada encerrar
        // a sugestão como desatualizada (`stale`), e não como erro de leitura.
        if let Operation::Reclassify { entry, sources, .. } = operation {
            ensure!(super::sleep_model::sources_still(tx, *entry, sources)?, "memory_revision_changed");
        }
        let origin = super::origin::derive(tx, &r.context, evidence, operation.text())?;
        let rev = crate::store::next_rev(tx)?;
        let result = apply_one(tx, &r.context, &cfg, operation, &origin, now, rev);
        let saved = match result {
            Ok(saved) => saved,
            Err(error)
                if matches!(
                    error.downcast_ref::<mem::Error>(),
                    Some(mem::Error::Refused(codync_mem::check::Refusal::Duplicate))
                ) =>
            {
                // D539: repetição em outra mensagem limpa reforça o fato, sem promover sua natureza.
                // A duplicata continua sem provar leitura para o sono (SEG-507-02).
                let layer = match operation {
                    Operation::Remember { shared, .. } => Some(own_layer(&r.context, *shared, &cfg)?),
                    Operation::Revise { entry, .. } => Some(mem::get(tx, *entry)?.ok_or(mem::Error::NotFound)?.bot),
                    _ => None,
                };
                if let Some(layer) = layer {
                    let blocked = mem::fact_blocked(tx, Some(&r.context.bot_id), operation.text(), evidence)?;
                    let reinforced = !blocked
                        && super::origin::confirms(tx, &r.context, evidence, operation.text())?
                        && codync_mem::lifecycle::reinforce(
                            tx,
                            layer.as_deref(),
                            operation.text(),
                            evidence,
                            &origin,
                            now,
                            rev,
                        )?;
                    if reinforced {
                        reinforcements += 1;
                        tx.execute(
                            "INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=?1",
                            [rev],
                        )?;
                        shared_changed |= layer.is_none();
                    }
                }
                unproven = true;
                audit_changed |=
                    super::audit::record_job(tx, &r.id, super::audit::Reason::Duplicate, Some(evidence), now)?;
                continue;
            }
            Err(error)
                if matches!(
                    error.downcast_ref::<mem::Error>(),
                    Some(mem::Error::Refused(codync_mem::check::Refusal::Forgotten))
                ) =>
            {
                unproven = true;
                continue;
            }
            Err(error) => return Err(error),
        };
        // SEG-507-02 (R2): nada a gravar (por exemplo, arquivar o que já está arquivado) também não prova a leitura.
        let Some(saved) = saved else {
            unproven = true;
            continue;
        };
        let (id, shared) = receipt(tx, (&r.id, &r.context), operation, &saved, rev, now)?;
        shared_changed |= shared;
        ids.push(id);
        if let Some(p) = saved.projection {
            projections.push(p);
        }
    }
    if approval.is_none() {
        super::worker::validate(tx, r)?;
        authorize(tx, &r.context)?;
        // R1-01 = SEG-499-01: `off` lido no commit suprime todas as operações e fecha o pedido como `ineligible`
        // (D389 "Não aprende nada": nada agora nem depois, sem guardar propostas), nunca "concluído sem mudança".
        // Só o `noChange` com o aprendizado ligado nesta Tx ganha a prova `learningOn`, a única que o sono aceita.
        let off = learning == crate::fork::memory_learning::Mode::Off;
        let status = if !finish {
            "reserved"
        } else if off {
            "ineligible"
        } else if ids.is_empty() {
            "noChange"
        } else {
            "applied"
        };
        tx.execute(
            "UPDATE donna_memory_auto_jobs SET status=?2,reason=CASE WHEN ?2='ineligible' THEN 'ineligible' END,
            proof=CASE WHEN ?2 IN ('noChange','applied') AND ?3 THEN 'unproven'
                WHEN ?2='noChange' THEN 'learningOn' END,
            reservation=CASE WHEN ?2='reserved' THEN reservation ELSE NULL END WHERE id=?1",
            params![r.id, status, unproven],
        )?;
        tx.execute("DELETE FROM donna_memory_auto_proposals WHERE job=?1", [&r.id])?;
    }
    authorize(tx, &r.context)?;
    prepared_bindings_live(tx, &r.context, &bindings)?;
    if let Some(suggestion) = approval {
        super::suggestions::settle(tx, suggestion, ids.first(), now)?;
    }
    Ok(Committed {
        ids,
        projections,
        shared_changed,
        audit_changed: audit_changed || !refusal_events.is_empty(),
        reinforcements,
        parked,
        refusal_events,
        refusal_notices,
        context: r.context.clone(),
    })
}

/// D246: cada proposta preparada só vale com o vínculo ORIGINAL da execução que a criou, lido por
/// `sessoes::bindings::resolve` (existe, não revogado, sessão aberta, bot vivo; não exige estar ATIVO) e do mesmo bot,
/// conversa e sessão ACP do `Context` do job. Legado sem vínculo, inexistente ou revogado recusa o lote (preparado não
/// salvo); não procura outro vínculo da sessão nem resolve Authority de novo. A conta não fica no vínculo: os gatilhos
/// de `sessoes::bindings` o revogam quando a conta ou o motor da conversa mudam (invariante, não leitura da conta).
fn prepared_bindings_live(db: &Connection, c: &Context, bindings: &[Option<String>]) -> Result<()> {
    for binding in bindings {
        let id = binding.as_deref().ok_or_else(|| anyhow::anyhow!("memory_prepared_binding_missing"))?;
        // Produtor canônico de Sessões, pelo ID ORIGINAL, nesta Connection/Tx; não procura o ativo nem outro vínculo.
        let original =
            crate::fork::sessoes::bindings::resolve(db, id, &c.bot_id).context("memory_prepared_binding_revoked")?;
        ensure!(
            original.session_ref.chat_id == c.chat_id && original.acp_session.as_deref() == Some(c.session.as_str()),
            "memory_prepared_binding_revoked"
        );
    }
    Ok(())
}

enum Refused {
    Duplicate,
    Forgotten,
    Other(anyhow::Error),
}
fn refusal(error: anyhow::Error) -> Refused {
    match error.downcast_ref::<mem::Error>() {
        Some(mem::Error::Refused(codync_mem::check::Refusal::Duplicate)) => Refused::Duplicate,
        Some(mem::Error::Refused(codync_mem::check::Refusal::Forgotten)) => Refused::Forgotten,
        _ => Refused::Other(error),
    }
}

pub(crate) struct Saved {
    entry: Option<i64>,
    file_version: Option<i64>,
    previous: Option<Value>,
    origin: Origin,
    /// A nota a reprojetar depois do commit (camada, arquivo); `Some` só no braço Note.
    pub(crate) projection: Option<(Option<String>, String)>,
}

fn visible(db: &Connection, c: &Context, cfg: &BotConfig, id: i64, expected: i64) -> Result<mem::Entry> {
    let entry = mem::get(db, id)?.ok_or(mem::Error::NotFound)?;
    ensure!(entry.rev == expected, "memory_revision_changed");
    ensure!(
        entry.bot.as_deref() == Some(c.memory_id.as_str())
            || (entry.bot.is_none() && c.space_id == crate::fork::spaces::PERSONAL && crate::chat::memory::shared(cfg)),
        "memory_scope_mismatch"
    );
    Ok(entry)
}

/// A liberação manual do dono (`injecao_gate::release`) rejoga a operação recusada por este MESMO
/// caminho, como `Writer::Override`: só o veredito do scanner de injeção é dispensado — camada,
/// dono (`memory_user_owned`), origem externa × compartilhada, expectativa original, bloqueio de
/// fato, deduplicação, conflito e teto seguem valendo exatamente como na escrita normal.
pub(crate) fn apply_released(
    db: &Connection,
    c: &Context,
    cfg: &BotConfig,
    op: &Operation,
    origin: &Origin,
    now: i64,
    rev: i64,
) -> Result<Option<Saved>> {
    apply_one_as(db, c, cfg, op, origin, now, rev, Writer::Override(&c.bot_id))
}

fn apply_one(
    db: &Connection,
    c: &Context,
    cfg: &BotConfig,
    op: &Operation,
    origin: &Origin,
    now: i64,
    rev: i64,
) -> Result<Option<Saved>> {
    apply_one_as(db, c, cfg, op, origin, now, rev, Writer::Bot(&c.bot_id))
}

#[allow(clippy::too_many_arguments)]
fn apply_one_as(
    db: &Connection,
    c: &Context,
    cfg: &BotConfig,
    op: &Operation,
    origin: &Origin,
    now: i64,
    rev: i64,
    writer: Writer<'_>,
) -> Result<Option<Saved>> {
    let (entry, previous, origin) = match op {
        Operation::NotDurable { .. } => anyhow::bail!("memory_not_durable_requires_extractor"),
        // Sono 2b1c: mesma lembrança, mesmo texto, natureza `inferred` → `said`, com versão nova (Desfazer volta a ela).
        // Só inferência sem conflito, escrita pelo bot, sem origem externa, e só se o Hub achar o texto numa mensagem
        // inteira do dono, limpa (`origin::confirms`). Conflito nunca: um literal antigo não elimina a alternativa.
        Operation::Confirm { entry, expected_rev, evidence } => {
            let current = mem::get(db, *entry)?.ok_or(mem::Error::NotFound)?;
            let old = visible(db, c, cfg, *entry, current.rev)?;
            ensure!(old.rev == *expected_rev, "memory_revision_changed");
            ensure!(
                old.author != mem::Author::User
                    && old.state == mem::State::Active
                    && old.nature == KindNature::Inferred
                    && old.conflict_text.is_none()
                    && !old.origin.external
                    && old.origin.kind != codync_mem::origin::Kind::Unknown,
                "memory_confirm_not_eligible"
            );
            if mem::fact_blocked(db, old.bot.as_deref(), &old.text, evidence)? {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Forgotten).into());
            }
            ensure!(super::origin::confirms(db, c, evidence, &old.text)?, "memory_confirm_requires_literal");
            let next = mem::revise(db, writer, *entry, &old.text, now, rev)?;
            nature::set(db, *entry, KindNature::Said, None, None)?;
            db.execute("UPDATE memory_entries SET evidence=?2 WHERE id=?1", params![entry, evidence.join(",")])?;
            // A origem do texto agora é a fala do dono recalculada pelo Hub (`confirms` já exigiu `Kind::User`); a de
            // antes não era externa nem desconhecida, fica na versão anterior (Desfazer) e na linhagem.
            let spoken = super::origin::derive(db, c, evidence, &old.text)?;
            (next, Some(serde_json::to_value(old)?), spoken)
        }
        // Sono 2b1d-2, (a) aprovado pelo dono: mesma lembrança e texto, `inferred` → `confirmed`, versão nova (Desfazer
        // volta a ela). A lembrança e cada fonte da linhagem ainda estão nas revisões da sugestão; senão, `stale`.
        Operation::Reclassify { entry, expected_rev, evidence, sources } => {
            let current = mem::get(db, *entry)?.ok_or(mem::Error::NotFound)?;
            let old = visible(db, c, cfg, *entry, current.rev)?;
            ensure!(
                old.rev == *expected_rev && super::sleep_model::sources_still(db, *entry, sources)?,
                "memory_revision_changed"
            );
            ensure!(super::sleep_model::reclassifiable(&old), "memory_confirm_not_eligible");
            if mem::fact_blocked(db, old.bot.as_deref(), &old.text, evidence)? {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Forgotten).into());
            }
            let next = mem::revise(db, writer, *entry, &old.text, now, rev)?;
            nature::set(db, *entry, KindNature::Confirmed, None, None)?;
            let origin = old.origin.clone();
            (next, Some(serde_json::to_value(old)?), origin)
        }
        Operation::Remember { text, shared, evidence, .. } => {
            ensure!(op.nature() != Some(nature::Hint::Conflict), "memory_conflict_requires_existing_memory");
            ensure!(!*shared || !origin.external, "memory_private_source_cannot_be_shared");
            let layer = own_layer(c, *shared, cfg)?;
            let entry = mem::add(db, writer, mem::New { bot: layer.as_deref(), text, room: None, evidence }, now, rev)?;
            let literal = super::origin::literal_owner(db, c, evidence, text)?;
            let kind = nature::proposed(literal, op.nature());
            nature::set(
                db,
                entry.id,
                kind,
                matches!(kind, KindNature::Inferred | KindNature::Conflict).then_some(c.bot_id.as_str()),
                None,
            )?;
            (entry, None, origin.clone())
        }
        Operation::Revise { entry, expected_rev, text, evidence, .. } => {
            let current = mem::get(db, *entry)?.ok_or(mem::Error::NotFound)?;
            let old = visible(db, c, cfg, *entry, current.rev)?;
            ensure!(old.author != mem::Author::User, "memory_user_owned");
            // `Writer::Override` dispensa só o veredito de conteúdo; segredo e limites seguem no crate.
            if !matches!(writer, Writer::Override(_)) {
                codync_mem::check::text(text)?;
            }
            ensure!(old.bot.is_some() || !origin.external, "memory_private_source_cannot_be_shared");
            ensure!(old.rev == *expected_rev, "memory_revision_changed");
            if mem::fact_blocked(db, old.bot.as_deref(), text, evidence)? {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Forgotten).into());
            }
            let literal = super::origin::literal_owner(db, c, evidence, text)?;
            // CAS e tombstone vencem a equivalência. Um hint repetido não pode
            // criar versão/recibo nem mudar a certeza da mesma informação.
            if mem::fingerprint(&old.text) == mem::fingerprint(text)
                || (nature::proposed(literal, op.nature()) != KindNature::Said
                    && old.nature == KindNature::Conflict
                    && old
                        .conflict_text
                        .as_deref()
                        .is_some_and(|candidate| mem::fingerprint(candidate) == mem::fingerprint(text)))
            {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Duplicate).into());
            }
            let next = nature::revise(
                db,
                writer,
                nature::Revision { entry: *entry, text, literal_owner: literal, hint: op.nature(), bot: &c.bot_id },
                now,
                rev,
            )?;
            let merged = Origin::merge([old.origin.clone(), origin.clone()]);
            db.execute("UPDATE memory_entries SET evidence=?2 WHERE id=?1", params![entry, evidence.join(",")])?;
            (next, Some(serde_json::to_value(old)?), merged)
        }
        Operation::Archive { entry, expected_rev, evidence, .. } => {
            let current = mem::get(db, *entry)?.ok_or(mem::Error::NotFound)?;
            let old = visible(db, c, cfg, *entry, current.rev)?;
            ensure!(old.author != mem::Author::User, "memory_user_owned");
            if old.state == mem::State::Archived {
                return Ok(None);
            }
            ensure!(old.rev == *expected_rev, "memory_revision_changed");
            if mem::fact_blocked(db, old.bot.as_deref(), &old.text, evidence)? {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Forgotten).into());
            }
            let next = mem::archive(db, writer, *entry, None, now, rev)?;
            let merged = Origin::merge([old.origin.clone(), origin.clone()]);
            (next, Some(serde_json::to_value(old)?), merged)
        }
        Operation::Note { file, content, shared, expected_hash, evidence, .. } => {
            ensure!(mf::valid_name(file), "memory_invalid_file");
            ensure!(!*shared || file != crate::chat::memory::profile::NAME, "memory_user_owned");
            ensure!(!*shared || !origin.external, "memory_private_source_cannot_be_shared");
            let layer = own_layer(c, *shared, cfg)?;
            let before = mf::get(db, layer.as_deref(), file)?;
            if before.as_ref().is_none_or(|f| f.deleted) && content.is_some() {
                ensure!(
                    mf::layer(db, layer.as_deref())?.iter().filter(|f| !f.deleted).count() < mf::FOLDER_CAP,
                    "memory_note_folder_full"
                );
            }
            let current = mf::current(db, layer.as_deref(), file)?;
            ensure!(current.as_deref().map(mf::hash) == *expected_hash, "memory_note_changed");
            if mem::fact_blocked(db, layer.as_deref(), &note_fact(file, content.as_deref()), evidence)? {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Forgotten).into());
            }
            ensure!(
                current.is_some() || op.nature() != Some(nature::Hint::Conflict),
                "memory_conflict_requires_existing_memory"
            );
            // Reexaminar o mesmo conteúdo não é edição. Um hint do modelo não
            // pode criar outra versão/recibo nem invalidar o Desfazer anterior.
            if current == *content {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Duplicate).into());
            }
            if let Some(text) = content {
                // `Writer::Override` dispensa só o veredito de conteúdo; segredo, teto e descrição seguem.
                if matches!(writer, Writer::Override(_)) {
                    mf::check_released(text)?;
                } else {
                    mf::check(text)?;
                }
            }
            let previous = before.as_ref().map(|f| mf::version(db, f.version)).transpose()?.flatten();
            // Arquivo existente também conserva marcas anteriores, inclusive origem desconhecida.
            let origin =
                previous.as_ref().map_or_else(|| origin.clone(), |v| Origin::merge([v.origin.clone(), origin.clone()]));
            let literal = content
                .as_deref()
                .map(|text| super::origin::literal_owner(db, c, evidence, text))
                .transpose()?
                .unwrap_or(false);
            let proposed = nature::proposed(literal, op.nature());
            let candidate = content.as_deref().unwrap_or("Proposta de remover esta nota.");
            if proposed != KindNature::Said
                && previous.as_ref().is_some_and(|version| {
                    version.nature == KindNature::Conflict && version.conflict_text.as_deref() == Some(candidate)
                })
            {
                return Err(mem::Error::Refused(codync_mem::check::Refusal::Duplicate).into());
            }
            let conflict = current.is_some() && proposed != KindNature::Said;
            let kind = if conflict { KindNature::Conflict } else { proposed };
            let actual = if conflict { current.as_deref() } else { content.as_deref() };
            let alternative = conflict.then(|| content.as_deref().unwrap_or("Proposta de remover esta nota."));
            let change = mf::Change {
                bot: layer.as_deref(),
                name: file,
                content: actual,
                author: mf::Author::Bot,
                author_bot: Some(&c.bot_id),
                cause: if actual.is_some() { mf::Cause::Edit } else { mf::Cause::Delete },
                mtime: now,
                size: actual.map_or(0, |v| v.len() as u64),
            };
            let version = mf::record(db, &change, now, rev)?;
            codync_mem::origin::set_file(db, version.id, &origin)?;
            nature::set_file(
                db,
                version.id,
                kind,
                matches!(kind, KindNature::Inferred | KindNature::Conflict).then_some(c.bot_id.as_str()),
                alternative,
            )?;
            return Ok(Some(Saved {
                entry: None,
                file_version: Some(version.id),
                previous: previous.map(serde_json::to_value).transpose()?,
                origin,
                projection: Some((layer, file.clone())),
            }));
        }
    };
    codync_mem::origin::set(db, entry.id, &origin)?;
    Ok(Some(Saved { entry: Some(entry.id), file_version: None, previous, origin, projection: None }))
}

/// Receipt ID é o entry.id do aviso. Repetição devolve o estado durável, inclusive changed.
pub(crate) fn undo(hub: &Hub, id: &str, authority: &crate::fork::autoridade::Autoridade) -> Result<Value> {
    let boundary = MemorySection::enter(hub);
    let emission = hub.emit_lock.locked();
    let mut committed = authority.with_db(hub, None, |tx| {
        let row: (String, Option<i64>, Option<i64>, i64, Option<String>, String, String) = tx.query_row(
            "SELECT operation,entry,file_version,rev,previous,status,job FROM donna_memory_auto_receipts WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )?;
        let raw: String =
            tx.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [&row.6], |r| r.get(0))?;
        let mut c: Context = serde_json::from_str(&raw)?;
        let data: String = tx.query_row("SELECT data FROM entries WHERE id=?1", [id], |r| r.get(0))?;
        let data: Value = serde_json::from_str(&data)?;
        // Só dados do diário, como em commit_in; publish é compartilhado e roda fora das guardas.
        let committed = |context: Context| {
            let shared = data["memory"]["shared"] == true;
            let projections = data["memory"]["file"]
                .as_str()
                .filter(|_| data["memory"]["action"] == "note")
                .map(|file| ((!shared).then(|| context.memory_id.clone()), file.to_owned()))
                .into_iter()
                .collect();
            Committed {
                ids: vec![id.to_owned()],
                projections,
                shared_changed: shared,
                audit_changed: false,
                reinforcements: 0,
                parked: false,
                refusal_events: Vec::new(),
                refusal_notices: Vec::new(),
                context,
            }
        };
        if row.5 == "undone" || row.5 == "changed" {
            return Ok(committed(c));
        }
        // A intenção de undo é atual; mudanças de configuração sem troca de destino não a invalidam.
        c.bot_revision = tx.query_row(
            "SELECT m.revision FROM donna_memory_auto_bot_bindings m JOIN bots b ON b.id=m.bot
             WHERE m.bot=?1 AND b.deleted=0",
            [&c.bot_id],
            |r| r.get(0),
        )?;
        if c.conversation_revision.is_some() {
            c.conversation_revision = Some(tx.query_row(
                "SELECT revision FROM donna_space_conversations WHERE id=?1 AND bot_id=?2 AND space_id=?3",
                params![c.chat_id, c.bot_id, c.space_id],
                |r| r.get(0),
            )?);
        }
        super::worker::validate_context(tx, &c)?;
        let op: Operation = serde_json::from_str(&row.0)?;
        let previous: Option<Value> = row.4.as_ref().map(|raw| serde_json::from_str(raw)).transpose()?;
        let now = crate::store::now_ms();
        let rev = crate::store::next_rev(tx)?;
        let mut changed = false;
        if let Some(memory) = row.1 {
            if let Some(current) = mem::get(tx, memory)?.filter(|e| e.rev == row.3) {
                let withdrawn = if op.text().is_empty() { current.text.as_str() } else { op.text() };
                mem::block_fact(tx, current.bot.as_deref(), withdrawn, op.evidence())?;
                if let Some(old) = &previous {
                    let previous_rev = old["rev"].as_i64().ok_or(mem::Error::NotFound)?;
                    nature::restore(tx, memory, previous_rev, now, rev)?;
                } else {
                    tx.execute("DELETE FROM memory_entries WHERE id=?1", [memory])?;
                }
            } else {
                changed = true;
            }
        } else if let Operation::Note { file, content, shared, .. } = &op {
            let layer = if *shared { None } else { Some(c.memory_id.as_str()) };
            if mf::get(tx, layer, file)?.is_some_and(|f| Some(f.version) == row.2 && f.rev == row.3) {
                mem::block_fact(tx, layer, &note_fact(file, content.as_deref()), op.evidence())?;
                let content = previous.as_ref().and_then(|v| v["content"].as_str());
                let version = mf::record(
                    tx,
                    &mf::Change {
                        bot: layer,
                        name: file,
                        content,
                        author: mf::Author::User,
                        author_bot: None,
                        cause: mf::Cause::Undo,
                        mtime: now,
                        size: content.map_or(0, |s| s.len() as u64),
                    },
                    now,
                    rev,
                )?;
                let origin =
                    previous.as_ref().map(|p| codync_mem::origin::decode(&p["origin"].to_string())).unwrap_or_default();
                codync_mem::origin::set_file(tx, version.id, &origin)?;
                let kind = previous
                    .as_ref()
                    .map_or(KindNature::Unknown, |p| nature::decode(p["nature"].as_str().unwrap_or("unknown")));
                nature::set_file(
                    tx,
                    version.id,
                    kind,
                    previous.as_ref().and_then(|p| p["natureBot"].as_str()),
                    previous.as_ref().and_then(|p| p["conflictText"].as_str()),
                )?;
            } else {
                changed = true;
            }
        }
        let status = if changed { "changed" } else { "undone" };
        tx.execute("UPDATE donna_memory_auto_receipts SET status=?2 WHERE id=?1", params![id, status])?;
        tx.execute(
            "UPDATE entries SET data=json_set(data,'$.memory.status',?2,'$.text',?3),rev=?4,updated_at=?5 WHERE id=?1",
            params![id, status, if changed { "Esta memória mudou depois" } else { "Memória desfeita" }, rev, now],
        )?;
        tx.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [rev])?;
        Ok(committed(c))
    })?;
    committed.emit_entries(hub);
    drop(emission);
    drop(boundary);
    committed.project(hub);
    let _ = committed.finish(hub);
    let entry = hub.store.entry(id).ok_or_else(|| anyhow::anyhow!("memory_receipt_not_found"))?;
    Ok(json!({"entry":entry}))
}
