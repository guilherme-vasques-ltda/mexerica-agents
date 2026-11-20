// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Projeções dos arquivos de memória: pastas, reconciliação com o diário e substituição atômica no disco.
//! As versões e o CAS ficam no diário (`codync_mem::files`), antes da projeção.

use crate::LockExt;
use crate::hub::Hub;
use crate::store::{BotConfig, Store};
use anyhow::Result;
use codync_mem::files as mf;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

#[path = "file_directory.rs"]
mod directory;
use directory::Directory;

/// A file changed more recently than this may still be mid-write: the next scan takes it.
const SETTLE_MS: i64 = 2_000;

/// A layer's folder: the shared one (`None`), or a bot's own.
pub fn folder(bot: Option<&str>) -> PathBuf {
    let data = crate::service::data_dir();
    match bot {
        Some(id) => data.join("bots").join(id).join("bot-memory"),
        None => data.join("shared-memory"),
    }
}

/// The layers a bot sees: its own, and the shared one when it uses it.
pub fn layers(cfg: &BotConfig) -> Vec<Option<String>> {
    let mut layers = vec![Some(cfg.memory_id().to_owned())];
    if super::shared(cfg) {
        layers.push(None);
    }
    layers
}

/// The folders a bot reads, created without granting the engine write access. A shared layer whose leftover
/// "Sobre você" note can't be removed is not offered: the folder would expose it to the engine.
pub fn folders(cfg: &BotConfig) -> Result<Vec<PathBuf>> {
    if crate::fork::motor_exec::configured() {
        return Ok(Vec::new());
    }
    layers(cfg)
        .iter()
        .map(|layer| {
            let dir = folder(layer.as_deref());
            let opened = Directory::open(layer.as_deref())?;
            // A pasta só pode ser oferecida depois da remoção da projeção legada.
            if layer.is_none() {
                opened.put(super::profile::NAME, None)?;
            }
            Ok(dir)
        })
        .collect()
}

/// The index of the files a bot sees, for its opening.
pub fn index(store: &Store, cfg: &BotConfig) -> String {
    match store.memory_read(|c| mf::index(c, cfg.memory_id(), super::shared(cfg))) {
        Ok(mut index) => {
            // D6: a nota do dono na camada compartilhada não aparece nem na listagem sem a pessoa comprovada.
            if cfg.person.as_deref() != Some(super::profile::OWNER)
                && let Some(yours) = &mut index.yours
            {
                yours.retain(|f| f.name != super::profile::NAME);
            }
            index.render(|bot| {
                if crate::fork::motor_exec::configured() {
                    "MCP memory/read_memory".to_owned()
                } else {
                    folder(bot).display().to_string()
                }
            })
        }
        Err(error) => {
            tracing::warn!(bot = %cfg.id, error = format!("{error:#}"), "couldn't read the memory files");
            String::new()
        }
    }
}

/// Serializa a reconciliação e a projeção dos arquivos.
static SCANNING: Mutex<()> = Mutex::new(());

/// Restores unjournaled changes in the folders a bot sees. Returns whether a file was still settling.
pub fn scan(hub: &Hub, cfg: &BotConfig) -> bool {
    // MX-280: depois do sinal de parada nenhuma escrita de memória começa —
    // inclusive numa varredura destacada que sobreviveu ao prazo do shutdown.
    let Ok(_write) = hub.memory_write() else { return false };
    scan_admitted(hub, cfg)
}

// The caller already holds the memory-write guard; avoid recursive RwLock acquisition.
pub(super) fn scan_admitted(hub: &Hub, cfg: &BotConfig) -> bool {
    #[cfg(test)]
    {
        let probe = hub.store.before_memory_scan.locked().take();
        if let Some(probe) = probe {
            probe();
        }
    }
    let _one = SCANNING.locked();
    let mut deferred = false;
    for layer in layers(cfg) {
        match scan_layer(hub, layer.as_deref()) {
            Ok(fresh) => deferred |= fresh,
            Err(error) => tracing::warn!(bot = %cfg.id, error = format!("{error:#}"), "couldn't scan the memory files"),
        }
    }
    deferred
}

