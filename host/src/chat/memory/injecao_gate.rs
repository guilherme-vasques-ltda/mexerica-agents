//! O portão do scanner de injeção da memória pessoal: o veredito de `check::content` sobre o
//! conteúdo INTEGRAL que seria gravado (a nota inteira, não só as linhas novas de `check_change`).
//!
//! Usado pelos dois portões de escrita — a promoção automática (`memory_auto::apply`) e as
//! ferramentas do bot (`apply::stage`) — e pela liberação manual do dono (`release`), que só vale
//! para origem comprovada pelo mesmo critério de `write_gate::external_origin`, fica presa à
//! expectativa original recusada, gera recibo separado e nunca libera segredo.
//!
//! A liberação é explícita em dois passos: `releaseMemoryRefusal` sem `confirm` devolve o texto
//! pendente para o dono ler; só com `confirm: true` a escrita acontece. Ela rejoga a operação pelo
//! MESMO caminho da escrita normal (`memory_auto::apply`), como `Writer::Override`: só o veredito
//! do scanner é dispensado — camada, dono, origem externa × compartilhada, bloqueio, deduplicação,
//! conflito, teto e a expectativa original seguem valendo — e a proveniência fica honesta.
use super::write_gate;
use crate::fork::{
    autoridade::Autoridade,
    memory_auto::{
        apply,
        queue::Context,
        recibo_injecao,
        worker::{MemorySection, Operation},
    },
    spaces::PERSONAL,
};
use crate::hub::Hub;
use crate::store::{BotConfig, now_ms};
use anyhow::{Result, anyhow, bail, ensure};
use codync_mem::{
    check,
    origin::{Kind, Origin},
};
use rusqlite::Connection;
use serde_json::{Value, json};

/// O conteúdo que as guardas de texto conferem sobre a operação; `None` quando nada a checar.
fn content_of(op: &Operation) -> Option<&str> {
    match op {
        Operation::Remember { text, .. } | Operation::Revise { text, .. } => Some(text.as_str()),
        Operation::Note { content, .. } => content.as_deref(),
        _ => None,
    }
}

/// Segredo é recusa SEM pendência nem prévia: roda ANTES do scanner nos dois portões, para o texto
/// nunca chegar a `donna_memory_injecao_pendentes` — nem que também haja instrução plantada.
pub(crate) fn secrets(op: &Operation) -> std::result::Result<(), check::Refusal> {
    // Só os campos de texto livre que viram memória, aviso ou nome de arquivo. Hash esperado, ids de
    // evidência e números de revisão são do próprio Hub e podiam parecer cartão (Luhn): recusa falsa.
    match op {
        Operation::Remember { text, .. } | Operation::Revise { text, .. } => check::secrets(text),
        Operation::Note { file, content, .. } => {
            check::secrets(file)?;
            content.as_deref().map_or(Ok(()), check::secrets)
        }
        Operation::Archive { .. }
        | Operation::NotDurable { .. }
        | Operation::Confirm { .. }
        | Operation::Reclassify { .. } => Ok(()),
    }
}

/// O veredito do scanner sobre o conteúdo integral da operação; `None` quando nada a checar.
pub(crate) fn scan(op: &Operation) -> Option<check::Refusal> {
    check::content(content_of(op)?).err()
}

/// A prova de origem do dono: a mesma de `write_gate::external_origin` (digitado ou voz,
/// `appReported`, sem anexo nem resposta), sobre uma mensagem humana da conversa do bot,
/// enviada depois da recusa — a reação ao aviso, não uma mensagem antiga qualquer.
fn owner_proof(db: &Connection, recusa: &recibo_injecao::Recusa, message: &str) -> Result<()> {
    let entry = crate::store::Store::entry_on(db, message)?.ok_or_else(|| anyhow!("unknown_message"))?;
    ensure!(entry.bot_id == recusa.chat && entry.is_user_authored(), "memory_release_not_owner");
    ensure!(!write_gate::external_origin(&entry.data), "memory_release_external_origin");
    ensure!(entry.created_at >= recusa.at, "memory_release_stale_message");
    Ok(())
}

/// O contexto mínimo que as guardas do caminho normal leem: bot, conversa, space e camada da recusa.
/// Sem mensagens de sessão — o conteúdo liberado nunca é fala literal do dono — e o restante
/// (`visible`, `own_layer`, `fact_blocked`, `expected_rev`/`expected_hash`) lê só o banco.
fn released_context(r: &recibo_injecao::Recusa, cfg: &BotConfig) -> Context {
    Context {
        bot_id: r.bot.clone(),
        chat_id: r.chat.clone(),
        space_id: r.space.clone(),
        memory_id: cfg.memory_id().to_owned(),
        backend: String::new(),
        bot_revision: 0,
        conversation_revision: None,
        session: String::new(),
        visible_session: None,
        owner_question: None,
        token: String::new(),
        messages: Vec::new(),
        responses: Vec::new(),
        closing_turn: None,
        started_at: 0,
        initial_marked: true,
    }
}

