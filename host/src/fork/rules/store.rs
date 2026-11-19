use super::rule::{ApprovalLevel, Rule};
use crate::{LockExt, store::Store};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub bot_id: Option<String>,
    pub level: ApprovalLevel,
    pub revision: i64,
    pub permission_chosen_by_user: bool,
    pub dangerous_categories: Vec<String>,
    pub engine: Option<String>,
}

#[derive(Debug)]
pub struct GateDecision<'a> {
    pub bot_id: &'a str,
    pub origin: &'a str,
    pub tool: Option<&'a str>,
    pub verdict: &'a str,
    pub reason: &'a str,
    pub rule_id: Option<&'a str>,
    pub rule_sentence: Option<String>,
    pub policy_revision: i64,
    pub floor_category: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub rules: Vec<Rule>,
    pub approvals: Vec<Approval>,
    pub revision: i64,
}

fn revision(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT revision FROM donna_policy_meta WHERE id=1", [], |r| r.get(0))?)
}

fn changed(c: &Connection, kind: &str, bot: Option<&str>, rule: Option<&str>) -> Result<()> {
    changed_by(c, kind, bot, rule, "owner_channel")
}

fn changed_by(c: &Connection, kind: &str, bot: Option<&str>, rule: Option<&str>, origin: &str) -> Result<()> {
    c.execute("UPDATE donna_policy_meta SET revision=revision+1 WHERE id=1", [])?;
    c.execute(
        "INSERT INTO donna_rule_events(at,kind,bot_id,origin,rule_id,policy_revision)
         VALUES(?1,?2,?3,?6,?4,?5)",
        params![crate::store::now_ms(), kind, bot, rule, revision(c)?.to_string(), origin],
    )?;
    Ok(())
}

fn agent(c: &Connection, id: &str) -> Result<()> {
    let raw: Option<String> =
        c.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [id], |r| r.get(0)).optional()?;
    let bot: crate::store::BotConfig = serde_json::from_str(&raw.context("bot_not_found: esse bot não existe.")?)?;
    if bot.is_group() {
        bail!("bot_not_found: esse bot não existe.");
    }
    Ok(())
}

