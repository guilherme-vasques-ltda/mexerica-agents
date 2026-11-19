//! Preparação inerte D502: registro canônico, sem escritor produtivo ou liberação.
//! Prazo, lease, boot e ausência de ACK nunca removem a causa de hold.
use anyhow::{Result, ensure};
use rusqlite::Connection;

pub(crate) const DENIED: &str = "busy: revisão protegida indisponível; trabalho não admitido";

// Um único predicado para o índice parcial e para ambos os leitores. Não reduzir
// a IN de fases conhecidas: estado futuro ou aborto inconsistente também bloqueia.
macro_rules! blocked_rows {
    () => {
        "NOT (phase IS 'waiting' OR (phase IS 'aborted' AND gesture_eligible IS 0 AND delivery_eligible IS 0))"
    };
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(concat!(
        "CREATE TABLE IF NOT EXISTS donna_reviews (
            request_ref TEXT PRIMARY KEY NOT NULL,
            instance_ref TEXT NOT NULL UNIQUE,
            descriptor TEXT NOT NULL CHECK(json_valid(descriptor)),
            original_deadline INTEGER NOT NULL,
            display_deadline INTEGER NOT NULL CHECK(display_deadline<=original_deadline),
            phase TEXT NOT NULL CHECK(phase IN ('waiting','holding','delivery_admitted','aborted')),
            gesture_eligible INTEGER NOT NULL CHECK(gesture_eligible IN (0,1)),
            delivery_eligible INTEGER NOT NULL CHECK(delivery_eligible IN (0,1)),
            CHECK(phase<>'aborted' OR (gesture_eligible=0 AND delivery_eligible=0))
        );
        CREATE INDEX IF NOT EXISTS donna_reviews_blocked ON donna_reviews(request_ref) WHERE ",
        blocked_rows!(),
    ))?;
    Ok(())
}

// Só estes dois estados não têm causa de hold. Estado desconhecido ou erro SQL fecha.
// INDEXED BY é requisito do plano, não hint: índice ausente/incompatível recusa.
pub(in crate::fork::atividades) const BLOCKED: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM donna_reviews INDEXED BY donna_reviews_blocked WHERE ",
    blocked_rows!(),
    " LIMIT 1)",
);

/// Leitura somente pela Connection recebida, sem Store recursivo, relógio ou Authority.
pub(crate) fn work_allowed_on(db: &Connection) -> Result<()> {
    let blocked: bool = db.query_row(BLOCKED, [], |r| r.get(0))?;
    ensure!(!blocked, DENIED);
    Ok(())
}

/// Fixtures internas ao binário de teste. Nenhum produtor é compilado em produção.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod fixture {
    use crate::{LockExt, hub::Hub};

    pub(crate) fn hold(hub: &Hub, instance: &str) {
        let _gate = hub.computers.gate.locked();
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        seed(&tx, instance, "holding");
        tx.commit().unwrap();
        hub.computers.wake();
    }

    fn descriptor() -> super::super::ReviewDescriptor {
        use serde_json::json;
        let id = |n: u32| format!("00000000-0000-4000-8000-{n:012x}");
        let context = json!({"version":1,"hubId":id(1),"spaceId":id(2),"conversationId":id(3),"botId":id(3),
            "audience":{"kind":"principal"},"teamId":null,"privacyDomainId":id(4),"domainRevision":1,"conversationRevision":2});
        let target =
            json!({"contextRef":context,"kind":"candidate","objectId":id(9),"revision":1,"digest":"a".repeat(64)});
        let binding: super::super::ReviewBinding = serde_json::from_value(json!({"version":1,"contextRef":context,
            "callerDevice":{"principalId":id(5),"deviceId":id(6),"sessionId":id(7),"principalEpoch":1},
            "recoveryEpoch":id(8),"policyRevision":1,"action":"accept","target":target,"before":null,
            "effect":{"body":"synthetic","account":null,"destination":null,"attachments":[]},"sources":[],"bindings":[]})).unwrap();
        serde_json::from_value(json!({"version":1,"reviewId":id(20),
            "prepareOperation":{"version":1,"operationId":id(21),"kind":"prepare_review"},"prepareNonce":id(24),
            "decisionOperation":{"version":1,"operationId":id(22),"kind":"decide_review"},"decisionNonce":id(23),
            "semanticDigest":binding.digest().unwrap(),"binding":binding,"window":{"issuedAt":1000,"expiresAt":121_000}})).unwrap()
    }

    pub(super) fn seed(db: &rusqlite::Connection, instance: &str, phase: &str) {
        // Descriptor descritivo sintético, nunca lido como prova ou autoridade.
        db.execute(
            "INSERT INTO donna_reviews VALUES(?1,?1,?2,90000,90000,?3,1,1)",
            rusqlite::params![instance, serde_json::to_string(&descriptor()).unwrap(), phase],
        )
        .unwrap();
    }

    pub(crate) fn abort(hub: &Hub, instance: &str) -> bool {
        let _gate = hub.computers.gate.locked();
        let changed =
            crate::fork::atividades::store::with_store_tx(&hub.store, |tx| tx.review_abort_undelivered(instance))
                .unwrap();
        hub.computers.wake();
        changed
    }
}

#[cfg(test)]
mod tests;
