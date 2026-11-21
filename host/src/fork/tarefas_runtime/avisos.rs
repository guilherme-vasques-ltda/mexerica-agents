//! Entrega de avisos do dono aos bots responsáveis por menções em comentários
//! de tarefa. O outbox é uma linha por (comentário, bot) gravada por trigger
//! SQLite NA MESMA transação do comentário — crash entre commit e entrega não
//! perde o aviso, e o tick reprocessa com claim idempotente por nonce.
//!
//! Regras duras: só `author_kind == "owner"` entrega; `onlyNote`, ausência de
//! menção e autor bot/system nunca geram entrega (o trigger já filtra, e
//! `deliver` reconfere — fail-closed).
use super::store;
use crate::{LockExt, hub::Hub, store::now_ms};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::sync::Arc;

const MAX_ATTEMPTS: i64 = 5;
const STALE_SENDING_MS: i64 = 30_000;

fn nonce(comment_id: &str, bot_id: &str) -> String {
    format!("mx142-task-aviso-{comment_id}-{bot_id}")
}

/// Resultado da validação pré-envio: `Send` carrega tudo para o poke;
/// `Reject` é terminal (menção forjada, autor inelegível, bot/tarefa fora do
/// ar) — vai para `failed` com reason, nunca retenta.
/// Audiência auditada na escrita (resíduo 06 / TEC-06): só `mentionCheck==2`
/// — marcador gravado pelo Hub no entry, impossível de forjar via body
/// (`deny_unknown_fields`) — torna `mentions` confiável. Legado sem marcador
/// falha fechado: lista divergente ou forjada nunca entrega. Rename-safe:
/// não re-deriva @Nome do nome atual.
pub(crate) fn mention_authorized(entry: &Value, bot_id: &str) -> bool {
    if entry["mentionCheck"].as_i64() != Some(2) {
        return false;
    }
    entry["mentions"].as_array().is_some_and(|list| list.iter().any(|m| m.as_str() == Some(bot_id)))
}

