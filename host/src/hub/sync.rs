//! Bot snapshots shared by full, incremental and live synchronization.

use super::Hub;
use crate::store::BotRow;
use anyhow::Result;
use serde_json::{Value, json};

impl Hub {
    pub fn bot_json(&self, row: &BotRow) -> Value {
        let cfg = &row.config;
        let mut v = serde_json::to_value(cfg).expect("BotConfig is plain data and always serializes");
        v["spaceId"] = cfg.space_id.clone().into();
        v["conversationId"] = cfg.id.clone().into();
        v["managedWorkspace"] = crate::agent::workspace::is_managed(cfg).into();
        v["workspaceIssue"] = crate::fork::pasta::aviso(cfg).into();
        // Null where there is no switch to show.
        v["harnessMemory"] =
            (!cfg.is_group() && crate::agent::own_memory::switchable(cfg)).then_some(cfg.harness_memory).into();
        if !cfg.is_group() {
            v["sharedMemory"] = crate::chat::memory::shared(cfg).into();
            v["memoryRev"] = self.store.memory_rev().into();
        }
        let rt = if cfg.is_group() {
            // A group has no agent: Fast mode belongs to its members.
            self.observed_group_runtime(cfg)
                .map(|rt| crate::hub::Runtime { fast_mode: crate::agent::config::FastModeSupport::default(), ..rt })
        } else {
            self.observed_runtime(&cfg.id)
        };
        let last = self.store.last_visible_message(&cfg.id);
        v["skillSelection"] =
            row.skills_version.as_ref().map(|version| json!({"protocol": 1, "version": version})).into();
        v["screenAccess"] = crate::fork::tela_por_computador::projection(self, &cfg.id).unwrap_or(Value::Null);
        v["rev"] = row.rev.into();
        v["deleted"] = row.deleted.into();
        crate::fork::runtime_audiencia::project(self, cfg, rt.as_ref(), &mut v);
        let fast_mode = rt.map(|rt| rt.fast_mode).unwrap_or_default();
        v["fastModeSupport"] = fast_mode.as_str().into();
        v["fastModeIssue"] = fast_mode.issue().into();
        v["unread"] = self.store.unread(&cfg.id, row.read_rev).into();
        let preview = last.as_ref().map(|l| match &l.author {
            // A group names who spoke.
            Some(author) if cfg.is_group() => format!("{}: {}", self.bot_name(author), l.text),
            Some(_) => l.text.clone(),
            None => format!("You: {}", l.text),
        });
        v["lastMessage"] = preview.map(|p| crate::agent::acp::truncate(&p, 280)).into();
        v["lastAt"] = last.map_or(cfg.created_at, |l| l.at).into();
        v
    }

    /// Full refreshes retain cached client state, so they need deletion records too.
    #[cfg(test)]
    pub fn bots_json(&self, since: i64) -> Result<Vec<Value>> {
        self.bots_json_in_space(since, crate::fork::spaces::PERSONAL)
    }
    pub fn bots_json_in_space(&self, since: i64, space: &str) -> Result<Vec<Value>> {
        Ok(self
            .store
            .bots()?
            .iter()
            .filter(|b| b.config.space_id == space && b.rev > since)
            .map(|b| self.bot_json(b))
            .collect())
    }
}
