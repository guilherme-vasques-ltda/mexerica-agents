//! Disparo do início de tarefa: claim durável → sessão vinculada → poke com
//! nonce determinístico → marcação. O `send` deduplica por `clientNonce`, então
//! a prova de "já disparou" é a entrada existente, nunca uma suposição.
use super::store::{self, Agenda, Gate};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{autoridade::Autoridade, sessoes::bindings},
    hub::Hub,
    store::now_ms,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::sync::Arc;

const STALE_STARTING_MS: i64 = 30_000;
const MAX_ATTEMPTS: i64 = store::MAX_ATTEMPTS;

pub(crate) struct Fire {
    pub space_id: String,
    pub project_id: String,
    pub task_id: String,
    pub bot_id: String,
    pub title: String,
    pub project_name: String,
    pub nonce: String,
    pub attempts: i64,
    pub session_id: Option<String>,
    pub origin: String,
    /// Resumo da tarefa para o destinatário da delegação (descrição+aceite
    /// truncados). Vazio nos caminhos que não delegam.
    pub brief: String,
}

fn poke_text(f: &Fire) -> String {
    format!(
        "Início de tarefa. Projeto «{}»: «{}».\n\nVocê é o responsável por esta subtarefa no Hub. \
         Leia a tarefa e o contexto pelo projeto e execute o trabalho agora; atualize estado e \
         evidências pelas ferramentas de tarefas e comente o progresso.{}",
        f.project_name,
        f.title,
        if f.origin.is_empty() { String::new() } else { format!("\n\nOrigem do início: {}.", f.origin) }
    )
}

fn attempt_nonce(f: &Fire) -> String {
    // Nonce estável por claim: uma nova tentativa após crash reenvia a MESMA
    // nonce, então o dedupe do `send` prova o aceite em vez de duplicar turno.
    format!("mx142-task-start-{}", f.nonce)
}

/// Prova de aceite POR NONCE, sem escopo de bot (TEC P2-27): o clientNonce
/// `mx142-task-*` carrega o uuid do claim — único e de namespace reservado
/// por trigger — então uma entry aceita sob o responsável ANTERIOR é a
/// mesma tentativa. Filtrar por `bot_id` atual não acharia a evidência após
/// reatribuição e o retry criaria uma SEGUNDA entry com o mesmo nonce.
pub(crate) fn nonce_accepted(c: &Connection, nonce: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE kind='user'
         AND json_valid(data) AND json_extract(data,'$.clientNonce')=?1)",
        params![nonce],
        |r| r.get(0),
    )?)
}

async fn nonce_accepted_on(hub: &Arc<Hub>, nonce: &str) -> Result<bool> {
    let hub = hub.clone();
    let nonce = nonce.to_owned();
    tokio::task::spawn_blocking(move || {
        let db = hub.store.db.locked();
        nonce_accepted(&db, &nonce)
    })
    .await?
}

/// Reconciliação pós-erro de UM fire (TEC P2-31): a prova de aceite por
/// nonce — tentativa única, uuid, namespace reservado — vence qualquer
/// recusa local (sessão, revalidação, dispatch, dedupe). Sem prova o erro
/// propaga INTACTO: nunca mascaramos falha real como sucesso.
/// `pub(crate)` para teste: é a função real chamada por `perform` nos
/// três caminhos de erro — remover a reconciliação muda este resultado.
pub(crate) fn reconcile_delivery(c: &Connection, nonce: &str, error: anyhow::Error) -> Result<bool> {
    if nonce_accepted(c, nonce)? { Ok(true) } else { Err(error) }
}

async fn reconcile_on_error(hub: &Arc<Hub>, nonce: &str, error: anyhow::Error) -> Result<bool> {
    let hub = hub.clone();
    let nonce = nonce.to_owned();
    tokio::task::spawn_blocking(move || {
        let db = hub.store.db.locked();
        reconcile_delivery(&db, &nonce, error)
    })
    .await?
}

/// Prefixo dos nonces de início delegado via bot (D220 `message_bot`,
/// one-shot). Essas linhas NUNCA passam por `perform` — não há `ExecutionRef`
/// vivo no tick e o `send` do dono emprestaria autoridade.
const BOT_NONCE_PREFIX: &str = "mx142-bot-";

fn is_bot_fire(nonce: &str) -> bool {
    nonce.starts_with(BOT_NONCE_PREFIX)
}

fn clone_fire(f: &Fire) -> Fire {
    Fire {
        space_id: f.space_id.clone(),
        project_id: f.project_id.clone(),
        task_id: f.task_id.clone(),
        bot_id: f.bot_id.clone(),
        title: f.title.clone(),
        project_name: f.project_name.clone(),
        nonce: f.nonce.clone(),
        attempts: f.attempts,
        session_id: f.session_id.clone(),
        origin: f.origin.clone(),
        brief: f.brief.clone(),
    }
}

/// Sessão vinculada durável + `session_id` na agenda (mesma lógica do perform).
async fn linked_session(hub: &Arc<Hub>, f: &Fire) -> Result<String> {
    let hub = hub.clone();
    let f = Fire {
        space_id: f.space_id.clone(),
        project_id: f.project_id.clone(),
        task_id: f.task_id.clone(),
        bot_id: f.bot_id.clone(),
        title: f.title.clone(),
        project_name: f.project_name.clone(),
        nonce: f.nonce.clone(),
        attempts: f.attempts,
        session_id: f.session_id.clone(),
        origin: f.origin.clone(),
        brief: f.brief.clone(),
    };
    tokio::task::spawn_blocking(move || -> Result<String> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        let id = store::ensure_session(
            &tx,
            &hub.host_id,
            &f.space_id,
            &f.project_id,
            &f.task_id,
            &f.bot_id,
            &format!("{} — {}", f.project_name, f.title),
        )?;
        // Fenced por nonce: fire de claim velho (reagendado) não escreve
        // session_id sobre a agenda nova — affected=0 denuncia o stale.
        let written = tx.execute(
            "UPDATE donna_task_agenda SET session_id=?5,updated_at=?6
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?7",
            params![hub.host_id, f.space_id, f.project_id, f.task_id, id, now_ms(), f.nonce],
        )?;
        ensure!(written > 0, "claim_stale: agenda reagendada durante a execução.");
        tx.commit()?;
        Ok(id)
    })
    .await?
}

