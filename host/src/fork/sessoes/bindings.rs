//! Identidade de sessão emitida pelo Hub. Guardas SQL são síncronas e não voltam ao Store.
use super::store::SessionRef;
use crate::{LockExt, hub::Hub, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(crate) const ENV: &str = "DONNABOT_SESSION_BINDING";
const STALE: &str = "chamada vencida: a sessão de origem não está em andamento";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BindingId(String);
impl BindingId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TurnToken(String);

/// Só este módulo emite os identificadores. A variante Hub será acrescentada pela D246;
/// consumidores devem conferir pela função, sem fabricar uma ACP para turnos do Hub.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum ExecutionRef {
    Acp {
        binding_id: BindingId,
        turn_token: TurnToken,
        tool_fence: Option<Box<crate::fork::spaces::tool_context::ToolFence>>,
    },
}
impl ExecutionRef {
    pub(crate) fn bind_tool_fence(&mut self, fence: crate::fork::spaces::tool_context::ToolFence) {
        match self {
            Self::Acp { tool_fence, .. } => *tool_fence = Some(Box::new(fence)),
        }
    }

    /// Token por execução já emitido pelo Hub; pode identificar uma futura cerca de parada.
    pub(crate) fn turn_token(&self) -> &str {
        match self {
            Self::Acp { turn_token, .. } => &turn_token.0,
        }
    }

    #[allow(clippy::unnecessary_wraps, reason = "a variante Hub da D246 não terá vínculo ACP")]
    pub(crate) fn acp_binding(&self) -> Option<&str> {
        match self {
            Self::Acp { binding_id, .. } => Some(binding_id.as_str()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BindingRef {
    pub id: BindingId,
    pub bot: String,
    pub session_ref: SessionRef,
    pub acp_session: Option<String>,
    /// Slot de execução do produtor (`None` = sessão principal da conversa; `session:<id>` = extra; rotina, canal e
    /// Space têm o próprio). D372: só a principal e a extra seguem a seleção própria da conversa.
    pub slot: Option<String>,
}

/// `donna_bindings_account` é recriado a cada migração (substitui a definição instalada): a troca de conta ou motor
/// do bot revoga os vínculos dele, menos os da principal e das extras com seleção própria (D372), cuja execução não
/// depende do padrão do bot; a ativação confere a conta delas. Vínculos de rotina e canal (`slot` próprio), mesmo na
/// sessão da principal, seguem o padrão e são revogados como antes. Space como antes.
///
/// `donna_bindings_session_engine` (também recriado) é a revogação verdadeira da conversa: quando a seleção EFETIVA
/// (`engine_choice`, nunca `engine_next`) muda de motor ou de conta no mesmo comando que a grava (promoção), os
/// vínculos da própria principal ou extra (`slot` NULL ou `session:`) são revogados; rotina, canal e Space não.
/// Só modelo/opções ou a migração v1→v2 com os mesmos valores não revogam. A normalização v1/v2 com `json_valid`
/// serve só para invalidar (JSON ilegível nunca quebra o UPDATE; de ou para ilegível ou NULL revoga): não valida
/// seleção, cuja leitura canônica (`engine_choice::parse`) continua estrita e falha fechado.
pub(super) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_session_bindings(
        id TEXT PRIMARY KEY, bot TEXT NOT NULL, chat TEXT NOT NULL, session TEXT NOT NULL,
        acp_session TEXT, revoked_at INTEGER);",
    )?;
    let has_slot: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_session_bindings') WHERE name='slot')",
        [],
        |r| r.get(0),
    )?;
    if !has_slot {
        c.execute("ALTER TABLE donna_session_bindings ADD COLUMN slot TEXT", [])?;
    }
    let has_backend: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_session_bindings') WHERE name='backend')",
        [],
        |r| r.get(0),
    )?;
    if !has_backend {
        c.execute("ALTER TABLE donna_session_bindings ADD COLUMN backend TEXT", [])?;
    }
    c.execute_batch("CREATE TABLE IF NOT EXISTS donna_session_bindings(
        id TEXT PRIMARY KEY, bot TEXT NOT NULL, chat TEXT NOT NULL, session TEXT NOT NULL,
        acp_session TEXT, revoked_at INTEGER);
        CREATE INDEX IF NOT EXISTS donna_session_bindings_acp ON donna_session_bindings(bot,acp_session);
        CREATE TABLE IF NOT EXISTS donna_session_gateway_pending(
            bot TEXT NOT NULL,binding TEXT NOT NULL,at INTEGER NOT NULL,PRIMARY KEY(bot,binding));
        CREATE TRIGGER IF NOT EXISTS donna_bindings_close AFTER UPDATE OF state ON donna_bot_sessions
        WHEN NEW.state <> 'open' BEGIN
            UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,strftime('%s','now')*1000)
            WHERE session=NEW.id;
        END;
        CREATE TRIGGER IF NOT EXISTS donna_bindings_delete AFTER UPDATE OF deleted ON bots
        WHEN NEW.deleted<>0 BEGIN
            UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,strftime('%s','now')*1000)
            WHERE bot=NEW.id;
        END;
        DROP TRIGGER IF EXISTS donna_bindings_account;
        CREATE TRIGGER donna_bindings_account AFTER UPDATE OF config ON bots
        WHEN json_extract(NEW.config,'$.forkEditor.engineAccountId') IS NOT json_extract(OLD.config,'$.forkEditor.engineAccountId')
            OR json_extract(NEW.config,'$.backend') IS NOT json_extract(OLD.config,'$.backend') BEGIN
            UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,strftime('%s','now')*1000) WHERE bot=NEW.id
                AND NOT ((donna_session_bindings.slot IS NULL OR donna_session_bindings.slot LIKE 'session:%')
                    AND EXISTS(SELECT 1 FROM donna_bot_sessions s WHERE s.id=donna_session_bindings.session
                        AND s.chat_id=donna_session_bindings.chat AND s.engine_choice IS NOT NULL));
        END;
        DROP TRIGGER IF EXISTS donna_bindings_session_engine;
        CREATE TRIGGER donna_bindings_session_engine AFTER UPDATE OF engine_choice ON donna_bot_sessions
        WHEN (CASE WHEN json_valid(NEW.engine_choice) THEN COALESCE(json_extract(NEW.engine_choice,'$.backend'),
                json_extract(NEW.engine_choice,'$.selection.backend')) END)
            IS NOT (CASE WHEN json_valid(OLD.engine_choice) THEN COALESCE(json_extract(OLD.engine_choice,'$.backend'),
                json_extract(OLD.engine_choice,'$.selection.backend')) END)
            OR (CASE WHEN json_valid(NEW.engine_choice) THEN json_extract(NEW.engine_choice,'$.accountId') END)
            IS NOT (CASE WHEN json_valid(OLD.engine_choice) THEN json_extract(OLD.engine_choice,'$.accountId') END)
        BEGIN
            UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,strftime('%s','now')*1000)
            WHERE session=NEW.id AND chat=NEW.chat_id AND (slot IS NULL OR slot LIKE 'session:%');
        END;
        CREATE TRIGGER IF NOT EXISTS donna_bindings_space_account AFTER UPDATE OF engine_account_id,closed ON donna_space_conversations
        WHEN NEW.engine_account_id IS NOT OLD.engine_account_id OR NEW.closed<>0 BEGIN
            UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,strftime('%s','now')*1000) WHERE chat=NEW.id;
        END;")?;
    Ok(())
}