/// At the end of a turn: the bot's edits usually come right before its reply, so a file still settling is taken
/// once it has.
pub fn scan_after_turn(hub: &Arc<Hub>, cfg: BotConfig) {
    let label = format!("scan:{}", cfg.id);
    let task = {
        let hub = hub.clone();
        tokio::spawn(async move {
            let (h, c) = (hub.clone(), cfg.clone());
            if tokio::task::spawn_blocking(move || scan(&h, &c)).await.unwrap_or(false) {
                tokio::time::sleep(std::time::Duration::from_millis(u64::try_from(SETTLE_MS).unwrap_or(2_000) + 100))
                    .await;
                let _ = tokio::task::spawn_blocking(move || scan(&hub, &cfg)).await;
            }
        })
    };
    // MX-280: o Hub::shutdown aguarda esta tarefa; sem o registro o segundo
    // spawn_blocking (~2 s depois) podia gravar após o desligamento.
    hub.track_memory_task(&label, task);
}

/// Scans one folder; true when a file was too fresh to take yet.
fn scan_layer(hub: &Hub, layer: Option<&str>) -> Result<bool> {
    let dir = Directory::open(layer)?;
    let mut deferred = false;
    let recorded: HashMap<String, mf::File> =
        hub.store.memory_read(|c| mf::layer(c, layer))?.into_iter().map(|f| (f.name.clone(), f)).collect();
    let now = crate::store::now_ms();
    let mut present = HashSet::new();
    for name in dir.names()? {
        // MX-280: parada no meio da camada adia o resto; `present` incompleto
        // nunca chega ao sweep de deleções abaixo.
        if hub.routines.stopping() {
            return Ok(true);
        }
        if !mf::valid_name(&name) {
            continue;
        }
        let Some((content, meta)) = dir.read(&name)? else { continue };
        present.insert(name.clone());
        let (mtime, size) = metadata(&meta);
        let known = recorded.get(&name);
        // Size and mtime are controlled by disk writers. Only content proves integrity.
        // D6: a nota "Sobre você" nunca é projetada na camada compartilhada — um arquivo com esse nome nunca
        // confere (nem com conteúdo idêntico ao diário) e cai no `found`, que o remove.
        let reserved = layer.is_none() && name == super::profile::NAME;
        let projected_matches = !reserved && known.is_some_and(|f| !f.deleted && f.hash == mf::hash(&content));
        if projected_matches {
            if known.is_some_and(|f| f.mtime != mtime || f.size != size) {
                hub.store.memory_write(|c, _, _| mf::observed(c, layer, &name, mtime, size))?;
            }
            continue;
        }
        // Known notes never wait on timestamps an outside writer can keep changing.
        if known.is_none() && (0..SETTLE_MS).contains(&(now - mtime)) {
            deferred = true;
            continue;
        }
        found(hub, layer, &name, &dir)?;
    }
    for name in recorded
        .values()
        // D6: a nota do dono não é projetada — ausente do disco está correto, não há o que restaurar.
        .filter(|f| !f.deleted && !present.contains(&f.name) && (layer.is_some() || f.name != super::profile::NAME))
        .map(|f| &f.name)
    {
        if hub.routines.stopping() {
            return Ok(true);
        }
        found(hub, layer, name, &dir)?;
    }
    Ok(deferred)
}

fn metadata(meta: &std::fs::Metadata) -> (i64, u64) {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|t| i64::try_from(t.as_millis()).ok())
        .unwrap_or_default();
    (mtime, meta.len())
}

/// Disk is a projection of the Hub's journal. No unjournaled change proves owner intent.
fn found(hub: &Hub, layer: Option<&str>, name: &str, dir: &Directory) -> Result<()> {
    let path = folder(layer).join(name);
    hub.store.kv_delete(&format!("memory.touched.{}", path.display()))?;
    let before = hub.store.memory_read(|c| mf::current(c, layer, name))?;
    // D6: a nota do dono nunca vai ao disco — a pasta compartilhada é legível sem pessoa comprovada.
    let projection = before.as_deref().filter(|_| !(layer.is_none() && name == super::profile::NAME));
    dir.put(name, projection)?;
    if projection.is_some() {
        let (mtime, size) = dir.read(name)?.map(|(_, meta)| metadata(&meta)).unwrap_or_default();
        hub.store.memory_write(|c, _, _| mf::observed(c, layer, name, mtime, size))?;
    }
    // No file names, content or proposed facts in the audit log.
    tracing::warn!(shared = layer.is_none(), "memory change outside the Hub reverted");
    Ok(())
}

