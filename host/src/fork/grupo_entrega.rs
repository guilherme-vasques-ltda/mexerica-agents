//! Entrega da mensagem do dono num grupo. A linha nasce `sending` e só vira `sent` quando a primeira rodada da sala
//! terminou de passá-la aos membros. Quem recebeu e quem já respondeu ficam no SQLite, antes e depois de cada
//! entrega; o Stop interrompe a mensagem pendente na mesma transação, antes de ser confirmado. No reinício, só a
//! mensagem pendente mais nova de cada conversa, enviada deste computador, pode ser retomada: uma única vez, numa
//! rodada só, sozinha no prompt e sem entregar de novo a quem já recebeu. Entrega sem confirmação, retomada que também
//! caiu, registro ilegível, pendente mais antiga, citação sem prova ou remetente cuja autoridade o boot não consegue
//! provar (aparelho remoto) nunca viram `sent`: a mensagem fica interrompida e fora dos prompts automáticos da sala
//! (inclusive citada), com um aviso para o dono conferir. Uma sala que o boot não consegue classificar fica bloqueada
//! até o próximo reinício; as outras seguem.
use crate::LockExt;
use crate::hub::Hub;
use crate::store::{BotConfig, Entry, EntryKind, Lane, Store};
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) const PENDING: &str = "sending";
/// Marca da mensagem sem prova de entrega (ou parada): a sala não a mostra aos membros, nem citada.
pub(crate) const GROUP_REVIEW: &str = "groupReview";
const PREFIX: &str = "donna.group.delivery.";
/// Mensagens de usuário pendentes da sala; uma linha ilegível nunca derruba a consulta.
const PENDING_SQL: &str = "bot_id = ?1 AND kind = 'user'
    AND CASE WHEN json_valid(data) THEN json_extract(data, '$.status') END = ?2";

/// O que a sala já fez com esta mensagem.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    #[serde(skip)]
    pub entry_id: String,
    /// Só na retomada: a sala mostra só esta mensagem, numa rodada.
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

    fn save(&self, hub: &Hub, group: &str) -> Result<()> {
        anyhow::ensure!(
            hub.store.kv_set_if_bot(group, &key(&self.entry_id), &serde_json::to_string(self)?)?,
            "group deleted"
        );
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

    /// A primeira rodada acabou (ou uma mensagem mais nova a encerrou): a mensagem conta como enviada. Uma parada
    /// já saiu de `sending` e fica como está.
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

/// Mensagem que a sala não mostra aos membros, nem citada: ilegível, sem prova de entrega (ou parada), ou ainda
/// pendente.
pub(crate) fn unproven(entry: &Entry) -> bool {
    !entry.data.is_object() || entry.data[GROUP_REVIEW] == true || entry.data["status"] == PENDING
}

/// Muda o estado das mensagens ainda pendentes e apaga os recibos numa só transação. Erro de leitura é erro: nada
/// muda e o recibo fica.
fn settle(hub: &Hub, ids: &[String], status: &str, review: bool) -> Result<()> {
    let _emit = hub.emit_lock.locked();
    let saved = {
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut saved = Vec::new();
        for id in ids {
            if let Some(mut entry) = Store::entry_on(&tx, id)?
                && entry.data["status"] == PENDING
            {
                entry.data["status"] = json!(status);
                if review {
                    entry.data[GROUP_REVIEW] = json!(true);
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

/// Stop: interrompe toda mensagem pendente do grupo, fora dos prompts, antes de o pedido ser confirmado.
pub(crate) fn stop(hub: &Hub, group: &str) -> Result<()> {
    let ids: Vec<String> = {
        let c = hub.store.db.locked();
        let mut st = c.prepare(&format!("SELECT id FROM entries WHERE {PENDING_SQL}"))?;
        st.query_map(params![group, PENDING], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    settle(hub, &ids, "cancelled", true)
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

/// No boot, depois dos atores. Uma sala que não dá para classificar fica bloqueada até o próximo reinício, com aviso;
/// só a falha ao listar os grupos (o banco inteiro) impede o Hub de abrir.
pub(crate) fn recover(hub: &Arc<Hub>) -> Result<()> {
    for row in hub.store.bots()? {
        if row.deleted || !row.config.is_group() {
            continue;
        }
        if let Err(error) = recover_room(hub, &row.config) {
            tracing::error!(
                group = row.config.id,
                error = format!("{error:#}"),
                "sala bloqueada: entregas não classificadas"
            );
            hub.groups.block(&row.config.id);
            let _ = hub.add_entry(
                &Lane::main(&row.config.id),
                EntryKind::Notice,
                0,
                &json!({"text":"O Hub não conseguiu conferir as mensagens pendentes deste grupo ao reiniciar. O grupo fica bloqueado e nada foi entregue de novo. Reinicie o Hub depois de corrigir o problema.","style":"info","code":"group_blocked"}),
            );
        }
    }
    Ok(())
}

/// Cada conversa retoma no máximo a sua mensagem pendente mais nova, sozinha e uma vez.
fn recover_room(hub: &Arc<Hub>, group: &BotConfig) -> Result<()> {
    let pending: Vec<Entry> = {
        let c = hub.store.db.locked();
        let invalid: i64 = c.query_row(
            "SELECT COUNT(*) FROM entries WHERE bot_id = ?1 AND kind = 'user' AND NOT json_valid(data)",
            [&group.id],
            |r| r.get(0),
        )?;
        anyhow::ensure!(invalid == 0, "{invalid} mensagens ilegíveis na sala");
        let sql = format!("SELECT {} FROM entries WHERE {PENDING_SQL} ORDER BY seq", crate::store::ENTRY_COLS);
        let mut st = c.prepare(&sql)?;
        st.query_map(params![group.id, PENDING], crate::store::row_entry)?.collect::<rusqlite::Result<_>>()?
    };
    let mut lanes: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    for entry in pending {
        lanes.entry(entry.session_id.clone()).or_default().push(entry);
    }
    for (session, mut entries) in lanes {
        let lane = Lane { session: (session != group.id).then_some(session), chat: group.id.clone() };
        // A mais nova é escolhida antes da elegibilidade: as anteriores nunca rodam no lugar dela.
        let Some(entry) = entries.pop() else { continue };
        for older in &entries {
            review(hub, &lane, older)?;
        }
        // Só o envio deste computador tem autoridade que o boot prova; um aparelho pode ter sido revogado com o Hub
        // parado, antes de a revogação chegar.
        let local = entry.data["contentOrigin"]["ingress"]["caller"] == "local";
        // A mensagem que responde a outra sem prova dependeria de uma citação que o prompt omite.
        let quote_ok = match entry.data["replyTo"]["id"].as_str() {
            Some(id) => Store::entry_on(&hub.store.db.locked(), id)?.is_some_and(|quoted| !unproven(&quoted)),
            None => true,
        };
        let mut delivery = match Delivery::load(hub, &entry.id) {
            Ok(delivery) => delivery,
            Err(error) => {
                tracing::warn!(entry = entry.id, error = format!("{error:#}"), "registro de entrega ilegível");
                review(hub, &lane, &entry)?;
                continue;
            }
        };
        if !local || !quote_ok || delivery.resumed || !delivery.handed.is_subset(&delivery.done) {
            review(hub, &lane, &entry)?;
            continue;
        }
        delivery.resumed = true;
        delivery.upto = Some(entry.seq);
        delivery.save(hub, &group.id)?;
        crate::chat::group::start(hub, group, lane, delivery);
    }
    Ok(())
}
