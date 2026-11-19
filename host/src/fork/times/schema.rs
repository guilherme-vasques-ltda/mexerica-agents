use anyhow::Result;
use rusqlite::Connection;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_team_revisions(
           space_id TEXT PRIMARY KEY, revision INTEGER NOT NULL CHECK(revision>=0));
         CREATE TABLE IF NOT EXISTS donna_team_nodes(
           space_id TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL,
           parent_id TEXT, manager_bot_id TEXT, position INTEGER NOT NULL CHECK(position>=0),
           PRIMARY KEY(space_id,id),
           CHECK(parent_id IS NULL OR parent_id<>id),
           FOREIGN KEY(space_id) REFERENCES donna_team_revisions(space_id),
           FOREIGN KEY(space_id,parent_id) REFERENCES donna_team_nodes(space_id,id) DEFERRABLE INITIALLY DEFERRED,
           FOREIGN KEY(space_id,manager_bot_id) REFERENCES donna_bot_org(space_id,bot_id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TABLE IF NOT EXISTS donna_bot_org(
           space_id TEXT NOT NULL, bot_id TEXT NOT NULL UNIQUE, team_id TEXT, parent_bot_id TEXT,
           role TEXT NOT NULL CHECK(role IN ('manager','executor')),
           reports_to_bot_id TEXT, owner_contact TEXT NOT NULL CHECK(owner_contact IN ('reports','urgent_only','on_request')),
           PRIMARY KEY(space_id,bot_id),
           CHECK(parent_bot_id IS NULL OR parent_bot_id<>bot_id),
           CHECK(reports_to_bot_id IS NULL OR reports_to_bot_id<>bot_id),
           CHECK(reports_to_bot_id IS NOT NULL OR owner_contact='reports'),
           FOREIGN KEY(space_id) REFERENCES donna_team_revisions(space_id),
           FOREIGN KEY(space_id,team_id) REFERENCES donna_team_nodes(space_id,id) DEFERRABLE INITIALLY DEFERRED,
           FOREIGN KEY(space_id,parent_bot_id) REFERENCES donna_bot_org(space_id,bot_id) DEFERRABLE INITIALLY DEFERRED,
           FOREIGN KEY(space_id,reports_to_bot_id) REFERENCES donna_bot_org(space_id,bot_id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TABLE IF NOT EXISTS donna_team_routes(
           space_id TEXT NOT NULL, from_bot_id TEXT NOT NULL, to_bot_id TEXT NOT NULL,
           state TEXT NOT NULL CHECK(state='proposed'),
           PRIMARY KEY(space_id,from_bot_id,to_bot_id), CHECK(from_bot_id<>to_bot_id),
           FOREIGN KEY(space_id,from_bot_id) REFERENCES donna_bot_org(space_id,bot_id) DEFERRABLE INITIALLY DEFERRED,
           FOREIGN KEY(space_id,to_bot_id) REFERENCES donna_bot_org(space_id,bot_id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TABLE IF NOT EXISTS donna_team_receipts(
           space_id TEXT NOT NULL, client_request_id TEXT NOT NULL, digest TEXT NOT NULL,
           revision INTEGER NOT NULL CHECK(revision>=0), PRIMARY KEY(space_id,client_request_id),
           FOREIGN KEY(space_id) REFERENCES donna_team_revisions(space_id));
         CREATE INDEX IF NOT EXISTS donna_bot_org_team ON donna_bot_org(space_id,team_id);
         CREATE INDEX IF NOT EXISTS donna_bot_org_parent ON donna_bot_org(parent_bot_id);
         CREATE INDEX IF NOT EXISTS donna_bot_org_report ON donna_bot_org(reports_to_bot_id);
         CREATE INDEX IF NOT EXISTS donna_team_nodes_manager ON donna_team_nodes(manager_bot_id);",
    )?;
    tx.commit()?;
    Ok(())
}