/// The file's content on disk now, `None` when there is none. Refuses names the folders don't hold.
pub fn read(layer: Option<&str>, name: &str) -> Result<Option<String>> {
    Ok(Directory::open(layer)?.read(name)?.map(|(content, _)| content))
}

/// A deleted bot's folder.
pub fn remove_bot(id: &str) {
    if let Err(error) = std::fs::remove_dir_all(folder(Some(id)))
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %error, "couldn't remove a deleted bot's memory files");
    }
}

/// Reconstitui a projeção a partir da versão que já foi confirmada atomicamente pelo Hub.
pub(crate) fn project_committed(hub: &Hub, layer: Option<&str>, name: &str) -> Result<()> {
    let _one = SCANNING.locked();
    let content = hub.store.memory_read(|c| mf::current(c, layer, name))?;
    // D6: a nota do dono nunca vai ao disco — a pasta compartilhada é legível sem pessoa comprovada.
    let projection = content.as_deref().filter(|_| !(layer.is_none() && name == super::profile::NAME));
    Directory::open(layer)?.put(name, projection)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    use crate::remote::identity::Identity;
    use crate::store::Store;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn pasta_insegura_ou_sobra_irremovivel_nao_e_oferecida() {
        use std::os::unix::fs::symlink;
        const CHILD: &str = "MEXERICA_TEST_PASTA_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root = std::env::temp_dir().join(format!("memory-folder-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "chat::memory::files::tests::pasta_insegura_ou_sobra_irremovivel_nao_e_oferecida"])
                .env(CHILD, "1")
                .env("MEXERICA_HOME", &root)
                .status()
                .unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert!(status.success());
            return;
        }
        let root = crate::service::data_dir();
        let external = root.join("outside");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(external.join(super::super::profile::NAME), "nota privada").unwrap();
        let shared = root.join("shared-memory");
        symlink(&external, &shared).unwrap();
        let cfg: BotConfig =
            serde_json::from_value(serde_json::json!({"id":"bot","name":"Bot","sharedMemory":true})).unwrap();
        assert!(folders(&cfg).is_err());
        assert!(external.join(super::super::profile::NAME).exists());
        std::fs::remove_file(&shared).unwrap();
        std::fs::create_dir(&shared).unwrap();
        std::fs::create_dir(shared.join(super::super::profile::NAME)).unwrap();
        assert!(folders(&cfg).is_err());
    }

    #[tokio::test]
    async fn barreira_aguarda_escritor_apos_checagem_mesmo_sem_wrapper() {
        let dir = std::env::temp_dir().join(format!("scan-barreira-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(serde_json::json!({"id":"bot", "name":"Bot", "cwd":dir})).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        hub.store.before_memory_scan.locked().replace(Box::new(move || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv_timeout(std::time::Duration::from_secs(30));
        }));
        let worker = {
            let hub = hub.clone();
            tokio::task::spawn_blocking(move || scan(&hub, &cfg))
        };
        entered_rx.await.unwrap(); // a guarda e a checagem já foram adquiridas
        let mut stopped = hub.memory_stop.subscribe();
        let shutdown = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.shutdown().await })
        };
        stopped.changed().await.unwrap();
        assert!(!shutdown.is_finished());
        release_tx.send(()).unwrap();
        worker.await.unwrap();
        shutdown.await.unwrap();
        assert!(hub.memory_write().is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Sonda dentro do spawn_blocking da varredura: prova a entrada do trabalho
    // bloqueante e o segura até o shutdown estar esperando — sem sleep fixo.
    #[tokio::test]
    async fn desligamento_espera_varredura_apos_turno() {
        let dir = std::env::temp_dir().join(format!("scan-shutdown-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(serde_json::json!({"id":"bot", "name":"Bot", "cwd":dir})).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        hub.store.before_memory_scan.locked().replace(Box::new(move || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv_timeout(std::time::Duration::from_secs(30));
        }));
        scan_after_turn(&hub, cfg);
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(std::time::Duration::from_secs(30)))
            .await
            .unwrap()
            .expect("a varredura entrou no spawn_blocking");
        let mut stopped = hub.memory_stop.subscribe();
        let shutdown = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.shutdown().await })
        };
        stopped.changed().await.unwrap();
        assert!(!shutdown.is_finished(), "shutdown voltou com a varredura após turno em andamento");
        release_tx.send(()).unwrap();
        shutdown.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
