//! Projeção preparatória: nunca altera `Permission` nem configura um motor.
use anyhow::Result;
use rusqlite::Connection;

pub fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_policy_meta(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL);
         INSERT OR IGNORE INTO donna_policy_meta VALUES(1,1);
         CREATE TABLE IF NOT EXISTS donna_rules(
           id TEXT PRIMARY KEY, effect TEXT NOT NULL CHECK(effect IN ('allow','deny')),
           field TEXT NOT NULL, op TEXT NOT NULL, value TEXT NOT NULL, bot_id TEXT, tool TEXT, scope_json TEXT,
           revision INTEGER NOT NULL, enabled INTEGER NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS donna_approvals(
           subject TEXT PRIMARY KEY, level TEXT NOT NULL CHECK(level IN ('ask','auto_approve','full_access')),
           revision INTEGER NOT NULL DEFAULT 1, chosen_by_user INTEGER NOT NULL DEFAULT 0,
           dangerous_json TEXT NOT NULL DEFAULT '[]', engine TEXT,
           legacy_permission TEXT NOT NULL, legacy_chosen INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS donna_rule_events(
           seq INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, kind TEXT NOT NULL,
           bot_id TEXT, origin TEXT, tool TEXT, summary TEXT, verdict TEXT, decided_by TEXT,
           rule_id TEXT, rule_sentence TEXT, action_ref TEXT, grant_ref TEXT, request_revision TEXT,
           policy_revision TEXT, floor_category TEXT, judge_job_ref TEXT, delivery_status TEXT);
         CREATE INDEX IF NOT EXISTS donna_rule_events_bot ON donna_rule_events(bot_id,seq);
         INSERT OR IGNORE INTO donna_approvals(subject,level,legacy_permission)
           SELECT '', CASE WHEN json_valid(v) AND json_extract(v,'$.permission')='auto' THEN 'auto_approve' ELSE 'ask' END,
             CASE WHEN json_valid(v) AND json_extract(v,'$.permission')='auto' THEN 'auto' ELSE 'ask' END
           FROM kv WHERE k='team.settings.v1';
         INSERT OR IGNORE INTO donna_approvals(subject,level,legacy_permission) VALUES('','ask','ask');
         INSERT OR IGNORE INTO donna_approvals(subject,level,chosen_by_user,legacy_permission,legacy_chosen)
           SELECT id, CASE WHEN COALESCE(json_extract(config,'$.permission'),'auto')='auto' THEN 'auto_approve' ELSE 'ask' END,
             COALESCE(json_extract(config,'$.permissionChosenByUser'),0),
             COALESCE(json_extract(config,'$.permission'),'auto'), COALESCE(json_extract(config,'$.permissionChosenByUser'),0)
           FROM bots WHERE deleted=0 AND json_valid(config) AND COALESCE(json_extract(config,'$.kind'),'agent')='agent';",
    )?;
    // Os setters herdados já escrevem em transação. Os triggers mantêm a projeção na MESMA
    // transação, inclusive recálculo D78 e saves fora de Hub::update_bot. Save de nome não a apaga.
    for (event, condition) in [
        ("INSERT", ""),
        (
            "UPDATE OF config",
            "AND (COALESCE(json_extract(OLD.config,'$.permission'),'auto') IS NOT COALESCE(json_extract(NEW.config,'$.permission'),'auto') OR COALESCE(json_extract(OLD.config,'$.permissionChosenByUser'),0) IS NOT COALESCE(json_extract(NEW.config,'$.permissionChosenByUser'),0))",
        ),
    ] {
        let name = if event == "INSERT" { "insert" } else { "update" };
        tx.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS donna_approval_bot_{name}; CREATE TRIGGER donna_approval_bot_{name} AFTER {event} ON bots
             WHEN NEW.deleted=0 AND json_valid(NEW.config) AND COALESCE(json_extract(NEW.config,'$.kind'),'agent')='agent' {condition}
             BEGIN
               INSERT INTO donna_approvals(subject,level,chosen_by_user,legacy_permission,legacy_chosen)
               VALUES(NEW.id,CASE WHEN COALESCE(json_extract(NEW.config,'$.permission'),'auto')='auto' THEN 'auto_approve' ELSE 'ask' END,
                 COALESCE(json_extract(NEW.config,'$.permissionChosenByUser'),0), COALESCE(json_extract(NEW.config,'$.permission'),'auto'),
                 COALESCE(json_extract(NEW.config,'$.permissionChosenByUser'),0))
               ON CONFLICT(subject) DO UPDATE SET
                 level=CASE WHEN excluded.level='ask' THEN 'ask' WHEN chosen_by_user=1 AND excluded.chosen_by_user=0 THEN level ELSE excluded.level END,
                 chosen_by_user=MAX(chosen_by_user,excluded.chosen_by_user),
                 legacy_permission=excluded.legacy_permission, legacy_chosen=excluded.legacy_chosen,
                 revision=revision+1, dangerous_json='[]', engine=NULL;
               UPDATE donna_policy_meta SET revision=revision+1;
               INSERT INTO donna_rule_events(at,kind,bot_id,origin,policy_revision)
                 VALUES(CAST(strftime('%s','now') AS INTEGER)*1000,'mode_changed',NEW.id,'legacy',
                   (SELECT revision FROM donna_policy_meta WHERE id=1));
             END;"
        ))?;
    }
    for event in ["INSERT", "UPDATE OF v"] {
        let name = if event == "INSERT" { "insert" } else { "update" };
        tx.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS donna_approval_general_{name}; CREATE TRIGGER donna_approval_general_{name} AFTER {event} ON kv
             WHEN NEW.k='team.settings.v1' AND json_valid(NEW.v)
               AND (SELECT legacy_permission FROM donna_approvals WHERE subject='') IS NOT
                 CASE WHEN json_extract(NEW.v,'$.permission')='auto' THEN 'auto' ELSE 'ask' END
             BEGIN
               UPDATE donna_approvals SET level=CASE WHEN json_extract(NEW.v,'$.permission')='auto' THEN 'auto_approve' ELSE 'ask' END,
                 legacy_permission=CASE WHEN json_extract(NEW.v,'$.permission')='auto' THEN 'auto' ELSE 'ask' END,
                 revision=revision+1, dangerous_json='[]', engine=NULL WHERE subject='';
               UPDATE donna_policy_meta SET revision=revision+1;
               INSERT INTO donna_rule_events(at,kind,origin,policy_revision)
                 VALUES(CAST(strftime('%s','now') AS INTEGER)*1000,'mode_changed','legacy',
                   (SELECT revision FROM donna_policy_meta WHERE id=1));
             END;"
        ))?;
    }
    tx.execute("DELETE FROM donna_rule_events WHERE at < ?1", [crate::store::now_ms() - 365 * 86_400_000])?;
    tx.commit()?;
    Ok(())
}
