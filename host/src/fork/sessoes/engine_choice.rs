//! `donna_bot_sessions.engine_choice`: a seleção de motor própria de cada conversa do Pessoal (D372). Só Sessões
//! migra, lê e escreve esta coluna, sempre na `Connection` recebida (sem Store, Hub, rede ou transação aninhada).
//!
//! - **Nascimento:** a principal (na criação do bot) e cada extra (no `sessionCreate`) gravam um retrato do padrão do
//!   bot naquele instante, `source: botDefault`. É retrato, não vínculo vivo: mudar o padrão do bot depois só vale
//!   para conversas novas.
//! - **Efetiva × próxima:** `engine_choice` é a seleção EFETIVA, a que os turnos executam; `engine_next` é a PRÓXIMA,
//!   gravada só por `sessionEngineSet` (`source: own`, CAS na revisão da sessão, [`set`]). A próxima vira efetiva só no
//!   início de um turno novo ([`promote`]), nunca durante um turno, uma espera de limite ou uma retomada: a efetiva
//!   não muda enquanto um turno a usa, e as guardas de execução comparam com ela. O app vê `próxima ?? efetiva`.
//! - **Migração única** ([`migrate`]): conversas existentes do Pessoal ganham o estado vigente (escolha já fixada pelo
//!   PR4, ou o padrão atual do bot), sem inventar conta/modelo; uma escolha ilegível continua ilegível.
//! - **Fora do escopo:** principal de canal e de conversa de Space ficam NULL e seguem a fonte canônica deles.
//! - **Comando próprio** (`command`) é configuração de execução do bot, não parte da seleção: a conversa guarda e
//!   aplica conta/motor/modelo/opções/velocidade como qualquer outra; o comando continua o do bot.
//!
//! `accountId`/`model` `null` representam ausência real (conta legada sem cadastro, modelo padrão do motor); não
//! tornam a combinação executável. A coluna não concede autoridade nem guarda a revisão da conta, que pertence ao
//! carimbo e ao recibo do Roteamento; a execução continua pela validação canônica.
use crate::store::{BotConfig, next_rev};
use anyhow::{Result, anyhow, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Marcador da migração única D372, gravado na mesma transação.
const MIGRATED: &str = "sessoes.engine.v2";

/// De onde veio a seleção: retrato do padrão no nascimento ou troca explícita nesta conversa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EngineSource {
    BotDefault,
    Own,
}

/// `{schemaVersion: 2, source, accountId, backend, model, modelOptions, fastMode}`, sem chave extra.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionEngine {
    pub schema_version: u8,
    pub source: EngineSource,
    pub account_id: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub model_options: BTreeMap<String, String>,
    pub fast_mode: bool,
}

/// A projeção pública (`session.engine`): os mesmos campos, mais a revisão da sessão usada no CAS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EngineView {
    pub account_id: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub model_options: BTreeMap<String, String>,
    pub fast_mode: bool,
    pub source: EngineSource,
    pub revision: i64,
}

/// O pedido de troca (`sessionEngineSet.engine`): o cliente nunca escolhe `source` nem revisão. Todas as chaves são
/// obrigatórias (`null` explícito só em `accountId`/`model`); a presença é conferida em [`ENGINE_KEYS`] antes do parse.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EngineRequest {
    pub account_id: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub model_options: BTreeMap<String, String>,
    pub fast_mode: bool,
}

/// As chaves que `sessionEngineSet.engine` precisa trazer, mesmo quando o valor é `null`.
pub(crate) const ENGINE_KEYS: [&str; 5] = ["accountId", "backend", "model", "modelOptions", "fastMode"];

/// O formato do PR4 (`schemaVersion: 1`), lido só pela migração única.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyChoice {
    schema_version: u8,
    account_id: String,
    selection: crate::fork::spaces::EngineSelection,
}

