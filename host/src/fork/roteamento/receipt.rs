//! Gravador isolado real. Consumidor: `fork::sessoes::admissao` (PR4), no encaixe `ClaimHooks` da VG-03.
//! Dispatch/Resume e o CAS de `engine_choice` são de Sessões; este módulo só grava e lê o próprio checkpoint.
use super::{
    admission::{
        AdmissionCapture, CapturedText, CheckpointKey, DOCUMENT_MAX, DecidedBy, Destination, EntryEvidence,
        ExecutionSeal, Reason, SelectionSnapshot, StampCapture, UnattestedFacts, decode,
    },
    config::{self, Document, Policy, Reserve},
    selection,
};
use crate::{
    fork::{
        engine_accounts::{Registry, managed},
        sessoes,
        spaces::{self, EngineSelection},
    },
    store::BotConfig,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_routing_checkpoints(
            bot_id TEXT NOT NULL, space_id TEXT NOT NULL, chat_id TEXT NOT NULL,
            session_id TEXT NOT NULL, execution_token TEXT NOT NULL,
            checkpoint_json TEXT NOT NULL CHECK(length(CAST(checkpoint_json AS BLOB)) BETWEEN 2 AND 65536),
            PRIMARY KEY(bot_id,space_id,chat_id,session_id,execution_token)
         ) WITHOUT ROWID;",
    )?;
    Ok(())
}

/// Sem Serializable/Debug: não reutilizar checkpoint privado como resposta pública.
pub(crate) struct Checkpoint {
    capture: AdmissionCapture,
}
impl Checkpoint {
    pub(crate) fn captured(&self) -> &AdmissionCapture {
        &self.capture
    }

    pub(crate) fn receipt(&self) -> ReceiptView<'_> {
        let r = &self.capture.routing;
        ReceiptView {
            schema_version: self.capture.schema_version,
            destination: &self.capture.key.destination,
            requested: &r.requested,
            admitted: &r.admitted,
            decided_by: &r.decided_by,
            reason: &r.reason,
            required_facts: &r.required_facts,
        }
    }
}

/// Projeção sem token, binding, carimbo, selo ou marcadores privados da origem.
/// Único produtor público: `entry_receipt` (RPC `donnaRoutingReceipt`, só do dono).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiptView<'a> {
    schema_version: u8,
    destination: &'a Destination,
    requested: &'a SelectionSnapshot,
    admitted: &'a SelectionSnapshot,
    decided_by: &'a DecidedBy,
    reason: &'a Reason,
    required_facts: &'a UnattestedFacts,
}

fn transaction(c: &Connection) -> Result<()> {
    ensure!(!c.is_autocommit(), "routing_transaction_required");
    Ok(())
}

/// Grava só a tabela própria, sem abrir transação. O chamador DEVE propagar falha
/// e desfazer a transação inteira. O retorno ainda depende do commit externo.
/// PR4: na mesma Tx IMMEDIATE/guarda original, chamar antes de alterar revisão/carimbo;
/// depois aplicar CAS/claim originais. A captura é pré-imagem, não prova pós-escrita.
/// Referências recebidas não atestam token/binding vivo nem autorizam Dispatch/Resume.
pub(crate) fn record_dispatch(c: &Connection, captured: &AdmissionCapture) -> Result<Checkpoint> {
    record(c, captured).map_err(|_| anyhow!("routing_record_failed"))
}
fn record(c: &Connection, captured: &AdmissionCapture) -> Result<Checkpoint> {
    transaction(c)?;
    captured.validate()?;
    let raw = serde_json::to_string(captured)?;
    ensure!(raw.len() <= DOCUMENT_MAX, "routing_document_invalid");
    // Valida o mesmo codec de persistência também na escrita, sem defaults silenciosos.
    let _: AdmissionCapture = decode(&raw, DOCUMENT_MAX)?;
    if let Some(original) = load(c, &captured.key)? {
        ensure!(config::same(original.captured(), captured)?, "routing_checkpoint_conflict");
        return Ok(original);
    }
    validate_references(c, captured)?;
    let d = &captured.key.destination;
    c.execute(
        "INSERT INTO donna_routing_checkpoints(bot_id,space_id,chat_id,session_id,execution_token,checkpoint_json)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![d.bot_id, d.space_id, d.session.chat_id, d.session.id, captured.key.execution_token, raw],
    )?;
    Ok(Checkpoint { capture: captured.clone() })
}

