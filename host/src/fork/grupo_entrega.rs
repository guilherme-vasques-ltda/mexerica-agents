//! Entrega da mensagem do dono num grupo. A linha nasce `sending` e só vira `sent` quando a primeira rodada da sala
//! terminou de passá-la aos membros. Quem recebeu e quem já respondeu ficam no SQLite, antes e depois de cada
//! entrega, e o Stop grava a interrupção antes de ser confirmado. No reinício, só a mensagem pendente mais nova de
//! cada conversa, enviada deste computador, é retomada: uma única vez, numa rodada só, sem as outras pendentes e sem
//! entregar de novo a quem já recebeu. Entrega sem confirmação, retomada que também caiu, registro ilegível, pendente
//! mais antiga, mensagem parada ou remetente cuja autoridade o boot não consegue provar (aparelho remoto) nunca viram
//! `sent`: a mensagem fica interrompida e fora dos prompts automáticos da sala (inclusive citada); as não paradas
//! ganham um aviso para o dono conferir.
use crate::LockExt;
use crate::hub::Hub;
use crate::store::{Entry, EntryKind, Lane, Store};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) const PENDING: &str = "sending";
/// Marca da mensagem que o boot não retomou por falta de prova: a sala não a mostra aos membros.
pub(crate) const REVIEW: &str = "groupReview";
const PREFIX: &str = "donna.group.delivery.";

/// O que a sala já fez com esta mensagem.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    #[serde(skip)]
    pub entry_id: String,
    /// Só na retomada: a sala vai até esta mensagem, numa rodada.
    #[serde(skip)]
    pub upto: Option<i64>,
    /// Membros a quem a mensagem foi passada (o turno pode ainda não ter terminado).
    #[serde(default)]
    pub handed: BTreeSet<String>,
    /// Membros cujo turno terminou (resposta, passe, erro ou prazo).
    #[serde(default)]
    pub done: BTreeSet<String>,
    #[serde(default)]
    pub resumed: bool,
    /// Stop gravado antes de ser confirmado: a mensagem nunca é retomada.
    #[serde(default)]
    pub stopped: bool,
}

fn key(entry: &str) -> String {
    format!("{PREFIX}{entry}")
}

impl Delivery {
    pub(crate) fn new(entry_id: &str) -> Self {
        Self { entry_id: entry_id.to_owned(), ..Self::default() }
    }

    fn load(hub: &Hub, entry_id: &str) -> Result<Self> {
        let saved = hub.store.kv_read(&key(entry_id))?;
        let mut delivery: Self = match saved {
            Some(raw) => serde_json::from_str(&raw)?,
            None => Self::default(),
        };
        entry_id.clone_into(&mut delivery.entry_id);
        Ok(delivery)
    }

