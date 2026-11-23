//! VG-03a: produtor interno, sem RPC ou autoridade nova. O único registro de
//! despacho continua sendo turn.inflight; reserva não autoriza session/prompt.
use crate::{
    LockExt,
    store::{BotConfig, Entry, Store, now_ms},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod conversation_pause;
mod human_setup;
mod memory_policy;
pub(crate) use conversation_pause::{
    ConversationPaused, pause as pause_conversation, paused as conversation_paused, release as release_conversation,
};
pub(crate) use human_setup::{begin_human_setup, mark_human_setup_ready, reuse_reserved};
pub(crate) use memory_policy::MemoryPolicy;

pub(crate) const INSTRUCTIONS: &str = "O Hub usa um produtor interno integrado de permissão durável antes dos novos prompts. Preparação não ativa escrita de memória nem Trabalhando; Undo continua protegido. Reserva não comprova envio. Política efetiva de memória acompanha a captura; revogação anterior à permissão impede o prompt antigo. Inflight e checkpoint D72 existente usam o mesmo token do runtime ativado; retomada sent sem token comprovado não fabrica identidade nem repete prompt. O binding de conversa/conta é revalidado na mesma transação da permissão; mudança concorrente anterior recusa o prompt antigo, sem trocar sua origem. Pausar próximos ainda não tem controle público no app, Voz ou MCP; não anuncie essa capacidade. Após reinício, uma permissão sem conclusão pode ter produzido efeitos: confira o histórico e não repita a ação automaticamente. Fila restaurada conserva a sessão da entrada; uma extra nunca é enviada à principal. Cada permit grava se o turno foi roteado (recibo do Roteamento) ou não roteado, e a espera de assinatura conserva essa classificação como o token; turnos anteriores ao gravador são não roteados e retomam como antes; espera sem classificação fica pausada, sem reclassificar. Preparo do mesmo pedido humano queued tenta automaticamente até três falhas totais antes da permissão, na mesma posição e com a identidade original; a terceira encerra com erro visível para reenvio manual. O orçamento é durável antes do preparo e não renova no reinício; pausa/espera por permissão não é falha. Entrada terminal não é reenviada. A enumeração inicial falha mantém consumidor no ator mesmo com fila vazia; permissão incerta não é repetida. Origem inválida no boot aguarda reconstrução no contexto original sem abandonar entradas válidas posteriores. A identidade preparada só alimenta snapshots e vínculo de transporte; ferramentas mutantes continuam exigindo execução ativa. Retomadas conservam o job/contexto de memória do mesmo pedido e não ressuscitam failed/ineligible só por reutilizar token. Rotinas conservam o recibo original; falha entre permissão e confirmação de início interrompe sem replay nem novo recibo. A negativa legada sem recibo é revalidada no produtor e não gasta orçamento só para interromper. Serviços internos de memória/título não são outro bot e não concedem autoridade.";

const VERSION: i64 = 1;

/// Classificação de recuperação; nunca prova de autoria ou concessão de acesso.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum Origin {
    Human,
    Routine { id: String },
    Ask,
    Message,
    Group,
    Compaction,
    Recovery,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    Reserved,
    Permitted,
}

/// Classificação de roteamento do turno (D305): o tipo canônico do Roteamento, gravado no permit pelo escritor único
/// do registro de admissão e copiado para a espera D72 como o token. Não concede autoridade nem identidade.
pub(crate) use crate::fork::roteamento::receipt::{NotRoutedReason, RoutingClass};

/// Marca, na mesma transação da migração, que todos os registros anteriores já foram classificados.
pub(crate) const LEGACY_MARKED: &str = "donna.routing.legacy_marked";

/// `execution_token` é a identidade já criada pelo Hub. attempt é só a cerca CAS.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Dispatch {
    pub admission_version: i64,
    pub execution_token: String,
    pub attempt: String,
    pub phase: Phase,
    /// Parada da mesma tentativa; não encerra o checkpoint nem altera a memória.
    #[serde(default)]
    pub stop_requested: bool,
    pub conversation_id: String,
    pub origin: Origin,
    #[serde(default)]
    pub memory_policy: Option<MemoryPolicy>,
    #[serde(default)]
    pub entry_ids: Vec<String>,
    /// Orçamento interno reservado antes do preparo, preservado no mesmo inflight.
    #[serde(default)]
    pub setup_attempts: u8,
    #[serde(default)]
    pub setup_ready: bool,
    /// Presente a partir do permit (D305); reserva ainda não permitida não tem classificação.
    #[serde(default)]
    pub routing: Option<RoutingClass>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ticket {
    pub execution_token: String,
    pub attempt: String,
}

