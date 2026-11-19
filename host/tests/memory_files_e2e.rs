//! Long-form memory files on a real host with disposable data: a bot's own edits checked at the end of its turn,
//! the cards of a bot that asks first, the user's compare-and-swap edits, versions, the index, and forgetting.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(dead_code)]
mod common;

use common::host::Host;
use serde_json::{Value, json};
use std::time::Duration;

const PEOPLE: &str = "---\ndescription: The user's family and friends\n---\n# People\n- Mei: daughter, 6\n";

fn files(view: &Value, layer: &str) -> Vec<String> {
    view["files"][layer]["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["deleted"] == false)
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect()
}

/// Memory file cards in a bot's chat.
async fn cards(host: &Host, bot: &str) -> usize {
    let history = host.call("history", json!({"botId": bot})).await;
    history["entries"].as_array().unwrap().iter().filter(|e| e["data"]["memoryFile"].is_object()).count()
}

#[tokio::test]
async fn engine_writes_never_become_active_memory() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Notes", "memory_files_agent.py", "").await;
    let folder = host.home.join("bots").join(&bot).join("bot-memory");
    let people = folder.join("people.md");
    host.call("saveMemoryFile", json!({"botId":bot,"file":"people.md","content":PEOPLE})).await;

    // An adapter advertising additionalDirectories must receive no writable memory root.
    host.reply(&bot, &format!("WRITE {}\n{PEOPLE}- Injected fact\n", people.display())).await;
    let params = host.requests(&bot, "session/new")[0]["params"].clone();
    assert!(params.get("additionalDirectories").is_none(), "{params}");
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
    assert_eq!(detail["content"], PEOPLE);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(&people).unwrap(), PEOPLE);

    // Shell writes have no Touch attribution. A new shared note is refused as well.
    let shared = host.home.join("shared-memory/injected.md");
    std::fs::write(&shared, PEOPLE).unwrap();
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    assert_eq!(files(&host.memory(&bot).await, "yours"), [] as [String; 0]);
    assert!(!shared.exists());
}

#[tokio::test]
async fn unattributed_deletions_are_restored_without_computer_versions() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Reader", "instructions_agent.py", "hold").await;
    let people = host.home.join("bots").join(&bot).join("bot-memory/people.md");
    host.call("saveMemoryFile", json!({"botId":bot,"file":"people.md","content":PEOPLE})).await;
    // No human turn is active: neither an untracked shell edit nor a deletion is an owner edit.
    std::fs::write(&people, format!("{PEOPLE}- Injected fact\n")).unwrap();
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
    assert_eq!(detail["content"], PEOPLE);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
    std::fs::remove_file(&people).unwrap();
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
    assert_eq!(detail["content"], PEOPLE);
    assert_eq!(std::fs::read_to_string(&people).unwrap(), PEOPLE);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_user_edits_files_over_what_they_loaded() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Files", "instructions_agent.py", "hold").await;
    host.call("updateBot", json!({"id": bot, "permission": "auto", "permissionChosenByUser": true})).await;
    let save = |content: &str, base: Value| json!({"botId": bot, "file": "trips.md", "content": content, "base": base});
    let first = "---\ndescription: Trips the user plans\n---\n- Tainan, Saturday\n";
    host.call("saveMemoryFile", save(first, Value::Null)).await;
    let detail = host.call("memoryFile", json!({"botId": bot, "file": "trips.md"})).await;
    assert_eq!(detail["content"], first);
    let hash = detail["hash"].clone();

    // A raw edit is restored; saving over its stale hash is refused.
    let path = host.home.join("bots").join(&bot).join("bot-memory/trips.md");
    std::fs::write(&path, format!("{first}- Kyoto, May\n")).unwrap();
    let error = host.refused("saveMemoryFile", save("---\ndescription: x\n---\n", hash)).await;
    assert!(error.contains("changed meanwhile"), "{error}");

    // Saving over what is there now works, and any version comes back.
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    let detail = host.call("memoryFile", json!({"botId": bot, "file": "trips.md"})).await;
    let versions = detail["versions"].as_array().unwrap().clone();
    assert_eq!(versions.iter().map(|v| v["author"].as_str().unwrap()).collect::<Vec<_>>(), ["user"]);
    let restore = json!({"botId": bot, "file": "trips.md", "version": versions[0]["id"], "base": detail["hash"]});
    host.call("restoreMemoryFile", restore).await;
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);

    // Ferramentas leem somente pelo vínculo do turno humano real.
    host.hold_user_turn(&bot, "Change the trip to Sunday.").await;
    // The bot's own tools read it and search finds its lines.
    assert_eq!(host.tool(&bot, "read_memory", json!({"file": "trips.md"})).await["content"], first);
    let found = host.tool(&bot, "search_memory", json!({"query": "tainan"})).await;
    assert_eq!(found["files"][0]["file"], "trips.md");

    // edit_memory refuses what the checks refuse, and writes the rest.
    let error = host
        .tool_refused(
            &bot,
            "edit_memory",
            json!({"file": "trips.md", "old": "Tainan", "new": "my password is hunter2"}),
        )
        .await;
    assert!(error.contains("never stored"), "{error}");
    let proposal =
        host.tool(&bot, "edit_memory", json!({"file": "trips.md", "old": "Saturday", "new": "Sunday"})).await;
    assert_eq!(proposal["status"], "prepared");
    assert!(std::fs::read_to_string(&path).unwrap().contains("Tainan, Saturday"));

    // Forget everything takes the files too.
    host.call("clearMemory", json!({"botId": bot})).await;
    assert!(!path.exists());
    assert_eq!(files(&host.memory(&bot).await, "bot"), [] as [String; 0]);
}