/// Pode estar em abertura, antes de session/new devolver a ACP. Não concede autoridade de turno.
pub(crate) fn resolve(c: &Connection, id: &str, bot: &str) -> Result<BindingRef> {
    c.query_row(
        "SELECT b.chat,b.session,b.acp_session,b.slot FROM donna_session_bindings b
        JOIN donna_bot_sessions s ON s.id=b.session AND s.chat_id=b.chat AND s.state='open'
        JOIN bots owner ON owner.id=b.bot AND owner.deleted=0
        WHERE b.id=?1 AND b.bot=?2 AND b.revoked_at IS NULL",
        params![id, bot],
        |r| {
            Ok(BindingRef {
                id: BindingId(id.into()),
                bot: bot.into(),
                session_ref: SessionRef { chat_id: r.get(0)?, id: r.get(1)? },
                acp_session: r.get(2)?,
                slot: r.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!(STALE))
}

/// Na mesma transação IMMEDIATE que grava. Usa só a Connection recebida; nunca Store/async.
pub(crate) fn assert_binding_live(c: &Connection, binding: &BindingRef) -> Result<BindingRef> {
    let live = resolve(c, binding.id.as_str(), &binding.bot)?;
    ensure!(live.session_ref == binding.session_ref && live.acp_session == binding.acp_session, STALE);
    let current: Option<String> =
        c.query_row("SELECT v FROM kv WHERE k=?1", [active_key(&live.bot)], |r| r.get(0)).optional()?;
    ensure!(current.as_deref() == Some(live.id.as_str()) && live.acp_session.is_some(), STALE);
    Ok(live)
}

/// Devolve a `SessionRef` comum às execuções ACP e, no futuro, Hub; não fabrica `BindingRef` para Hub.
pub(crate) fn assert_execution_live(c: &Connection, execution: &ExecutionRef) -> Result<SessionRef> {
    Ok(acp_execution_live(c, execution)?.session_ref)
}

fn acp_execution_live(c: &Connection, execution: &ExecutionRef) -> Result<BindingRef> {
    match execution {
        ExecutionRef::Acp { binding_id, tool_fence, .. } => {
            if let Some(fence) = tool_fence {
                fence.check(c)?;
            }
            // O token é do turno capturado pelo Hub, não um argumento exigido do motor (D229 A).
            ensure!(!execution.turn_token().is_empty(), STALE);
            let bot: String = c
                .query_row("SELECT bot FROM donna_session_bindings WHERE id=?1", [binding_id.as_str()], |r| r.get(0))
                .optional()?
                .ok_or_else(|| anyhow!(STALE))?;
            let binding = resolve(c, binding_id.as_str(), &bot)?;
            assert_binding_live(c, &binding)
        }
    }
}

fn active_key(bot: &str) -> String {
    format!("session.binding.active.{bot}")
}

/// Atalho dos testes para o vínculo da sessão principal (`slot` NULL).
#[cfg(test)]
pub(crate) fn issue(store: &Store, bot: &str, chat: &str, session: &str, acp: Option<&str>) -> Result<BindingRef> {
    issue_in(store, bot, chat, session, None, acp)
}

/// Emite o vínculo com o slot de execução do produtor (`None` = sessão principal da conversa).
pub(crate) fn issue_in(
    store: &Store,
    bot: &str,
    chat: &str,
    session: &str,
    slot: Option<&str>,
    acp: Option<&str>,
) -> Result<BindingRef> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE donna_session_bindings SET revoked_at=?2 WHERE bot=?1 AND acp_session IS NULL AND revoked_at IS NULL",
        params![bot, crate::store::now_ms()],
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO donna_session_bindings(id,bot,chat,session,acp_session,slot) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id, bot, chat, session, acp, slot],
    )?;
    let binding = resolve(&tx, &id, bot)?;
    tx.commit()?;
    Ok(binding)
}