impl Dispatch {
    pub(crate) fn ticket(&self) -> Ticket {
        Ticket { execution_token: self.execution_token.clone(), attempt: self.attempt.clone() }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pause {
    pub paused: bool,
    pub revision: i64,
    pub changed_at: Option<i64>,
}

pub(crate) fn key(bot: &str) -> String {
    format!("turn.inflight.{bot}")
}
fn pause_key(bot: &str) -> String {
    format!("donna.bot.admission.{bot}")
}

fn read(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
}
fn write(c: &Connection, key: &str, value: &Value) -> Result<()> {
    c.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2", params![key, value.to_string()])?;
    Ok(())
}

pub(crate) fn decode(raw: &str) -> Result<Option<Dispatch>> {
    // Números e objetos started/conversationId legados conservam o caminho antigo.
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        ensure!(!raw.contains("admissionVersion"), "admission_record_invalid");
        return Ok(None);
    };
    if value.get("admissionVersion").is_none() {
        return Ok(None);
    }
    let record: Dispatch = serde_json::from_value(value).context("admission_record_invalid")?;
    ensure!(record.admission_version == VERSION, "admission_version_unsupported");
    ensure!(!record.execution_token.is_empty() && !record.attempt.is_empty(), "admission_record_invalid");
    ensure!(
        record.setup_attempts <= 3 && (!record.setup_ready || record.setup_attempts > 0),
        "admission_setup_budget_invalid"
    );
    Ok(Some(record))
}

pub(crate) fn current(store: &Store, bot: &str) -> Result<Option<Dispatch>> {
    store.kv_read(&key(bot))?.as_deref().map(decode).transpose().map(Option::flatten)
}
/// Leitura canônica sem Store para consumers já sob a Connection do efeito.
pub(crate) fn current_on(c: &Connection, bot: &str) -> Result<Option<Dispatch>> {
    read(c, &key(bot))?.as_deref().map(decode).transpose().map(Option::flatten)
}
fn pause_in(c: &Connection, bot: &str) -> Result<Pause> {
    let state =
        read(c, &pause_key(bot))?.map(|raw| serde_json::from_str::<Pause>(&raw)).transpose()?.unwrap_or_default();
    ensure!(state.revision >= 0 && (!state.paused || state.changed_at.is_some()), "admission_pause_invalid");
    Ok(state)
}
/// Mesma leitura na `Connection` recebida (resumo do dia, MB-3).
pub(crate) fn paused_in(c: &Connection, bot: &str) -> Result<bool> {
    Ok(pause_in(c, bot)?.paused)
}

pub(crate) fn paused(store: &Store, bot: &str) -> Result<bool> {
    Ok(pause_in(&store.db.locked(), bot)?.paused)
}

/// Reserva própria do ator; nenhum status humano é consumido aqui.
#[cfg(test)]
pub(crate) fn reserve(
    store: &Store,
    cfg: &BotConfig,
    chat: &str,
    execution_token: &str,
    origin: &Origin,
) -> Result<Ticket> {
    reserve_for_entries(store, cfg, chat, execution_token, origin, &[])
}

pub(crate) fn reserve_for_entries(
    store: &Store,
    cfg: &BotConfig,
    chat: &str,
    execution_token: &str,
    origin: &Origin,
    ids: &[String],
) -> Result<Ticket> {
    let bot = &cfg.id;
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    if let Some(raw) = read(&tx, &key(bot))? {
        ensure!(decode(&raw)?.is_none(), "admission_dispatch_active");
    }
    ensure!(!execution_token.is_empty(), "admission_execution_missing");
    let ticket = Ticket { execution_token: execution_token.into(), attempt: uuid::Uuid::new_v4().to_string() };
    write(
        &tx,
        &key(bot),
        &json!({
            "admissionVersion": VERSION, "executionToken": ticket.execution_token,
            "attempt": ticket.attempt, "phase":"reserved", "started":now_ms(),
            "conversationId":chat, "origin":origin, "memoryPolicy":MemoryPolicy::of_config(cfg),
            "entryIds":ids,
        }),
    )?;
    tx.commit()?;
    Ok(ticket)
}

fn captured(c: &Connection, bot: &str, ticket: &Ticket) -> Result<(Dispatch, Value)> {
    let raw = read(c, &key(bot))?.context("admission_dispatch_missing")?;
    let record = decode(&raw)?.context("admission_dispatch_legacy")?;
    ensure!(
        record.execution_token == ticket.execution_token && record.attempt == ticket.attempt,
        "admission_dispatch_conflict"
    );
    Ok((record, serde_json::from_str(&raw)?))
}

/// Leitor do preparo: mesma reserva canônica, sem claim, migração ou autoridade nova.
pub(crate) fn prepared_on(c: &Connection, bot: &str, ticket: &Ticket) -> Result<Dispatch> {
    let (record, _) = captured(c, bot, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    Ok(record)
}

fn memory_matches_on(c: &Connection, cfg: &BotConfig, ticket: &Ticket) -> Result<bool> {
    let (record, _) = captured(c, &cfg.id, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    let Some(policy) = record.memory_policy else { return Ok(false) };
    Ok(policy == MemoryPolicy::of_config(cfg) && policy == crate::fork::spaces::current_memory_policy_on(c, cfg)?)
}

pub(crate) fn prepared_memory_matches(store: &Store, cfg: &BotConfig, ticket: &Ticket) -> Result<bool> {
    memory_matches_on(&store.db.locked(), cfg, ticket)
}

/// Última fronteira síncrona antes do envio. Pausa e permissão usam o mesmo DB
/// lock/transação; nenhum lock sai daqui. Retrying pode reclamar entradas sent
/// somente com o checkpoint já capturado pelo ator (contrato D72 existente).
/// Encaixe do PR4 (Sessões) no produtor VG03. Inerte hoje (`ClaimHooks::default()` e só o
/// `binding` preenchido pelo ator); o PR4 pluga o gravador depois do merge.
/// - Recebe a MESMA `&Connection` da Tx IMMEDIATE do permit: síncrono, falível, sem
///   Store/Hub/Cloud/Tx aninhada/rede/await.
/// - Ordem: guardas e referências → encaixe (pré-imagem) → claim queued→sent → inflight
///   `permitted` → commit → ativação/envio. Falha do encaixe desfaz a Tx inteira: nada de
///   `permitted`, evento ou envio; a reserva continua `Reserved`.
/// - Dispatch × Resume: o primeiro despacho chama só `dispatch`, com o `BindingRef` ORIGINAL
///   capturado no setup (validado nesta Connection, nunca recapturado). A retomada de
///   checkpoint chama só `resume`, nunca o gravador inicial, sem D72 artificial.
#[derive(Default)]
pub(crate) struct ClaimHooks<'a> {
    pub binding: Option<&'a crate::fork::sessoes::bindings::BindingRef>,
    pub dispatch: Option<DispatchHook<'a>>,
    pub resume: Option<ResumeHook<'a>>,
    /// Conferência do teto de gasto: roda na MESMA Tx do permit — decisão e
    /// autorização são atômicas. `Hold` já gravou a espera durável e apagou a
    /// reserva; o commit sai só com essa evidência (sem `permitted`).
    pub gate: Option<GateHook<'a>>,
}
/// Primeiro despacho: pré-imagem com o vínculo original. Devolve a classificação que este permit grava.
pub(crate) type DispatchHook<'a> =
    &'a mut dyn FnMut(&Connection, &crate::fork::sessoes::bindings::BindingRef) -> Result<RoutingClass>;
/// Retomada de checkpoint (texto original); nunca o gravador inicial. Devolve a classificação original conferida.
pub(crate) type ResumeHook<'a> = &'a mut dyn FnMut(&Connection, &str) -> Result<RoutingClass>;
/// Teto de gasto na Tx do permit: recebe a `Connection` da Tx e a reserva capturada.
pub(crate) type GateHook<'a> = &'a mut dyn FnMut(&Connection, &Dispatch) -> Result<crate::fork::teto::Verdict>;

/// Apaga o registro inflight do bot na mesma conexão — a Tx que trava o teto
/// morre com a reserva que a gerou; a espera durável passa a ser a evidência.
pub(crate) fn drop_inflight(c: &Connection, bot: &str) -> Result<()> {
    c.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    Ok(())
}

/// A classificação gravada na espera D72 (`routing`). Ausente ou ilegível falha fechado: nunca vira legado.
pub(crate) fn checkpoint_routing(checkpoint: &str) -> Result<RoutingClass> {
    let value: Value = serde_json::from_str(checkpoint).context("admission_routing_missing")?;
    let routing = value.get("routing").filter(|v| !v.is_null()).context("admission_routing_missing")?;
    serde_json::from_value(routing.clone()).context("admission_routing_invalid")
}

/// O destino original gravado na espera D72: conversa (`input.chat`) e sessão visível (`input.visibleSession`; ausente
/// é a principal, como em esperas anteriores ao PR4). Ilegível falha fechado.
fn checkpoint_destination(checkpoint: &str) -> Result<(String, Option<String>)> {
    let value: Value = serde_json::from_str(checkpoint).context("admission_checkpoint_invalid")?;
    let input = &value["input"];
    let chat = input["chat"].as_str().context("admission_checkpoint_invalid")?.to_owned();
    let session = match &input["visibleSession"] {
        Value::Null => None,
        other => Some(other.as_str().context("admission_checkpoint_invalid")?.to_owned()),
    };
    Ok((chat, session))
}

#[cfg(test)]
pub(crate) fn permit(
    store: &Store,
    cfg: &BotConfig,
    ticket: &Ticket,
    checkpoint: Option<&str>,
    ids: &[String],
    expected_session: Option<&str>,
) -> Result<Option<Vec<Entry>>> {
    // Fixture do consumidor de Sessões para os testes do produtor: classifica como a principal legada (sem Fixo
    // provado) e, na retomada, conserva a classificação da espera. Não grava recibo nem escolha.
    let binding = crate::fork::sessoes::bindings::fixture_binding(store, &cfg.id);
    let mut dispatch = |_: &Connection, _: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        Ok(RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection })
    };
    let mut resume = |_: &Connection, checkpoint: &str| checkpoint_routing(checkpoint);
    let hooks =
        ClaimHooks { binding: Some(&binding), dispatch: Some(&mut dispatch), resume: Some(&mut resume), gate: None };
    permit_with(store, cfg, ticket, checkpoint, ids, expected_session, hooks)
}