#[tokio::test]
async fn a_bot_that_asks_first_writes_its_notes_too() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Careful", "instructions_agent.py", "hold").await;
    let edit =
        json!({"file": "health.md", "content": "---\ndescription: The user's health\n---\n- Allergic to peanuts\n"});
    let refused = host.tool_refused(&bot, "edit_memory", edit.clone()).await;
    assert!(refused.contains("chamada vencida"), "{refused}");
    host.hold_user_turn(&bot, "I am allergic to peanuts. Please remember that.").await;
    assert_eq!(host.tool(&bot, "edit_memory", edit).await["status"], "prepared");
    let path = host.home.join("bots").join(&bot).join("bot-memory/health.md");
    assert!(!path.exists(), "proposta antes do sucesso não grava nota");
    assert_eq!(cards(&host, &bot).await, 0);
}

#[tokio::test]
async fn both_engines_keep_memory_outside_their_writable_directories() {
    let host = Host::start().await;
    for mode in ["codex", "claude"] {
        let bot = host.fixture_bot(mode, "instructions_agent.py", mode).await;
        host.reply(&bot, "hello").await;
        let params = host.requests(&bot, "session/new")[0]["params"].clone();
        assert!(params.get("additionalDirectories").is_none(), "{mode}");
        if mode == "claude" {
            let permissions = &params["_meta"]["claudeCode"]["options"]["settings"]["permissions"];
            let own = host.home.join("bots").join(&bot).join("bot-memory");
            for folder in [own, host.home.join("shared-memory")] {
                let absolute = format!("//{}", folder.display().to_string().trim_start_matches('/'));
                assert!(permissions["allow"].as_array().unwrap().contains(&json!(format!("Read({absolute}/**)"))));
                for tool in ["Edit", "Write"] {
                    assert!(permissions["deny"].as_array().unwrap().contains(&json!(format!("{tool}({absolute}/**)"))));
                }
            }
        } else {
            let config = host.requests(&bot, "initialize")[0]["codexConfig"].clone();
            assert_eq!(config["sandbox_workspace_write"]["writable_roots"], json!([]));
        }
    }
}