/// Só leitura da chave e documento originais. Não resolve Fixo/guias/conta atual,
/// não cria recibo e não autoriza retomada.
/// `Ok(None)` só quando a linha comprovadamente não existe. Chave inválida, documento
/// oversize/inválido, identidade divergente ou erro de banco → `routing_checkpoint_unavailable`.
/// Ausência não autoriza executar, repetir turno nem decidir de novo: o fluxo legado
/// exige classificação/evidência própria.
pub(crate) fn read_checkpoint(c: &Connection, key: &CheckpointKey) -> Result<Option<Checkpoint>> {
    let read = || -> Result<Option<Checkpoint>> {
        key.validate()?;
        load(c, key)
    };
    read().map_err(|_| anyhow!("routing_checkpoint_unavailable"))
}

/// Marca de roteamento do registro em voo/espera, decidida no permit (PR4) a partir dos
/// dados capturados, nunca por omissão do hook nem pela conta atual. Turno `Routed` sempre
/// chama `record_dispatch`; `NotRouted`, nunca.
/// `Routed {}` (não unitária): com `tag`, variante unitária aceitaria campo extra.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub(crate) enum RoutingClass {
    Routed {},
    NotRouted { reason: NotRoutedReason },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum NotRoutedReason {
    /// Só a migração de boot do PR4 grava; nunca o permit.
    PreRoteamento,
    /// Configuração legítima que o gravador não atesta: bot fora do Pessoal, sem conta
    /// gerenciada (login legado/API/local), ou sessão PRINCIPAL com conta gerenciada válida e
    /// modelo padrão do motor. Conta gerenciada quebrada, cadastro ausente, revisão divergente,
    /// comando próprio com conta `ac_` ou erro de banco nunca são Unproven: Err e pausa visível.
    UnprovenSelection,
    /// Origin do inflight diferente de Human (Routine, Ask, Message, Group, Compaction, Recovery).
    NotHumanTurn,
}
pub(crate) enum ResumeRoute {
    Routed(Box<Checkpoint>),
    Legacy,
}

/// Regra canônica de Resume (D305). Todo `Err` é pausa visível pelo chamador, nunca caminho
/// legado. `Legacy` não autoriza nada além do caminho atual do D72. `reason` é decidida no
/// permit (PR4), nunca inferida da conta atual. Classe ausente → `routing_class_missing`.
pub(crate) fn classify_resume(
    c: &Connection,
    key: &CheckpointKey,
    class: Option<&RoutingClass>,
) -> Result<ResumeRoute> {
    transaction(c)?;
    let class = class.ok_or_else(|| anyhow!("routing_class_missing"))?;
    match (class, read_checkpoint(c, key)?) {
        (RoutingClass::Routed {}, Some(checkpoint)) => Ok(ResumeRoute::Routed(Box::new(checkpoint))),
        (RoutingClass::Routed {}, None) => Err(anyhow!("routing_resume_checkpoint_missing")),
        (RoutingClass::NotRouted { .. }, None) => Ok(ResumeRoute::Legacy),
        (RoutingClass::NotRouted { .. }, Some(_)) => Err(anyhow!("routing_resume_contradiction")),
    }
}