pub(crate) fn permit_with(
    store: &Store,
    cfg: &BotConfig,
    ticket: &Ticket,
    checkpoint: Option<&str>,
    ids: &[String],
    expected_session: Option<&str>,
    hooks: ClaimHooks<'_>,
) -> Result<Option<Vec<Entry>>> {
    let bot = &cfg.id;
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let (record, mut value) = captured(&tx, bot, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    ensure!(record.setup_attempts == 0 || record.setup_ready, "admission_setup_not_ready");
    let pause = pause_in(&tx, bot)?;
    if !crate::fork::usage_limits::allow_on(&tx, checkpoint)? {
        return Ok(None);
    }
    if pause.paused {
        return Ok(None);
    }
    // Registro de pausa ilegível conta como pausa: a reserva sai e o bot não fica preso tentando de novo.
    let paused = conversation_pause::paused_on(&tx, bot, &record.conversation_id).unwrap_or_else(|error| {
        tracing::error!(%error, bot, conversation_id = %record.conversation_id, "pausa da conversa ilegível");
        true
    });
    if paused {
        tracing::warn!(bot, conversation_id = %record.conversation_id, "conversa pausada: reserva apagada sem despacho");
        drop_inflight(&tx, bot)?;
        tx.commit()?;
        return Err(ConversationPaused.into());
    }
    // Teto de gasto na mesma Tx: bloqueado grava a espera durável e a Tx morre
    // sem `permitted` (e sem a reserva, já apagada pela decisão).
    if let Some(gate) = hooks.gate
        && gate(&tx, &record)? == crate::fork::teto::Verdict::Hold
    {
        tx.commit()?;
        return Ok(None);
    }
    ensure!(memory_matches_on(&tx, cfg, ticket)?, "admission_memory_policy_changed");
    crate::fork::spaces::validate_dispatch_on(&tx, cfg, &record.conversation_id, ids)?;
    let memory = crate::fork::memory_auto::admission::before_claim(
        &tx,
        cfg,
        &ticket.execution_token,
        &record.conversation_id,
        ids,
    )?;
    // A classificação deriva da origem capturada nesta reserva, nunca da presença ou ausência do encaixe: turno
    // humano exige o consumidor de Sessões (sem ele, falha fechado); os demais são `notHumanTurn`.
    let human = record.origin == Origin::Human;
    if !human
        && cfg.context.is_none()
        && cfg.command.is_none()
        && cfg.space_id == crate::fork::spaces::PERSONAL
        && record.conversation_id == cfg.id
    {
        crate::fork::sessoes::admissao::conferir_troca_de_conta(
            &tx,
            expected_session.unwrap_or(&record.conversation_id),
            cfg,
        )?;
    }
    let routing = match checkpoint {
        None if human => {
            let dispatch = hooks.dispatch.ok_or_else(|| anyhow::anyhow!("admission_routing_consumer_missing"))?;
            let binding = hooks.binding.ok_or_else(|| anyhow::anyhow!("admission_binding_missing"))?;
            let live = crate::fork::sessoes::bindings::resolve(&tx, binding.id.as_str(), bot)?;
            ensure!(live.session_ref == binding.session_ref, "admission_binding_changed");
            dispatch(&tx, binding)?
        }
        // Classe canônica do Roteamento para origem não humana; o consumidor humano não é chamado.
        None => crate::fork::roteamento::receipt::admission_class(
            &tx,
            cfg,
            false,
            true,
            None,
            &crate::fork::spaces::EngineSelection {
                backend: cfg.backend.clone(),
                model: cfg.model.clone(),
                model_options: cfg.model_options.clone(),
            },
        )?,
        // A retomada conserva a classificação original da espera; sem ela, falha fechado. O vínculo do setup DESTA
        // tentativa é exigido e revalidado aqui, como no Dispatch, e precisa ser do destino original da espera; um
        // vínculo revogado durante os awaits do setup recusa antes do claim, sem recapturar outro.
        Some(checkpoint) if human => {
            let resume = hooks.resume.ok_or_else(|| anyhow::anyhow!("admission_routing_consumer_missing"))?;
            let binding = hooks.binding.ok_or_else(|| anyhow::anyhow!("admission_binding_missing"))?;
            let live = crate::fork::sessoes::bindings::resolve(&tx, binding.id.as_str(), bot)?;
            ensure!(live.session_ref == binding.session_ref, "admission_binding_changed");
            let (chat, session) = checkpoint_destination(checkpoint)?;
            ensure!(
                binding.session_ref.chat_id == chat
                    && binding.session_ref.id == session.unwrap_or_else(|| chat.clone()),
                "admission_binding_changed"
            );
            resume(&tx, checkpoint)?
        }
        Some(checkpoint) => checkpoint_routing(checkpoint)?,
    };
    let entries = if ids.is_empty() && checkpoint.is_none() {
        Vec::new()
    } else {
        Store::claim_user_turn_on(
            &tx,
            bot,
            &record.conversation_id,
            checkpoint,
            ids,
            expected_session,
            &ticket.execution_token,
        )?
    };
    crate::fork::memory_auto::admission::after_claim(&tx, memory, &entries)?;
    value["entryIds"] = json!(ids);
    // Mesmo escritor e mesma Tx: a classificação nasce junto com `permitted` e sai junto no rollback.
    value["routing"] = serde_json::to_value(&routing)?;
    value["phase"] = json!("permitted");
    value["permittedAt"] = json!(now_ms());
    value["pauseRevision"] = json!(pause.revision);
    write(&tx, &key(bot), &value)?;
    tx.commit()?;
    Ok(Some(entries))
}

/// Migração de boot (D305), antes de qualquer ator, numa transação IMMEDIATE: todo `turn.inflight.*` e
/// `turn.waiting.*` que é objeto JSON sem `routing` foi gravado antes do primeiro binário com o gravador ligado, logo
/// é pré-Roteamento por construção, e recebe `notRouted{preRoteamento}`; a flag `LEGACY_MARKED` é gravada na mesma
/// transação. O inflight numérico histórico (`started` em ms) vira o objeto legado equivalente
/// `{started, conversationId: <bot>, routing}`, com o mesmo `started` e o mesmo chat que o leitor atual deduz.
/// Idempotente: com a flag presente, não remarca nada, e um registro novo sem `routing` pausa. Valor vazio (`""`, "sem
/// turno") ou que não é número nem objeto JSON (corrompido) fica intocado e não vira legado; erro de DB desfaz tudo,
/// sem flag parcial. Conserva todos os campos originais.
pub(crate) fn mark_pre_routing(store: &Store) -> Result<usize> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if read(&tx, LEGACY_MARKED)?.is_some() {
        return Ok(0);
    }
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT k, v FROM kv WHERE k GLOB 'turn.inflight.*' OR k GLOB 'turn.waiting.*' ORDER BY k")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mark = serde_json::to_value(RoutingClass::NotRouted { reason: NotRoutedReason::PreRoteamento })?;
    let mut marked = 0;
    for (k, raw) in rows {
        // Formato histórico numérico do inflight (`started` em ms, chat = o próprio bot), lido hoje por
        // `fork::spaces::inflight` com `raw.parse::<i64>()`: normalizado para o objeto legado equivalente.
        if let (Some(bot), Ok(started)) = (k.strip_prefix("turn.inflight."), raw.parse::<i64>()) {
            write(&tx, &k, &json!({"started": started, "conversationId": bot, "routing": mark}))?;
            marked += 1;
            continue;
        }
        let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(&raw) else { continue };
        if fields.contains_key("routing") {
            continue;
        }
        fields.insert("routing".into(), mark.clone());
        write(&tx, &k, &Value::Object(fields))?;
        marked += 1;
    }
    write(&tx, LEGACY_MARKED, &json!({"version":1,"markedAt":now_ms(),"marked":marked}))?;
    tx.commit()?;
    Ok(marked)
}

