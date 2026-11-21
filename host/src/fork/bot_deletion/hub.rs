//! R1/P2: a exclusão só encerra ator/trabalho após confirmação durável.
//! Compilado como filho do Hub para não ampliar visibilidade dos mapas/locks.
use super::{Cmd, Hub, LockExt};
use anyhow::Result;
use serde_json::json;

impl Hub {
    pub(super) fn commit_bot_deletion(&self, id: &str) -> Result<i64> {
        let rev = {
            // memory_boundary (delete_bot) -> emit -> runtime -> bots -> routines.state -> Store.db.
            // ACP valida Store; team libera runtime/requests antes de emitir/enviar.
            // updateBot usa boundary -> emit -> owner/DB. A barreira nunca atravessa await.
            // emit já consulta runtime em bot_json; o Desfazer usa runtime ->
            // routines.state -> DB. Não tomar runtime/bots/emit de dentro da Tx.
            // As reservas são reversíveis: Err só libera locks, sem Shutdown,
            // remoção de mapas, cancelamento ou publicação de estado candidato.
            let _emit = self.emit_lock.locked();
            let group_space = self.store.bot_group_space(id)?;
            let mut runtime = self.runtime.locked();
            let mut bots = self.bots.locked();
            let rev = self.routines.delete_bot_committed(&self.store, id)?;
            if let Some(handle) = bots.remove(id) {
                let _ = handle.tx.send(Cmd::Shutdown);
            }
            runtime.remove(id);
            drop(bots);
            drop(runtime);
            let _ = self.events.send(json!({"type":"bot","rev":rev,"bot":{"id":id,"deleted":true,"rev":rev}}));
            if let Some(space) = group_space {
                crate::hub::bot_groups::publish_locked(self, &space);
            }
            rev
        };
        // Nenhum desses efeitos reversíveis apenas por nova execução ocorre
        // antes do commit. Rooms::stop pode ler Store e enviar comandos: sem locks.
        self.team.cancel_bot(id);
        crate::fork::open_chat::stop(id);
        self.groups.stop(self, id);
        Ok(rev)
    }

    /// D543: os mesmos efeitos pós-commit do `commit_bot_deletion`, em lote, para os
    /// bots já marcados `deleted` na Tx do Space. Não toca no banco.
    pub(crate) fn finish_deleted_bots(&self, deleted: &[(String, i64)]) -> Result<()> {
        {
            let _emit = self.emit_lock.locked();
            let mut runtime = self.runtime.locked();
            let mut bots = self.bots.locked();
            for (id, _) in deleted {
                if let Some(handle) = bots.remove(id) {
                    let _ = handle.tx.send(Cmd::Shutdown);
                }
                runtime.remove(id);
            }
            drop(bots);
            drop(runtime);
            for (id, rev) in deleted {
                let _ = self.events.send(json!({"type":"bot","rev":rev,"bot":{"id":id,"deleted":true,"rev":rev}}));
            }
        }
        let gone: std::collections::HashSet<&str> = deleted.iter().map(|(id, _)| id.as_str()).collect();
        for (id, _) in deleted {
            self.team.cancel_bot(id);
            crate::fork::open_chat::stop(id);
            self.groups.stop(self, id);
            crate::fork::owner_question::deleted(self, id);
            crate::fork::runtime_audiencia::screen::changed(self, id);
            crate::fork::whatsapp::bot_deleted(self, id);
            // Its shared files are unreachable once the bot is deleted.
            let files = crate::chat::files::root(id);
            let bot = id.clone();
            tokio::task::spawn_blocking(move || {
                crate::chat::memory::files::remove_bot(&bot);
                if let Err(error) = std::fs::remove_dir_all(&files)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(error = %error, "couldn't remove a deleted bot's shared files");
                }
            });
        }
        // A deleted bot leaves its groups; what it said there stays.
        for row in self.store.bots()? {
            let mut group = row.config;
            if !row.deleted && group.is_group() && group.members.iter().any(|m| gone.contains(m.as_str())) {
                group.members.retain(|m| !gone.contains(m.as_str()));
                {
                    let _g = self.emit_lock.locked();
                    self.store.save_bot(&group)?;
                }
                self.emit_bot(&group.id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