/// Fonte única da classificação na admissão. Decidida no permit, na Tx aberta, com os dados
/// capturados: para extra com escolha própria, conta/seleção vêm do `engine_choice` da sessão;
/// senão, do bot. Nunca pela conta atual em outro instante. `Routed` ⇒ chamar `record_dispatch`
/// na mesma Tx; `NotRouted` ⇒ nunca. `account` = id e revisão capturados da conta gerenciada.
/// Conta gerenciada alegada que não se confirma → `routing_selection_unavailable` (pausa
/// visível), nunca `UnprovenSelection` nem legado.
pub(crate) fn admission_class(
    c: &Connection,
    cfg: &BotConfig,
    human: bool,
    principal: bool,
    account: Option<(&str, u64)>,
    selection: &EngineSelection,
) -> Result<RoutingClass> {
    transaction(c)?;
    let unproven = RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection };
    if !human {
        return Ok(RoutingClass::NotRouted { reason: NotRoutedReason::NotHumanTurn });
    }
    let Some((account_id, account_revision)) = account.filter(|_| cfg.space_id == spaces::PERSONAL) else {
        return Ok(unproven);
    };
    let routed = || -> Result<bool> {
        // `selection::validate` zera o comando no candidato; aqui ele é conferido antes.
        ensure!(cfg.command.is_none(), "account_incompatible");
        let registry = Registry::load_db(c)?;
        let found = registry.account(account_id)?;
        ensure!(found.managed.as_ref().map_or(1, |m| m.revision) == account_revision, "account_conflict");
        let mut candidate = cfg.clone();
        candidate.backend.clone_from(&selection.backend);
        candidate.model.clone_from(&selection.model);
        candidate.model_options.clone_from(&selection.model_options);
        candidate.fork_editor.engine_account_id = Some(account_id.to_owned());
        managed::validate_selection_in(&registry, &candidate)?;
        if selection.model.is_none() && principal {
            // A principal conserva o caminho legado. A extra registra o padrão como model:null e continua roteada.
            return Ok(false);
        }
        let reserve = Reserve { account_id: account_id.to_owned(), account_revision, selection: selection.clone() };
        selection::validate_receipt(&registry, cfg, &reserve)?;
        Ok(true)
    };
    match routed() {
        Ok(true) => Ok(RoutingClass::Routed {}),
        Ok(false) => Ok(unproven),
        Err(_) => Err(anyhow!("routing_selection_unavailable")),
    }
}