#[tokio::test]
async fn integrity_same_size_and_mtime_cannot_hide_a_disk_edit() {
    let host = Host::start().await;
    let bot = host.bot("Integrity", "auto").await;
    host.call("saveMemoryFile", json!({"botId":bot,"file":"people.md","content":PEOPLE})).await;
    let path = host.home.join("bots").join(&bot).join("bot-memory/people.md");
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, PEOPLE.replace("Mei", "Eve")).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    for _ in 0..2 {
        let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
        assert_eq!(detail["content"], PEOPLE);
        assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PEOPLE);
    }
    // A forged future timestamp must not defer restoration indefinitely either.
    std::fs::write(&path, PEOPLE.replace("Mei", "Eve")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(3_600))
        .unwrap();
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
    assert_eq!(detail["content"], PEOPLE);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn integrity_a_missing_folder_is_restored_from_the_journal() {
    let host = Host::start().await;
    let bot = host.bot("Missing", "auto").await;
    host.call("saveMemoryFile", json!({"botId":bot,"file":"people.md","content":PEOPLE})).await;
    let folder = host.home.join("bots").join(&bot).join("bot-memory");
    std::fs::rename(&folder, folder.with_extension("moved")).unwrap();
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
    assert_eq!(detail["content"], PEOPLE);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(folder.join("people.md")).unwrap(), PEOPLE);
}

#[cfg(unix)]
#[tokio::test]
async fn integrity_directory_symlinks_never_publish_into_another_bot() {
    let host = Host::start().await;
    let bot = host.bot("Victim", "auto").await;
    let other = host.bot("Other", "auto").await;
    let workspace = host.home.join("bots").join(&other).join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    host.call("saveMemoryFile", json!({"botId":bot,"file":"people.md","content":PEOPLE})).await;
    let bot_dir = host.home.join("bots").join(&bot);
    let folder = bot_dir.join("bot-memory");
    let moved = folder.with_extension("moved");
    std::fs::rename(&folder, &moved).unwrap();
    std::os::unix::fs::symlink(&workspace, &folder).unwrap();
    host.memory(&bot).await;
    let (ok, _) = host.post("memoryFile", &json!({"botId":bot,"file":"people.md"})).await;
    assert!(!workspace.join("people.md").exists(), "scanner leaked the victim's note");
    assert!(!ok, "reading through a directory symlink must be refused");
    let (ok, _) = host.post("saveMemoryFile", &json!({"botId":bot,"file":"new.md","content":PEOPLE})).await;
    assert!(!ok);
    assert!(!workspace.join("new.md").exists());
    std::fs::remove_file(&folder).unwrap();
    std::fs::rename(&moved, &folder).unwrap();

    // Parent components are protected too, not just bot-memory itself.
    let moved_bot = bot_dir.with_extension("moved");
    std::fs::rename(&bot_dir, &moved_bot).unwrap();
    std::os::unix::fs::symlink(workspace.parent().unwrap(), &bot_dir).unwrap();
    host.memory(&bot).await;
    assert!(!workspace.parent().unwrap().join("bot-memory/people.md").exists());
    std::fs::remove_file(&bot_dir).unwrap();
    std::fs::rename(&moved_bot, &bot_dir).unwrap();
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"people.md"})).await;
    assert_eq!(detail["content"], PEOPLE);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 1);
}