fn approval(c: &Connection, subject: &str) -> Result<Approval> {
    let (level, rev, chosen, categories, engine): (String, i64, bool, String, Option<String>) = c.query_row(
        "SELECT level,revision,chosen_by_user,dangerous_json,engine FROM donna_approvals WHERE subject=?1",
        [subject],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    Ok(Approval {
        bot_id: (!subject.is_empty()).then(|| subject.to_owned()),
        level: serde_json::from_value(json!(level))?,
        revision: rev,
        permission_chosen_by_user: chosen,
        dangerous_categories: serde_json::from_str(&categories)?,
        engine,
    })
}

fn read_rule(c: &Connection, id: &str) -> Result<Rule> {
    // Campos/op desconhecidos permanecem visíveis ao avaliador; não são descartados silenciosamente.
    let value = c
        .query_row(
            "SELECT json_object('id',id,'effect',effect,'field',field,'op',op,'value',value,'botId',bot_id,'tool',tool,
          'scope',json(scope_json),'revision',revision,'enabled',json(CASE WHEN enabled=1 THEN 'true' ELSE 'false' END),
          'createdAt',created_at,'updatedAt',updated_at) FROM donna_rules WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .context("rule_not_found: essa regra não existe.")?;
    Ok(serde_json::from_str(&value)?)
}

fn check_revision(actual: i64, expected: i64) -> Result<()> {
    if actual != expected {
        bail!("policy_conflict: as regras mudaram; releia antes de salvar.");
    }
    Ok(())
}

impl Store {
    pub fn donna_rules(&self) -> Result<Snapshot> {
        let c = self.db.locked();
        // Uma leitura consistente também se outra conexão escrever em WAL.
        let tx = c.unchecked_transaction()?;
        let ids = tx
            .prepare("SELECT id FROM donna_rules ORDER BY effect DESC,created_at,id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let subjects = tx
            .prepare("SELECT subject FROM donna_approvals ORDER BY subject")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let snapshot = Snapshot {
            rules: ids.iter().map(|id| read_rule(&tx, id)).collect::<Result<_>>()?,
            approvals: subjects.iter().map(|id| approval(&tx, id)).collect::<Result<_>>()?,
            revision: revision(&tx)?,
        };
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn donna_rule_save(&self, mut rule: Rule, expected: i64, creating: bool) -> Result<Rule> {
        if creating {
            rule.id = uuid::Uuid::new_v4().to_string();
        }
        rule.revision = if creating { 1 } else { expected };
        super::rule::validate_with_floor(&rule, &validation_floor())?;
        // 13.1c guarda respostas de envio depois dos porteiros. Tipo já existe, sem bypass genérico.
        if rule.scope.is_some() {
            bail!("not_available: respostas salvas de envio ainda não estão disponíveis.");
        }
        let _boundary = self.memory_boundary.locked();
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(bot) = &rule.bot_id {
            agent(&tx, bot)?;
        }
        let now = crate::store::now_ms();
        if creating {
            check_revision(revision(&tx)?, expected)?;
            let count: i64 = tx.query_row("SELECT count(*) FROM donna_rules", [], |r| r.get(0))?;
            if count >= 100 {
                bail!("rule_limit: limite de 100 regras.");
            }
            rule.created_at = now;
        } else {
            let old = read_rule(&tx, &rule.id)?;
            check_revision(old.revision, expected)?;
            // Editar conteúdo não liga uma regra. O interruptor tem intenção/revisão próprias.
            rule.enabled = old.enabled;
            rule.revision = old.revision + 1;
            rule.created_at = old.created_at;
        }
        rule.updated_at = now;
        tx.execute("INSERT INTO donna_rules(id,effect,field,op,value,bot_id,tool,scope_json,revision,enabled,created_at,updated_at)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
          ON CONFLICT(id) DO UPDATE SET effect=excluded.effect,field=excluded.field,op=excluded.op,value=excluded.value,
            bot_id=excluded.bot_id,tool=excluded.tool,scope_json=excluded.scope_json,revision=excluded.revision,
            enabled=excluded.enabled,updated_at=excluded.updated_at",
          params![rule.id,serde_json::to_value(rule.effect)?.as_str(),rule.field,rule.op,rule.value,rule.bot_id,rule.tool,
            rule.scope.as_ref().map(serde_json::to_string).transpose()?,rule.revision,rule.enabled,rule.created_at,rule.updated_at])?;
        changed(&tx, if creating { "rule_added" } else { "rule_changed" }, rule.bot_id.as_deref(), Some(&rule.id))?;
        tx.commit()?;
        Ok(rule)
    }

    pub fn donna_rule_enabled(&self, id: &str, enabled: bool, expected: i64) -> Result<Rule> {
        let _boundary = self.memory_boundary.locked();
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut rule = read_rule(&tx, id)?;
        check_revision(rule.revision, expected)?;
        if enabled {
            super::rule::validate_with_floor(&rule, &validation_floor())?;
        }
        tx.execute(
            "UPDATE donna_rules SET enabled=?2,revision=revision+1,updated_at=?3 WHERE id=?1",
            params![id, enabled, crate::store::now_ms()],
        )?;
        changed(&tx, if enabled { "rule_enabled" } else { "rule_disabled" }, rule.bot_id.as_deref(), Some(id))?;
        rule = read_rule(&tx, id)?;
        tx.commit()?;
        Ok(rule)
    }

    pub fn donna_rule_delete(&self, id: &str, expected: i64) -> Result<()> {
        let _boundary = self.memory_boundary.locked();
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let rule = read_rule(&tx, id)?;
        check_revision(rule.revision, expected)?;
        tx.execute("DELETE FROM donna_rules WHERE id=?1", [id])?;
        changed(&tx, "rule_removed", rule.bot_id.as_deref(), Some(id))?;
        tx.commit()?;
        Ok(())
    }

    /// Registra o veredito automático do porteiro; pedidos que viram cartão
    /// já são visíveis na conversa e não geram evento duplicado.
    pub fn donna_rule_gate_decision(&self, d: &GateDecision) -> Result<()> {
        self.db.locked().execute(
            "INSERT INTO donna_rule_events(at,kind,bot_id,origin,tool,summary,verdict,decided_by,rule_id,rule_sentence,policy_revision,floor_category)
             VALUES(?1,'gate_decision',?2,?3,?4,?5,?6,'rules_gate',?7,?8,?9,?10)",
            params![
                crate::store::now_ms(),
                d.bot_id,
                d.origin,
                d.tool,
                d.reason,
                d.verdict,
                d.rule_id,
                d.rule_sentence,
                d.policy_revision.to_string(),
                d.floor_category
            ],
        )?;
        Ok(())
    }

    /// Apenas estado preparatório: não muda o Permission legado nem manda Reconfigure.
    pub fn donna_approval_set(&self, bot: Option<&str>, level: ApprovalLevel, expected: i64) -> Result<Approval> {
        if level == ApprovalLevel::FullAccess {
            bail!("not_available: Acesso total depende de prova do chão por motor.");
        }
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(bot) = bot {
            agent(&tx, bot)?;
        }
        let subject = bot.unwrap_or("");
        check_revision(approval(&tx, subject)?.revision, expected)?;
        // O legado ainda é a autoridade efetiva: não preparar uma ampliação que
        // a reconfirmação do mesmo valor no editor antigo não conseguiria restringir.
        let legacy: String =
            tx.query_row("SELECT legacy_permission FROM donna_approvals WHERE subject=?1", [subject], |row| {
                row.get(0)
            })?;
        if level == ApprovalLevel::AutoApprove && legacy != "auto" {
            bail!(
                "legacy_limit: enquanto o controle antigo estiver em Pedir, o novo nível também precisa ser Pedir liberação. Altere primeiro o controle antigo deste bot ou da configuração geral."
            );
        }
        tx.execute("UPDATE donna_approvals SET level=?2,revision=revision+1,chosen_by_user=1,dangerous_json='[]',engine=NULL WHERE subject=?1",
          params![subject,serde_json::to_value(level)?.as_str()])?;
        changed(&tx, "mode_changed", bot, None)?;
        let state = approval(&tx, subject)?;
        tx.commit()?;
        Ok(state)
    }
}

pub fn forget_bot(c: &Connection, id: &str) -> Result<()> {
    if id.is_empty() {
        return Ok(());
    }
    let rules = c.execute("DELETE FROM donna_rules WHERE bot_id=?1", [id])?;
    let approvals = c.execute("DELETE FROM donna_approvals WHERE subject=?1 AND subject<>''", [id])?;
    if rules + approvals > 0 {
        changed_by(c, "bot_removed", Some(id), None, "bot_lifecycle")?;
    }
    Ok(())
}

pub fn rule_value(rule: &Rule) -> Result<Value> {
    let mut v = serde_json::to_value(rule)?;
    v["sentence"] = rule.sentence().into();
    v["mcpBroad"] = (rule.effect == super::rule::Effect::Deny && rule.field == "alvo").into();
    Ok(v)
}

fn validation_floor() -> super::floor::FloorContext {
    super::floor::FloorContext { hub_paths: vec![crate::service::data_dir().to_string_lossy().into_owned()] }
}
