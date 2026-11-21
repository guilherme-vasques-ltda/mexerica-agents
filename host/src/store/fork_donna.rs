//! D190: a principal de um Hub novo, sem executar a IA ou reabrir escolhas do dono.

use super::{BotConfig, Store, next_rev, now_ms};
use crate::LockExt;
use anyhow::Result;
use rusqlite::{TransactionBehavior, params};
use serde_json::json;

const KEY: &str = "donna.defaultBot.v1";
/// D366: a base canônica vive em `fork::donna_fixa::BASE`; o perfil salvo é só o registro da criação.
const PROFILE: &str = crate::fork::donna_fixa::BASE;
const SETUP_NOTICE: &str = "O Berga já está criado como seu bot principal. Para conversar, conecte e escolha uma conta de IA em Configurações do bot → Motor → Gerenciar contas de IA. Se este Hub já tem um login configurado para o Codex, ele continua disponível. Contas e ferramentas só são ligadas quando você escolher.";
const OPEN_SETUP_NOTICE: &str = "O Berga já está criado como seu bot principal. Para conversar, conecte uma conta do Codex em Ajustes → Contas de IA. Nesta edição a conversa é só por texto.";

impl Store {
    /// Antes de subir os atores e aceitar clientes. O marcador sobrevive à exclusão da Donna.
    pub(crate) fn initialize_default_donna(&self) -> Result<()> {
        self.initialize_donna(crate::agent::workspace::prepare)
    }

    fn initialize_donna(&self, prepare_workspace: impl FnOnce(&mut BotConfig) -> Result<()>) -> Result<()> {
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let initialized: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [KEY], |r| r.get(0))?;
        if initialized {
            return Ok(());
        }
        // Não usar bots(): perfis ilegíveis e tombstones também representam escolhas anteriores.
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM bots) OR EXISTS(SELECT 1 FROM entries)
                OR EXISTS(SELECT 1 FROM kv WHERE k='team.settings.v1')",
            [],
            |r| r.get(0),
        )?;
        if existing {
            tx.execute("INSERT INTO kv(k,v) VALUES(?1,'preserved')", [KEY])?;
            tx.commit()?;
            return Ok(());
        }
        let mut cfg: BotConfig = serde_json::from_value(json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": "Berga", "description": PROFILE,
            "backend": "codex", "permission": "ask", "manageTeam": true,
            "mexeriquinha": crate::fork_avatars::principal_mexeriquinha(),
            "createdAt": now_ms()
        }))?;
        crate::hub::validate(&mut cfg)?;
        prepare_workspace(&mut cfg)?;
        let settings = super::TeamSettings { primary_bot_id: Some(cfg.id.clone()), ..Default::default() };
        // A identidade precisa existir antes do salvamento, que recusa manageTeam fora da principal.
        tx.execute("INSERT INTO kv(k,v) VALUES('team.settings.v1',?1)", [serde_json::to_string(&settings)?])?;
        super::bots::save_bot(&tx, &cfg)?;
        let data = if crate::environment::Environment::is_open() {
            json!({"text": OPEN_SETUP_NOTICE, "code": "open_setup"})
        } else {
            json!({"text": SETUP_NOTICE})
        };
        let notice = crate::fork::spaces::stamp_entry(&tx, &cfg.id, &data)?;
        tx.execute(
            "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at)
                VALUES(?1,?2,?3,'notice',0,?4,?5,?5)",
            params![uuid::Uuid::new_v4().to_string(), cfg.id, next_rev(&tx)?, notice.to_string(), cfg.created_at],
        )?;
        tx.execute("INSERT INTO kv(k,v) VALUES(?1,?2)", params![KEY, cfg.id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "fork_donna_tests.rs"]
mod tests;