fn account_id_ok(id: &str) -> bool {
    id.len() == 67 && id.starts_with("ac_") && id[3..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Motor vazio só representa o padrão legado sem motor escolhido (sem conta); nunca é executável por si.
fn backend_ok(value: &str) -> bool {
    value.len() <= 40 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Texto de modelo/opção como o padrão do bot o aceita: sem controle, curto e não vazio.
fn text_ok(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= 200 && !value.chars().any(char::is_control)
}

impl SessionEngine {
    fn check(&self) -> Result<()> {
        ensure!(
            self.schema_version == 2
                && self.account_id.as_deref().is_none_or(account_id_ok)
                && backend_ok(&self.backend)
                && self.model.as_deref().is_none_or(text_ok)
                && self.model_options.len() <= 16
                && self.model_options.iter().all(|(k, v)| text_ok(k) && text_ok(v))
                && (self.account_id.is_none() || matches!(self.backend.as_str(), "codex" | "claude")),
            "engine_choice_invalid"
        );
        Ok(())
    }

    /// Retrato do padrão do bot, sem completar nada.
    pub(crate) fn snapshot(cfg: &BotConfig) -> Self {
        Self {
            schema_version: 2,
            source: EngineSource::BotDefault,
            account_id: cfg.fork_editor.engine_account_id.clone(),
            backend: cfg.backend.clone(),
            model: cfg.model.clone(),
            model_options: cfg.model_options.clone(),
            fast_mode: cfg.fast_mode,
        }
    }

    /// A seleção no formato do Roteamento (motor, modelo, opções), sem conta nem velocidade.
    pub(crate) fn selection(&self) -> crate::fork::spaces::EngineSelection {
        crate::fork::spaces::EngineSelection {
            backend: self.backend.clone(),
            model: self.model.clone(),
            model_options: self.model_options.clone(),
        }
    }

    /// A configuração `cfg` já executa exatamente esta seleção?
    pub(crate) fn runs_in(&self, cfg: &BotConfig) -> bool {
        self.account_id == cfg.fork_editor.engine_account_id
            && self.backend == cfg.backend
            && self.model == cfg.model
            && self.model_options == cfg.model_options
            && self.fast_mode == cfg.fast_mode
    }

    /// Aplica a seleção à configuração de execução.
    pub(crate) fn apply(&self, cfg: &mut BotConfig) {
        cfg.backend.clone_from(&self.backend);
        cfg.model.clone_from(&self.model);
        cfg.model_options.clone_from(&self.model_options);
        cfg.fast_mode = self.fast_mode;
        cfg.fork_editor.engine_account_id.clone_from(&self.account_id);
    }

    pub(crate) fn view(&self, revision: i64) -> EngineView {
        EngineView {
            account_id: self.account_id.clone(),
            backend: self.backend.clone(),
            model: self.model.clone(),
            model_options: self.model_options.clone(),
            fast_mode: self.fast_mode,
            source: self.source,
            revision,
        }
    }
}

/// Cria as colunas (NULL em todas as linhas). Chamada antes de qualquer linha nova, para o retrato na criação.
pub(crate) fn add_column(c: &Connection) -> Result<()> {
    for column in ["engine_choice", "engine_next"] {
        let has: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_bot_sessions') WHERE name=?1)",
            [column],
            |r| r.get(0),
        )?;
        if !has {
            c.execute(&format!("ALTER TABLE donna_bot_sessions ADD COLUMN {column} TEXT"), [])?;
        }
    }
    Ok(())
}

/// Migração única D372, na transação do chamador. Para cada conversa do Pessoal (bot agente vivo): NULL ganha o
/// retrato do padrão atual do bot (o que a principal e a extra ainda sem envio usariam); a escolha fixada do PR4
/// vira v2 com os mesmos valores e a velocidade vigente do bot. Ilegível fica como está (falha fechado ao ler);
/// canal, Space e bot ilegível ficam NULL. Não revisa nem republica a sessão: o estado vigente não muda.
pub(crate) fn migrate(c: &Connection) -> Result<()> {
    add_column(c)?;
    let done: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [MIGRATED], |r| r.get(0))?;
    if done {
        return Ok(());
    }
    let rows: Vec<(String, String, Option<String>)> = c
        .prepare("SELECT id, chat_id, engine_choice FROM donna_bot_sessions")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, chat, raw) in rows {
        let Some(cfg) = personal_bot(c, &chat)? else { continue };
        let engine = match raw.as_deref() {
            None => SessionEngine::snapshot(&cfg),
            Some(raw) => match serde_json::from_str::<LegacyChoice>(raw) {
                Ok(old) if old.schema_version == 1 => SessionEngine {
                    schema_version: 2,
                    source: EngineSource::BotDefault,
                    account_id: Some(old.account_id),
                    backend: old.selection.backend,
                    model: old.selection.model,
                    model_options: old.selection.model_options,
                    fast_mode: cfg.fast_mode,
                },
                // v2 já gravado ou ilegível: não reescreve nem transforma erro em padrão.
                _ => continue,
            },
        };
        if engine.check().is_err() {
            // Padrão do bot fora do formato canônico: NULL continua NULL (falha fechado na extra).
            tracing::warn!(session = %id, "seleção da conversa não migrada: padrão do bot fora do formato");
            continue;
        }
        let raw = serde_json::to_string(&engine)?;
        c.execute("UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1", params![id, raw])?;
    }
    c.execute("INSERT INTO kv(k,v) VALUES(?1,'1')", [MIGRATED])?;
    Ok(())
}

