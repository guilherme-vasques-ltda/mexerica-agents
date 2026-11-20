//! MX-142 Hub-B: início explícito/agendado durável de tarefas, sessão vinculada
//! e entrega de avisos do dono. Módulo próprio; os dados das tarefas vivem em
//! `fork::projetos` e os comentários em `donna_task_comments` (frente de dados).
//!
//! Atribuir nunca dispara: o início só acontece por `donnaTaskStart` (dono),
//! `donnaTaskSchedule` (dono, futuro) ou `start_task_in_execution` (bot
//! autenticado). O disparo é uma mensagem do dono na sessão vinculada do bot,
//! com `clientNonce` determinístico: reenvio após crash é deduplicado pelo
//! `send`, e a evidência é a entrada existente, nunca um palpite.
mod acordar;
pub(crate) mod avisos;
mod inspecao;
mod store;

pub(crate) use acordar::start_in_execution as start_task_in_execution;
pub(crate) use store::session_task;
#[cfg(test)]
mod tests;

use crate::{
    LockExt,
    api::devices::{Caller, Forbidden},
    fork::autoridade::Autoridade,
    hub::Hub,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const METHODS: &[&str] = &["donnaTaskStart", "donnaTaskSchedule", "donnaTaskAgenda", "donnaTaskSessions"];

/// Política de integridade referencial (TEC-18 / P3-18 / P3-17):
/// NÃO há FK física — e não é omissão. Alvos tabulares REAIS existem em
/// três campos: `bot_id -> bots.id`, `session_id -> donna_bot_sessions.id`
/// (tabela de `sessoes::store`) e `project_id -> donna_projects`
/// (composta `hub_id`+`space_id`+`id`). Sem alvo físico: `task_id` vive dentro
/// do JSON `tasks` do projeto e `space_id` no catálogo kv
/// `donna.spaces.v1`. Uma FK seria tecnicamente possível para os três
/// primeiros (o soft-delete de bots é UPDATE de flag, que não remove a
/// chave referenciada), mas cobriria só parte das colunas e criaria
/// acoplamento de schema entre runtimes; a política escolhida é
/// revalidação fail-closed em todo ponto de efeito, igualmente segura
/// contra órfãos porque nenhum caminho usa a referência sem revalidar.
///
/// Integridade EFETIVA implementada em vez de FK:
/// - Toda transição de efeito (schedule, tick, deliver, start, settle)
///   revalida `load_task` (projeto ativo + tarefa existente) e `roster`
///   (bot vivo no Space) NA MESMA transação do efeito;
/// - Estado terminal para alvo removido: `invalid_field` -> `refused`/
///   `failed` com `last_error`, nunca retry infinito (tick marca refused
///   por linha, sem envenenar o lote);
/// - Prova causal por `clientNonce` de entry, não por sessão/linha viva —
///   sessão excluída apaga entries (excluir.rs) e o ack simplesmente não
///   casa (false honesto), sem estado mentiroso;
/// - `task_sessions`/`deliveries` de tarefa removida ficam inertes: todo
///   uso revalida a tarefa antes de agir.
/// - JANELA claim->efeito, FECHADA no ponto de aceite para os sends:
///   `revalidate_on` segue como fast-path, mas a garantia real são os
///   triggers `donna_task_{start,aviso}_guard` — BEFORE INSERT em `entries`
///   validam tarefa ativa + responsável + bot no roster DENTRO do insert
///   do clientNonce, ou seja, atomicamente com o aceite. Como o SQLite
///   serializa escritores, exclusão e aceite têm ordem total: quem commita
///   primeiro define o resultado — sem TOCTOU residual no caminho send.
///   `donna_task_nonce_guard` reserva o namespace: sub-prefixo
///   desconhecido/forjado aborta (fail-closed; o cliente não é autoridade
///   do formato).
/// - JANELA claim->admissão D220, FECHADA no ponto de aceite: o kickoff
///   delegado não tem clientNonce, mas o notice entry carrega
///   botMessage.text terminando em Referência de despacho: mx142-bot-<req>.
///   O trigger `donna_task_deleg_guard` resolve o claim pelo campo
///   ESTRUTURADO `botMessage.taskDispatch` (injetado só pelo caminho
///   interno tipado — nunca por argumentos do modelo), casa nonce +
///   state='starting' + coordenadas de referência e exige tarefa ativa +
///   responsável + bot no roster DENTRO da tx de admissão — RAISE(ABORT)
///   desfaz notices, recibo D220 e fila juntos. Mensagem comum que cite o
///   marcador textual não dispara o guard (sem taskDispatch).
///
/// Limitação aceita: linhas "órfãs" podem existir (tarefa/sessão excluída
/// depois do vínculo). São inertes por construção — todo consumo revalida
/// antes de agir; faxina futura pode removê-las. Tarefa própria se a base
/// normalizar tarefas/sessões em tabelas — aí FK física passa a fazer
/// sentido.
pub(crate) fn migrate(c: &rusqlite::Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_task_agenda(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, project_id TEXT NOT NULL, task_id TEXT NOT NULL,
        bot_id TEXT NOT NULL, start_at INTEGER NOT NULL, not_before INTEGER NOT NULL,
        state TEXT NOT NULL, session_id TEXT, nonce TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
        last_error TEXT, request_id TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
        PRIMARY KEY(hub_id,space_id,project_id,task_id));
        CREATE INDEX IF NOT EXISTS donna_task_agenda_due
        ON donna_task_agenda(state,not_before);
        CREATE TABLE IF NOT EXISTS donna_task_receipts(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, request_id TEXT NOT NULL,
        payload TEXT NOT NULL, result TEXT NOT NULL,
        PRIMARY KEY(hub_id,space_id,request_id));
        CREATE TABLE IF NOT EXISTS donna_task_sessions(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, project_id TEXT NOT NULL, task_id TEXT NOT NULL,
        bot_id TEXT NOT NULL, session_id TEXT NOT NULL, created_at INTEGER NOT NULL,
        PRIMARY KEY(hub_id,space_id,project_id,task_id,bot_id));
        CREATE TABLE IF NOT EXISTS donna_task_deliveries(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, project_id TEXT NOT NULL, task_id TEXT NOT NULL,
        comment_id TEXT NOT NULL, bot_id TEXT NOT NULL, session_id TEXT, entry_id TEXT,
        status TEXT NOT NULL, reason TEXT, attempts INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL, answered_at INTEGER,
        PRIMARY KEY(hub_id,space_id,project_id,task_id,comment_id,bot_id));

        -- Cerca TOCTOU no ponto de aceite (TEC-18): o claim commita e o
        -- send acontece fora da tx; a ÚNICA forma de ordenar validade até o
        -- aceite é validar DENTRO do INSERT da entry. Todo poke do runtime
        -- usa clientNonce com prefixo mx142-task-*; o trigger resolve a
        -- tarefa via donna_task_sessions(session,bot) e exige que ela ainda
        -- exista, projeto ativo e estado não terminal — atomicamente com o
        -- insert. Recusa -> RAISE(ABORT) -> send falha -> retry/failed
        -- honesto, nunca efeito órfão.
        DROP TRIGGER IF EXISTS donna_task_start_guard;
        CREATE TRIGGER donna_task_start_guard
        BEFORE INSERT ON entries
        WHEN NEW.kind='user'
         AND json_valid(NEW.data)
         AND json_extract(NEW.data,'$.clientNonce') LIKE 'mx142-task-start-%'
        BEGIN
          -- Fence de claim: o sufixo do clientNonce É o nonce da agenda.
          -- Reagendamento trocou o nonce -> linha velha não casa -> abort.
          -- E a entry precisa cair na sessão vinculada da tarefa.
          SELECT RAISE(ABORT,'task_effect_target_gone')
          WHERE NOT EXISTS(
            SELECT 1 FROM donna_task_agenda a,
                 donna_projects p, json_each(p.tasks) t
            WHERE a.nonce = substr(json_extract(NEW.data,'$.clientNonce'), 18)
              AND a.state='starting' AND a.bot_id=NEW.bot_id
              AND p.id=a.project_id AND p.space_id=a.space_id
              AND json_extract(p.project,'$.state')='active'
              AND t.value->>'id'=a.task_id
              AND t.value->>'state' NOT IN ('done','cancelled')
              AND t.value->>'assigneeBotId'=a.bot_id
              AND EXISTS(SELECT 1 FROM bots b
                WHERE b.id=a.bot_id AND COALESCE(b.deleted,0)=0
                  AND json_extract(b.config,'$.spaceId')=a.space_id
                  AND COALESCE(json_extract(b.config,'$.hidden'),0)=0)
              AND EXISTS(SELECT 1 FROM donna_task_sessions ts
                WHERE ts.session_id=NEW.session AND ts.bot_id=NEW.bot_id
                  AND ts.project_id=a.project_id AND ts.task_id=a.task_id
                  AND ts.space_id=a.space_id)
              -- Dedupe ATÔMICA por nonce (TEC P2-27/P2-30): o nonce é
              -- único por claim e a tentativa legítima NUNCA reinsere —
              -- o dedupe do send devolve a entry existente antes do
              -- INSERT. Qualquer segunda entry com o mesmo clientNonce
              -- (outro bot OU o mesmo bot em envio concorrente que
              -- atravessou as consultas) aborta aqui, dentro da mesma
              -- instrução: primeiro a commitar vence, sem janela.
              AND NOT EXISTS(SELECT 1 FROM entries e2
                WHERE e2.kind='user' AND json_valid(e2.data)
                  AND json_extract(e2.data,'$.clientNonce')
                      = json_extract(NEW.data,'$.clientNonce'))
          );
        END;
        DROP TRIGGER IF EXISTS donna_task_aviso_guard;
        CREATE TRIGGER donna_task_aviso_guard
        BEFORE INSERT ON entries
        WHEN NEW.kind='user'
         AND json_valid(NEW.data)
         AND json_extract(NEW.data,'$.clientNonce') LIKE 'mx142-task-aviso-%'
        BEGIN
          -- Fence de claim: a entrega precisa estar 'sending' (em voo) e o
          -- nonce reconstruído da linha precisa ser o do clientNonce —
          -- vincula comment+bot+session exatos, não só o prefixo.
          SELECT RAISE(ABORT,'task_effect_target_gone')
          WHERE NOT EXISTS(
            SELECT 1 FROM donna_task_deliveries d,
                 donna_projects p, json_each(p.tasks) t
            WHERE d.status='sending' AND d.session_id=NEW.session
              AND 'mx142-task-aviso-'||d.comment_id||'-'||d.bot_id
                  = json_extract(NEW.data,'$.clientNonce')
              AND p.id=d.project_id AND p.space_id=d.space_id
              AND json_extract(p.project,'$.state')='active'
              AND t.value->>'id'=d.task_id
              -- Comentário @ em tarefa concluída/cancelada é legítimo:
              -- o guard veda INÍCIO em done/cancelled, não notificação.
              AND EXISTS(SELECT 1 FROM bots b
                WHERE b.id=d.bot_id AND COALESCE(b.deleted,0)=0
                  AND json_extract(b.config,'$.spaceId')=d.space_id
                  AND COALESCE(json_extract(b.config,'$.hidden'),0)=0)
          );
        END;

        -- Namespace mx142-task-* é reservado: sub-prefixo desconhecido ou
        -- forjado aborta sempre — fail-closed, o cliente nunca é autoridade
        -- sobre o formato do nonce.
        DROP TRIGGER IF EXISTS donna_task_nonce_guard;
        CREATE TRIGGER donna_task_nonce_guard
        BEFORE INSERT ON entries
        WHEN NEW.kind='user'
         AND json_valid(NEW.data)
         AND json_extract(NEW.data,'$.clientNonce') LIKE 'mx142-task-%'
         AND json_extract(NEW.data,'$.clientNonce') NOT LIKE 'mx142-task-start-%'
         AND json_extract(NEW.data,'$.clientNonce') NOT LIKE 'mx142-task-aviso-%'
        BEGIN
          SELECT RAISE(ABORT,'task_nonce_unknown');
        END;

        -- Cerca TOCTOU do kickoff D220 (TEC-18): message_bot não aceita
        -- clientNonce, mas o notice carrega botMessage.text — e o nosso
        -- kickoff termina em Referência de despacho: mx142-bot-<req>.
        -- O trigger resolve a linha da agenda pelo nonce ESTRUTURADO do
        -- taskDispatch (nonce + state='starting'), exigindo tarefa ativa, responsável
        -- atual e bot no roster — DENTRO da tx de admissão D220
        -- (insert_team_notices), atomicamente com o aceite. Recusa ->
        -- RAISE(ABORT) desfaz notices+recibo D220 inteiros: nenhuma
        -- delegação admitida para alvo inválido. O nonce vem do campo
        -- ESTRUTURADO taskDispatch (só o caminho interno tipado grava) —
        -- texto livre citando o marcador não dispara o guard; nonce
        -- velho/forjado/coords divergentes não casam -> abort.
        DROP TRIGGER IF EXISTS donna_task_deleg_guard;
        CREATE TRIGGER donna_task_deleg_guard
        BEFORE INSERT ON entries
        WHEN NEW.kind='notice'
         AND json_valid(NEW.data)
         AND json_extract(NEW.data,'$.botMessage.taskDispatch.nonce') IS NOT NULL
        BEGIN
          -- Contexto estruturado autenticado (R2-22): taskDispatch só é
          -- gravado pelo caminho interno tipado de message_bot — o modelo
          -- não consegue forjá-lo via argumentos. nonce -> agenda corrente
          -- ('starting') é a prova; projectId/taskId/spaceId/hubId são só
          -- referências de consistência, não autoridade.
          -- O par D220 grava DOIS notices com o mesmo taskDispatch: ambos
          -- são validados explicitamente — a.bot_id precisa ser o
          -- targetBotId do payload (o responsável), e NEW.bot_id precisa
          -- ser exatamente sourceBotId (cópia do remetente) ou
          -- targetBotId (entrega ao destinatário), com source!=target.
          -- Qualquer outro papel/origem/destino divergente aborta.
          SELECT RAISE(ABORT,'task_effect_target_gone')
          WHERE NOT EXISTS(
            SELECT 1 FROM donna_task_agenda a,
                 donna_projects p, json_each(p.tasks) t
            WHERE a.nonce = json_extract(NEW.data,'$.botMessage.taskDispatch.nonce')
              AND a.state='starting'
              AND a.hub_id = json_extract(NEW.data,'$.botMessage.taskDispatch.hubId')
              AND a.bot_id = json_extract(NEW.data,'$.botMessage.targetBotId')
              AND json_extract(NEW.data,'$.botMessage.taskDispatch.projectId')=a.project_id
              AND json_extract(NEW.data,'$.botMessage.taskDispatch.taskId')=a.task_id
              AND json_extract(NEW.data,'$.botMessage.taskDispatch.spaceId')=a.space_id
              AND (NEW.bot_id = json_extract(NEW.data,'$.botMessage.targetBotId')
                   OR (NEW.bot_id = json_extract(NEW.data,'$.botMessage.sourceBotId')
                       AND json_extract(NEW.data,'$.botMessage.sourceBotId')
                           != json_extract(NEW.data,'$.botMessage.targetBotId')))
              AND p.id=a.project_id AND p.space_id=a.space_id
              AND json_extract(p.project,'$.state')='active'
              AND t.value->>'id'=a.task_id
              AND t.value->>'state' NOT IN ('done','cancelled')
              AND t.value->>'assigneeBotId'=a.bot_id
              AND EXISTS(SELECT 1 FROM bots b
                WHERE b.id=a.bot_id AND COALESCE(b.deleted,0)=0
                  AND json_extract(b.config,'$.spaceId')=a.space_id
                  AND COALESCE(json_extract(b.config,'$.hidden'),0)=0)
          );
        END;",
    )?;
    // `request_id`: requestId D220 do kickoff delegado (início via bot),
    // gravado no mesmo claim que a agenda — rastreio tarefa→delegação.
    let has_request_id: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_task_agenda') WHERE name='request_id')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_request_id {
        tx.execute("ALTER TABLE donna_task_agenda ADD COLUMN request_id TEXT", [])?;
    }
    // O outbox só existe quando a tabela de comentários da frente de dados já
    // está migrada; sem ela o boot não pode falhar (merge independente).
    let comments_ready: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_task_comments')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if comments_ready {
        // Só autor owner, sem onlyNote e com menções gera linha; bot/system,
        // só-anotar e comentário sem @ nunca acordam ninguém.
        tx.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS donna_task_deliveries_outbox
        AFTER INSERT ON donna_task_comments
        WHEN json_extract(NEW.entry,'$.author')='owner'
         AND COALESCE(json_extract(NEW.entry,'$.onlyNote'),0)=0
         AND json_array_length(COALESCE(json_extract(NEW.entry,'$.mentions'),'[]'))>0
        BEGIN
          INSERT INTO donna_task_deliveries(hub_id,space_id,project_id,task_id,comment_id,bot_id,status,created_at)
          SELECT NEW.hub_id,NEW.space_id,NEW.project_id,NEW.task_id,NEW.comment_id,je.value,'queued',
                 CAST(unixepoch('subsec')*1000 AS INTEGER)
          FROM json_each(json_extract(NEW.entry,'$.mentions')) je;
        END;",
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn scope(c: &rusqlite::Connection, hub: &str, space: &str, expected_hub: &str) -> Result<()> {
    if hub != expected_hub {
        return Err(Forbidden.into());
    }
    crate::fork::spaces::require_space(c, space).map_err(|_| Forbidden)?;
    Ok(())
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::api::devices::permit(caller, method)?;
    let authority = Autoridade::capturar(hub, caller)?;
    if method == "donnaTaskStart" {
        // O início devolve o estado real do disparo: claim+poke+marcação em
        // etapas assíncronas, não uma única transação síncrona.
        crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        return acordar::start_rpc(hub, &authority, body).await;
    }
    ensure!(serde_json::to_vec(&body)?.len() <= 524_288, "tasks_limit: solicitação excede 512 KiB.");
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    let method = method.to_owned();
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let _guard = guard;
        if matches!(method.as_str(), "donnaTaskStart" | "donnaTaskSchedule") {
            crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        }
        let space = body["spaceId"].as_str().unwrap_or_default().to_owned();
        let (answer, changed) = authority.with_db(&hub, None, |db| store::call(db, &hub.host_id, &method, body))?;
        crate::fork::autoridade::com_saida_validada(
            &hub,
            crate::fork::autoridade::OrigemSaida::Controle(&authority, None),
            |db, _, _, _, _| scope(db, &hub.host_id, &space, &hub.host_id),
            || {
                if changed {
                    let _ = hub.events.send(json!({"type":"donnaProjectsChanged","hubId":hub.host_id,"spaceId":space}));
                }
                answer
            },
        )
    })
    .await?
}