/// Parada processada pelo ator, no registro canônico e com o Ticket original.
pub(crate) fn request_stop(store: &Store, bot: &str, ticket: &Ticket) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let (_, mut value) = captured(&tx, bot, ticket)?;
    value["stopRequested"] = json!(true);
    write(&tx, &key(bot), &value)?;
    tx.commit()?;
    Ok(())
}

/// Só a mesma tentativa pode liberar o registro, inclusive na falha anterior ao
/// envio. Não aceita booleano de 'atual', nem apaga evidência de um sucessor.
pub(crate) fn finish(store: &Store, bot: &str, ticket: &Ticket) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    if read(&tx, &key(bot))?.is_none() {
        return Ok(());
    }
    captured(&tx, bot, ticket)?;
    tx.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    tx.commit()?;
    Ok(())
}

/// Retry de setup só libera reserva comprovadamente anterior à permissão.
/// Uma permissão ou captura divergente permanece intacta, sem novo prompt.
pub(crate) fn release_reserved(store: &Store, bot: &str, ticket: &Ticket) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let (record, _) = captured(&tx, bot, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    tx.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    tx.commit()?;
    Ok(())
}

/// Limpeza de compatibilidade nunca remove registro novo, mesmo com tombstone
/// D72 antigo. Os campos opcionais do registro novo permanecem intocados.
pub(crate) fn clear_legacy_on(c: &Connection, bot: &str) -> Result<()> {
    if let Some(raw) = read(c, &key(bot))?
        && decode(&raw)?.is_none()
    {
        c.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    }
    Ok(())
}

/// Somente a captura legada alcançada pela recuperação/parada pode ser limpa.
pub(crate) fn clear_legacy_captured(store: &Store, bot: &str, raw: &str) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    ensure!(read(&tx, &key(bot))?.as_deref() == Some(raw) && decode(raw)?.is_none(), "admission_dispatch_conflict");
    tx.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn routine_uncertain(store: &Store, bot: &str, id: &str) -> Result<bool> {
    Ok(current(store, bot)?
        .is_some_and(|d| d.phase == Phase::Permitted && d.origin == (Origin::Routine { id: id.into() })))
}

#[cfg(test)]
pub(crate) fn set_paused_for_test(store: &Store, bot: &str, paused: bool) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let state = pause_in(&tx, bot)?;
    if state.paused != paused {
        write(&tx, &pause_key(bot), &json!({"paused":paused,"revision":state.revision+1,"changedAt":now_ms()}))?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests;
