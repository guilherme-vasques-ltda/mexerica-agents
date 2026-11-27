//! Canais da Open: a mensagem do dono chega à sala e responde o bot citado ou, sem citação, o líder do canal (sem líder,
//! o primeiro membro). Cada resposta é um turno só texto desse membro (`fork::open_chat`), gravado na conversa do canal.
use crate::chat::group::{handles, mentions};
use crate::fork::autoridade::Autoridade;
use crate::hub::Hub;
use crate::store::{BotConfig, Entry, Lane};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// Quem responde: os membros citados pelo nome; sem citação, o líder; sem líder, o primeiro membro vivo.
pub(crate) fn responders(hub: &Hub, group: &BotConfig, text: &str) -> Result<Vec<BotConfig>> {
    let members: Vec<BotConfig> = group
        .members
        .iter()
        .filter_map(|id| hub.store.bot(id).ok().flatten())
        .filter(|row| !row.deleted)
        .map(|row| row.config)
        .collect();
    ensure!(!members.is_empty(), "open_canal_sem_membros");
    Ok(pick(group, &members, text))
}

fn pick(group: &BotConfig, members: &[BotConfig], text: &str) -> Vec<BotConfig> {
    let named: Vec<&BotConfig> =
        members.iter().filter(|m| handles(&m.name).iter().any(|handle| mentions(text, handle))).collect();
    let chosen = if !named.is_empty() {
        named
    } else if let Some(leader) = members.iter().find(|m| group.leader.as_deref() == Some(m.id.as_str())) {
        vec![leader]
    } else {
        vec![&members[0]]
    };
    chosen.into_iter().cloned().collect()
}

fn prompt(group: &BotConfig, member: &BotConfig, text: &str) -> String {
    format!("[Canal \"{}\" - você é {}]\nUsuário: {text}", group.name.trim(), member.name.trim())
}

/// Entrega a mensagem já gravada: um bot comum segue o caminho de sempre; num canal, cada respondente recebe o turno
/// dele, na conversa do canal.
pub(crate) fn deliver(
    hub: &Arc<Hub>,
    cfg: &BotConfig,
    lane: Lane,
    entry: &Entry,
    authority: Option<&Autoridade>,
    text: &str,
) {
    if !cfg.is_group() {
        crate::fork::open_chat_fila::submit(
            hub,
            cfg,
            lane,
            entry.turn,
            (entry.id.clone(), authority.cloned()),
            text.to_owned(),
        );
        return;
    }
    let members = match responders(hub, cfg, text) {
        Ok(members) => members,
        Err(error) => {
            tracing::warn!(%error, "canal da Open sem quem responda");
            crate::fork::open_chat::mark_unsent(hub, &entry.id, "open_chat_failed", "");
            return;
        }
    };
    for member in &members {
        crate::fork::open_chat_fila::submit(
            hub,
            member,
            lane.clone(),
            entry.turn,
            (entry.id.clone(), authority.cloned()),
            prompt(cfg, member, text),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::pick;
    use crate::store::BotConfig;
    use serde_json::json;

    fn bot(id: &str, name: &str) -> BotConfig {
        serde_json::from_value(json!({"id": id, "name": name, "backend": "codex", "cwd": ""})).unwrap()
    }

    fn ids(group: &BotConfig, members: &[BotConfig], text: &str) -> Vec<String> {
        pick(group, members, text).into_iter().map(|b| b.id).collect()
    }

    #[test]
    fn o_citado_responde_senao_o_lider_senao_o_primeiro() {
        let members = [bot("a", "Berga"), bot("b", "Byte")];
        let mut group = bot("g", "power");
        assert_eq!(ids(&group, &members, "@Berga tudo certo"), ["a"]);
        assert_eq!(ids(&group, &members, "oi @byte"), ["b"]);
        assert_eq!(ids(&group, &members, "E ai pessoal"), ["a"], "sem líder, o primeiro membro");
        group.leader = Some("b".into());
        assert_eq!(ids(&group, &members, "E ai pessoal"), ["b"], "sem citação, o líder");
        assert_eq!(ids(&group, &members, "@Berga e @Byte"), ["a", "b"]);
        let acentos = [bot("a", "Lu"), bot("b", "Luís")];
        assert_eq!(ids(&group, &acentos, "oi @Luís"), ["b"]);
        assert_eq!(ids(&group, &acentos, "oi @Lu, tudo bem"), ["a"]);
    }

    #[tokio::test]
    async fn o_canal_com_dois_bots_entrega_a_quem_foi_citado_ou_ao_lider() {
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let berga = bot("berga", "Berga");
        let byte = bot("byte", "Byte");
        let mut canal: BotConfig = serde_json::from_value(
            json!({"id": "canal", "name": "power", "backend": "codex", "cwd": "", "kind": "group", "members": ["berga", "byte"]}),
        )
        .unwrap();
        for cfg in [&berga, &byte, &canal] {
            store.save_bot(cfg).unwrap();
        }
        let dados = std::env::temp_dir().join(format!("open-canal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dados).unwrap();
        let identidade = crate::remote::identity::Identity::load_or_create(&dados).unwrap();
        let hub = crate::hub::Hub::new(store, "fixture".into(), identidade, "fixture".into(), 0);
        let ids = |canal: &BotConfig, texto: &str| -> Vec<String> {
            super::responders(&hub, canal, texto).unwrap().into_iter().map(|b| b.id).collect()
        };
        assert_eq!(ids(&canal, "@Byte, e você?"), ["byte"]);
        assert_eq!(ids(&canal, "oi"), ["berga"]);
        canal.leader = Some("byte".into());
        assert_eq!(ids(&canal, "oi"), ["byte"]);
        let vazio: BotConfig =
            serde_json::from_value(json!({"id": "v", "name": "v", "backend": "codex", "cwd": "", "kind": "group"}))
                .unwrap();
        assert!(super::responders(&hub, &vazio, "oi").is_err());
        let _ = std::fs::remove_dir_all(&dados);
    }
}
