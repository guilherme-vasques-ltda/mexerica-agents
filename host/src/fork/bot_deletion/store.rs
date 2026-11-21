//! Exclusão durável: bot, grants, rotinas e limpeza SQL confirmam juntos.
//! Compilado sob `store::bots` para manter os helpers SQL privados à Store.
use crate::{
    LockExt,
    store::{Store, next_rev},
};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

impl Store {
    /// Mesmo inventário da Tx, inclusive conversas de bots já tombstonados.
    pub(crate) fn space_delete_conversations(&self, space: &str) -> Result<Vec<(String, String)>> {
        space_conversations(&self.db.locked(), space)
    }

    pub(crate) fn delete_bot_with_routines<'a>(
        &self,
        id: &str,
        routines: &str,
        live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<i64> {
        self.delete_bot_committed(id, Some(routines), live_runs)
    }

    /// Err significa que não houve commit. Toda limpeza SQL é parte da Tx;
    /// depois do commit resta apenas descarte infalível no cache do guard.
    pub(super) fn delete_bot_committed<'a>(
        &self,
        id: &str,
        routines: Option<&str>,
        live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<i64> {
        let namespace = self.guard_runtime_namespace()?;
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut settings = super::super::team::read(&tx)?;
        if settings.primary_bot_id.as_deref() == Some(id) {
            settings.primary_bot_id = None;
            tx.execute("UPDATE kv SET v = ?1 WHERE k = 'team.settings.v1'", [serde_json::to_string(&settings)?])?;
        }
        let rev = next_rev(&tx)?;
        let deleted = tx.execute(
            "UPDATE bots SET rev = ?2, deleted = 1, session_id = NULL WHERE id = ?1 AND deleted = 0",
            params![id, rev],
        )?;
        super::super::bot_groups::forget_bot(&tx, id, rev)?;
        if deleted > 0 {
            crate::fork::on_bot_deleted(&tx, id)?;
        }
        tx.execute("DELETE FROM entries WHERE bot_id = ?1", [id])?;
        codync_mem::store::drop_bot(&tx, id)?;
        crate::fork::memory_auto::recibo_injecao::purge(&tx, id)?;
        // Its dreaming bookmark, notices' revisions and the record of its moved memory.
        tx.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("memory.*{id}*")])?;
        // Its routine runs' sessions (see `run_session_key`).
        tx.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("lane.session.{id}@*")])?;
        tx.execute("DELETE FROM kv WHERE substr(k, 1, length(?1)) = ?1", [format!("open_chat.thread.{id}.")])?;
        // ponytail: snapshots of replaced sessions stay until the bot goes; prune by session if kv grows.
        tx.execute(
            "DELETE FROM kv WHERE k GLOB ?1 OR k GLOB ?2",
            [format!("context.{id}.*"), format!("context.epoch.{id}.*")],
        )?;
        let removed = match routines {
            Some(raw) => super::super::guard_hub::routines_commit_in(&tx, raw, live_runs)?,
            None => super::super::guard_hub::prune(&tx)?,
        };
        tx.commit()?;
        drop(c);
        crate::guard_hub::mcp::runtime::forget_contexts(&namespace, &removed);
        Ok(rev)
    }

    /// D543: as linhas de UM bot numa `Connection` já aberta (a Tx do Space ou a do
    /// próprio bot). Não commita, não toca em rotinas nem em contextos em memória.
    /// Devolve a revisão atribuída, para os eventos pós-commit.
    pub(crate) fn delete_bot_rows_on(c: &rusqlite::Connection, id: &str) -> Result<i64> {
        let mut settings = super::super::team::read(c)?;
        if settings.primary_bot_id.as_deref() == Some(id) {
            settings.primary_bot_id = None;
            c.execute("UPDATE kv SET v = ?1 WHERE k = 'team.settings.v1'", [serde_json::to_string(&settings)?])?;
        }
        let rev = next_rev(c)?;
        let deleted = c.execute(
            "UPDATE bots SET rev = ?2, deleted = 1, session_id = NULL WHERE id = ?1 AND deleted = 0",
            params![id, rev],
        )?;
        super::super::bot_groups::forget_bot(c, id, rev)?;
        if deleted > 0 {
            crate::fork::on_bot_deleted(c, id)?;
        }
        c.execute("DELETE FROM entries WHERE bot_id = ?1", [id])?;
        codync_mem::store::drop_bot(c, id)?;
        crate::fork::memory_auto::recibo_injecao::purge(c, id)?;
        c.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("memory.*{id}*")])?;
        c.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("lane.session.{id}@*")])?;
        c.execute("DELETE FROM kv WHERE substr(k, 1, length(?1)) = ?1", [format!("open_chat.thread.{id}.")])?;
        c.execute(
            "DELETE FROM kv WHERE k GLOB ?1 OR k GLOB ?2",
            [format!("context.{id}.*"), format!("context.epoch.{id}.*")],
        )?;
        Ok(rev)
    }

    /// D543: a exclusão inteira do Space na `Connection` da transação da autoridade.
    /// Ordem: relê e valida → `space_busy` só com nomes → marcador inflight → bots →
    /// dados do Space → rotinas → snapshot → fim do marcador. Tudo ou nada.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn delete_space_on(
        c: &rusqlite::Connection,
        space_id: &str,
        expected_revision: i64,
        confirm: &str,
        device_key: Option<&str>,
        hub_id: &str,
        expected_bots: &[String],
        expected_conversations: &[(String, String)],
        routines_raw: &str,
        live_runs: &[(String, String)],
        live_bots: &[String],
    ) -> Result<crate::fork::spaces::delete::SpaceDelete> {
        use crate::fork::spaces::delete::SpaceDelete;
        use rusqlite::OptionalExtension;
        let mut state = crate::fork::spaces::store::read(c)?;
        let Some(index) = state.spaces.iter().position(|s| s.id == space_id) else {
            return Err(anyhow::anyhow!("space_not_found"));
        };
        let space = &state.spaces[index];
        anyhow::ensure!(space.kind == "space", "personal_space_immutable");
        anyhow::ensure!(space.name == confirm, "space_delete_confirmation_mismatch");
        // O padrão GLOB/LIKE abaixo só é seguro com ids do formato que o Hub emite.
        anyhow::ensure!(uuid::Uuid::parse_str(space_id).is_ok(), "invalid_space");
        if state.revision != expected_revision {
            return Err(crate::remote::cloud::Conflict("spaces_revision_conflict").into());
        }
        // Só computador: lista de permissão das plataformas do app de desktop,
        // relida na Tx; ausente ou desconhecida recusa. Local é o próprio Hub.
        if let Some(key) = device_key {
            let platform: Option<String> =
                c.query_row("SELECT platform FROM devices WHERE key=?1", [key], |r| r.get(0)).optional()?;
            anyhow::ensure!(
                matches!(platform.as_deref(), Some("macos" | "linux" | "windows")),
                "space_delete_desktop_only"
            );
        }
        let bots = space_bot_ids(c, space_id)?;
        // O conjunto preparado fora (rotinas, torre, canais) é o mesmo que a Tx exclui.
        anyhow::ensure!(bots.as_slice() == expected_bots, "space_delete_membership_changed");
        anyhow::ensure!(space_conversations(c, space_id)? == expected_conversations, "space_delete_membership_changed");
        if let Some(busy) = space_busy(c, space_id, &bots, live_bots)? {
            return Ok(SpaceDelete::Busy(busy));
        }
        c.execute("INSERT INTO donna_space_delete_spaces(space_id) VALUES(?1)", [space_id])?;
        if let Some(principal) = state.spaces[index].principal_bot_id.clone() {
            c.execute("INSERT INTO donna_space_delete_bots(bot_id) VALUES(?1)", [principal])?;
        }
        let mut deleted = Vec::with_capacity(bots.len());
        for bot in &bots {
            let rev = Self::delete_bot_rows_on(c, bot)?;
            deleted.push((bot.clone(), rev));
        }
        let layers = delete_space_data(c, space_id, hub_id, &bots)?;
        // Contextos removidos voltam para o descarte pós-commit (mesmo desenho do
        // delete_bot): guard_runtime_namespace leria o banco e travaria a Tx.
        let removed = super::super::guard_hub::routines_commit_in(
            c,
            routines_raw,
            live_runs.iter().map(|(b, r)| (b.as_str(), r.as_str())),
        )?;
        state.spaces.remove(index);
        for (i, s) in state.spaces.iter_mut().enumerate() {
            s.position = i64::try_from(i)?;
        }
        // A revisão sobe mesmo sem bots: Space sem bots também muda o snapshot.
        state.revision = next_rev(c)?;
        c.execute(
            "UPDATE kv SET v=?1 WHERE k=?2",
            params![serde_json::to_string(&state)?, crate::fork::spaces::store::KEY],
        )?;
        c.execute("DELETE FROM donna_space_delete_bots", [])?;
        c.execute("DELETE FROM donna_space_delete_spaces", [])?;
        Ok(SpaceDelete::Done { snapshot: state, deleted, removed, layers })
    }
}