/// Evidência durável da delegação D220: o aviso do destinatário gravado por
/// Prova de admissão D220 — SOMENTE contexto estruturado autenticado
/// (TEC R2-22): `botMessage.taskDispatch.nonce` só existe porque o caminho
/// interno tipado o gravou na mesma tx do `insert_team_notices` (o modelo não
/// forja via argumentos), ou `delegationId` igual ao `request_id` já
/// persistido na agenda (legado admitido com vínculo gravado). Texto livre
/// NUNCA é prova — citação/forja de `Referência de despacho:` não conta.
/// Sem evidência => nada commitou (tx atômica) => redispatch seguro;
/// com evidência => execução incerta, sem replay.
async fn delegation_evidence(hub: &Arc<Hub>, f: &Fire) -> Result<bool> {
    let hub = hub.clone();
    let f = clone_fire(f);
    tokio::task::spawn_blocking(move || -> Result<bool> {
        let db = hub.store.db.locked();
        let request_id: Option<String> = db
            .query_row(
                "SELECT request_id FROM donna_task_agenda
                 WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?5",
                params![hub.host_id, f.space_id, f.project_id, f.task_id, f.nonce],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        // Sem filtro de bot (TEC P2-27): taskDispatch.nonce é uuid do claim
        // e delegationId é único por delegação — a notice admitida sob o
        // responsável anterior continua sendo a MESMA evidência; filtrar
        // por bot atual marcaria 'failed' uma admissão que commitou.
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries e WHERE e.kind='notice' AND (
               json_extract(e.data,'$.botMessage.taskDispatch.nonce')=?1
               OR (?2 IS NOT NULL AND json_extract(e.data,'$.delegationId')=?2)))",
            params![f.nonce, request_id],
            |r| r.get(0),
        )?)
    })
    .await?
}

/// Resumo da tarefa para o destinatário: descrição + aceite truncados.
/// Contexto de leitura — o bot valida pelas tools, não por este texto.
pub(crate) fn task_brief(task: &Value) -> String {
    let desc = task["description"].as_str().unwrap_or_default();
    let acc = task["acceptance"].as_str().unwrap_or_default();
    let mut brief = String::new();
    if !desc.is_empty() {
        brief.push_str(desc);
    }
    if !acc.is_empty() {
        if !brief.is_empty() {
            brief.push_str("\n\nCritério de aceite: ");
        }
        brief.push_str(acc);
    }
    brief.chars().take(1200).collect()
}

/// Desfecho do kickoff delegado.
enum Kickoff {
    /// `message_bot` devolveu requestId — delegação admitida na fila D220.
    Sent {
        request_id: String,
    },
    /// Self-start: a execução viva já é o contexto; não há delegação.
    SelfStart,
    /// Evidência durável sem prova de execução (crash na janela
    /// commit→fila em memória). Nunca redispatch cego.
    Uncertain,
    Failed(String),
}

/// Kickoff via bot: `message_bot` D220 na lane principal do responsável,
/// com o `ExecutionRef` vivo do chamador — cadeia, hops e Retornos intactos.
/// `add_team_notices` commita antes do `send_cmd` da fila em memória: um erro
/// aqui pode esconder delegação já admitida, então a evidência decide.
async fn perform_bot_delegation(
    hub: &Arc<Hub>,
    execution: &bindings::ExecutionRef,
    requester: &str,
    f: &Fire,
) -> Kickoff {
    let brief = if f.brief.is_empty() { String::new() } else { format!("\n\nResumo da tarefa:\n{}", f.brief) };
    // R2-23: projectId/taskId no texto são IDENTIFICADORES para as tools do
    // destinatário (task_read/update/comment exigem coords) — não autoridade:
    // a prova do despacho segue sendo taskDispatch.nonce -> agenda corrente.
    let message = format!(
        "Início de tarefa delegado.{}\n\n{}\n\nTarefa: projectId={}, taskId={}\n\nReferência de despacho: {}",
        brief,
        poke_text(f),
        f.project_id,
        f.task_id,
        f.nonce
    );
    // Caminho tipado interno (R2-22): mesmo message_bot/D220/Retornos, com
    // TaskDispatch estruturado — o guard de admissão resolve o claim sem
    // parsing do texto livre. O sufixo textual fica só como referência.
    let dispatch = crate::chat::team::TaskDispatch {
        nonce: f.nonce.clone(),
        hub_id: hub.host_id.clone(),
        space_id: f.space_id.clone(),
        project_id: f.project_id.clone(),
        task_id: f.task_id.clone(),
    };
    match crate::chat::team::message_bot_task_dispatch(hub, requester, &f.bot_id, &message, Some(execution), &dispatch)
    {
        Ok(out) => match out["requestId"].as_str() {
            Some(id) => Kickoff::Sent { request_id: id.to_owned() },
            None => Kickoff::Uncertain,
        },
        Err(error) => match delegation_evidence(hub, f).await {
            Ok(true) => Kickoff::Uncertain,
            _ => Kickoff::Failed(format!("{error:#}")),
        },
    }
}

