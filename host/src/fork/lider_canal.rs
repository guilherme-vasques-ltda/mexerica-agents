//! Líder do canal (dono, 05/11/2026): o dono escolhe um bot do canal em Configurações. A menção manda (D327): com
//! nomes, só os chamados respondem, o líder primeiro se chamado; sem nome, o líder abre. Ele é orientado a
//! coordenar, chamando os outros pelo nome. É só instrução: não há fila, atribuição nem
//! acompanhamento. Canal sem líder segue o revezamento de sempre, e ninguém vira líder sozinho.
use crate::store::{BotConfig, Store, next_rev};
use anyhow::{Result, bail};
use rusqlite::{Connection, params};

pub(crate) const NOT_A_MEMBER: &str = "O líder precisa ser um bot do canal.";
pub(crate) const SWAP_FIRST: &str = "Troque o líder do canal antes de tirar esse bot.";

/// Confere o líder ao gravar, depois de `valid_members`. `before` é a configuração salva (nenhuma ao criar).
pub(crate) fn validate(cfg: &mut BotConfig, before: Option<&BotConfig>) -> Result<()> {
    if !cfg.is_group() {
        cfg.leader = None;
        return Ok(());
    }
    let Some(leader) = cfg.leader.as_deref() else { return Ok(()) };
    if cfg.members.iter().any(|m| m == leader) {
        return Ok(());
    }
    if before.is_some_and(|b| b.leader.as_deref() == Some(leader)) {
        bail!(SWAP_FIRST);
    }
    bail!(NOT_A_MEMBER)
}

/// Os canais que `id` lidera, lidos antes da exclusão para serem publicados logo depois do commit.
pub(crate) fn led_by(store: &Store, id: &str) -> Result<Vec<String>> {
    Ok(store
        .bots()?
        .into_iter()
        .filter(|r| !r.deleted && r.config.is_group() && r.config.leader.as_deref() == Some(id))
        .map(|r| r.config.id)
        .collect())
}

/// Na transação da exclusão do bot `id`: o canal que ele liderava fica sem líder, com revisão nova. Só usa a
/// `Connection` recebida; a saída de `members` continua na limpeza depois do commit (`Hub::delete_bot`).
pub(crate) fn on_bot_deleted(c: &Connection, id: &str) -> Result<()> {
    let groups: Vec<String> = c
        .prepare(
            "SELECT id FROM bots WHERE deleted=0 AND json_extract(config,'$.kind')='group'
             AND json_extract(config,'$.leader')=?1",
        )?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for group in groups {
        c.execute(
            "UPDATE bots SET config=json_remove(config,'$.leader'), rev=?2 WHERE id=?1",
            params![group, next_rev(c)?],
        )?;
    }
    Ok(())
}

/// O líder, se vai falar nesta rodada, fala primeiro; os outros mantêm a ordem que já tinham.
pub(crate) fn leader_first(group: &BotConfig, speakers: &mut [&BotConfig]) {
    if let Some(i) = speakers.iter().position(|m| group.leader.as_deref() == Some(m.id.as_str())) {
        speakers[..=i].rotate_right(1);
    }
}

/// O líder entre os membros vivos.
fn leader<'a>(group: &BotConfig, members: &'a [BotConfig]) -> Option<&'a BotConfig> {
    let id = group.leader.as_deref()?;
    members.iter().find(|m| m.id == id)
}

/// O trecho do cabeçalho `[Channel: …]` que diz quem lidera.
pub(crate) fn header(group: &BotConfig, members: &[BotConfig], me: &BotConfig) -> String {
    match leader(group, members) {
        Some(l) if l.id == me.id => " - leader: you".to_owned(),
        Some(l) => format!(" - leader: {}", l.name),
        None => String::new(),
    }
}

/// A orientação do turno no canal com líder.
pub(crate) fn guidance(group: &BotConfig, members: &[BotConfig], me: &BotConfig) -> Option<String> {
    let l = leader(group, members)?;
    Some(if l.id == me.id {
        "You lead this channel: when the user names nobody, you answer first. Coordinate the work: when it \
         helps, split it by @-mentioning the other participants by name with what each should do. Don't wait \
         for them or track their progress; each answers in its own turn."
            .to_owned()
    } else {
        format!("{} leads this channel; when they split the work, do the part asked of you.", l.name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bot(id: &str) -> BotConfig {
        serde_json::from_value(json!({"id": id, "name": id.to_uppercase(), "backend": "claude", "cwd": "/tmp"}))
            .expect("valid bot")
    }

    fn group(members: &[&str], leader: Option<&str>) -> BotConfig {
        serde_json::from_value(
            json!({"id": "g", "kind": "group", "name": "Casa", "members": members, "leader": leader}),
        )
        .expect("valid group")
    }

    #[test]
    fn the_leader_must_stay_a_member_until_swapped() {
        let before = group(&["a", "b"], Some("a"));
        let mut removed = group(&["b"], Some("a"));
        assert_eq!(validate(&mut removed, Some(&before)).unwrap_err().to_string(), SWAP_FIRST);
        let mut stranger = group(&["a", "b"], Some("z"));
        assert_eq!(validate(&mut stranger, Some(&before)).unwrap_err().to_string(), NOT_A_MEMBER);
        let mut on_create = group(&["a"], Some("b"));
        assert_eq!(validate(&mut on_create, None).unwrap_err().to_string(), NOT_A_MEMBER);
        for mut ok in [group(&["b"], Some("b")), group(&["b"], None)] {
            validate(&mut ok, Some(&before)).expect("swapped or cleared");
        }
        let mut agent = bot("a");
        agent.leader = Some("b".into());
        validate(&mut agent, None).unwrap();
        assert_eq!(agent.leader, None, "only channels have a leader");
    }

    #[test]
    fn the_leader_speaks_first_and_the_rest_keep_their_order() {
        let bots = [bot("a"), bot("b"), bot("c")];
        let order = |g: &BotConfig| {
            let mut speakers: Vec<&BotConfig> = bots.iter().collect();
            leader_first(g, &mut speakers);
            speakers.iter().map(|b| b.id.as_str()).collect::<Vec<_>>().join("")
        };
        assert_eq!(order(&group(&["a", "b", "c"], Some("c"))), "cab");
        assert_eq!(order(&group(&["a", "b", "c"], None)), "abc", "without a leader nothing changes");
    }
}
