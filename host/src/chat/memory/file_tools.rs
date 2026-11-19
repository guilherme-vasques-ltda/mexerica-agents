//! The bot's long-form memory tools: the only supported way an engine edits these files.

use super::files;
use crate::hub::{Hub, Runtime};
use crate::store::BotConfig;
use anyhow::{Result, anyhow, bail};
use codync_mem::files as mf;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn definitions() -> [Value; 2] {
    let file = json!({"type": "string", "description": "The file name, like people.md."});
    let shared = json!({"type": "boolean", "description": "True for a file in the memory all the user's bots share."});
    [
        json!({
            "name": "read_memory",
            "description": "Read one of your long-form memory files, or without a file, list them with their descriptions.",
            "inputSchema": {"type": "object", "properties": {"file": file, "shared": shared}},
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "edit_memory",
            "description": "Create, change or delete one of your long-form memory files: notes on one topic about the user, too detailed for a memory entry. Give old and new to replace text that appears once, content to write the whole file, or delete. A file starts with frontmatter whose description: line says when to read it, and holds at most 2,000 tokens. Prepares a proposal only in an owner turn; applied after success with origin and undo. The user sees every version in the bot's memory settings and can bring one back there.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "shared": shared,
                    "old": {"type": "string", "description": "Text the file has exactly once."},
                    "new": {"type": "string", "description": "What replaces old."},
                    "content": {"type": "string", "description": "The whole file, frontmatter first."},
                    "delete": {"type": "boolean"},
                },
                "required": ["file"],
            },
        }),
    ]
}

pub(crate) async fn call_in_turn(
    hub: &Arc<Hub>,
    cfg: &BotConfig,
    name: &str,
    args: &Value,
    turn: &Runtime,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
) -> Result<Value> {
    crate::fork::sessoes::bindings::recheck_optional(hub, turn.execution.as_ref())?;
    // A captura original vem do roteador; o worker nunca adota outro turno.
    let turn = turn.clone();
    let context = context.cloned();
    if name == "edit_memory" && cfg.backend == "codex" {
        bail!("codex_memory_mutation_refused");
    }
    if name == "edit_memory" && turn.memory_turn_token.is_none() {
        bail!("Não há turno ativo de memória.");
    }
    let (hub, cfg, name, args) = (hub.clone(), cfg.clone(), name.to_owned(), args.clone());
    tokio::task::spawn_blocking(move || {
        // MX-280: closure destacada não pode gravar depois do sinal de parada.
        let _write = hub.memory_write()?;
        if let Some(context) = &context {
            context.check(&hub)?;
        }
        match name.as_str() {
            "read_memory" => read(&hub, &cfg, &args),
            "edit_memory" => edit(&hub, &cfg, &args, &turn, context.as_ref()),
            _ => bail!("unknown memory tool: {name}"),
        }
    })
    .await?
}

fn layer(cfg: &BotConfig, args: &Value) -> Result<Option<String>> {
    if args["shared"] != true {
        return Ok(Some(cfg.memory_id().to_owned()));
    }
    if !super::shared(cfg) {
        bail!("This bot doesn't use the shared memory.");
    }
    Ok(None)
}

fn read(hub: &Hub, cfg: &BotConfig, args: &Value) -> Result<Value> {
    files::scan_admitted(hub, cfg);
    // D6: a nota do dono na camada compartilhada só sai para a conversa comprovadamente dele — nem na listagem.
    let owner = cfg.person.as_deref() == Some(crate::chat::memory::profile::OWNER);
    let Some(name) = args["file"].as_str() else {
        let files: Vec<Value> = files::layers(cfg)
            .iter()
            .flat_map(|layer| {
                hub.store
                    .memory_read(|c| mf::layer(c, layer.as_deref()))
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|f| !f.deleted && (owner || f.bot.is_some() || f.name != crate::chat::memory::profile::NAME))
            })
            .map(|f| json!({"file": f.name, "shared": f.bot.is_none(), "description": f.description, "tokens": f.tokens,"origin":f.origin,"nature":f.nature,"natureBot":f.nature_bot,"conflictText":f.conflict_text}))
            .collect();
        return Ok(json!({"files": files}));
    };
    let layer = layer(cfg, args)?;
    anyhow::ensure!(layer.is_some() || name != crate::chat::memory::profile::NAME || owner, "memory_user_owned");
    let content = hub
        .store
        .memory_read(|c| mf::current(c, layer.as_deref(), name))?
        .ok_or_else(|| anyhow!("There's no memory file {name}."))?;
    let file = hub
        .store
        .memory_read(|c| mf::get(c, layer.as_deref(), name))?
        .ok_or_else(|| anyhow!("Memory file not found."))?;
    Ok(
        json!({"file":name,"content":if layer.is_none() && name == crate::chat::memory::profile::NAME { super::profile::visible(&content) } else { &content },"origin":file.origin,"nature":file.nature,"natureBot":file.nature_bot,"conflictText":file.conflict_text}),
    )
}