/// Relógio do agendador: reivindica inícios vencidos e dispara o poke.
/// Chamado por `Hub::new` ao lado de `routines::start`.
pub fn start(hub: &Arc<Hub>) {
    let weak = Arc::downgrade(hub);
    tokio::spawn(async move {
        let mut clock = tokio::time::interval(std::time::Duration::from_secs(1));
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            clock.tick().await;
            let Some(hub) = weak.upgrade() else { break };
            if let Err(error) = acordar::tick(&hub).await {
                tracing::warn!(%error, "tarefas_runtime: tick do início agendado falhou");
            }
            if let Err(error) = avisos::tick(&hub).await {
                tracing::warn!(%error, "tarefas_runtime: tick de avisos falhou");
            }
        }
    });
}

/// Recuperação de boot (síncrona, como `routines::restore`): `starting`
/// órfãs voltam a `scheduled` quando a nonce prova que o poke não foi aceito;
/// `sending` de avisos idem.
pub(crate) fn restore(hub: &Arc<Hub>) -> Result<()> {
    let db = hub.store.db.locked();
    let tx = db.unchecked_transaction()?;
    acordar::restore_starting(&tx, &hub.host_id)?;
    avisos::restore_sending(&tx, &hub.host_id)?;
    tx.commit()?;
    Ok(())
}
