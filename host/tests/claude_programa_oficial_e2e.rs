//! A conversa de um bot Claude passa pelo Hub real, pela ponte do programa oficial (`__claude-ponte`) e
//! por um programa falso que imita o Claude Code no modo sem tela. Prova ponta a ponta: resposta, ferramenta
//! que exigiria aprovação devolvida negada sem nenhum cartão, custo no relatório e retomada depois de
//! reiniciar o Hub.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;
use common::host::{Host, typed_origin};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

async fn esperar(host: &Host, bot: &str, pronto: impl Fn(&Value) -> bool) -> Value {
    let limite = Instant::now() + Duration::from_secs(30);
    loop {
        let historico = host.call("history", json!({"botId": bot})).await;
        if let Some(e) = historico["entries"].as_array().unwrap().iter().rev().find(|e| pronto(e)) {
            return e.clone();
        }
        assert!(Instant::now() < limite, "sem o esperado: {historico}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn finais(historico: &Value) -> Vec<String> {
    historico["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "agent" && e["data"]["final"] == true)
        .map(|e| e["data"]["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn lancamentos(perfil: &Path) -> Vec<Value> {
    std::fs::read_to_string(perfil.join("lancamentos.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn aspas(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "'\\''"))
}

#[tokio::test]
async fn bot_claude_conversa_pelo_programa_oficial_com_negacao_custo_e_retomada() {
    let mut host = Host::start().await;
    // O Hub do teste usa CLAUDE_CONFIG_DIR=<home>/claude-test: é o perfil da conta para o programa falso.
    let perfil = host.home.join("claude-test");
    std::fs::create_dir_all(&perfil).unwrap();
    std::fs::write(perfil.join("logado"), "").unwrap();
    std::fs::write(perfil.join(".credentials.json"), "{\"segredo\":\"nao-deve-sair\"}").unwrap();
    let bin = host.home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let programa = bin.join("claude");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_oficial_falso.py"), &programa)
        .unwrap();
    std::fs::set_permissions(&programa, std::fs::Permissions::from_mode(0o700)).unwrap();
    let comando =
        format!("{} __claude-ponte {}", aspas(Path::new(env!("CARGO_BIN_EXE_donnabot-host"))), aspas(&programa));
    let criar = json!({"name": "Claude oficial", "backend": "claude", "command": comando, "connectors": [],
        "notify": false, "permission": "ask"});
    let bot = host.call("createBot", criar).await["bot"]["id"].as_str().unwrap().to_owned();

    host.reply(&bot, "oi").await;
    let historico = host.call("history", json!({"botId": bot})).await;
    assert!(finais(&historico).last().unwrap().contains("olá de claude-falso-padrao, turno 1"), "{historico}");

    let texto = "use a [[ferramenta]]";
    host.call(
        "send",
        json!({"botId": bot, "text": texto, "clientNonce": uuid::Uuid::new_v4().to_string(),
        "contentOrigin": typed_origin(texto)}),
    )
    .await;
    esperar(&host, &bot, |e| e["kind"] == "agent" && e["data"]["final"] == true && e["data"]["text"] == "não rodei")
        .await;
    let historico = host.call("history", json!({"botId": bot})).await;
    assert!(
        historico["entries"].as_array().unwrap().iter().all(|e| e["kind"] != "permission"),
        "sem cartão de aprovação nesta versão: a ponte nega a ferramenta sem pedir ao dono: {historico}"
    );
    let ferramenta = esperar(&host, &bot, |e| e["kind"] == "tool" && e["data"]["toolCallId"] == "toolu_2").await;
    assert_eq!(ferramenta["data"]["status"], "failed", "a ferramenta volta negada, sem rodar: {ferramenta}");
    assert_eq!(ferramenta["data"]["toolKind"], "execute");

    let custo = host.call("donnaCostReport", json!({"botId": bot})).await;
    let total = &custo["conversation"]["totals"];
    assert_eq!(total["turns"], 2, "{custo}");
    assert!((total["costs"][0]["amount"].as_f64().unwrap() - 0.02).abs() < 1e-9, "{custo}");
    assert_eq!(total["costs"][0]["source"], "motor");
    assert_eq!(total["cachedReadTokens"], 200);

    host.restart(|_| {}).await;
    host.reply(&bot, "de novo").await;
    let historico = host.call("history", json!({"botId": bot})).await;
    assert!(finais(&historico).last().unwrap().contains("turno 3"), "a conversa do programa continuou: {historico}");
    let l = lancamentos(&perfil);
    let valor = |l: &Value, nome: &str| {
        l["args"].as_array().unwrap().iter().find_map(|a| a.as_str()?.strip_prefix(nome).map(str::to_owned))
    };
    let sessao = valor(&l[0], "--session-id=").unwrap();
    assert_eq!(
        valor(l.last().unwrap(), "--resume="),
        Some(sessao),
        "depois de reiniciar o Hub, a mesma sessão do programa: {l:?}"
    );
    assert!(l.iter().all(|x| x["env_sdk"] == json!({})), "nenhum lançamento pelo SDK de agentes: {l:?}");
    let custo = host.call("donnaCostReport", json!({"botId": bot})).await;
    assert!((custo["conversation"]["totals"]["costs"][0]["amount"].as_f64().unwrap() - 0.03).abs() < 1e-9, "{custo}");
    let tudo = format!("{historico}{custo}");
    assert!(!tudo.contains("nao-deve-sair"), "a credencial do perfil nunca chega ao Hub");
}

/// A mesma impressão do registro de contas (`Registry::fingerprint`): sal, motor, espaço e identidade.
fn impressao(sal: &str, motor: &str, espaco: &str, identidade: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hash = Sha256::new();
    for parte in [sal, motor, espaco, identidade] {
        hash.update(parte.as_bytes());
        hash.update([0]);
    }
    let mut id = String::from("ac_");
    for byte in hash.finalize() {
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// Sem comando próprio no bot e sem a opção do caminho anterior: uma conta Claude gerenciada sobe pela ponte do
/// programa oficial, no perfil isolado da conta.
#[tokio::test]
async fn conta_claude_gerenciada_usa_a_ponte_por_padrao() {
    let host = Host::start().await;
    let bin = host.home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let programa = bin.join("claude");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_oficial_falso.py"), &programa)
        .unwrap();
    std::fs::set_permissions(&programa, std::fs::Permissions::from_mode(0o700)).unwrap();
    let sal = uuid::Uuid::new_v4().to_string();
    let identidade = "a@example.invalid\0org";
    let id = impressao(&sal, "claude", "account", identidade);
    let quota = impressao(&sal, "claude", "quota", identidade);
    let perfil = host.home.join("engine-setup").join("accounts").join(&id).join("a");
    std::fs::create_dir_all(&perfil).unwrap();
    std::fs::write(perfil.join("logado"), "").unwrap();
    let launch = json!({"kind": "cli_profile", "executable": programa, "directory": perfil});
    let prova = json!({"method": "claude_auth_status", "plan": "max"});
    let conta = json!({"id": id, "backend": "claude", "billing": "subscription", "state": "connected", "launch": launch,
        "scope": "owner", "quotaIdentity": quota, "verifiedAt": 1, "subscriptionProof": prova, "models": [{"id": "sonnet", "options": []}],
        "managed": {"label": "Conta A", "revision": 1, "generation": 0, "enabled": true, "requestId": "fixture", "principal": "fixture",
            "profile": null, "setupRequest": null,
            "catalog": {"accountId": id, "principal": "fixture", "launch": launch, "revision": 1, "subscription": prova}}});
    let registro = json!({"salt": sal, "accounts": [conta]});
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    db.execute("INSERT OR REPLACE INTO kv(k,v) VALUES('fork.engine-accounts.v1',?1)", [registro.to_string()]).unwrap();
    drop(db);
    let criar = json!({"name": "Claude da conta", "backend": "claude", "connectors": [], "notify": false, "model": "sonnet",
        "forkEditor": {"engineAccountId": id}});
    let bot = host.call("createBot", criar).await["bot"]["id"].as_str().unwrap().to_owned();
    host.reply(&bot, "oi").await;
    let historico = host.call("history", json!({"botId": bot})).await;
    assert!(finais(&historico).last().unwrap().contains("olá de claude-falso-sonnet"), "{historico}");
    let l = lancamentos(&perfil);
    assert!(!l.is_empty(), "o programa oficial da conta foi aberto no perfil dela");
    assert!(l.iter().all(|x| x["env_sdk"] == json!({})), "nenhum lançamento pelo SDK de agentes: {l:?}");
}