/// Fogo via bot órfão (crash entre claim e despacho): NUNCA passa por
/// `perform` — sem `ExecutionRef` vivo e o `send` do dono emprestaria
/// autoridade. Evidência admitida => `uncertain`; sem => `failed` honesto.
async fn settle_orphan_bot_start(hub: &Arc<Hub>, f: &Fire) -> Result<()> {
    let admitted = delegation_evidence(hub, f).await.unwrap_or(false);
    let hub = hub.clone();
    let f = clone_fire(f);
    let (state, error): (&str, Option<&str>) =
        if admitted { ("uncertain", Some("delegation_unconfirmed")) } else { ("failed", Some("dispatch_interrupted")) };
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "UPDATE donna_task_agenda SET state=?5,last_error=?6,updated_at=?7
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?8",
            params![hub.host_id, f.space_id, f.project_id, f.task_id, state, error, now_ms(), f.nonce],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?
}

/// Garante sessão e dispara o poke. Devolve `true` quando a entrada foi
/// aceita pelo `send` (agora ou em tentativa anterior — dedupe por nonce).
/// Nunca retorna sucesso sem a entrada durável correspondente.
/// Revalidação imediatamente antes do efeito: o claim commita e o send/D220
/// acontece FORA da transação — tarefa pode ter sido excluída, revogada,
/// encerrada ou ter trocado de responsável na janela. A checagem fica
/// adjacente ao dispatch (janela residual mínima, sem tx distribuída).
/// `pub(crate)` para teste: `revalidate_on` é a parte síncrona.
pub(crate) fn revalidate_on(c: &Connection, hub: &str, f: &Fire) -> Result<()> {
    match store::gate(c, hub, &f.space_id, &f.project_id, &f.task_id)? {
        Gate::Fire { bot_id, .. } if bot_id == f.bot_id => Ok(()),
        Gate::Fire { .. } => Err(anyhow::anyhow!("invalid_field: responsável da tarefa mudou.")),
        Gate::Refused(reason) => Err(anyhow::anyhow!("invalid_field: {reason}")),
    }
}

async fn revalidate_before_effect(hub: &Arc<Hub>, f: &Fire) -> Result<()> {
    let hub = hub.clone();
    let f = clone_fire(f);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        revalidate_on(&db, &hub.host_id, &f)
    })
    .await?
}

async fn perform(hub: &Arc<Hub>, f: &Fire) -> Result<bool> {
    let nonce = attempt_nonce(f);
    // Evidência primeiro, POR NONCE e sem escopo de bot (TEC P2-27): se a
    // entrada já existe — mesmo aceita sob o responsável anterior — o poke
    // já foi dado e o retry não pode duplicar o efeito.
    if nonce_accepted_on(hub, &nonce).await? {
        return Ok(true);
    }
    // Todo erro APÓS a suspensão reconcilia com a prova de aceite por
    // nonce ANTES de propagar (TEC P2-31): se outro send commitou a
    // tentativa enquanto este fire estava suspenso (ex.: claim recuperado
    // para o novo responsável, que aceitou N), a recusa local — sessão,
    // revalidação ou dispatch — não pode virar 'failed'/'scheduled'
    // durável sobre prova existente. Sem prova, o erro propaga intacto.
    let session_id = match linked_session(hub, f).await {
        Ok(id) => id,
        Err(error) => return reconcile_on_error(hub, &nonce, error).await,
    };
    if nonce_accepted_on(hub, &nonce).await? {
        return Ok(true);
    }
    // Janela claim->efeito: exclusão/revogação na janela é recusada aqui,
    // imediatamente antes do dispatch.
    if let Err(error) = revalidate_before_effect(hub, f).await {
        return reconcile_on_error(hub, &nonce, error).await;
    }
    let sent = crate::api::dispatch(
        hub,
        &Caller::Local,
        "send",
        json!({
            "botId": f.bot_id,
            "sessionId": session_id,
            "text": poke_text(f),
            "clientNonce": nonce,
            "attachments": [],
        }),
    )
    .await;
    match sent {
        Ok(_) => Ok(true),
        Err(error) => {
            // TEC P2-31: a recusa pode ser a NOSSA dedupe atômica — outro
            // send com o mesmo nonce commitou primeiro (ex.: claim aceito
            // sob o responsável anterior). Antes de propagar falha,
            // reconcilia com a prova de aceite POR NONCE: se a entry
            // existe, o efeito JÁ aconteceu — 'started' honesto, nunca um
            // recibo 'failed' durável sobre prova existente.
            if let Ok(true) = reconcile_on_error(hub, &nonce, anyhow::anyhow!("{error}")).await {
                return Ok(true);
            }
            // Sem prova: a entrada pode ter sido gravada entre esta
            // checagem e o fim do send — a próxima tentativa prova pela
            // nonce. Propagamos como falha retentável.
            tracing::warn!(%error, bot = %f.bot_id, "tarefas_runtime: poke de início recusado");
            Err(error)
        }
    }
}

pub(crate) fn mark(c: &Connection, hub: &str, f: &Fire, state: &str, error: Option<&str>, bump: bool) -> Result<()> {
    let attempts = if bump { f.attempts + 1 } else { f.attempts };
    let not_before = if state == "scheduled" { now_ms() + (5_000i64 << attempts.min(6)) } else { now_ms() };
    c.execute(
        "UPDATE donna_task_agenda SET state=?5,attempts=?6,last_error=?7,not_before=?8,updated_at=?9
         WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?10",
        params![hub, f.space_id, f.project_id, f.task_id, state, attempts, error, not_before, now_ms(), f.nonce],
    )?;
    Ok(())
}