#[cfg(test)]
pub(crate) fn activate(store: &Store, binding: &BindingRef, acp: &str) -> Result<()> {
    activate_inner(store, binding, acp, None)
}

pub(crate) fn activate_for_config(
    store: &Store,
    binding: &BindingRef,
    acp: &str,
    cfg: &crate::store::BotConfig,
) -> Result<()> {
    activate_inner(store, binding, acp, Some(cfg))
}

fn activate_inner(
    store: &Store,
    binding: &BindingRef,
    acp: &str,
    expected: Option<&crate::store::BotConfig>,
) -> Result<()> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let live = resolve(&tx, binding.id.as_str(), &binding.bot)?;
    if let Some(cfg) = expected {
        ensure!(cfg.id == binding.bot, STALE);
        if let Some(context) = &cfg.context {
            let account: Option<String> = tx.query_row(
                "SELECT engine_account_id FROM donna_space_conversations WHERE id=?1 AND closed=0",
                [&context.conversation_id],
                |row| row.get(0),
            )?;
            ensure!(account == cfg.fork_editor.engine_account_id, STALE);
        } else if let Some(engine) = own_engine(&tx, binding)? {
            // Conversa com seleção própria (D372): a execução segue a seleção EFETIVA da sessão (imutável durante o
            // turno; a troca do dono fica na próxima), não o padrão atual do bot. A conta precisa continuar válida no
            // cadastro desta transação. Sem conta, só a principal legada.
            ensure!(cfg.backend == engine.backend && cfg.fork_editor.engine_account_id == engine.account_id, STALE);
            if cfg.fork_editor.engine_account_id.is_some() {
                crate::fork::engine_accounts::managed::validate_selection_in(
                    &crate::fork::engine_accounts::Registry::load_db(&tx)?,
                    cfg,
                )?;
            } else {
                ensure!(binding.session_ref.id == binding.session_ref.chat_id, STALE);
            }
        } else {
            let (backend,account):(String,Option<String>)=tx.query_row(
                "SELECT COALESCE(json_extract(config,'$.backend'),''),json_extract(config,'$.forkEditor.engineAccountId') FROM bots WHERE id=?1 AND deleted=0",[&cfg.id],|row|Ok((row.get(0)?,row.get(1)?)))?;
            ensure!(backend == cfg.backend && account == cfg.fork_editor.engine_account_id, STALE);
        }
    }
    ensure!(
        live.session_ref == binding.session_ref && live.acp_session.as_deref().is_none_or(|current| current == acp),
        STALE
    );
    let collision:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM donna_session_bindings WHERE bot=?1 AND acp_session=?2 AND session<>?3 AND revoked_at IS NULL)",
        params![binding.bot,acp,binding.session_ref.id],|row|row.get(0))?;
    ensure!(!collision, "chamada vencida: ACP pertence a outra sessão");
    tx.execute(
        "UPDATE donna_session_bindings SET acp_session=?2 WHERE id=?1 AND acp_session IS NULL",
        params![binding.id.as_str(), acp],
    )?;
    if let Some(cfg) = expected {
        // O motor que a execução admitida usa neste vínculo, gravado pelo Hub na ativação: é ele que o porteiro das
        // ferramentas do Hub consulta, nunca o cadastro do bot, que pode mudar depois de o turno começar.
        tx.execute(
            "UPDATE donna_session_bindings SET backend=?2 WHERE id=?1",
            params![binding.id.as_str(), cfg.backend],
        )?;
    }
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![active_key(&binding.bot), binding.id.as_str()],
    )?;
    super::gateway::clear(&tx, &binding.bot)?;
    super::exposure::bind(&tx, &binding.bot, acp, &binding.session_ref.id)?;
    let pending: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_session_gateway_pending WHERE bot=?1 AND binding=?2)",
        params![binding.bot, binding.id.as_str()],
        |r| r.get(0),
    )?;
    if pending {
        mark(&tx, &binding.bot, acp, "gateway MCP")?;
    }
    tx.execute(
        "DELETE FROM donna_session_gateway_pending WHERE bot=?1 AND binding=?2",
        params![binding.bot, binding.id.as_str()],
    )?;
    tx.commit()?;
    Ok(())
}