enum Prepared {
    Send { session_id: String, text: String, attachments: Vec<String>, project: String, task_title: String },
    Reject(&'static str),
}

fn prepare_delivery(c: &Connection, hub: &str, p: &Pending) -> Result<Prepared> {
    // Tarefa/projeto: load_task já falha fechado em arquivado/inexistente.
    let row = match store::load_task(c, hub, &p.space_id, &p.project_id, &p.task_id) {
        Ok(row) => row,
        Err(error) => {
            if error.to_string().starts_with("invalid_field") {
                return Ok(Prepared::Reject("task_unavailable"));
            }
            return Err(error);
        }
    };
    // Comentário: reconfere autor/onlyNote no entry (defesa em profundidade —
    // o trigger já filtra, mas a linha pode vir de caminho futuro).
    let entry_raw: Option<String> = c
        .query_row(
            "SELECT entry FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5",
            params![hub, p.space_id, p.project_id, p.task_id, p.comment_id],
            |r| r.get(0),
        )
        .optional()?;
    let entry: Value = entry_raw.map(|raw| serde_json::from_str(&raw)).transpose()?.unwrap_or(Value::Null);
    if entry["author"].as_str() != Some("owner") || entry["onlyNote"].as_bool() == Some(true) {
        return Ok(Prepared::Reject("comment_ineligible"));
    }
    let text = entry["text"].as_str().unwrap_or_default().to_owned();
    // Roster ATUAL do Space: bot revogado/deletado/fora do Space falha aqui.
    let roster = crate::fork::projetos::store::bots(c, &p.space_id)?;
    let in_roster =
        roster.as_array().is_some_and(|bots| bots.iter().any(|b| b["id"].as_str() == Some(p.bot_id.as_str())));
    if !in_roster {
        return Ok(Prepared::Reject("bot_unavailable"));
    }
    if !mention_authorized(&entry, &p.bot_id) {
        return Ok(Prepared::Reject("mention_unverified"));
    }
    let attachments = entry["attachmentIds"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let title = row.task["title"].as_str().unwrap_or_default();
    let pname = row.project["name"].as_str().unwrap_or_default();
    let session_id = store::ensure_session(
        c,
        hub,
        &p.space_id,
        &p.project_id,
        &p.task_id,
        &p.bot_id,
        &format!("{pname} — {title}"),
    )?;
    // Sessão gravada ANTES do send, na mesma tx do claim 'sending': um ack que
    // chegue enquanto a entrega ainda está em voo já encontra session_id.
    // A prova causal primária segue sendo a entry (clientNonce+session+turn).
    c.execute(
        "UPDATE donna_task_deliveries SET session_id=?7
         WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6
         AND status='sending' AND (session_id IS NULL OR session_id=?7)",
        params![hub, p.space_id, p.project_id, p.task_id, p.comment_id, p.bot_id, session_id],
    )?;
    Ok(Prepared::Send { session_id, text, attachments, project: pname.to_owned(), task_title: title.to_owned() })
}

/// Chamado pela frente de dados após o commit do comentário. Com o outbox por
/// trigger as linhas já existem; este passo só garante (INSERT OR IGNORE) e
/// nunca falha o comentário. Erros sobem para o chamador, que os ignora.
#[allow(
    clippy::too_many_arguments,
    reason = "Coords+comentário+notify chegam prontos da frente de dados; agrupar criaria struct descartável."
)]
pub(crate) async fn deliver(
    hub: &Arc<Hub>,
    space: &str,
    project: &str,
    task: &str,
    comment_id: &str,
    author_kind: &str,
    _text: &str,
    _attachment_ids: &[String],
    notify: &Value,
) -> Result<()> {
    if author_kind != "owner" || notify["state"].as_str() != Some("notifying") {
        return Ok(());
    }
    let bots: Vec<String> = notify["botIds"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if bots.is_empty() {
        return Ok(());
    }
    let hub = hub.clone();
    let (space, project, task, comment_id) =
        (space.to_owned(), project.to_owned(), task.to_owned(), comment_id.to_owned());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        for bot in bots {
            tx.execute(
                "INSERT OR IGNORE INTO donna_task_deliveries
                 (hub_id,space_id,project_id,task_id,comment_id,bot_id,status,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,'queued',?7)",
                params![hub.host_id, space, project, task, comment_id, bot, now_ms()],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await??;
    Ok(())
}

struct Pending {
    space_id: String,
    project_id: String,
    task_id: String,
    comment_id: String,
    bot_id: String,
    attempts: i64,
}

/// Tick das entregas: reivindica `queued` vencidas e `sending` órfãs, revalida
/// tudo no envio (autor, @ real, roster, tarefa ativa) e só então dispara.
pub(crate) async fn tick(hub: &Arc<Hub>) -> Result<()> {
    let now = now_ms();
    let pending = {
        let hub = hub.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Pending>> {
            let db = hub.store.db.locked();
            let tx = db.unchecked_transaction()?;
            // `queued` respeita o backoff gravado em created_at; `sending`
            // só é retomada depois de STALE_SENDING_MS sem conclusão.
            let mut st = tx.prepare(
                "SELECT space_id,project_id,task_id,comment_id,bot_id,attempts FROM donna_task_deliveries
                 WHERE hub_id=?1 AND status='queued' AND created_at<=?2
                 UNION ALL
                 SELECT space_id,project_id,task_id,comment_id,bot_id,attempts FROM donna_task_deliveries
                 WHERE hub_id=?1 AND status='sending' AND created_at<=?3",
            )?;
            let rows = st.query_map(params![hub.host_id, now, now - STALE_SENDING_MS], |r| {
                Ok(Pending {
                    space_id: r.get(0)?,
                    project_id: r.get(1)?,
                    task_id: r.get(2)?,
                    comment_id: r.get(3)?,
                    bot_id: r.get(4)?,
                    attempts: r.get(5)?,
                })
            })?;
            let pending: Vec<Pending> = rows.collect::<rusqlite::Result<_>>()?;
            for p in &pending {
                tx.execute(
                    "UPDATE donna_task_deliveries SET status='sending',attempts=attempts+1,created_at=?7
                     WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6
                     AND status IN ('queued','sending')",
                    params![hub.host_id, p.space_id, p.project_id, p.task_id, p.comment_id, p.bot_id, now],
                )?;
            }
            drop(st);
            tx.commit()?;
            Ok(pending)
        })
        .await??
    };
    for p in pending {
        deliver_one(hub, &p, now).await;
    }
    Ok(())
}

/// Desfecho de uma entrega: `sent`, `queued` (retry seguro — o `send`
/// deduplica por clientNonce de forma durável) ou `failed` (terminal).
enum Outcome {
    Sent,
    Retry,
    /// Fila da conversa cheia: espera a vez sem gastar tentativa.
    Wait,
    Failed(&'static str),
}

async fn deliver_one(hub: &Arc<Hub>, p: &Pending, _now: i64) {
    // Sessão em que o poke foi (ou será) entregue: gravada na delivery só
    // quando o estado vira `sent` — é a prova causal do mark_answered_in_session.
    let mut delivered_session: Option<String> = None;
    let outcome: Result<Outcome> = async {
        let prepared = {
            let hub = hub.clone();
            let p = Pending {
                space_id: p.space_id.clone(),
                project_id: p.project_id.clone(),
                task_id: p.task_id.clone(),
                comment_id: p.comment_id.clone(),
                bot_id: p.bot_id.clone(),
                attempts: p.attempts,
            };
            tokio::task::spawn_blocking(move || -> Result<Prepared> {
                let db = hub.store.db.locked();
                let tx = db.unchecked_transaction()?;
                let prepared = prepare_delivery(&tx, &hub.host_id, &p)?;
                // ensure_session escreve: commit junto da leitura quando Send.
                if matches!(prepared, Prepared::Send { .. }) {
                    tx.commit()?;
                }
                Ok(prepared)
            })
            .await??
        };
        let (session_id, text, attachments, project, task_title) = match prepared {
            Prepared::Send { session_id, text, attachments, project, task_title } => {
                (session_id, text, attachments, project, task_title)
            }
            Prepared::Reject(reason) => return Ok(Outcome::Failed(reason)),
        };
        delivered_session = Some(session_id.clone());
        let nonce = nonce(&p.comment_id, &p.bot_id);
        if hub.store.find_by_nonce(&p.bot_id, &nonce).is_some() {
            return Ok(Outcome::Sent);
        }
        let body = format!(
            "Comentário do dono na tarefa «{}» (projeto «{}», tarefa {}, comentário {}):\n\n{}",
            task_title, project, p.task_id, p.comment_id, text
        );
        match crate::api::dispatch(
            hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({
                "botId": p.bot_id,
                "sessionId": session_id,
                "text": body,
                "clientNonce": nonce,
                "attachments": attachments,
            }),
        )
        .await
        {
            Ok(_) => Ok(Outcome::Sent),
            Err(error) => {
                // O send pode ter aplicado a entrada antes de falhar: sem prova
                // atômica de dedupe, reenvio cego duplicaria o turno. Marca
                // `uncertain` — visível, sem retry automático.
                if hub.store.find_by_nonce(&p.bot_id, &nonce).is_some() {
                    return Ok(Outcome::Sent);
                }
                if crate::fork::owner_question::queue_full(&error) {
                    return Ok(Outcome::Wait);
                }
                tracing::warn!(%error, bot = %p.bot_id, "tarefas_runtime: entrega de aviso falhou");
                Ok(if p.attempts + 1 >= MAX_ATTEMPTS { Outcome::Failed("delivery_failed") } else { Outcome::Retry })
            }
        }
    }
    .await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::warn!(%error, bot = %p.bot_id, "tarefas_runtime: preparação de entrega falhou");
            if p.attempts + 1 >= MAX_ATTEMPTS { Outcome::Failed("delivery_failed") } else { Outcome::Retry }
        }
    };
    let wait = matches!(outcome, Outcome::Wait);
    let (status, reason) = match outcome {
        Outcome::Sent => ("sent", None),
        Outcome::Retry | Outcome::Wait => ("queued", None),
        Outcome::Failed(reason) => ("failed", Some(reason)),
    };
    let hub = hub.clone();
    let p = Pending {
        space_id: p.space_id.clone(),
        project_id: p.project_id.clone(),
        task_id: p.task_id.clone(),
        comment_id: p.comment_id.clone(),
        bot_id: p.bot_id.clone(),
        attempts: p.attempts,
    };
    let _ = tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        let settled = tx.execute(
            "UPDATE donna_task_deliveries SET status=?7,reason=?8,
             session_id=CASE WHEN ?7='sent' THEN COALESCE(session_id,?9) ELSE session_id END
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6
             AND status='sending'",
            params![
                hub.host_id,
                p.space_id,
                p.project_id,
                p.task_id,
                p.comment_id,
                p.bot_id,
                status,
                reason,
                delivered_session
            ],
        )?;
        if wait && settled == 1 {
            tx.execute(
                "UPDATE donna_task_deliveries SET attempts=MAX(attempts-1,0)
                 WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6",
                params![hub.host_id, p.space_id, p.project_id, p.task_id, p.comment_id, p.bot_id],
            )?;
        }
        if status == "queued" && settled == 1 {
            // Backoff por tentativa: created_at funciona como "disponível em".
            tx.execute(
                "UPDATE donna_task_deliveries SET created_at=?7,reason=NULL
                 WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6",
                params![
                    hub.host_id,
                    p.space_id,
                    p.project_id,
                    p.task_id,
                    p.comment_id,
                    p.bot_id,
                    now_ms() + 5_000 * (p.attempts + 1)
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await;
}

/// Agregado honesto do notify de um comentário (TEC-15 / contrato UI
/// `thr_94s5gpgkkd`): `recipients` traz UM item por entrega com
/// `{botId, state, reason?}` usando o estado cru da delivery; `state` é o
/// agregado — `not_notified` SOMENTE quando nenhum destinatário recebeu,
/// `partial` em qualquer mistura terminal (ex.: answered+failed), e
/// `notifying`/`uncertain`/`responded` conforme o pior caso pendente.
pub(crate) fn notify_summary(rows: &[(String, String, Option<String>)]) -> Option<Value> {
    if rows.is_empty() {
        return None;
    }
    let states: Vec<&str> = rows.iter().map(|(_, s, _)| s.as_str()).collect();
    let recipients: Vec<Value> = rows
        .iter()
        .map(|(b, s, r)| {
            let mut item = json!({"botId": b, "state": s});
            if let Some(reason) = r {
                item["reason"] = json!(reason);
            }
            item
        })
        .collect();
    let answered = states.iter().filter(|s| **s == "answered").count();
    let failed = states.iter().filter(|s| **s == "failed").count();
    let pending = states.iter().any(|s| matches!(*s, "queued" | "sending" | "sent"));
    let state = if states.contains(&"uncertain") {
        "uncertain"
    } else if pending {
        "notifying"
    } else if answered == states.len() {
        "responded"
    } else if failed == states.len() {
        "not_notified"
    } else {
        "partial"
    };
    let reason = if state == "not_notified" { Some("delivery_failed") } else { None };
    Some(json!({
        "botIds": rows.iter().map(|(b, _, _)| b.clone()).collect::<Vec<_>>(),
        "state": state,
        "reason": reason,
        "recipients": recipients,
    }))
}

/// `status_of` para a leitura de comentários: agrega as entregas do comentário
/// no objeto `notify` que a UI já conhece via `notify_summary`.
/// `None` = sem entregas registradas.
pub(crate) async fn status_of(
    hub: &Arc<Hub>,
    space: &str,
    project: &str,
    task: &str,
    comment_id: &str,
) -> Result<Option<Value>> {
    let hub = hub.clone();
    let (space, project, task, comment_id) =
        (space.to_owned(), project.to_owned(), task.to_owned(), comment_id.to_owned());
    tokio::task::spawn_blocking(move || -> Result<Option<Value>> {
        let db = hub.store.db.locked();
        let mut st = db.prepare(
            "SELECT bot_id,status,reason FROM donna_task_deliveries
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5",
        )?;
        let rows = st
            .query_map(params![hub.host_id, space, project, task, comment_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(notify_summary(&rows))
    })
    .await?
}

/// Variante transacional para o Hub-A chamar DENTRO da tx do comentário:
/// elimina a janela commit-comentário → marca (crash deixaria 'sent'
/// para sempre). Mesma prova causal: `session_id` + `message_turn` do poke entry.
#[allow(
    clippy::too_many_arguments,
    reason = "Mesma assinatura da prova causal store::mark_answered_in_session; tx não pode agrupar sem cópia."
)]
pub(crate) fn mark_answered_tx(
    c: &Connection,
    hub: &str,
    space: &str,
    project: &str,
    task: &str,
    comment_id: &str,
    bot_id: &str,
    session_id: &str,
    message_turn: i64,
) -> Result<bool> {
    store::mark_answered_in_session(c, hub, space, project, task, comment_id, bot_id, session_id, message_turn)
}

/// Boot: `sending` cuja nonce prova aceite vira `sent`; as demais voltam a
/// `queued` (a reentrega usa a mesma nonce → dedupe real do `send`).
pub(crate) fn restore_sending(c: &Connection, hub_id: &str) -> Result<()> {
    // Só usa `c`: reabrir `hub.store.db` aqui deadlockaria o boot.
    let mut st = c.prepare(
        "SELECT space_id,project_id,task_id,comment_id,bot_id FROM donna_task_deliveries
         WHERE hub_id=?1 AND status='sending'",
    )?;
    let rows = st
        .query_map(params![hub_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (space, project, task, comment, bot) in rows {
        let attempt = nonce(&comment, &bot);
        // Mesma prova do find_by_nonce, na conexão já travada — e recupera a
        // sessão da entry entregue: sem ela o ack (mark_answered_in_session)
        // nunca conseguiria a prova causal e a entrega ficaria 'sent' eterna.
        let delivered: Option<String> = c
            .query_row(
                "SELECT session FROM entries WHERE bot_id=?1 AND kind='user'
                 AND json_valid(data) AND json_extract(data,'$.clientNonce')=?2 LIMIT 1",
                params![bot, attempt],
                |r| r.get(0),
            )
            .optional()?;
        c.execute(
            "UPDATE donna_task_deliveries SET status=?7,session_id=COALESCE(session_id,?8)
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6",
            params![
                hub_id,
                space,
                project,
                task,
                comment,
                bot,
                if delivered.is_some() { "sent" } else { "queued" },
                delivered
            ],
        )?;
    }
    Ok(())
}