/// Marca final compartilhada por RPC e tick. `scheduled` só quando ainda há
/// tentativa; acima do limite vira `failed` visível.
async fn conclude(hub: &Arc<Hub>, f: &Fire, ok: bool, error: Option<String>) -> Result<()> {
    let hub = hub.clone();
    let f = Fire {
        space_id: f.space_id.clone(),
        project_id: f.project_id.clone(),
        task_id: f.task_id.clone(),
        bot_id: f.bot_id.clone(),
        title: f.title.clone(),
        project_name: f.project_name.clone(),
        nonce: f.nonce.clone(),
        attempts: f.attempts,
        session_id: f.session_id.clone(),
        origin: f.origin.clone(),
        brief: f.brief.clone(),
    };
    let error = error.map(|e| e.chars().take(400).collect::<String>());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        let (state, bump) = if ok {
            ("started", false)
        } else if error.as_deref().is_some_and(|e| e.contains("queue_full")) {
            // Fila da conversa cheia: tenta de novo mais tarde sem gastar tentativa.
            ("scheduled", false)
        } else if f.attempts + 1 >= MAX_ATTEMPTS {
            ("failed", true)
        } else {
            ("scheduled", true)
        };
        mark(&tx, &hub.host_id, &f, state, error.as_deref(), bump)?;
        tx.commit()?;
        Ok(())
    })
    .await?
}

fn coords_of(body: &Value) -> Result<(String, String, String, String, String)> {
    let get =
        |k: &str| body[k].as_str().map(str::to_owned).ok_or_else(|| anyhow::anyhow!("invalid_field: {k} ausente."));
    let hub_id = get("hubId")?;
    let space_id = get("spaceId")?;
    let project_id = get("projectId")?;
    let task_id = get("taskId")?;
    let request = get("clientRequestId")?;
    for id in [&hub_id, &space_id, &project_id, &task_id, &request] {
        uuid::Uuid::parse_str(id).map_err(|_| anyhow::anyhow!("invalid_field: identificador inválido."))?;
    }
    Ok((hub_id, space_id, project_id, task_id, request))
}

fn start_answer(state: &str, reason: Option<&str>) -> Value {
    json!({"receipt": {}, "start": {"state": state, "reason": reason}})
}

/// `donnaTaskStart` do dono: claim+disparo inline para devolver o estado real.
pub(crate) async fn start_rpc(hub: &Arc<Hub>, authority: &Autoridade, body: Value) -> Result<Value> {
    enum Step {
        Answer(Value),
        Fire(Fire),
    }
    let (hub_id, space_id, project_id, task_id, request) = coords_of(&body)?;
    ensure!(hub_id == hub.host_id, "forbidden");
    let payload = serde_json::to_string(&body)?;
    let step = authority.with_db(hub, None, |c| -> Result<Step> {
        crate::fork::tarefas_runtime::scope(c, &hub.host_id, &space_id, &hub.host_id)?;
        if let Some(saved) = c
            .query_row(
                "SELECT payload,result FROM donna_task_receipts WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
                params![hub.host_id, space_id, request],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure!(saved.0 == payload, "idempotency_conflict: clientRequestId reutilizado com payload diferente.");
            return Ok(Step::Answer(serde_json::from_str(&saved.1)?));
        }
        match store::gate(c, &hub.host_id, &space_id, &project_id, &task_id)? {
            Gate::Refused(reason) => {
                let answer = start_answer("refused", Some(reason.as_str()));
                let final_answer = json!({"receipt": {"clientRequestId": request.as_str()}, "start": answer["start"].clone()});
                c.execute(
                    "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
                    params![hub.host_id, space_id, request, payload, serde_json::to_string(&final_answer)?],
                )?;
                Ok(Step::Answer(final_answer))
            }
            Gate::Fire { bot_id, title, project_name } => {
                let now = now_ms();
                let existing = store::agenda_get(c, &hub.host_id, &space_id, &project_id, &task_id)?;
                if let Some(a) = &existing {
                    match a.state.as_str() {
                        "started" => {
                            let answer = json!({"receipt": {"clientRequestId": request.as_str()}, "start": {"state": "already_running"}});
                            c.execute(
                                "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
                                params![hub.host_id, space_id, request, payload, serde_json::to_string(&answer)?],
                            )?;
                            return Ok(Step::Answer(answer));
                        }
                        "starting" if now - c.query_row::<i64, _, _>(
                            "SELECT updated_at FROM donna_task_agenda WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
                            params![hub.host_id, space_id, project_id, task_id],
                            |r| r.get(0),
                        )? < STALE_STARTING_MS => {
                            let answer = json!({"receipt": {"clientRequestId": request.as_str()}, "start": {"state": "already_running"}});
                            c.execute(
                                "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
                                params![hub.host_id, space_id, request, payload, serde_json::to_string(&answer)?],
                            )?;
                            return Ok(Step::Answer(answer));
                        }
                        _ => {}
                    }
                }
                // Novo início lógico: nonce novo, tentativas zeradas, disparo agora.
                let nonce = uuid::Uuid::new_v4().to_string();
                let agenda = Agenda {
                    bot_id: bot_id.clone(),
                    start_at: now,
                    state: "starting".into(),
                    session_id: existing.as_ref().and_then(|a| a.session_id.clone()),
                    attempts: 0,
                    nonce,
                    last_error: None,
                    request_id: None,
                };
                store::save_agenda(c, &hub.host_id, &space_id, &project_id, &task_id, &agenda, now)?;
                Ok(Step::Fire(Fire {
                    space_id,
                    project_id,
                    task_id,
                    bot_id,
                    title,
                    project_name,
                    nonce: agenda.nonce,
                    attempts: 0,
                    session_id: agenda.session_id,
                    origin: String::new(),
                    brief: String::new(),
                }))
            }
        }
    })?;
    let fire = match step {
        Step::Answer(answer) => return Ok(answer),
        Step::Fire(fire) => fire,
    };
    let space = fire.space_id.clone();
    let result = perform(hub, &fire).await;
    let ok = result.is_ok();
    // Fila da conversa cheia: a agenda fica `scheduled` e tenta de novo sem gastar tentativa.
    let waiting = result.as_ref().err().is_some_and(crate::fork::owner_question::queue_full);
    let error = match (ok, waiting) {
        (true, _) => None,
        (false, true) => Some("queue_full".to_owned()),
        (false, false) => Some("send_failed".to_owned()),
    };
    conclude(hub, &fire, ok, error).await?;
    // O recibo segue o contrato do app (`started`/`failed`); com a fila cheia a agenda tenta de novo sozinha.
    let (state, reason) = match (ok, waiting) {
        (true, _) => ("started", Value::Null),
        (false, true) => ("failed", json!("queue_full")),
        (false, false) => ("failed", json!("send_unavailable")),
    };
    let answer = json!({
        "receipt": {"clientRequestId": request.as_str()},
        "start": {"state": state, "reason": reason},
    });
    // Recibo final com o resultado real (o claim não grava recibo).
    let hub = hub.clone();
    let stored = answer.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
            params![hub.host_id, space, request, payload, serde_json::to_string(&stored)?],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await??;
    Ok(answer)
}