/// Libera, pelo dono e presa à expectativa original recusada, um candidato recusado pelo scanner.
///
/// Sem `confirm` devolve a prévia (o texto integral que seria gravado), só ao dono comprovado e sob
/// a autoridade original; com `confirm: true` re-checa origem e camada na mesma transação da escrita,
/// que é a de `apply` com `Writer::Override` — segredo, `memory_user_owned`, a separação
/// privado/externo × compartilhada, `fact_blocked`, deduplicação, conflito, `FOLDER_CAP` e a
/// expectativa original da operação valem como na escrita normal.
pub(crate) fn release(hub: &Hub, refusal: &str, message: &str, autoridade: &Autoridade, body: &Value) -> Result<Value> {
    let _section = MemorySection::enter(hub);
    // A limpeza tem transação própria: um pedido expirado não pode desfazê-la.
    autoridade.with_db(hub, None, |db| recibo_injecao::prune(db, now_ms(), None))?;
    // Recusa que já saiu pelo prazo de vida responde vencida, não desconhecida.
    let lookup = |db: &Connection| -> Result<recibo_injecao::Recusa> {
        match recibo_injecao::recusa(db, refusal)? {
            Some(r) => Ok(r),
            None if recibo_injecao::vencida(db, refusal)? => anyhow::bail!("memory_refusal_expired"),
            None => anyhow::bail!("unknown_memory_refusal"),
        }
    };
    if !body["confirm"].as_bool().unwrap_or(false) {
        return autoridade.with_db(hub, None, |db| {
            let r = lookup(db)?;
            recibo_injecao::liberavel(&r)?;
            owner_proof(db, &r, message)?;
            let op: Operation =
                recibo_injecao::pendente(db, &r.id)?.ok_or_else(|| anyhow!("memory_refusal_expired"))?;
            secrets(&op)?;
            Ok(json!({
                "pending": {
                    "id": r.id, "candidate": r.candidate, "entry": r.entry, "note": r.note,
                    "revision": r.revision, "rules": r.rules, "text": op.text(),
                }
            }))
        });
    }
    let (event, note, r_bot, shared) = autoridade.with_db(hub, None, |db| {
        let r = lookup(db)?;
        recibo_injecao::liberavel(&r)?;
        ensure!(body["spaceId"].as_str().is_none_or(|space| space == PERSONAL), "invalid_space");
        ensure!(body["botId"].as_str().is_none_or(|bot| bot == r.bot), "memory_refusal_wrong_bot");
        owner_proof(db, &r, message)?;
        let op: Operation = recibo_injecao::pendente(db, &r.id)?.ok_or_else(|| anyhow!("memory_refusal_expired"))?;
        secrets(&op)?;
        let cfg: BotConfig = db
            .query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [&r.bot], |row| row.get(0))
            .map_err(|_| anyhow!("unknown bot"))
            .and_then(|raw: String| Ok(serde_json::from_str(&raw)?))?;
        ensure!(cfg.space_id == PERSONAL, "invalid_space");
        if let Operation::Note { file, shared, .. } = &op {
            let layer = if *shared { None } else { Some(cfg.memory_id()) };
            let current = codync_mem::files::get(db, layer, file)?.map(|note| note.rev);
            ensure!(current == r.revision, "memory_revision_changed");
        }
        let now = now_ms();
        let rev = crate::store::next_rev(db)?;
        let context = released_context(&r, &cfg);
        let Some(saved) = apply::apply_released(db, &context, &cfg, &op, &released_origin(), now, rev)? else {
            bail!("memory_refusal_not_releasable")
        };
        db.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [rev])?;
        let released = recibo_injecao::released(db, &r, now)?;
        let shared = matches!(op, Operation::Remember { shared: true, .. } | Operation::Note { shared: true, .. });
        Ok((released, saved.projection, r.bot.clone(), shared))
    })?;
    if let Some((layer, file)) = &note
        && crate::chat::memory::files::project_committed(hub, layer.as_deref(), file).is_err()
    {
        tracing::warn!("projeção de nota de memória aguardando reconciliação");
    }
    let _ = hub.events.send(event.clone());
    super::changed(hub, if shared { None } else { Some(&r_bot) });
    Ok(json!({"ok": true, "release": event}))
}

/// A proveniência honesta de uma escrita liberada: o texto veio de fora e o dono o aceitou.
fn released_origin() -> Origin {
    Origin { schema: 1, kind: Kind::External, sources: Vec::new(), external: true }
}