    /// Um Stop gravado no meio tempo continua valendo: a gravação da sala nunca o desfaz.
    fn save(&mut self, hub: &Hub, group: &str) -> Result<()> {
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        anyhow::ensure!(matches!(Store::bot_on(&tx, group)?, Some(row) if !row.deleted), "group deleted");
        let raw: Option<String> =
            tx.query_row("SELECT v FROM kv WHERE k=?1", [key(&self.entry_id)], |r| r.get(0)).optional()?;
        if raw.as_deref().and_then(|raw| serde_json::from_str::<Self>(raw).ok()).is_some_and(|saved| saved.stopped) {
            self.stopped = true;
        }
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
            params![key(&self.entry_id), serde_json::to_string(self)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Gravado antes de passar a mensagem ao membro: sem isso, ela não sai.
    pub(crate) fn hand(&mut self, hub: &Hub, group: &str, member: &str) -> Result<()> {
        self.handed.insert(member.to_owned());
        self.save(hub, group)
    }

    /// Falha ao gravar deixa o membro como «recebeu sem terminar»: no boot, isso vira conferência.
    pub(crate) fn finish_member(&mut self, hub: &Hub, group: &str, member: &str) {
        self.done.insert(member.to_owned());
        if let Err(error) = self.save(hub, group) {
            tracing::warn!(group, error = format!("{error:#}"), "entrega ao membro não registrada");
        }
    }

    /// Quem já recebeu esta mensagem não a recebe de novo na retomada.
    pub(crate) fn skips(&self, member: &str) -> bool {
        self.handed.contains(member)
    }

    /// A primeira rodada acabou (ou uma mensagem mais nova/Stop a encerrou): a mensagem conta como enviada, salvo
    /// Stop gravado, que prevalece.
    pub(crate) fn confirm(&self, hub: &Hub) {
        if let Err(error) = settle(hub, std::slice::from_ref(&self.entry_id), "sent", false) {
            tracing::warn!(
                entry = self.entry_id,
                error = format!("{error:#}"),
                "confirmação da entrega ao grupo não gravada"
            );
        }
    }
}

/// Mensagem que a sala não mostra aos membros, nem citada: sem prova de entrega (ou parada), ou ainda pendente.
pub(crate) fn unproven(entry: &Entry) -> bool {
    entry.data[REVIEW] == true || entry.data["status"] == PENDING
}

/// Muda o estado das mensagens ainda pendentes e apaga os recibos numa só transação. Um Stop gravado no recibo
/// prevalece: a mensagem fica interrompida e fora dos prompts. Erro de leitura é erro: nada muda e o recibo fica.
fn settle(hub: &Hub, ids: &[String], status: &str, review: bool) -> Result<()> {
    let _emit = hub.emit_lock.locked();
    let saved = {
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut saved = Vec::new();
        for id in ids {
            let raw: Option<String> =
                tx.query_row("SELECT v FROM kv WHERE k=?1", [key(id)], |r| r.get(0)).optional()?;
            let stopped =
                raw.as_deref().and_then(|raw| serde_json::from_str::<Delivery>(raw).ok()).is_some_and(|d| d.stopped);
            if let Some(mut entry) = Store::entry_on(&tx, id)?
                && entry.data["status"] == PENDING
            {
                entry.data["status"] = json!(if stopped { "cancelled" } else { status });
                if review || stopped {
                    entry.data[REVIEW] = json!(true);
                }
                saved.extend(Store::update_entry_on(&tx, id, &entry.data, None)?);
            }
            tx.execute("DELETE FROM kv WHERE k=?1", [key(id)])?;
        }
        tx.commit()?;
        saved
    };
    for entry in saved {
        let _ = hub.events.send(json!({"type": "entry", "rev": entry.rev, "entry": entry}));
    }
    Ok(())
}

/// Stop: grava a interrupção em toda mensagem pendente do grupo antes de o pedido ser confirmado.
pub(crate) fn stop(hub: &Hub, group: &str) -> Result<()> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let ids: Vec<String> = {
        let mut st =
            tx.prepare("SELECT id FROM entries WHERE bot_id=?1 AND kind='user' AND json_extract(data,'$.status')=?2")?;
        st.query_map(params![group, PENDING], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for id in ids {
        let raw: Option<String> = tx.query_row("SELECT v FROM kv WHERE k=?1", [key(&id)], |r| r.get(0)).optional()?;
        // Ilegível continua incerto: além de parada, conta como já retomada.
        let mut delivery = raw.map_or_else(Delivery::default, |raw| {
            serde_json::from_str(&raw).unwrap_or_else(|_| Delivery { resumed: true, ..Delivery::default() })
        });
        delivery.stopped = true;
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
            params![key(&id), serde_json::to_string(&delivery)?],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Sem prova para retomar: o aviso vem antes da marca, para uma queda entre os dois repetir só o aviso único.
fn review(hub: &Hub, lane: &Lane, entry: &Entry) -> Result<()> {
    hub.add_entry_once(
        &format!("group-review-{}", entry.id),
        lane,
        EntryKind::Notice,
        entry.turn,
        &json!({"text":"O Hub reiniciou antes de confirmar a entrega desta mensagem ao grupo. Ela pode já ter chegado a parte dos membros e não foi retomada; confira antes de reenviar.","style":"info"}),
    )?;
    settle(hub, std::slice::from_ref(&entry.id), "cancelled", true)
}

/// No boot, depois dos atores: cada conversa de grupo retoma no máximo a sua mensagem pendente mais nova, uma vez.
pub(crate) fn recover(hub: &Arc<Hub>) -> Result<()> {
    let pending: Vec<Entry> = {
        let c = hub.store.db.locked();
        let sql = format!(
            "SELECT {} FROM entries WHERE kind='user' AND json_extract(data,'$.status')=?1 ORDER BY seq",
            crate::store::ENTRY_COLS
        );
        let mut st = c.prepare(&sql)?;
        st.query_map([PENDING], crate::store::row_entry)?.collect::<rusqlite::Result<_>>()?
    };
    let mut lanes: BTreeMap<(String, String), Vec<Entry>> = BTreeMap::new();
    for entry in pending {
        lanes.entry((entry.bot_id.clone(), entry.session_id.clone())).or_default().push(entry);
    }
    for ((chat, session), entries) in lanes {
        let Some(group) = hub.store.bot(&chat)?.filter(|row| !row.deleted && row.config.is_group()) else {
            continue;
        };
        let lane = Lane { session: (session != chat).then_some(session), chat };
        let mut resumable: Vec<(Entry, Delivery)> = Vec::new();
        for entry in entries {
            // Só o envio deste computador tem autoridade que o boot prova; um aparelho pode ter sido revogado
            // com o Hub parado, antes de a revogação chegar.
            let local = entry.data["contentOrigin"]["ingress"]["caller"] == "local";
            match Delivery::load(hub, &entry.id) {
                Ok(delivery) if delivery.stopped => settle(hub, std::slice::from_ref(&entry.id), "cancelled", true)?,
                Ok(delivery) if local && !delivery.resumed && delivery.handed.is_subset(&delivery.done) => {
                    resumable.push((entry, delivery));
                }
                Ok(_) => review(hub, &lane, &entry)?,
                Err(error) => {
                    tracing::warn!(entry = entry.id, error = format!("{error:#}"), "registro de entrega ilegível");
                    review(hub, &lane, &entry)?;
                }
            }
        }
        let Some((entry, mut delivery)) = resumable.pop() else { continue };
        // Só a identificada é retomada: as pendentes anteriores não têm recibo por destinatário neste prompt.
        for (older, _) in resumable {
            review(hub, &lane, &older)?;
        }
        delivery.resumed = true;
        delivery.upto = Some(entry.seq);
        delivery.save(hub, &group.config.id)?;
        crate::chat::group::start(hub, &group.config, lane, delivery);
    }
    Ok(())
}