/// Os bots vivos cujas conversas usam a conta `account` na seleção EFETIVA ou na PRÓXIMA (D372), lidos só nesta
/// `Connection`. Seleção ilegível falha (nunca vira ausência): remoção e setup da conta não podem ignorar uso.
///
/// `with_fallback = false` é o modo **vínculo**: a conversa que roda na conta só por causa do fallback não conta como uso.
pub(crate) fn bots_using_account(c: &Connection, account: &str, with_fallback: bool) -> Result<Vec<String>> {
    let rows: Vec<(String, Option<String>, Option<String>)> = c
        .prepare(
            "SELECT s.chat_id, s.engine_choice, s.engine_next FROM donna_bot_sessions s
             JOIN bots b ON b.id=s.chat_id AND b.deleted=0
             WHERE s.engine_choice IS NOT NULL OR s.engine_next IS NOT NULL",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut bots = Vec::new();
    for (chat, effective, next) in rows {
        for raw in [effective, next].into_iter().flatten() {
            if parse(&raw)?.account_id.as_deref() == Some(account) {
                bots.push(chat.clone());
            }
        }
    }
    // F6-B: uma conversa que roda na conta por causa do fallback também a usa (remoção e setup recusam).
    for session in crate::fork::fallback_contas::estado::sessoes_em(c, account)?.into_iter().filter(|_| with_fallback) {
        if let Some(chat) = c
            .query_row("SELECT chat_id FROM donna_bot_sessions WHERE id=?1", [&session], |r| r.get::<_, String>(0))
            .optional()?
        {
            bots.push(chat);
        }
    }
    bots.sort();
    bots.dedup();
    Ok(bots)
}

/// A configuração do bot agente vivo do Pessoal dono deste chat; canal, Space ou bot ilegível → `None`.
/// Usa o leitor tolerante de bots (sem `json_extract` no SQL): uma config legada ilegível só pula o retrato e
/// nunca impede abrir o banco. A leitura da SELEÇÃO da conversa continua estrita (`parse`).
pub(crate) fn personal_bot(c: &Connection, chat: &str) -> Result<Option<BotConfig>> {
    Ok(crate::store::Store::bot_on(c, chat)?
        .filter(|b| !b.deleted)
        .map(|b| b.config)
        .filter(|cfg| !cfg.is_group() && cfg.space_id == crate::fork::spaces::PERSONAL))
}

/// Grava o retrato do padrão numa sessão recém-criada do Pessoal (principal ou extra), na transação do chamador.
/// Fora do Pessoal não grava. Padrão fora do formato recusa a criação da extra; na principal, fica NULL e ela segue
/// a fonte canônica.
pub(crate) fn snapshot_on_create(c: &Connection, session: &str, chat: &str, principal: bool) -> Result<()> {
    let Some(cfg) = personal_bot(c, chat)? else { return Ok(()) };
    let engine = SessionEngine::snapshot(&cfg);
    if let Err(error) = engine.check() {
        if principal {
            tracing::warn!(%chat, "principal sem retrato: padrão do bot fora do formato");
            return Ok(());
        }
        return Err(error.context("engine_unavailable"));
    }
    c.execute(
        "UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1 AND engine_choice IS NULL",
        params![session, serde_json::to_string(&engine)?],
    )?;
    Ok(())
}

/// Lê a coluna. Versão desconhecida, chave extra, seleção inválida ou JSON ilegível falham; nunca viram padrão.
pub(crate) fn parse(raw: &str) -> Result<SessionEngine> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| anyhow!("engine_choice_invalid"))?;
    let engine: SessionEngine = serde_json::from_value(value.clone()).map_err(|_| anyhow!("engine_choice_invalid"))?;
    ensure!(serde_json::to_value(&engine)? == value, "engine_choice_invalid");
    engine.check()?;
    Ok(engine)
}

