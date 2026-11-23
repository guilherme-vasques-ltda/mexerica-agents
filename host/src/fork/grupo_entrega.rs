//! Entrega da mensagem do dono num grupo. A linha nasce `sending` e só vira `sent` quando a primeira rodada da sala
//! terminou de passá-la aos membros. Quem recebeu e quem já respondeu ficam no SQLite, antes e depois de cada
//! entrega. No reinício, a mensagem pendente é retomada uma única vez, sem entregar de novo a quem já recebeu; quando a
//! entrega a algum membro ficou sem confirmação, ou a retomada também caiu, ela é marcada para conferência e nada é
//! repetido.
use crate::LockExt;
use crate::hub::Hub;
use crate::store::{EntryKind, Lane};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) const PENDING: &str = "sending";
const PREFIX: &str = "donna.group.delivery.";

/// O que a primeira rodada da sala já fez com esta mensagem.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    #[serde(skip)]
    pub entry_id: String,
    /// Membros a quem a mensagem foi passada (o turno pode ainda não ter terminado).
    pub handed: BTreeSet<String>,
    /// Membros cujo turno terminou (resposta, passe, erro ou prazo).
    pub done: BTreeSet<String>,
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

    pub(crate) fn finish_member(&mut self, hub: &Hub, group: &str, member: &str) {
        self.done.insert(member.to_owned());
        if let Err(error) = self.save(hub, group) {
            tracing::warn!(group, error = format!("{error:#}"), "entrega ao membro não registrada");
        }
    }

    pub(crate) fn skips(&self, member: &str) -> bool {
        self.done.contains(member)
    }

    /// A primeira rodada acabou (ou uma mensagem mais nova/Stop a encerrou): a mensagem conta como enviada.
    pub(crate) fn confirm(&self, hub: &Hub) {
        if let Some(mut entry) = hub.store.entry(&self.entry_id)
            && entry.data["status"] == PENDING
        {
            entry.data["status"] = json!("sent");
            if hub.set_entry(&self.entry_id, &entry.data).is_none() {
                tracing::warn!(entry = self.entry_id, "confirmação da entrega ao grupo não gravada");
                return;
            }
        }
        if let Err(error) = hub.store.kv_delete(&key(&self.entry_id)) {
            tracing::warn!(entry = self.entry_id, error = format!("{error:#}"), "registro de entrega não apagado");
        }
    }
}

/// No boot, depois dos atores: cada sala com mensagem pendente é retomada no máximo uma vez.
pub(crate) fn recover(hub: &Arc<Hub>) -> Result<()> {
    let pending: Vec<crate::store::Entry> = {
        let c = hub.store.db.locked();
        let sql = format!(
            "SELECT {} FROM entries WHERE kind='user' AND json_extract(data,'$.status')=?1 ORDER BY seq",
            crate::store::ENTRY_COLS
        );
        let mut st = c.prepare(&sql)?;
        st.query_map([PENDING], crate::store::row_entry)?.collect::<rusqlite::Result<_>>()?
    };
    // Só a mais nova de cada conversa é retomada; as anteriores já seriam lidas por ela, como numa sala viva.
    let mut newest: BTreeMap<(String, String), String> = BTreeMap::new();
    for entry in &pending {
        newest.insert((entry.bot_id.clone(), entry.session_id.clone()), entry.id.clone());
    }
    for entry in pending {
        let Some(group) = hub.store.bot(&entry.bot_id)?.filter(|row| !row.deleted && row.config.is_group()) else {
            continue;
        };
        let lane = Lane {
            chat: entry.bot_id.clone(),
            session: (entry.session_id != entry.bot_id).then(|| entry.session_id.clone()),
        };
        let mut delivery = Delivery::load(hub, &entry.id).unwrap_or_else(|error| {
            tracing::warn!(entry = entry.id, error = format!("{error:#}"), "registro de entrega ilegível");
            // Ilegível: trata como incerta, nunca como «ninguém recebeu».
            Delivery { entry_id: entry.id.clone(), resumed: true, ..Delivery::default() }
        });
        let uncertain = delivery.resumed || delivery.handed.iter().any(|m| !delivery.done.contains(m));
        if uncertain {
            hub.add_entry_once(
                &format!("group-review-{}", entry.id),
                &lane,
                EntryKind::Notice,
                entry.turn,
                &json!({"text":"O Hub reiniciou enquanto esta mensagem era entregue ao grupo. Ela pode já ter chegado a parte dos membros; confira antes de reenviar. Nenhuma entrega foi repetida.","style":"info"}),
            )?;
        }
        let latest = newest.get(&(entry.bot_id.clone(), entry.session_id.clone())) == Some(&entry.id);
        if uncertain || !latest {
            delivery.confirm(hub);
            continue;
        }
        delivery.resumed = true;
        delivery.save(hub, &group.config.id)?;
        crate::chat::group::start(hub, &group.config, lane, delivery);
    }
    Ok(())
}