/// Vaga de início de proposta no Space: no máximo duas ativas no total e uma
/// por bot. Ativa é linha com `proposal_id`, estado starting/started e tarefa
/// em `backlog/in_progress` no JSON do Projeto — review, done, blocked,
/// cancelled e tarefa removida liberam a vaga.
fn proposal_slot(c: &Connection, hub: &str, space: &str, bot: &str, now: i64) -> Result<bool> {
    let base = "SELECT COUNT(*) FROM donna_task_agenda a, donna_projects p, json_each(p.tasks) t
         WHERE a.hub_id=?1 AND a.space_id=?2 AND a.proposal_id IS NOT NULL
           AND a.state IN ('starting','started')
           AND (a.state='started' OR a.updated_at>?3 OR EXISTS(
             SELECT 1 FROM entries e WHERE e.kind='user' AND json_valid(e.data)
              AND json_extract(e.data,'$.clientNonce')='mx142-task-start-'||a.nonce))
           AND p.hub_id=a.hub_id AND p.space_id=a.space_id AND p.id=a.project_id
           AND t.value->>'id'=a.task_id AND t.value->>'state' IN ('backlog','in_progress')";
    let total: i64 = c.query_row(base, params![hub, space, now - STALE_STARTING_MS], |r| r.get(0))?;
    if total >= 2 {
        return Ok(false);
    }
    let mine: i64 =
        c.query_row(&format!("{base} AND a.bot_id=?4"), params![hub, space, now - STALE_STARTING_MS, bot], |r| {
            r.get(0)
        })?;
    Ok(mine < 1)
}

/// Tick do agendador: reivindica `scheduled` vencidas e `starting` órfãs,
/// dispara cada poke e conclui. Chamado por `tarefas_runtime::start`.
/// Claim das linhas vencidas do agendador, tudo numa tx: revalida gate por
/// linha (tarefa removida -> refused terminal, sem envenenar o lote), claima
/// fenced por nonce+state e devolve os fires. `pub(crate)` para teste.
pub(crate) fn claim_due(c: &Connection, hub: &str, now: i64) -> Result<Vec<Fire>> {
    let tx = c.unchecked_transaction()?;
    let mut st = tx.prepare(
        "SELECT space_id,project_id,task_id,bot_id,start_at,state,session_id,attempts,nonce,proposal_id,last_error,updated_at
         FROM donna_task_agenda
         WHERE hub_id=?1 AND ((state='scheduled' AND not_before<=?2)
            OR (state='starting' AND updated_at<=?3))",
    )?;
    let rows = st.query_map(params![hub, now, now - STALE_STARTING_MS], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, Option<String>>(9)?,
        ))
    })?;
    let mut fires = Vec::new();
    for row in rows {
        let (space, project, task, agenda_bot, start_at, state, session, attempts, nonce, proposal_id) = row?;
        // Revalida gate antes de gastar poke: tarefa sumiu/sem responsável → refused.
        // Tarefa/projeto removido é PERMANENTE (invalid_field): marca
        // refused terminal e segue — antes um único sumiço envenenava
        // o tick inteiro. Erros transitórios propagam.
        let gate = match store::gate(&tx, hub, &space, &project, &task) {
            Ok(gate) => gate,
            Err(error) => {
                if error.to_string().starts_with("invalid_field") {
                    tx.execute(
                        "UPDATE donna_task_agenda SET state='refused',last_error=?5,updated_at=?6
                         WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?7",
                        params![hub, space, project, task, "task_unavailable", now_ms(), nonce],
                    )?;
                    continue;
                }
                return Err(error);
            }
        };
        let (bot, title, project_name) = match gate {
            Gate::Fire { bot_id, title, project_name } => (bot_id, title, project_name),
            Gate::Refused(reason) => {
                tx.execute(
                    "UPDATE donna_task_agenda SET state='refused',last_error=?5,updated_at=?6
                     WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?7",
                    params![hub, space, project, task, reason, now_ms(), nonce],
                )?;
                continue;
            }
        };
        // Linha de proposta de tarefa que já avançou de estado é obsoleta:
        // o início automático não faz mais sentido e a vaga é liberada.
        if proposal_id.is_some() {
            let row = store::load_task(&tx, hub, &space, &project, &task)?;
            let task_state = row.task["state"].as_str().unwrap_or_default().to_owned();
            if !matches!(task_state.as_str(), "backlog" | "in_progress") {
                tx.execute(
                    "UPDATE donna_task_agenda SET state='refused',last_error='inicio dispensado: a tarefa avancou de estado',updated_at=?5
                     WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND nonce=?6",
                    params![hub, space, project, task, now_ms(), nonce],
                )?;
                continue;
            }
        }
        // Teto de início por proposta: quem não cabe no Space fica `scheduled`
        // e é reavaliada no próximo tick, sem gastar tentativa. Órfã sem
        // entrega provada não segura vaga; entrega provada ou claim recente
        // seguem contando, inclusive as claimadas neste mesmo laço.
        if proposal_id.is_some() && state == "scheduled" && !proposal_slot(&tx, hub, &space, &bot, now)? {
            continue;
        }
        // Órfã de `starting` reatribuída a outro bot: a vaga do destino é
        // conferida antes de transferir — um bot já ativo não recebe segunda
        // tarefa de proposta pela recuperação. Sem vaga, a linha espera o
        // próximo tick, sem gastar tentativa.
        if proposal_id.is_some()
            && state == "starting"
            && agenda_bot != bot
            && proposal_bot_busy(&tx, hub, &space, &bot, &task, now)?
        {
            continue;
        }
        // Claim fenced por nonce+state: um reagendamento concorrente
        // trocou o nonce — a linha não casa, o fire velho não dispara.
        // Reconcilia bot_id com o responsável atual do gate (TEC-25): se a
        // tarefa foi reatribuída sem reagendar, o claim passa a apontar
        // para o bot que o guard de aceite vai exigir; session_id de bot
        // divergente é limpo (a sessão vinculada é por bot, em
        // donna_task_sessions — a linha antiga fica como auditoria).
        let claimed = tx.execute(
            "UPDATE donna_task_agenda SET state='starting',bot_id=?8,updated_at=?5,
               session_id=CASE WHEN bot_id<>?8 THEN NULL ELSE session_id END
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND state=?6 AND nonce=?7",
            params![hub, space, project, task, now_ms(), state, nonce, bot],
        )?;
        if claimed == 0 {
            continue;
        }
        // A sessão vinculada só vale para o mesmo bot após reatribuição.
        let session_id = if agenda_bot == bot { session } else { None };
        fires.push(Fire {
            space_id: space,
            project_id: project,
            task_id: task,
            bot_id: bot,
            title,
            project_name,
            nonce,
            attempts,
            session_id,
            origin: String::new(),
            brief: String::new(),
        });
        let _ = start_at;
    }
    drop(st);
    tx.commit()?;
    Ok(fires)
}