/// A seleção EFETIVA da conversa do próprio vínculo (D372), lida estritamente nesta Connection, só no domínio dela:
/// sessão principal (`slot` nulo) ou extra. Rotina, canal e Space (slot próprio), sem linha ou NULL: `None` (guardas
/// originais, pelo padrão do bot ou pela conversa de Space). Ilegível falha fechado.
fn own_engine(c: &Connection, binding: &BindingRef) -> Result<Option<super::engine_choice::SessionEngine>> {
    if binding.slot.as_deref().is_some_and(|slot| !slot.starts_with("session:")) {
        return Ok(None);
    }
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_bot_sessions WHERE id=?1 AND chat_id=?2)",
        params![binding.session_ref.id, binding.session_ref.chat_id],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    super::engine_choice::read(c, &binding.session_ref.id)
}

pub(crate) fn for_acp(store: &Store, bot: &str, acp: &str) -> Result<BindingRef> {
    for_acp_on(&store.db.locked(), bot, acp)
}

/// Consulta pura para produtores que já conservam runtime/fonte/DB na ordem canônica.
pub(crate) fn for_acp_on(c: &Connection, bot: &str, acp: &str) -> Result<BindingRef> {
    let id:String=c.query_row("SELECT id FROM donna_session_bindings WHERE bot=?1 AND acp_session=?2 AND revoked_at IS NULL ORDER BY rowid DESC LIMIT 1",
        params![bot,acp],|r|r.get(0)).optional()?.ok_or_else(||anyhow!(STALE))?;
    resolve(c, &id, bot)
}

pub(crate) fn revoke(store: &Store, bot: &str, acp: Option<&str>) -> Result<()> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("UPDATE donna_session_bindings SET revoked_at=?3 WHERE bot=?1 AND (?2 IS NULL OR acp_session=?2) AND revoked_at IS NULL",
        params![bot,acp,crate::store::now_ms()])?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn capture(hub: &Hub, bot: &str, id: &str) -> Result<ExecutionRef> {
    let runtime = hub.runtime(bot);
    let c = hub.store.db.locked();
    let binding = resolve(&c, id, bot)?;
    assert_binding_live(&c, &binding)?;
    ensure!(
        matches!(runtime.status, crate::hub::BotStatus::Working | crate::hub::BotStatus::NeedsInput)
            && runtime.session == binding.acp_session
            && runtime.lane.as_ref().is_some_and(|lane| lane.chat == binding.session_ref.chat_id
                && lane.session.as_deref().unwrap_or(&lane.chat) == binding.session_ref.id),
        STALE
    );
    Ok(ExecutionRef::Acp {
        binding_id: binding.id,
        tool_fence: None,
        turn_token: TurnToken(runtime.memory_turn_token.ok_or_else(|| anyhow!(STALE))?),
    })
}