/// A seleção EFETIVA da sessão (`None` = NULL), a que os turnos executam. Sessão inexistente falha. Só leitura.
pub(crate) fn read(c: &Connection, session: &str) -> Result<Option<SessionEngine>> {
    let raw = read_own(c, session)?;
    // F6-B: durante o fallback a conversa roda na conta reserva; a escolha do dono, gravada, não muda.
    raw.map(|engine| overlay(c, session, engine)).transpose()
}

/// A escolha do dono, sem a sobreposição do fallback.
pub(crate) fn read_own(c: &Connection, session: &str) -> Result<Option<SessionEngine>> {
    let raw: Option<String> = c
        .query_row("SELECT engine_choice FROM donna_bot_sessions WHERE id=?1", [session], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("unknown_session"))?;
    raw.as_deref().map(parse).transpose()
}

/// A sobreposição só vale enquanto a conta de origem dela ainda é a escolha do dono; se o dono trocou, a velha cai.
fn overlay(c: &Connection, session: &str, mut engine: SessionEngine) -> Result<SessionEngine> {
    if let Some(over) = crate::fork::fallback_contas::estado::sobreposicao(c, session)?
        && engine.account_id.as_deref() == Some(over.origem.as_str())
    {
        engine.account_id = Some(over.atual);
    }
    Ok(engine)
}

/// A seleção CONFIGURADA para os próximos turnos: a próxima, se houver, senão a efetiva. Qualquer uma ilegível falha.
pub(crate) fn configured(c: &Connection, session: &str) -> Result<Option<SessionEngine>> {
    let (effective, next): (Option<String>, Option<String>) = c
        .query_row("SELECT engine_choice, engine_next FROM donna_bot_sessions WHERE id=?1", [session], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or_else(|| anyhow!("unknown_session"))?;
    let effective = effective.as_deref().map(parse).transpose()?;
    Ok(next.as_deref().map(parse).transpose()?.or(effective))
}

/// Início de um turno NOVO na sessão (nunca espera, retomada ou reconfiguração): a próxima vira a efetiva, num só
/// comando na `Connection` recebida, sem mexer na revisão (o app já via a próxima). Próxima ilegível falha fechado
/// sem promover. Devolve se promoveu.
pub(crate) fn promote(c: &Connection, session: &str) -> Result<bool> {
    let next: Option<String> = c
        .query_row("SELECT engine_next FROM donna_bot_sessions WHERE id=?1", [session], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("unknown_session"))?;
    let Some(raw) = next else { return Ok(false) };
    parse(&raw)?;
    let changed = c.execute(
        "UPDATE donna_bot_sessions SET engine_choice=engine_next, engine_next=NULL WHERE id=?1 AND engine_next=?2",
        params![session, raw],
    )?;
    Ok(changed == 1)
}

/// Como a troca muda a conversa: só o modelo/opções, ou conta/motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SwitchKind {
    Model,
    Provider,
}

pub(crate) fn switch_kind(from: Option<&SessionEngine>, to: &SessionEngine) -> SwitchKind {
    match from {
        Some(from) if from.account_id == to.account_id && from.backend == to.backend => SwitchKind::Model,
        _ => SwitchKind::Provider,
    }
}