fn edit(
    hub: &Hub,
    cfg: &BotConfig,
    args: &Value,
    turn: &Runtime,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
) -> Result<Value> {
    let name = args["file"].as_str().ok_or_else(|| anyhow!("file required"))?;
    let layer = layer(cfg, args)?;
    anyhow::ensure!(layer.is_some() || name != crate::chat::memory::profile::NAME, "memory_user_owned");
    let before = hub.store.memory_read(|c| mf::current(c, layer.as_deref(), name))?;
    let after = if args["delete"] == true {
        if before.is_none() {
            bail!("There is no memory file {name}.");
        }
        None
    } else if let Some(content) = args["content"].as_str() {
        Some(content.to_owned())
    } else {
        let (old, new) = (args["old"].as_str().unwrap_or_default(), args["new"].as_str().unwrap_or_default());
        let current = before.as_deref().ok_or_else(|| anyhow!("Read the memory file first."))?;
        if old.is_empty() || current.matches(old).count() != 1 {
            bail!("The replacement must match exactly once.");
        }
        Some(current.replacen(old, new, 1))
    };
    if let Some(content) = &after {
        mf::check_change(before.as_deref(), content)?;
    }
    let message = turn.message.clone().ok_or_else(|| anyhow!("Memory proposals require an owner turn."))?;
    crate::fork::memory_auto::apply::stage_in(
        hub,
        cfg,
        turn,
        &crate::fork::memory_auto::worker::Operation::Note {
            file: name.to_owned(),
            content: after,
            shared: layer.is_none(),
            expected_hash: before.as_deref().map(mf::hash),
            evidence: vec![message],
            nature: None,
        },
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hub::BotStatus,
        remote::identity::Identity,
        store::{EntryKind, Lane, Store},
    };

    #[test]
    fn worker_atrasado_nao_grava_usando_turno_novo() {
        let dir = std::env::temp_dir().join(format!("memory-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let id = format!("worker-{}", uuid::Uuid::new_v4());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({"id":id,"name":"Teste","cwd":dir})).unwrap();
        store.save_bot(&cfg).unwrap();
        store.memory_session_start(&id, "session-1", false).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let entry = hub
            .add_entry(
                &Lane::main(&id),
                EntryKind::User,
                1,
                &json!({"text":"lembre", "contentOrigin":{"status":"appReported","segments":[{"kind":"typed"}]}}),
            )
            .unwrap();
        hub.set_runtime(&id, |r| {
            r.status = BotStatus::Working;
            r.message = Some(entry.id);
            r.session = Some("session-1".into());
            r.started_at = Some(1);
            r.memory_turn_token = Some("turno-1".into());
        });
        let captured = hub.runtime(&id);
        let (ready, wait) = std::sync::mpsc::channel();
        let worker_hub = hub.clone();
        let worker_cfg = cfg.clone();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            edit(
                &worker_hub,
                &worker_cfg,
                &json!({"file":"teste.md","content":"---\ndescription: Teste\n---\n- Texto"}),
                &captured,
                None,
            )
        });
        hub.set_memory_runtime(&id, |r| {
            r.started_at = Some(2);
            r.memory_turn_token = Some("turno-2".into());
        });
        ready.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(hub.store.memory_read(|c| mf::layer(c, Some(&id))).unwrap().len(), 0);
        assert!(!files::folder(Some(&id)).join("teste.md").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // D6: sem a pessoa do dono comprovada no turno, a nota compartilhada não sai — nem pela ferramenta de
    // leitura, nem na listagem, nem projetada na pasta que o motor enxerga.
    #[test]
    fn nota_do_dono_some_dos_caminhos_compartilhados_sem_pessoa() {
        let dir = std::env::temp_dir().join(format!("memory-nota-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(files::folder(None)).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut cfg: BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Bot","cwd":dir,"sharedMemory":true})).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        hub.store
            .memory_write(|db, now, rev| {
                mf::record(
                    db,
                    &mf::Change {
                        bot: None,
                        name: crate::chat::memory::profile::NAME,
                        content: Some("SENTINELA_NOTA_DONO"),
                        author: mf::Author::User,
                        author_bot: None,
                        cause: mf::Cause::Edit,
                        mtime: now,
                        size: 17,
                    },
                    now,
                    rev,
                )?;
                Ok(())
            })
            .unwrap();
        // Mesmo pedindo a projeção, a nota não vai à pasta compartilhada.
        std::fs::write(files::folder(None).join(crate::chat::memory::profile::NAME), "SENTINELA_NOTA_DONO").unwrap();
        files::project_committed(&hub, None, crate::chat::memory::profile::NAME).unwrap();
        assert!(!files::folder(None).join(crate::chat::memory::profile::NAME).exists(), "a nota nunca vai ao disco");
        // Uma sobra recriada depois da projeção — inclusive com o conteúdo exato do diário — é removida pela
        // varredura: o nome reservado nunca confere, nem por hash.
        std::fs::write(files::folder(None).join(crate::chat::memory::profile::NAME), "SENTINELA_NOTA_DONO").unwrap();
        files::scan_admitted(&hub, &cfg);
        assert!(!files::folder(None).join(crate::chat::memory::profile::NAME).exists(), "a sobra legada é removida");
        // Sem pessoa (Ponte, origem desconhecida, pedido sem prova): leitura recusada e fora da listagem.
        let refused = read(&hub, &cfg, &json!({"shared":true,"file":crate::chat::memory::profile::NAME})).unwrap_err();
        assert!(refused.to_string().contains("memory_user_owned"), "{refused:#}");
        let listed = read(&hub, &cfg, &json!({"shared":true})).unwrap();
        assert!(
            listed["files"].as_array().unwrap().iter().all(|f| f["file"] != crate::chat::memory::profile::NAME),
            "{listed}"
        );
        // Na conversa comprovadamente do dono, a leitura vale normalmente.
        cfg.person = Some(crate::chat::memory::profile::OWNER.into());
        let got = read(&hub, &cfg, &json!({"shared":true,"file":crate::chat::memory::profile::NAME})).unwrap();
        assert_eq!(got["content"], "SENTINELA_NOTA_DONO");
        std::fs::remove_dir_all(dir).unwrap();
    }

    // D6: pelo roteador real (`memoryCall`), a pessoa vem da marca que o Hub fixou na ACP do turno — o cadastro do
    // bot nunca a traz. Sem marca, a nota do dono não sai nem com a sessão rodando.
    #[tokio::test]
    async fn roteador_deriva_a_pessoa_da_marca_da_acp() {
        let dir = std::env::temp_dir().join(format!("memory-roteador-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(files::folder(None)).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Bot","cwd":dir,"sharedMemory":true})).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        hub.store
            .memory_write(|db, now, rev| {
                mf::record(
                    db,
                    &mf::Change {
                        bot: None,
                        name: crate::chat::memory::profile::NAME,
                        content: Some("SENTINELA_NOTA_DONO"),
                        author: mf::Author::User,
                        author_bot: None,
                        cause: mf::Cause::Edit,
                        mtime: now,
                        size: 17,
                    },
                    now,
                    rev,
                )?;
                Ok(())
            })
            .unwrap();
        let args = json!({"shared":true,"file":crate::chat::memory::profile::NAME});
        // Turno rodando com ACP sem marca: nem a conversa do dono é presumida — recusado.
        hub.set_runtime("bot", |r| r.session = Some("s-qualquer".into()));
        let refused = crate::chat::memory::tool_call(&hub, "bot", "read_memory", &args).await.unwrap_err().to_string();
        assert!(refused.contains("memory_user_owned"), "{refused}");
        // ACP marcada com o dono pelo Hub: o roteador deriva a pessoa e a nota sai.
        crate::chat::memory::bind_profile_person(
            &hub.store,
            "bot",
            "s-dono",
            Some(crate::chat::memory::profile::OWNER),
        )
        .unwrap();
        hub.set_runtime("bot", |r| r.session = Some("s-dono".into()));
        let got = crate::chat::memory::tool_call(&hub, "bot", "read_memory", &args).await.unwrap();
        assert_eq!(got["content"], "SENTINELA_NOTA_DONO");
        // ACP de outra pessoa comprovada no mesmo bot: a do dono continua fora.
        crate::chat::memory::bind_profile_person(&hub.store, "bot", "s-outro", Some("membro-b")).unwrap();
        hub.set_runtime("bot", |r| r.session = Some("s-outro".into()));
        let refused = crate::chat::memory::tool_call(&hub, "bot", "read_memory", &args).await.unwrap_err().to_string();
        assert!(refused.contains("memory_user_owned"), "{refused}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
