// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! A routine body as `save` takes it, checked before anything is saved; `chat::manage` checks one this way before
//! its approval card goes up.

use super::{Routine, Routines, ScheduleDraft, State, Trigger, required};
use crate::{LockExt, store::now_ms};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

/// Lista de gatilhos vazia sem a intenção explícita de «só quando eu mandar», em pt-BR leigo.
pub(super) const NO_TRIGGER: &str = "A rotina precisa de pelo menos um gatilho.";
/// Rotina manual não tem gatilho: pedir as duas coisas é contradição.
const MANUAL_WITH_TRIGGERS: &str =
    "Uma rotina «só quando eu mandar» não tem horário nem gatilho. Tire o modo manual ou os gatilhos.";

/// A routine body's name, instruction, triggers and timeout, checked. Uma rotina só quando o dono manda
/// (`manual: true` no corpo, ou o rascunho do editor em `manual`) é a única com a lista de gatilhos vazia.
pub struct Checked {
    pub name: String,
    pub instruction: String,
    pub triggers: Vec<Trigger>,
    pub timeout: Option<u64>,
}

impl Checked {
    pub(super) fn parse(body: &Value) -> Result<Self> {
        let name = required(body, "name")?.trim();
        let instruction = required(body, "instruction")?.trim();
        if name.is_empty() || name.len() > 200 || instruction.is_empty() || instruction.len() > 32_000 {
            bail!("routine name or instruction is empty or too long");
        }
        let (triggers, manual): (Vec<Trigger>, bool) = if let Some(draft) = body.get("schedule") {
            let draft = serde_json::from_value::<ScheduleDraft>(draft.clone()).context("invalid schedule")?;
            (draft.triggers()?, draft.kind == ScheduleDraft::MANUAL)
        } else {
            let manual = body["manual"] == true;
            let raw = body.get("triggers").cloned().unwrap_or_else(|| if manual { json!([]) } else { Value::Null });
            (serde_json::from_value(raw).context("invalid triggers")?, manual)
        };
        if manual && !triggers.is_empty() {
            bail!("{MANUAL_WITH_TRIGGERS}");
        }
        if triggers.is_empty() && !manual {
            bail!("{NO_TRIGGER}");
        }
        if triggers.len() > 20 {
            bail!("{}", super::schedule::too_many_triggers(triggers.len()));
        }
        if let Some(refused) = super::schedule::duplicate_cron_message(&triggers) {
            bail!("{refused}");
        }
        let timeout = body
            .get("timeoutSeconds")
            .map(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str()?.parse::<u64>().ok())
                    .filter(|seconds| (1..=86_400).contains(seconds))
                    .context("timeoutSeconds must be an integer between 1 and 86400")
            })
            .transpose()?;
        Ok(Self { name: name.into(), instruction: instruction.into(), triggers, timeout })
    }

    /// Quando a rotina roda, em uma linha, para o cartão de aprovação.
    pub fn when(&self) -> String {
        if self.triggers.is_empty() {
            return "só quando o dono mandar".into();
        }
        self.triggers.iter().map(Trigger::description).collect::<Vec<_>>().join("; ")
    }
}

/// The routine of `bot`'s that `body`'s `id` names, if it names one; `triggers` must hold from `now` on, except a
/// one-time trigger that routine already has.
pub(super) fn replaced(s: &State, bot: &str, body: &Value, triggers: &[Trigger], now: i64) -> Result<Option<Routine>> {
    let old = match body["id"].as_str() {
        Some(id) => Some(
            s.routines
                .iter()
                .find(|r| r.id == id && r.bot_id == bot && !r.deleted)
                .cloned()
                .context("routine not found")?,
        ),
        None => None,
    };
    for trigger in triggers {
        if matches!(trigger, Trigger::Once { .. }) && old.as_ref().is_some_and(|r| r.triggers.contains(trigger)) {
            continue;
        }
        trigger.validate(now)?;
    }
    Ok(old)
}

impl Routines {
    /// Checks `body` the way `save` does for `bot`, saving nothing; a bot not created yet has no routine to replace.
    pub fn check(&self, bot: &str, body: &Value) -> Result<Checked> {
        let checked = Checked::parse(body)?;
        replaced(&self.state.locked(), bot, body, &checked.triggers, now_ms())?;
        Ok(checked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_refuses_a_repeated_time_and_keeps_the_trigger_limit() {
        let cron = |expression: &str| json!({"type":"cron","expression":expression,"timeZone":"UTC"});
        let body = json!({"name":"Café","instruction":"Avisar","triggers":[cron("0 15 * * *"),cron("0 15 * * *")]});
        let Err(error) = Checked::parse(&body) else { panic!("horário repetido deve recusar") };
        assert_eq!(format!("{error:#}"), "Dois horários iguais: 15:00. Troque um deles ou tire um.");
        let ok = json!({"name":"Café","instruction":"Avisar","triggers":[cron("0 15 * * *"),cron("30 15 * * *")]});
        assert_eq!(Checked::parse(&ok).unwrap().triggers.len(), 2);
        let many: Vec<_> = (0..21).map(|minute| cron(&format!("{minute} 15 * * *"))).collect();
        let over = json!({"name":"Café","instruction":"Avisar","triggers":many});
        assert!(Checked::parse(&over).is_err());
    }
}