/// Escritor único da troca (`sessionEngineSet`), por CAS na revisão da sessão, na transação da guarda do dono.
/// Só conversa do Pessoal aberta. Dois caminhos, ambos `source: own`:
///
/// - **Gerenciado** (`accountId` presente): sem comando próprio; a conta passa pela validação canônica
///   (`validate_selection_in`) com o cadastro lido nesta mesma `Connection`.
/// - **Legado estreito** (`accountId: null`): só na principal cuja seleção já é sem conta gerenciada (CLI ou comando
///   próprio), com o MESMO motor. Muda apenas modelo, opções e velocidade, pelas regras do editor do bot
///   (`bot_editor::checked_model*`). Não concede conta, não retira conta gerenciada, não troca motor e não torna o
///   turno roteado; extras continuam só gerenciadas.
///
/// Grava só a PRÓXIMA (`engine_next`): a efetiva, que o turno em andamento (inclusive o que espera o limite) usa e as
/// guardas conferem, não muda aqui. Não toca o padrão do bot, outras sessões nem vínculos. Devolve a configurada
/// anterior (ilegível = `None`) e a gravada.
pub(crate) fn set(
    c: &Connection,
    chat: &str,
    session: &str,
    expected_revision: i64,
    request: &EngineRequest,
) -> Result<(Option<SessionEngine>, SessionEngine)> {
    ensure!(!c.is_autocommit(), "engine_choice_transaction_required");
    let row = super::store::get(c, session)?.filter(|s| s.chat_id == chat).ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(row.state == "open", "session_closed");
    if row.revision != expected_revision {
        return Err(crate::remote::cloud::Conflict("session_revision_conflict").into());
    }
    let mut cfg = personal_bot(c, chat)?.ok_or_else(|| anyhow!("engine_not_allowed"))?;
    let current = configured(c, session);
    let engine = SessionEngine {
        schema_version: 2,
        source: EngineSource::Own,
        account_id: request.account_id.clone(),
        backend: request.backend.clone(),
        model: request.model.clone(),
        model_options: request.model_options.clone(),
        fast_mode: request.fast_mode,
    };
    engine.check().map_err(|_| anyhow!("engine_not_allowed"))?;
    crate::agent::backends::require_supported_execution(&engine.backend).map_err(|_| anyhow!("engine_not_allowed"))?;
    // Motor retirado da oferta: a conversa não troca para ele; a que já roda num (bot antigo) segue igual.
    if crate::fork::motores_retirados::retirado(&engine.backend)
        && current.as_ref().ok().and_then(|c| c.as_ref()).is_none_or(|antes| antes.backend != engine.backend)
    {
        bail!("engine_not_allowed");
    }
    if engine.account_id.is_some() {
        // Sem validador canônico de conta gerenciada com comando próprio: a troca não está disponível.
        ensure!(cfg.command.is_none(), "engine_unavailable");
        engine.apply(&mut cfg);
        let registry = crate::fork::engine_accounts::Registry::load_db(c).map_err(|_| anyhow!("engine_unavailable"))?;
        if let Err(error) = crate::fork::engine_accounts::managed::validate_selection_in(&registry, &cfg) {
            // Só o código vai ao app; nada do cadastro.
            if format!("{error:#}").starts_with("account_incompatible") {
                bail!("engine_not_allowed");
            }
            bail!("engine_unavailable");
        }
    } else {
        // Legado estreito: a seleção atual precisa ser legível, sem conta e no mesmo motor, na principal.
        let current = current.as_ref().ok().and_then(Option::as_ref).ok_or_else(|| anyhow!("engine_unavailable"))?;
        ensure!(
            row.principal && current.account_id.is_none() && current.backend == engine.backend,
            "engine_not_allowed"
        );
        crate::fork::bot_editor::checked_model(engine.model.as_deref()).map_err(|_| anyhow!("engine_not_allowed"))?;
        // Sem modelo explícito, opções não teriam a quem se aplicar: recusa em vez de ignorá-las na execução.
        ensure!(engine.model.is_some() || engine.model_options.is_empty(), "engine_not_allowed");
        crate::fork::bot_editor::checked_model_options(&engine.model_options)
            .map_err(|_| anyhow!("engine_not_allowed"))?;
    }
    // Ilegível não é anterior conhecida, mas a troca explícita e validada pode substituí-la (só no caminho gerenciado).
    let from = current.ok().flatten();
    let changed = c.execute(
        "UPDATE donna_bot_sessions SET engine_next=?3, revision=?4 WHERE id=?1 AND revision=?2 AND state='open'",
        params![session, expected_revision, serde_json::to_string(&engine)?, next_rev(c)?],
    )?;
    if changed != 1 {
        return Err(crate::remote::cloud::Conflict("session_revision_conflict").into());
    }
    Ok((from, engine))
}