/// Bots do Space numa Connection do chamador, id ordenado (conferência estável).
fn space_bot_ids(c: &rusqlite::Connection, space_id: &str) -> Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT id FROM bots WHERE deleted=0 AND json_valid(config)
         AND json_extract(config,'$.spaceId')=?1 ORDER BY id",
    )?;
    Ok(st.query_map([space_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

/// Conversas do Space (canônica + extras) com o bot dono de cada uma.
fn space_conversations(c: &rusqlite::Connection, space_id: &str) -> Result<Vec<(String, String)>> {
    let mut st = c.prepare("SELECT id,bot_id FROM donna_space_conversations WHERE space_id=?1 ORDER BY id")?;
    Ok(st.query_map([space_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
}

fn kv_get(c: &rusqlite::Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
}

/// Espera de retomada viva do chat (`turn.waiting.<chat>`): qualquer sessão conta;
/// `completing` (conclusão durável por publicar) e JSON ilegível contam como vivos.
fn waiting_busy(c: &rusqlite::Connection, chat: &str) -> Result<bool> {
    let Some(raw) = kv_get(c, &format!("turn.waiting.{chat}"))? else { return Ok(false) };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { return Ok(true) };
    if value["phase"] == "finished" {
        return Ok(false);
    }
    Ok(value["phase"] == "completing" || crate::agent::bot::checkpoint_waiting_active(&value))
}

/// Admissão em andamento do chat (`turn.inflight.<chat>`): registro ausente/ilegível
/// ou ocupando entradas ainda gravadas conta como trabalho vivo.
fn admission_busy(c: &rusqlite::Connection, chat: &str) -> Result<bool> {
    let Some(raw) = kv_get(c, &format!("turn.inflight.{chat}"))? else { return Ok(false) };
    let Ok(Some(record)) = crate::fork::bot_admission::decode(&raw) else { return Ok(true) };
    if record.entry_ids.is_empty() {
        return Ok(true);
    }
    let marks = record.entry_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("SELECT EXISTS(SELECT 1 FROM entries WHERE bot_id=?1 AND id IN ({marks}))");
    let params = std::iter::once(chat).chain(record.entry_ids.iter().map(String::as_str)).collect::<Vec<_>>();
    Ok(c.query_row(&sql, rusqlite::params_from_iter(params), |r| r.get(0))?)
}

/// Trabalho vivo de UM bot do Space: fila/permissão/pergunta em qualquer conversa
/// dele, espera ou admissão por chat, cartão de passagem, pergunta do dono, fluxo
/// `GenUI` pendente nas sessões conhecidas e rotina viva (`live_bots`).
/// Trabalho vivo de UM bot do Space: rotina viva (`live_bots`), pergunta do dono
/// pendente no bot e cada conversa dele (própria ou do Space) em `chat_busy`.
fn bot_busy(c: &rusqlite::Connection, bot: &str, convs: &[(String, String)], live_bots: &[String]) -> Result<bool> {
    if live_bots.iter().any(|b| b.as_str() == bot) {
        return Ok(true);
    }
    if !crate::fork::owner_question::pending_on(c, bot)?.is_empty() {
        return Ok(true);
    }
    let mut chats: Vec<String> = vec![bot.to_owned()];
    chats.extend(convs.iter().filter(|(_, owner)| owner == bot).map(|(id, _)| id.clone()));
    chats.sort();
    chats.dedup();
    for chat in chats {
        if chat_busy(c, &chat)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Trabalho vivo no nível de UMA conversa: espera/admissão, rascunho de passagem,
/// pergunta do dono e fluxo `GenUI` em todas as sessões conhecidas dela.
fn chat_busy(c: &rusqlite::Connection, chat: &str) -> Result<bool> {
    if waiting_busy(c, chat)? || admission_busy(c, chat)? {
        return Ok(true);
    }
    if crate::chat::handoff::draft_pending_on(c, &crate::store::Lane::main(chat))? {
        return Ok(true);
    }
    // Sessões conhecidas da conversa: a principal usa o próprio id como sessão.
    let mut sessions = vec![chat.to_owned()];
    let mut st = c.prepare("SELECT id FROM donna_bot_sessions WHERE chat_id=?1")?;
    sessions.extend(st.query_map([chat], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
    sessions.sort();
    sessions.dedup();
    for session in sessions {
        if session != chat
            && crate::chat::handoff::draft_pending_on(
                c,
                &crate::store::Lane { chat: chat.to_owned(), session: Some(session.clone()) },
            )?
        {
            return Ok(true);
        }
        if crate::fork::owner_question::pending_in_on(c, chat, &session)?
            || crate::fork::genui::flow::pending_on(c, chat, &session)?
        {
            return Ok(true);
        }
    }
    // Entrada na fila ou permissão pendente desta conversa.
    let pending: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE bot_id=?1 AND (
            (kind='user' AND json_extract(data,'$.status')='queued')
            OR (kind='permission' AND json_extract(data,'$.status')='pending')))",
        [chat],
        |r| r.get(0),
    )?;
    Ok(pending)
}

/// Bots ocupados do Space, com o nome para a recusa (`{botId, name}` apenas).
/// Conversas do Space cujo dono NÃO é bot do Space (o Berga do Pessoal legado,
/// por exemplo) também contam: o trabalho delas é do executor real, e a recusa
/// aponta esse executor — a exclusão nunca toca nele.
fn space_busy(
    c: &rusqlite::Connection,
    space_id: &str,
    bots: &[String],
    live_bots: &[String],
) -> Result<Option<Vec<(String, String)>>> {
    let convs = space_conversations(c, space_id)?;
    let mut busy = Vec::new();
    for bot in bots {
        if bot_busy(c, bot, &convs, live_bots)? {
            busy.push((bot.clone(), bot_name(c, bot)?));
        }
    }
    // Conversas estrangeiras: o dono fica fora da lista de bots do Space, mas a
    // conversa e seu trabalho são do Space e não podem sumir no meio do turno.
    for (chat, owner) in &convs {
        if bots.iter().any(|b| b == owner) {
            continue;
        }
        if live_bots.contains(owner) || chat_busy(c, chat)? {
            busy.push((owner.clone(), bot_name(c, owner)?));
        }
    }
    busy.sort();
    busy.dedup();
    Ok((!busy.is_empty()).then_some(busy))
}

fn bot_name(c: &rusqlite::Connection, bot: &str) -> Result<String> {
    let name: Option<String> = c
        .query_row("SELECT json_extract(config,'$.name') FROM bots WHERE id=?1", [bot], |r| r.get(0))
        .optional()?
        .flatten();
    Ok(name.unwrap_or_default())
}

/// Dados do Space que não são de um bot: conversas, uploads, recibos de nascimento,
/// seções, times, delegação, catálogo nativo, projetos, roteamento, memória automática,
/// plugins e sessões das conversas — tudo na mesma Tx.
fn delete_space_data(c: &rusqlite::Connection, space_id: &str, hub_id: &str, bots: &[String]) -> Result<Vec<String>> {
    let convs = space_conversations(c, space_id)?;
    let mut chat_ids: Vec<String> = convs.iter().map(|(id, _)| id.clone()).collect();
    chat_ids.extend(bots.iter().cloned());
    chat_ids.sort();
    chat_ids.dedup();
    if !chat_ids.is_empty() {
        let marks = chat_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let values = || rusqlite::params_from_iter(chat_ids.iter());
        for table in [
            "donna_memory_sleep_checks",
            "donna_memory_sleep_cursor",
            "donna_memory_turns",
            "donna_memory_turn_origin",
            "donna_memory_transports",
        ] {
            c.execute(&format!("DELETE FROM {table} WHERE chat IN ({marks})"), values())?;
        }
        c.execute(&format!("DELETE FROM entries WHERE bot_id IN ({marks})"), values())?;
        c.execute(&format!("DELETE FROM donna_genui_questions WHERE chat IN ({marks})"), values())?;
        c.execute(&format!("DELETE FROM donna_genui_charts WHERE chat IN ({marks})"), values())?;
        c.execute(
            &format!("DELETE FROM donna_session_gateway_pending WHERE binding IN (SELECT id FROM donna_session_bindings WHERE chat IN ({marks}))"),
            values(),
        )?;
        c.execute(&format!("DELETE FROM donna_session_bindings WHERE chat IN ({marks})"), values())?;
        c.execute(&format!("DELETE FROM donna_bot_sessions WHERE chat_id IN ({marks})"), values())?;
        for table in ["donna_session_acp", "donna_session_exposure"] {
            c.execute(&format!("DELETE FROM {table} WHERE session IN ({marks})"), values())?;
        }
        for chat in &chat_ids {
            c.execute(
                "DELETE FROM kv WHERE k=?1 OR k=?2 OR k=?3",
                [format!("turn.waiting.{chat}"), format!("turn.inflight.{chat}"), format!("donna.space.engine.{chat}")],
            )?;
            c.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("lane.session.{chat}@*")])?;
            c.execute("DELETE FROM kv WHERE substr(k, 1, length(?1)) = ?1", [format!("open_chat.thread.{chat}.")])?;
            c.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("handoff.{chat}/*")])?;
            c.execute(
                "DELETE FROM kv WHERE k GLOB ?1 OR k GLOB ?2",
                [format!("context.{chat}.*"), format!("context.epoch.{chat}.*")],
            )?;
            c.execute("DELETE FROM kv WHERE k=?1", [crate::chat::handoff::key_for(&crate::store::Lane::main(chat))])?;
        }
        // Vínculo de sessão do Space: a chave é opaca (b64), o valor é o chat.
        if !chat_ids.is_empty() {
            let marks = chat_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            c.execute(
                &format!("DELETE FROM kv WHERE k LIKE 'donna.space.session.%' AND v IN ({marks})"),
                rusqlite::params_from_iter(chat_ids.iter()),
            )?;
        }
    }
    c.execute(
        "DELETE FROM bot_group_members WHERE group_id IN (SELECT id FROM bot_groups WHERE space_id=?1)",
        [space_id],
    )?;
    c.execute("DELETE FROM bot_groups WHERE space_id=?1", [space_id])?;
    super::super::bot_groups::legado::delete_space(c, space_id)?;
    for table in [
        "donna_space_uploads",
        "donna_space_birth_receipts",
        "donna_delegation_versions",
        "donna_delegation_rules",
        "donna_delegation_receipts",
        "donna_native_slots",
        "donna_native_receipts",
        "donna_team_revisions",
        "donna_team_nodes",
        "donna_bot_org",
        "donna_team_routes",
        "donna_team_receipts",
        "donna_routing_checkpoints",
        "donna_plugin_resources",
        "donna_space_principal_receipts",
        "donna_nunca_fazer",
        "donna_nunca_fazer_revisoes",
        "donna_instrucao_geral",
    ] {
        c.execute(&format!("DELETE FROM {table} WHERE space_id=?1"), [space_id])?;
    }
    for table in ["donna_projects", "donna_project_history", "donna_project_receipts"] {
        c.execute(&format!("DELETE FROM {table} WHERE hub_id=?1 AND space_id=?2"), params![hub_id, space_id])?;
    }
    c.execute("DELETE FROM donna_background_space_selections WHERE space=?1", [space_id])?;
    // Memória automática: sugestões apontam para jobs; jobs apontam o Space.
    // Vêm ANTES dos deletes de jobs para a subconsulta ainda enxergar os ids.
    c.execute(
        "DELETE FROM donna_memory_suggestions WHERE job IN (SELECT id FROM donna_memory_auto_jobs WHERE space=?1)",
        [space_id],
    )?;
    c.execute(
        "DELETE FROM donna_memory_sleep_facts WHERE run IN (SELECT id FROM donna_memory_sleep_jobs WHERE space=?1)",
        [space_id],
    )?;
    c.execute(
        "DELETE FROM donna_memory_sleep_kept WHERE run IN (SELECT id FROM donna_memory_sleep_jobs WHERE space=?1)",
        [space_id],
    )?;
    c.execute("DELETE FROM donna_memory_sleep_jobs WHERE space=?1", [space_id])?;
    c.execute(
        "DELETE FROM donna_memory_auto_proposals WHERE job IN (SELECT id FROM donna_memory_auto_jobs WHERE space=?1)",
        [space_id],
    )?;
    c.execute(
        "DELETE FROM donna_memory_auto_receipts WHERE job IN (SELECT id FROM donna_memory_auto_jobs WHERE space=?1)",
        [space_id],
    )?;
    c.execute(
        "DELETE FROM donna_memory_auto_legacy WHERE job IN (SELECT id FROM donna_memory_auto_jobs WHERE space=?1)",
        [space_id],
    )?;
    c.execute("DELETE FROM donna_memory_auto_non_saved WHERE space=?1", [space_id])?;
    c.execute("DELETE FROM donna_memory_opening_omitted WHERE space=?1", [space_id])?;
    c.execute("DELETE FROM donna_memory_auto_jobs WHERE space=?1", [space_id])?;
    if !bots.is_empty() {
        let marks = bots.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        for table in [
            "donna_session_acp",
            "donna_session_exposure",
            "donna_memory_sessions",
            "donna_memory_gateway_pending",
            "donna_memory_unsettled",
            "donna_memory_turns",
            "donna_memory_turn_origin",
            "donna_memory_sleep_checks",
            "donna_memory_sleep_cursor",
            "donna_memory_transports",
            "donna_memory_transport_generations",
        ] {
            c.execute(&format!("DELETE FROM {table} WHERE bot IN ({marks})"), rusqlite::params_from_iter(bots.iter()))?;
        }
        c.execute(
            &format!("DELETE FROM donna_memory_suggestions WHERE bot IN ({marks})"),
            rusqlite::params_from_iter(bots.iter()),
        )?;
        c.execute(
            &format!("DELETE FROM donna_memory_auto_engines WHERE bot IN ({marks})"),
            rusqlite::params_from_iter(bots.iter()),
        )?;
        c.execute(
            &format!("DELETE FROM donna_memory_auto_bot_bindings WHERE bot IN ({marks})"),
            rusqlite::params_from_iter(bots.iter()),
        )?;
    }
    // Camadas de memória do Space (`donna.space.memory.{space}.{bot}` → id da
    // camada): coletar antes de apagar a referência e derrubar cada camada na
    // mesma Tx — apagar a chave sozinha deixaria a memória órfã.
    let mut layer_st = c.prepare("SELECT v FROM kv WHERE k GLOB ?1")?;
    let layers = layer_st
        .query_map([format!("donna.space.memory.{space_id}.*")], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(layer_st);
    let mut kept = Vec::new();
    for layer in &layers {
        if uuid::Uuid::parse_str(layer).is_ok() {
            codync_mem::store::drop_bot(c, layer)?;
            crate::fork::memory_auto::recibo_injecao::purge(c, layer)?;
            codync_mem::files::forget_layer(c, Some(layer))?;
            kept.push(layer.clone());
        }
    }
    // Documentos de roteamento por Space (`donna.routing.v1:{space}:…`).
    c.execute("DELETE FROM kv WHERE k=?1", [format!("donna.routing.v1:{space_id}:guide")])?;
    c.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("donna.routing.v1:{space_id}:bot:*")])?;
    c.execute("DELETE FROM donna_space_conversations WHERE space_id=?1", [space_id])?;
    c.execute("DELETE FROM kv WHERE k=?1", [format!("bot_groups.rev.{space_id}")])?;
    // Fronteira de memória por Space (`donna.space.memory.{space}.{bot}`).
    c.execute("DELETE FROM kv WHERE k GLOB ?1", [format!("donna.space.memory.{space_id}.*")])?;
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A limpeza de Space (`delete_space_data`, no caminho de `delete_space_on`)
    /// apaga as linhas de abertura omitida daquele Space e só daquele.
    #[test]
    fn space_deletion_drops_opening_omissions_of_that_space_only() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let space = "11111111-1111-4111-8111-111111111111";
        let other = "22222222-2222-4222-8222-222222222222";
        {
            let c = store.db.locked();
            for s in [space, other] {
                c.execute(
                    "INSERT INTO donna_memory_opening_omitted(bot, space, memory_id, audience, entry, layer, reason, at)
                     VALUES('viewer', ?1, 'mem', 'owner', 7, 'bot', 'overBudget', 1)",
                    [s],
                )
                .unwrap();
            }
        }
        {
            let c = store.db.locked();
            assert_eq!(delete_space_data(&c, space, "hub", &[]).unwrap(), Vec::<String>::new());
        }
        let left: Vec<String> = store
            .db
            .locked()
            .prepare("SELECT space FROM donna_memory_opening_omitted")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(left, [other.to_owned()]);
    }
}