pub(crate) fn recheck(hub: &Hub, execution: &ExecutionRef) -> Result<SessionRef> {
    let binding = acp_execution_live(&hub.store.db.locked(), execution)?;
    capture(hub, &binding.bot, binding.id.as_str())?;
    Ok(binding.session_ref)
}

/// Como `recheck`, e ainda exige que o turno vigente seja o mesmo da execução capturada: um pedido atrasado de um turno
/// anterior no mesmo vínculo não vale.
pub(crate) fn recheck_turn(hub: &Hub, execution: &ExecutionRef) -> Result<SessionRef> {
    let binding = acp_execution_live(&hub.store.db.locked(), execution)?;
    let current = capture(hub, &binding.bot, binding.id.as_str())?;
    ensure!(current.turn_token() == execution.turn_token(), STALE);
    Ok(binding.session_ref)
}

/// O ator grava o motor da execução assim que determina o vínculo, antes de a sessão abrir: pontes que chamam o Hub
/// durante a abertura da sessão (a descoberta de ferramentas, o gateway dos conectores) já têm o motor registrado.
pub(crate) fn record_backend(store: &Store, binding: &BindingRef, backend: &str) -> Result<()> {
    store.db.locked().execute(
        "UPDATE donna_session_bindings SET backend=?2 WHERE id=?1 AND revoked_at IS NULL",
        params![binding.id.as_str(), backend],
    )?;
    Ok(())
}

/// O motor gravado pelo ator no vínculo vivo; `None` se o vínculo não existe, foi revogado ou nunca foi ativado pelo
/// ator (nesse caso ninguém prova qual motor roda).
pub(crate) fn backend(c: &Connection, binding: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT backend FROM donna_session_bindings WHERE id=?1 AND revoked_at IS NULL", [binding], |r| {
        r.get::<_, Option<String>>(0)
    })
    .optional()?
    .flatten())
}

pub(crate) fn mark(c: &Connection, bot: &str, acp: &str, reason: &str) -> Result<()> {
    c.execute("INSERT INTO donna_memory_sessions(bot,session,marked_at,reason) VALUES(?1,?2,?3,?4)
        ON CONFLICT(bot,session) DO UPDATE SET marked_at=COALESCE(marked_at,excluded.marked_at),reason=COALESCE(reason,excluded.reason)",
        params![bot,acp,crate::store::now_ms(),reason])?;
    Ok(())
}

/// Chamadores internos do Hub podem não ter ACP; um MCP nunca chega aqui sem `ExecutionRef`.
pub(crate) fn check_optional(c: &Connection, execution: Option<&ExecutionRef>) -> Result<()> {
    if let Some(execution) = execution {
        assert_execution_live(c, execution)?;
    }
    Ok(())
}
pub(crate) fn recheck_optional(hub: &Hub, execution: Option<&ExecutionRef>) -> Result<()> {
    if let Some(execution) = execution {
        recheck(hub, execution)?;
    }
    Ok(())
}

/// A admissão pertence ao processo do Hub; vínculos antigos conservam sua origem.
pub(crate) fn reset_runtime(store: &Store) -> Result<()> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("DELETE FROM kv WHERE k LIKE 'session.binding.active.%'", [])?;
    tx.execute("DELETE FROM kv WHERE k LIKE 'session.binding.preparing.%'", [])?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn deactivate(store: &Store, bot: &str) -> Result<()> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("DELETE FROM kv WHERE k=?1", [active_key(bot)])?;
    super::gateway::clear(&tx, bot)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) fn fixture_binding(store: &Store, bot: &str) -> BindingRef {
    let binding = for_acp(store, bot, "fixture-acp")
        .unwrap_or_else(|_| issue(store, bot, bot, bot, Some("fixture-acp")).unwrap());
    activate(store, &binding, "fixture-acp").unwrap();
    binding
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) fn fixture_execution(hub: &Hub, bot: &str) -> ExecutionRef {
    let binding = fixture_binding(&hub.store, bot);
    hub.set_runtime(bot, |r| {
        r.status = crate::hub::BotStatus::Working;
        r.lane = Some(crate::store::Lane::main(bot));
        r.session = Some("fixture-acp".into());
        r.memory_turn_token = Some("fixture-turn".into());
    });
    capture(hub, bot, binding.id.as_str()).unwrap()
}

/// Envoltório de escrita; o callback síncrono usa somente a Connection recebida.
pub(crate) fn write<T>(
    store: &Store,
    execution: Option<&ExecutionRef>,
    change: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check_optional(&tx, execution)?;
    let out = change(&tx)?;
    tx.commit()?;
    Ok(out)
}