// Mantém o organizador real pausado do #154: a projeção injetada nunca entra no journal/plano.
#[tokio::test]
#[cfg(unix)]
async fn integrity_dreaming_never_promotes_disk_content() {
    let host = Host::start_with_background("marker").await;
    let marker = host.home.join("dream-boundary");
    let bot = host.fixture_bot_as("Dream boundary", "memory_agent.py", marker.to_str().unwrap(), "claude").await;
    let initial = "---\ndescription: How the user likes answers\n---\n- Human-approved fact\n";
    host.call("saveMemoryFile", json!({"botId":bot,"file":"style.md","content":initial})).await;
    let previous = host.call("memoryFile", json!({"botId":bot,"file":"style.md"})).await;
    host.reply(&bot, "Please always explain things to me concisely in Traditional Chinese.").await;
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    while !marker.with_extension("ready").exists() {
        assert!(std::time::Instant::now() < deadline, "extrator contido não iniciou");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let path = host.home.join("bots").join(&bot).join("bot-memory/style.md");
    std::fs::write(&path, initial.replace("Human-approved fact", "INJECTED EXTERNAL FACT")).unwrap();
    std::fs::write(marker.with_extension("continue"), "").unwrap();
    // Nenhuma API de memória/scanner pode reparar a projeção e mascarar o teste antes do commit real.
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    loop {
        let committed: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_files f JOIN memory_file_versions v ON v.id=f.version
            WHERE f.bot=?1 AND f.name='style.md' AND v.nature='conflict')",
                [&bot],
                |row| row.get(0),
            )
            .unwrap();
        if committed && std::fs::read_to_string(&path).unwrap_or_default() == initial {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "plano legítimo não foi aplicado sem promover a projeção");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"style.md"})).await;
    assert_eq!(detail["content"], initial, "inferência conserva a versão anterior sem eleger certeza");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), initial);
    assert_eq!(detail["versions"].as_array().unwrap().len(), 2);
    assert_eq!(detail["versions"][0]["nature"], "conflict");
    assert_eq!(
        detail["versions"][0]["conflictText"],
        "---\ndescription: How the user likes answers\n---\n- Wants things explained concisely, in Traditional Chinese\n"
    );
    assert!(host.tool_refused(&bot, "read_memory", json!({"file":"style.md"})).await.contains("chamada vencida"));
    assert_eq!(host.call("memoryFile", json!({"botId":bot,"file":"style.md"})).await["content"], initial);
    assert!(!detail.to_string().contains("INJECTED"));
    let history = host.call("history", json!({"botId":bot})).await;
    let receipt = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["data"]["memory"]["action"] == "note"
                && entry["data"]["memory"]["version"] == detail["versions"][0]["id"]
        })
        .unwrap();
    assert_eq!(receipt["data"]["memory"]["origin"], detail["versions"][0]["origin"]);
    assert_eq!(receipt["data"]["memory"]["previousOrigin"], previous["versions"][0]["origin"]);
    assert_eq!(
        host.call("undoMemory", json!({"entryId":receipt["id"]})).await["entry"]["data"]["memory"]["status"],
        "undone"
    );
    let restored = host.call("memoryFile", json!({"botId":bot,"file":"style.md"})).await;
    assert_eq!(restored["content"], initial);
    assert_eq!(restored["versions"][0]["nature"], "said");
    assert_eq!(restored["versions"][0]["origin"], previous["versions"][0]["origin"]);
}