fn load(c: &Connection, key: &CheckpointKey) -> Result<Option<Checkpoint>> {
    let d = &key.destination;
    let max_bytes = i64::try_from(DOCUMENT_MAX)?;
    // Não alocar documento excessivo vindo de DB adulterada. Some(None) é corrupção, não ausência.
    let raw: Option<Option<String>> = c
        .query_row(
            "SELECT CASE WHEN length(CAST(checkpoint_json AS BLOB))<=?6 THEN checkpoint_json ELSE NULL END
             FROM donna_routing_checkpoints
             WHERE bot_id=?1 AND space_id=?2 AND chat_id=?3 AND session_id=?4 AND execution_token=?5",
            params![d.bot_id, d.space_id, d.session.chat_id, d.session.id, key.execution_token, max_bytes],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let capture: AdmissionCapture = decode(&raw.ok_or_else(|| anyhow!("routing_document_invalid"))?, DOCUMENT_MAX)?;
    capture.validate()?;
    ensure!(config::same(&capture.key, key)?, "routing_checkpoint_identity_mismatch");
    Ok(Some(Checkpoint { capture }))
}

/// Revisões que `record_dispatch` confere. `policy` é o documento do bot (também a
/// revisão do guia do bot); `general_guide`, o guia geral do Space.
pub(crate) struct RoutingRevisions {
    pub policy: u64,
    pub general_guide: u64,
}

/// Leitura canônica, com o mesmo parser e erros fechados do gravador. Capturar dentro da
/// MESMA transação IMMEDIATE/Connection do hook do permit, imediatamente antes de
/// `record_dispatch`, sem nenhum await entre a leitura e o commit. O resultado é a revisão
/// do instante da admissão (Fixo, sem decisão Auto), não snapshot de proposta. Leitura
/// antes de um await não vale; `record` exige igualdade exata (`routing_policy_conflict`).
pub(crate) fn revisions(c: &Connection, space_id: &str, bot_id: &str) -> Result<RoutingRevisions> {
    transaction(c)?;
    let (policy_key, general_key) = config::keys(space_id, bot_id);
    let policy: Document<Policy> = config::load(c, &policy_key)?;
    let general: Document<String> = config::load(c, &general_key)?;
    Ok(RoutingRevisions { policy: policy.revision, general_guide: general.revision })
}

fn bot(c: &Connection, id: &str) -> Result<BotConfig> {
    let raw: String = c.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [id], |r| r.get(0))?;
    Ok(serde_json::from_str(&raw)?)
}
/// Carimbo (decisão A, D372): fora de Space, Session (`account.stamp.<bot>/session:<id>`) na extra
/// e na principal do Pessoal com seleção efetiva própria; Bot só na principal sem seleção; Space no
/// ator de Space. O setup grava cada um com a configuração que executa. Aqui se lê a seleção EFETIVA da sessão
/// (`engine_choice`, imutável durante o turno) na mesma Connection: presente ⇒ a admitida tem de
/// ser ela (principal ou extra); ilegível ⇒ Err. Só vale na gravação; checkpoints antigos com
/// carimbo Bot em extra seguem legíveis. Extra sem seleção não grava (o Sessões já recusa antes).
fn validate_references(c: &Connection, a: &AdmissionCapture) -> Result<()> {
    let d = &a.key.destination;
    let cfg = bot(c, &d.bot_id)?;
    ensure!(cfg.id == d.bot_id && !cfg.is_group(), "routing_destination_invalid");
    spaces::require_space(c, &d.space_id)?;
    let (space, executor, closed): (String, String, bool) = c.query_row(
        "SELECT space_id,bot_id,closed FROM donna_space_conversations WHERE id=?1",
        [&d.session.chat_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let group = if executor == d.bot_id {
        false
    } else {
        let chat = bot(c, &d.session.chat_id)?;
        ensure!(
            chat.is_group() && chat.space_id == d.space_id && chat.members.contains(&d.bot_id),
            "routing_destination_invalid"
        );
        true
    };
    ensure!(space == d.space_id && !closed, "routing_destination_invalid");
    if cfg.space_id != d.space_id {
        // Mesma exceção canônica da criação de conversas/runtime, sem copiar decoder/default/KEY.
        // É vínculo de destino, não prova de pagador ou Authority; rechecado na Connection do writer.
        ensure!(
            !group
                && d.session.chat_id != d.bot_id
                && crate::store::read_team_settings(c)?.primary_bot_id.as_deref() == Some(d.bot_id.as_str()),
            "routing_destination_invalid"
        );
    }
    let session = sessoes::store::get(c, &d.session.id)?.ok_or_else(|| anyhow!("routing_session_missing"))?;
    ensure!(
        session.chat_id == d.session.chat_id
            && session.state == "open"
            && session.revision == a.intent.expected_session_revision,
        "routing_session_conflict"
    );
    ensure!(config::same(&session.origin, &a.routing.session_origin)?, "routing_session_origin_conflict");
    for (id, origin) in a.intent.entry_ids.iter().zip(&a.routing.turn_provenance.entries) {
        ensure!(sessoes::store::session_ref_of_entry(c, &d.session.chat_id, id)? == d.session, "routing_entry_invalid");
        let kind: String = c.query_row("SELECT kind FROM entries WHERE id=?1", [id], |r| r.get(0))?;
        ensure!(kind == "user", "routing_entry_invalid");
        let field = match &origin.evidence {
            EntryEvidence::RequiredFactNotAttested => None,
            EntryEvidence::HubEntrySourceBotV1 { source_bot_id } => Some(("$.sourceBotId", source_bot_id)),
            EntryEvidence::HubEntryRunV1 { run_id } => Some(("$.runId", run_id)),
        };
        if let Some((path, expected)) = field {
            let actual: Option<String> = c.query_row(
                "SELECT json_extract(data,?2) FROM entries WHERE id=?1 AND json_valid(data)",
                params![id, path],
                |r| r.get(0),
            )?;
            ensure!(actual.as_ref() == Some(expected), "routing_provenance_conflict");
        }
    }
    // Conferência estática da seleção admitida. Não comprova Space/pagador ou Authority.
    // load_db cria um sal efêmero se não houver cadastro; este gravador não toma esse caminho.
    let has_registry: bool =
        c.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [crate::fork::engine_accounts::SLOT], |r| r.get(0))?;
    ensure!(has_registry, "routing_registry_missing");
    let registry = Registry::load_db(c)?;
    let validate = if session.principal { selection::validate } else { selection::validate_receipt };
    validate(
        &registry,
        &cfg,
        &Reserve {
            account_id: a.routing.admitted.account_id.clone(),
            account_revision: a.account_revision,
            selection: a.routing.admitted.selection.clone(),
        },
    )?;
    let current = revisions(c, &d.space_id, &d.bot_id)?;
    ensure!(
        current.policy == a.routing.policy_revision
            && current.policy == a.routing.bot_guide_revision
            && current.general_guide == a.routing.general_guide_revision,
        "routing_policy_conflict"
    );
    let space_actor = d.session.chat_id != d.bot_id && !group;
    // D372 (decisão A de Modelos): a seleção EFETIVA da sessão define o domínio do carimbo. Principal sem seleção:
    // carimbo do bot; principal ou extra com seleção (fora de Space): carimbo da própria sessão.
    let choice = sessoes::engine_choice::read(c, &session.id)?;
    ensure!(
        matches!(
            (&a.private_stamp, space_actor, session.principal, choice.is_some()),
            (StampCapture::BotAccountStampV1 { .. }, false, true, false)
                | (StampCapture::SpaceEngineStampV1 { .. }, true, _, _)
                | (StampCapture::SessionAccountStampV1 { .. }, false, _, true)
        ),
        "routing_stamp_source_conflict"
    );
    // Igualdade estrita efetiva × admitida; `Routed` nunca aceita conta nula.
    if let Some(choice) = choice {
        let admitted = &a.routing.admitted;
        ensure!(
            choice.account_id.as_deref() == Some(admitted.account_id.as_str())
                && config::same(&choice.selection(), &admitted.selection)?,
            "routing_stamp_source_conflict"
        );
    }
    if matches!(&a.execution_seal, ExecutionSeal::SpaceExecutionStampV1 { .. }) {
        ensure!(space_actor, "routing_seal_source_conflict");
    }
    let (key, capture) = a.private_stamp.source(d);
    let current: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [&key], |r| r.get(0)).optional()?;
    ensure!(config::same(capture, &CapturedText::from_captured(current))?, "routing_stamp_capture_conflict");
    Ok(())
}