/// Tick do agendador: reivindica `scheduled` vencidas e `starting` órfãs,
/// dispara cada poke e conclui. Chamado por `tarefas_runtime::start`.
/// Vaga do BOT de destino numa transferência de órfã de proposta: conta as
/// ativas dele, fora a própria linha que está sendo reatribuída.
fn proposal_bot_busy(c: &Connection, hub: &str, space: &str, bot: &str, task: &str, now: i64) -> Result<bool> {
    let busy: i64 = c.query_row(
        "SELECT COUNT(*) FROM donna_task_agenda a, donna_projects p, json_each(p.tasks) t
         WHERE a.hub_id=?1 AND a.space_id=?2 AND a.proposal_id IS NOT NULL
           AND a.state IN ('starting','started') AND a.bot_id=?3 AND a.task_id<>?4
           AND (a.state='started' OR a.updated_at>?5 OR EXISTS(
             SELECT 1 FROM entries e WHERE e.kind='user' AND json_valid(e.data)
              AND json_extract(e.data,'$.clientNonce')='mx142-task-start-'||a.nonce))
           AND p.hub_id=a.hub_id AND p.space_id=a.space_id AND p.id=a.project_id
           AND t.value->>'id'=a.task_id AND t.value->>'state' IN ('backlog','in_progress')",
        params![hub, space, bot, task, now - STALE_STARTING_MS],
        |r| r.get(0),
    )?;
    Ok(busy >= 1)
}

pub(crate) async fn tick(hub: &Arc<Hub>) -> Result<()> {
    let now = now_ms();
    let result = run_due(hub, now).await;
    let changed: Result<Vec<String>> = {
        let hub = hub.clone();
        let queried = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let db = hub.store.db.locked();
            let spaces = db
                .prepare("SELECT DISTINCT space_id FROM donna_task_agenda WHERE hub_id=?1 AND updated_at>=?2")?
                .query_map(params![hub.host_id, now], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(spaces)
        })
        .await;
        match queried {
            Ok(queried) => queried,
            Err(error) => Err(anyhow!("tasks_unavailable: {error:#}")),
        }
    };
    match (&result, &changed) {
        (Ok(()), Err(error)) => {
            return Err(anyhow!("falha ao avisar espaços com início de proposta alterado: {error:#}"));
        }
        (Err(_), Err(error)) => {
            tracing::warn!(%error, "sem aviso de agenda alterada neste tick");
        }
        _ => {}
    }
    if let Ok(spaces) = changed {
        for space in spaces {
            let _ =
                hub.events.send(json!({"type": crate::fork::projetos::EVENT, "hubId": hub.host_id, "spaceId": space}));
        }
    }
    result
}

async fn run_due(hub: &Arc<Hub>, now: i64) -> Result<()> {
    let due = {
        let hub = hub.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Fire>> {
            let db = hub.store.db.locked();
            claim_due(&db, &hub.host_id, now)
        })
        .await??
    };
    for fire in due {
        // Fogo via bot nunca vai por `perform` (send do dono = autoridade
        // emprestada): órfão de crash vira uncertain/failed pela evidência.
        if is_bot_fire(&fire.nonce) {
            settle_orphan_bot_start(hub, &fire).await?;
            continue;
        }
        match perform(hub, &fire).await {
            Ok(true) => conclude(hub, &fire, true, None).await?,
            Ok(false) => conclude(hub, &fire, false, Some("send_unavailable".into())).await?,
            Err(error) => conclude(hub, &fire, false, Some(format!("{error:#}"))).await?,
        }
    }
    Ok(())
}