#[tokio::test]
async fn nota_automatica_tem_versao_origem_e_undo_confirmados_no_hub() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Notas com origem", "memory_auto_agent.py", "").await;
    let content = "---\ndescription: Preferências de viagem\n---\n- Prefere viajar de trem\n";
    let text = format!(
        "MEMORY_AUTO {}",
        json!({"operations":[["edit_memory",{"file":"viagens.md","content":content}]],"outcome":"success"})
    );
    let mut origin = common::host::typed_origin(&text);
    origin["segments"][0]["kind"] = json!("pasted");
    host.call(
        "send",
        json!({"botId":bot,"text":text,"clientNonce":uuid::Uuid::new_v4().to_string(),"contentOrigin":origin}),
    )
    .await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let notice = loop {
        let history = host.call("history", json!({"botId":bot})).await;
        if let Some(entry) = history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["data"]["memory"]["action"] == "note" && e["data"]["memory"]["status"] == "done")
        {
            break entry.clone();
        }
        assert!(std::time::Instant::now() < deadline, "nota não foi confirmada: {history}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"viagens.md"})).await;
    assert_eq!(detail["content"], content);
    assert_eq!(detail["versions"][0]["origin"]["kind"], "external");
    assert_eq!(detail["versions"][0]["origin"], notice["data"]["memory"]["origin"]);
    let undo = host.call("undoMemory", json!({"entryId":notice["id"]})).await;
    assert_eq!(undo["entry"]["data"]["memory"]["status"], "undone");
    let detail = host.call("memoryFile", json!({"botId":bot,"file":"viagens.md"})).await;
    assert!(detail["content"].is_null());
    assert_eq!(
        host.call("undoMemory", json!({"entryId":notice["id"]})).await["entry"]["data"]["memory"]["status"],
        "undone"
    );
    // Restaurar versão externa e editá-la no editor não transforma sua origem em fala limpa.
    let original = notice["data"]["memory"]["version"].clone();
    let restored = host.call("restoreMemoryFile", json!({"botId":bot,"file":"viagens.md","version":original})).await;
    assert_eq!(restored["version"]["origin"], notice["data"]["memory"]["origin"]);
    let changed = format!("{content}- Edição humana explícita\n");
    let edited = host
        .call(
            "saveMemoryFile",
            json!({"botId":bot,"file":"viagens.md","content":changed,
        "base":codync_mem::files::hash(content)}),
        )
        .await;
    assert_eq!(edited["version"]["origin"], restored["version"]["origin"]);
    assert_eq!(edited["version"]["nature"], "said");
    host.call("deleteMemoryFile", json!({"botId":bot,"file":"viagens.md","base":codync_mem::files::hash(&changed)}))
        .await;
    let recreated = host.call("saveMemoryFile", json!({"botId":bot,"file":"viagens.md","content":content})).await;
    assert_eq!(recreated["version"]["origin"], restored["version"]["origin"], "nome com histórico não é criação nova");
}