/// Recibo de um turno humano para o app, só leitura. A chave vem da própria entrada (chat e
/// sessão). O SQLite acha o candidato pela entrada em `intent.entryIds`, depois de recusar a
/// sessão se alguma linha não permitir esse filtro (JSON inválido ou `entryIds` fora de lista).
/// Só o candidato passa pelo codec estrito de `read_checkpoint`: outra linha com `entryIds`
/// válido sem esta entrada, mas corrompida no resto, não afeta este recibo (e falha no dela).
/// Nada é recalculado ou gravado.
/// `routed` com a projeção pública; `pending` enquanto a entrada está na fila; `notRouted`
/// quando já saiu sem recibo (turno não roteado ou anterior ao Roteamento). Entrada de outro
/// bot/sessão/conversa/Space, que não é do usuário, candidato ilegível ou linha da sessão sem
/// filtro possível → `routing_receipt_unavailable`.
pub(crate) struct ReceiptQuery<'a> {
    pub bot: &'a str,
    pub space: &'a str,
    pub conversation: &'a str,
    pub session: &'a str,
    pub entry: &'a str,
}
pub(crate) fn entry_receipt(c: &Connection, q: &ReceiptQuery<'_>) -> Result<Value> {
    let read = || -> Result<Value> {
        let (chat, session, kind, status, turn): (String, String, String, Option<String>, i64) = c.query_row(
            "SELECT bot_id, COALESCE(session, bot_id), kind, json_extract(data,'$.status'), turn FROM entries WHERE id=?1",
            [q.entry],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        ensure!(chat == q.conversation && session == q.session && kind == "user", "routing_entry_invalid");
        let (space, executor): (String, String) =
            c.query_row("SELECT space_id,bot_id FROM donna_space_conversations WHERE id=?1", [&chat], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        ensure!(space == q.space, "routing_entry_invalid");
        if executor != q.bot {
            let group = bot(c, &chat)?;
            ensure!(group.is_group() && group.members.iter().any(|m| m == q.bot), "routing_entry_invalid");
        }
        // Guarda estrutural na mesma Tx, sem decodificar em Rust: linha da sessão em que o filtro não
        // se aplica (JSON inválido, sem `intent.entryIds` em lista) falha fechado, nunca "sem recibo".
        let session_args = params![q.bot, q.space, chat, session];
        let unfilterable: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_routing_checkpoints
                 WHERE bot_id=?1 AND space_id=?2 AND chat_id=?3 AND session_id=?4
                   AND (CASE WHEN json_valid(checkpoint_json)
                        THEN json_type(checkpoint_json,'$.intent.entryIds') END) IS NOT 'array')",
            session_args,
            |r| r.get(0),
        )?;
        ensure!(!unfilterable, "routing_checkpoint_unavailable");
        let tokens = c
            .prepare(
                "SELECT execution_token FROM donna_routing_checkpoints
                 WHERE bot_id=?1 AND space_id=?2 AND chat_id=?3 AND session_id=?4
                   AND EXISTS(SELECT 1 FROM json_each(checkpoint_json,'$.intent.entryIds') WHERE value=?5)",
            )?
            .query_map(params![q.bot, q.space, chat, session, q.entry], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(tokens.len() <= 1, "routing_checkpoint_conflict");
        let found = match tokens.into_iter().next() {
            None => None,
            Some(execution_token) => {
                let key = CheckpointKey {
                    destination: Destination {
                        bot_id: q.bot.to_owned(),
                        space_id: q.space.to_owned(),
                        session: sessoes::store::SessionRef { id: session.clone(), chat_id: chat.clone() },
                    },
                    execution_token,
                };
                // Só o candidato passa pelo codec estrito; o decodificado confirma o filtro.
                let checkpoint = read_checkpoint(c, &key)?.ok_or_else(|| anyhow!("routing_checkpoint_unavailable"))?;
                ensure!(
                    checkpoint.capture.intent.entry_ids.iter().any(|id| id == q.entry),
                    "routing_checkpoint_conflict"
                );
                Some(checkpoint)
            }
        };
        // O uso é gravado no turno da execução (o da última entrada do lote), que o recibo roteado conhece.
        let usage_turn = found.as_ref().map_or(turn, |checkpoint| checkpoint.captured().intent.hub_turn);
        // Só exibição: falhar ao ler o uso nunca derruba nem muda o recibo de roteamento.
        let usage = crate::fork::custo::turn_usage(c, q.bot, &chat, &session, usage_turn).unwrap_or_else(|error| {
            tracing::warn!(%error, "uso do turno indisponível para o recibo");
            None
        });
        Ok(if let Some(checkpoint) = found {
            serde_json::json!({"receipt": checkpoint.receipt(), "state": "routed", "usage": usage})
        } else {
            let state = if status.as_deref() == Some("queued") { "pending" } else { "notRouted" };
            serde_json::json!({"receipt": null, "state": state, "usage": usage})
        })
    };
    read().map_err(|_| anyhow!("routing_receipt_unavailable"))
}

/// Exclusão própria apenas pelo hook existente, na transação de exclusão do chamador.
pub(crate) fn on_bot_deleted(c: &Connection, bot_id: &str) -> Result<()> {
    let delete = || -> Result<()> {
        transaction(c)?;
        c.execute("DELETE FROM donna_routing_checkpoints WHERE bot_id=?1", [bot_id])?;
        Ok(())
    };
    delete().map_err(|_| anyhow!("routing_delete_failed"))
}