/// Boot: `starting` órfãs cuja nonce prova aceite viram `started`; as demais
/// voltam a `scheduled` (a tentativa seguinte usa a MESMA nonce → dedupe real).
pub(crate) fn restore_starting(c: &Connection, hub_id: &str) -> Result<()> {
    // Só usa `c`: o chamador já segura a conexão — reabrir `hub.store.db`
    // aqui deadlockaria o boot (mutex não-reentrante).
    let mut st = c.prepare(
        "SELECT space_id,project_id,task_id,bot_id,attempts,nonce FROM donna_task_agenda
         WHERE hub_id=?1 AND state='starting'",
    )?;
    let rows = st
        .query_map(params![hub_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (space, project, task, _bot, _attempts, nonce) in rows {
        // Início delegado via bot: prova é a notice D220 admitida, não o
        // clientNonce do send. Sem `ExecutionRef` vivo não há reenvio —
        // evidência => uncertain, ausência => failed honesto.
        if is_bot_fire(&nonce) {
            // Prova ESTRUTURADA apenas (TEC-22), POR NONCE sem escopo de bot
            // (TEC P2-27): taskDispatch.nonce só é gravado pelo caminho
            // interno tipado na tx de admissão — texto livre citando o nonce
            // NÃO é evidência (fail-closed => failed); e a notice admitida
            // sob o responsável anterior é a mesma admissão.
            let admitted: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM entries WHERE kind='notice'
                     AND json_extract(data,'$.botMessage.taskDispatch.nonce')=?1)",
                    params![nonce],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            c.execute(
                "UPDATE donna_task_agenda SET state=?5,last_error=?6,updated_at=?7
                 WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
                params![
                    hub_id,
                    space,
                    project,
                    task,
                    if admitted { "uncertain" } else { "failed" },
                    if admitted { Some("delegation_unconfirmed") } else { Some("dispatch_interrupted") },
                    now_ms()
                ],
            )?;
            continue;
        }
        let attempt = format!("mx142-task-start-{nonce}");
        // Mesma prova do perform, POR NONCE sem escopo de bot (TEC P2-27):
        // entry aceita sob o responsável anterior já entregou o poke.
        let delivered = nonce_accepted(c, &attempt).unwrap_or(false);
        c.execute(
            "UPDATE donna_task_agenda SET state=?5,not_before=?6,updated_at=?7
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
            params![hub_id, space, project, task, if delivered { "started" } else { "scheduled" }, now_ms(), now_ms()],
        )?;
    }
    Ok(())
}

/// Entrada autenticada do bot (executor A): início direto numa execução viva.
/// Sem chamador nas ferramentas correntes — o início de tarefa proposta flui
/// pela decisão do dono; mantida como contrato do runtime.
#[allow(
    dead_code,
    reason = "Início direto por bot autenticado aguarda consumidor; o caminho corrente de início é a decisão do dono."
)]
pub(crate) async fn start_in_execution(
    hub: &Arc<Hub>,
    execution: &bindings::ExecutionRef,
    space_id: &str,
    project_id: &str,
    task_id: &str,
    client_request_id: &str,
) -> Result<Value> {
    enum StepB {
        Answer(Value),
        Claim { fire: Fire, self_start: bool },
    }
    // Identidade do chamador: a sessão viva da execução carrega o chat (bot).
    let session = {
        let hub = hub.clone();
        let execution = execution.clone();
        tokio::task::spawn_blocking(move || -> Result<String> {
            let db = hub.store.db.locked();
            let tx = db.unchecked_transaction()?;
            let s = bindings::assert_execution_live(&tx, &execution)?;
            tx.commit()?;
            Ok(s.chat_id)
        })
        .await??
    };
    let bot_id = session;
    let (space_id, project_id, task_id, client_request_id) =
        (space_id.to_owned(), project_id.to_owned(), task_id.to_owned(), client_request_id.to_owned());
    let coords = json!({
        "hubId": hub.host_id,
        "spaceId": space_id,
        "projectId": project_id,
        "taskId": task_id,
        "clientRequestId": client_request_id,
    });
    // Mesma máquina do start do dono, mas com origem registrada e sem
    // `Autoridade` de app: a prova é a execução viva + vínculo de tarefa.
    let payload = serde_json::to_string(&coords)?;
    // Opção A (MX-152): kickoff na lane principal via `message_bot` D220 com
    // `ExecutionRef` vivo — cadeia, hops e Retornos intactos, sem canal
    // paralelo. O vínculo tarefa→sessão-extra é catálogo durável
    // (donna_task_sessions + agenda.session_id/request_id). Self-start não
    // delega: `message_bot` recusa self-message e a execução viva já é o
    // Cópias para depois da closure `move`: requester, payload e recibo.
    let requester = bot_id.clone();
    let payload_saved = payload.clone();
    let receipt_space = space_id.clone();
    let receipt_request = client_request_id.clone();
    let step = {
        let hub = hub.clone();
        tokio::task::spawn_blocking(move || -> Result<StepB> {
            let db = hub.store.db.locked();
            let tx = db.unchecked_transaction()?;
            crate::fork::tarefas_runtime::scope(&tx, &hub.host_id, &space_id, &hub.host_id)?;
            if let Some(saved) = tx
                .query_row(
                    "SELECT payload,result FROM donna_task_receipts WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
                    params![hub.host_id, space_id, client_request_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?
            {
                ensure!(saved.0 == payload, "idempotency_conflict");
                tx.commit()?;
                return Ok(StepB::Answer(serde_json::from_str(&saved.1)?));
            }
            let store_answer = |tx: &Connection, answer: &Value| -> Result<()> {
                tx.execute(
                    "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
                    params![hub.host_id, space_id, client_request_id, payload, serde_json::to_string(answer)?],
                )?;
                Ok(())
            };
            match store::gate(&tx, &hub.host_id, &space_id, &project_id, &task_id)? {
                Gate::Refused(reason) => {
                    let answer = json!({"start": {"state": "refused", "reason": reason.as_str(), "requestId": Value::Null}});
                    store_answer(&tx, &answer)?;
                    tx.commit()?;
                    Ok(StepB::Answer(answer))
                }
                Gate::Fire { bot_id: assignee_bot, title, project_name } => {
                    // Autorização: responsável da tarefa ou coordenador do projeto.
                    let row = store::load_task(&tx, &hub.host_id, &space_id, &project_id, &task_id)?;
                    let coordinator = row.project["coordinatorBotId"].as_str().unwrap_or_default();
                    ensure!(
                        bot_id == assignee_bot || bot_id == coordinator,
                        "forbidden: somente o responsável ou o coordenador inicia a tarefa."
                    );
                    let existing = store::agenda_get(&tx, &hub.host_id, &space_id, &project_id, &task_id)?;
                    if existing.as_ref().is_some_and(|a| a.state == "started" || a.state == "starting") {
                        let answer = json!({"start": {"state": "already_running", "reason": Value::Null, "requestId": Value::Null}});
                        store_answer(&tx, &answer)?;
                        tx.commit()?;
                        return Ok(StepB::Answer(answer));
                    }
                    // Claim durável ANTES do despacho: nonce estável por
                    // clientRequestId → replay não gera segundo despacho.
                    // ensure_session no MESMO commit: o vínculo tarefa→extra
                    // existe mesmo se o despacho falhar (catálogo honesto).
                    let nonce = format!("{BOT_NONCE_PREFIX}{client_request_id}");
                    let session_id = store::ensure_session(
                        &tx,
                        &hub.host_id,
                        &space_id,
                        &project_id,
                        &task_id,
                        &assignee_bot,
                        &format!("{project_name} — {title}"),
                    )?;
                    let agenda = Agenda {
                        bot_id: assignee_bot.clone(),
                        start_at: now_ms(),
                        state: "starting".into(),
                        session_id: Some(session_id),
                        attempts: 0,
                        nonce: nonce.clone(),
                        last_error: None,
                        request_id: None,
                    };
                    store::save_agenda(&tx, &hub.host_id, &space_id, &project_id, &task_id, &agenda, now_ms())?;
                    tx.commit()?;
                    let self_start = bot_id == assignee_bot;
                    Ok(StepB::Claim {
                        fire: Fire {
                            space_id,
                            project_id,
                            task_id,
                            bot_id: assignee_bot,
                            title,
                            project_name,
                            nonce,
                            attempts: 0,
                            session_id: agenda.session_id,
                            origin: String::new(),
                            brief: task_brief(&row.task),
                        },
                        self_start,
                    })
                }
            }
        })
        .await??
    };
    let (fire, self_start) = match step {
        StepB::Answer(answer) => return Ok(answer),
        StepB::Claim { fire, self_start } => (fire, self_start),
    };
    // Mesma janela claim->efeito do perform: revalida antes do kickoff.
    if let Err(error) = revalidate_before_effect(hub, &fire).await {
        settle_orphan_bot_start(hub, &fire).await?;
        return Err(error);
    }
    let kickoff =
        if self_start { Kickoff::SelfStart } else { perform_bot_delegation(hub, execution, &requester, &fire).await };
    let (state, reason, request_id) = match &kickoff {
        Kickoff::SelfStart => ("started", Value::Null, Value::Null),
        Kickoff::Sent { request_id } => ("started", Value::Null, json!(request_id)),
        Kickoff::Uncertain => ("uncertain", json!("delegation_unconfirmed"), Value::Null),
        Kickoff::Failed(error) => {
            tracing::warn!(%error, bot = %fire.bot_id, "tarefas_runtime: delegação de início falhou");
            ("failed", json!("delegation_failed"), Value::Null)
        }
    };
    // Estado terminal na agenda: request_id gravado só com prova de
    // requestId; uncertain/failed nunca viram redispatch automático.
    // `hub_upd` é um clone próprio: o `hub` do parâmetro segue vivo para o
    // recibo abaixo (TEC-01 — antes o move do closure invalidava o 2º clone).
    let hub_upd = hub.clone();
    let fire_c = clone_fire(&fire);
    let last_error = match &kickoff {
        Kickoff::Failed(e) => Some(e.chars().take(400).collect::<String>()),
        Kickoff::Uncertain => Some("delegation_unconfirmed".to_owned()),
        _ => None,
    };
    let rid = request_id.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub_upd.store.db.locked();
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "UPDATE donna_task_agenda SET state=?5,last_error=?6,request_id=?7,updated_at=?8
             WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND state='starting' AND nonce=?9",
            params![
                hub_upd.host_id,
                fire_c.space_id,
                fire_c.project_id,
                fire_c.task_id,
                state,
                last_error,
                rid.as_str(),
                now_ms(),
                fire_c.nonce
            ],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await??;
    let answer = json!({"start": {"state": state, "reason": reason, "requestId": request_id}});
    // Recibo final com o resultado real (o claim não grava recibo).
    let hub = hub.clone();
    let (space_id, client_request_id, payload, stored) =
        (receipt_space, receipt_request, payload_saved, answer.clone());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
            params![hub.host_id, space_id, client_request_id, payload, serde_json::to_string(&stored)?],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await??;
    Ok(answer)
}