#[tokio::test]
async fn conflito_de_nota_preserva_corpo_e_undo_restaura_natureza_da_versao_anterior() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Conflito de nota", "memory_auto_agent.py", "").await;
    let old = "---\ndescription: Viagens\n---\n- Prefere trem\n";
    host.call("saveMemoryFile", json!({"botId":bot,"file":"viagens.md","content":old})).await;
    let previous = host.call("memoryFile", json!({"botId":bot,"file":"viagens.md"})).await;
    assert_eq!(previous["versions"][0]["nature"], "said", "edição humana explícita");
    let alternative = "---\ndescription: Viagens\n---\n- Prefere avião\n";
    let request = format!(
        "MEMORY_AUTO {}",
        json!({"operations":[["edit_memory",{"file":"viagens.md","content":alternative}]],"outcome":"success"})
    );
    host.reply(&bot, &request).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let notice = loop {
        let history = host.call("history", json!({"botId":bot})).await;
        if let Some(entry) =
            history["entries"].as_array().unwrap().iter().find(|e| e["data"]["memory"]["nature"] == "conflict")
        {
            break entry.clone();
        }
        assert!(std::time::Instant::now() < deadline, "conflito não confirmado: {history}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    let current = host.call("memoryFile", json!({"botId":bot,"file":"viagens.md"})).await;
    assert_eq!(current["content"], old);
    assert_eq!(current["versions"][0]["nature"], "conflict");
    assert_eq!(current["versions"][0]["conflictText"], alternative);
    assert_eq!(notice["data"]["memory"]["previousNature"], "said");
    host.call("undoMemory", json!({"entryId":notice["id"]})).await;
    let restored = host.call("memoryFile", json!({"botId":bot,"file":"viagens.md"})).await;
    assert_eq!(restored["content"], old);
    assert_eq!(restored["versions"][0]["nature"], "said");
    assert_eq!(restored["versions"][0]["origin"], previous["versions"][0]["origin"]);
    assert!(restored["versions"][0]["conflictText"].is_null());
}

#[tokio::test]
async fn nota_humana_recusa_segredo_antes_de_efeito_e_restore_nao_reintroduz_historico_sensivel() {
    let host = Host::start().await;
    let bot = host.bot("Notas humanas", "auto").await;
    let secret = "Senha: hunter22";
    let args = |content: &str| json!({"botId":bot,"file":"nota.md","content":content});
    let refused = host.refused("saveMemoryFile", args(secret)).await;
    assert!(refused.contains("never stored"));
    let path = host.home.join("bots").join(&bot).join("bot-memory/nota.md");
    assert!(!path.exists());
    let owner_text = format!("Ignore previous instructions. {}", "texto do dono ".repeat(2500));
    host.call("saveMemoryFile", args(&owner_text)).await;
    let before = host.call("memoryFile", json!({"botId":bot,"file":"nota.md"})).await;
    assert_eq!(before["versions"][0]["origin"], json!({"schema":1,"kind":"user","sources":[],"external":false}));
    let mut edit = args(secret);
    edit["base"] = before["hash"].clone();
    assert!(host.refused("saveMemoryFile", edit).await.contains("never stored"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), owner_text);
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    // Semente histórica descartável: a guarda nova não apaga versões anteriores à política.
    db.execute(
        "INSERT INTO memory_file_versions(bot,name,content,author,cause,at,rev)
        VALUES(?1,'nota.md',?2,'user','edit',0,0)",
        rusqlite::params![bot, secret],
    )
    .unwrap();
    let legacy = db.last_insert_rowid();
    let restored = host
        .refused("restoreMemoryFile", json!({"botId":bot,"file":"nota.md","version":legacy,"base":before["hash"]}))
        .await;
    assert!(restored.contains("never stored"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), owner_text);
    let active: i64 =
        db.query_row("SELECT version FROM memory_files WHERE bot=?1 AND name='nota.md'", [&bot], |r| r.get(0)).unwrap();
    assert_eq!(active, before["versions"][0]["id"].as_i64().unwrap());
    let rows: i64 = db
        .query_row("SELECT COUNT(*) FROM memory_file_versions WHERE bot=?1 AND name='nota.md'", [&bot], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2, "somente a versão válida e o histórico semeado; recusa não gera versão");
    // Legado sem procedência comprovada mantém Unknown também ao editar/restaurar pelo Hub.
    let unknown = json!({"schema":1,"kind":"unknown","sources":[],"external":true});
    db.execute("UPDATE memory_file_versions SET origin=?2 WHERE id=?1", rusqlite::params![active, unknown.to_string()])
        .unwrap();
    let clean = "Edição humana limpa do legado";
    let edited =
        host.call("saveMemoryFile", json!({"botId":bot,"file":"nota.md","content":clean,"base":before["hash"]})).await;
    assert_eq!(edited["version"]["origin"], unknown);
    let restored = host
        .call(
            "restoreMemoryFile",
            json!({"botId":bot,"file":"nota.md","version":active,
        "base":codync_mem::files::hash(clean)}),
        )
        .await;
    assert_eq!(restored["version"]["origin"], unknown);
}

#[tokio::test]
#[cfg(unix)]
async fn consolidacao_nao_aplica_se_a_selecao_muda_durante_o_trabalho() {
    let host = Host::start_with_background("marker").await;
    let marker = host.home.join("dream-boundary");
    let bot = host.fixture_bot_as("Dream boundary", "memory_agent.py", marker.to_str().unwrap(), "claude").await;
    host.reply(&bot, "Please always explain things to me concisely in Traditional Chinese.").await;
    host.call("newSession", json!({"botId":bot})).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    while !marker.with_extension("ready").exists() {
        assert!(std::time::Instant::now() < deadline, "trabalho de memória não iniciou");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let models = host.call("functionModels", json!({})).await;
    let revision = models["document"]["revision"].as_u64().unwrap();
    host.call("restoreFunctionModel", json!({"functionId":"memory_consolidation","expectedRevision":revision})).await;
    std::fs::write(marker.with_extension("continue"), "").unwrap();
    loop {
        let history = host.call("backgroundHistory", json!({})).await;
        if history
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["status"] == "failed" && entry["failureStage"] == "revalidation")
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "mudança de seleção não foi rejeitada: {history}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(host.memory(&bot).await["bot"]["entries"], json!([]));
}
