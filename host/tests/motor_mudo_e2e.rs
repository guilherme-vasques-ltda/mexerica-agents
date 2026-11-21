//! O Hub avisa, sem matar nada, quando o motor fica mudo no meio de um turno. Um Hub real com motores de
//! mentira (`tests/fixtures/silent_agent.py`) e o limite encurtado para 3 s.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::host::Host;
use serde_json::{Value, json};
use std::process::Command;
use std::time::{Duration, Instant};

const LIMITE: &str = "3";

fn python_missing() -> bool {
    let missing = Command::new(common::python()).arg("--version").output().is_err();
    if missing {
        eprintln!("skipping: Python not available");
    }
    missing
}

async fn bot_com(modo: &str) -> (Host, String) {
    let host = Host::start_with_env(&[("MEXERICA_MOTOR_MUDO_SEGUNDOS", LIMITE.into())]).await;
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/silent_agent.py");
    let comando = format!("{} {modo}", common::python_agent(&script));
    let corpo = json!({"name": "Mudo", "backend": "custom", "command": comando, "connectors": [], "notify": false, "permission": "ask"});
    let bot = host.call("createBot", corpo).await["bot"]["id"].as_str().unwrap().to_owned();
    (host, bot)
}

async fn entradas(host: &Host, bot: &str) -> Vec<Value> {
    host.call("history", json!({"botId": bot})).await["entries"].as_array().unwrap().clone()
}

async fn esperar(host: &Host, bot: &str, prazo: Duration, pred: impl Fn(&Value) -> bool) -> Value {
    let limite = Instant::now() + prazo;
    loop {
        if let Some(e) = entradas(host, bot).await.into_iter().rev().find(|e| pred(e)) {
            return e;
        }
        assert!(Instant::now() < limite, "tempo esgotado; histórico: {:?}", entradas(host, bot).await);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn avisos_de_motor_mudo(entradas: &[Value]) -> Vec<&Value> {
    entradas.iter().filter(|e| e["kind"] == "notice" && !e["data"]["motorMudo"].is_null()).collect()
}

async fn status(host: &Host, bot: &str) -> String {
    let sync = host.snapshot().await;
    common::snapshot_bot(&sync, bot)["status"].as_str().unwrap().to_owned()
}

async fn esperar_ocioso(host: &Host, bot: &str) {
    let limite = Instant::now() + Duration::from_secs(60);
    while status(host, bot).await != "idle" {
        assert!(Instant::now() < limite, "o turno não terminou");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn motor_mudo_gera_um_aviso_sem_matar_nem_repetir_o_turno() {
    if python_missing() {
        return;
    }
    let (host, bot) = bot_com("mudo").await;
    host.call("send", json!({"botId": bot, "text": "oi"})).await;
    let aviso = esperar(&host, &bot, Duration::from_secs(30), |e| {
        e["kind"] == "notice" && e["data"]["motorMudo"]["estado"] == "mudo"
    })
    .await;
    assert_eq!(aviso["data"]["motorMudo"]["segundos"], 3);
    assert!(aviso["data"]["text"].as_str().unwrap().contains("sem dar sinal"), "{aviso}");

    // Passa mais de um limite inteiro: o aviso não se repete, o turno segue aberto e o motor não foi morto.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let tudo = entradas(&host, &bot).await;
    assert_eq!(avisos_de_motor_mudo(&tudo).len(), 1, "{tudo:?}");
    assert_eq!(status(&host, &bot).await, "working");
    assert_eq!(tudo.iter().filter(|e| e["data"]["text"] == "Começando").count(), 1, "o turno não foi repetido");
    assert!(!tudo.iter().any(|e| e["kind"] == "notice" && e["data"]["style"] == "error"), "{tudo:?}");

    // O aviso diz que dá para parar: Parar encerra o turno e o aviso deixa de valer.
    host.call("stop", json!({"botId": bot})).await;
    esperar_ocioso(&host, &bot).await;
    let fim = esperar(&host, &bot, Duration::from_secs(10), |e| {
        e["id"] == aviso["id"] && e["data"]["motorMudo"]["estado"] == "voltou"
    })
    .await;
    assert_eq!(fim["id"], aviso["id"]);
}

#[tokio::test]
async fn motor_lento_que_da_sinal_nao_recebe_aviso() {
    if python_missing() {
        return;
    }
    let (host, bot) = bot_com("lento").await;
    host.call("send", json!({"botId": bot, "text": "oi"})).await;
    // Seis segundos no total, o dobro do limite, mas nunca mais de um segundo entre dois sinais.
    esperar(&host, &bot, Duration::from_secs(40), |e| {
        e["data"]["text"].as_str().is_some_and(|t| t.contains("passo 5"))
    })
    .await;
    esperar_ocioso(&host, &bot).await;
    let tudo = entradas(&host, &bot).await;
    assert!(avisos_de_motor_mudo(&tudo).is_empty(), "{tudo:?}");
}

#[tokio::test]
async fn motor_que_volta_depois_do_aviso_atualiza_o_mesmo_aviso() {
    if python_missing() {
        return;
    }
    let (host, bot) = bot_com("volta").await;
    host.call("send", json!({"botId": bot, "text": "oi"})).await;
    let aviso = esperar(&host, &bot, Duration::from_secs(30), |e| {
        e["kind"] == "notice" && e["data"]["motorMudo"]["estado"] == "mudo"
    })
    .await;
    esperar(&host, &bot, Duration::from_secs(30), |e| e["data"]["text"].as_str().is_some_and(|t| t.contains("voltei")))
        .await;
    let depois = esperar(&host, &bot, Duration::from_secs(10), |e| {
        e["id"] == aviso["id"] && e["data"]["motorMudo"]["estado"] == "voltou"
    })
    .await;
    assert_eq!(depois["id"], aviso["id"]);
    esperar_ocioso(&host, &bot).await;
    let tudo = entradas(&host, &bot).await;
    assert_eq!(avisos_de_motor_mudo(&tudo).len(), 1, "{tudo:?}");
}

#[tokio::test]
async fn ferramenta_em_execucao_nao_recebe_aviso() {
    if python_missing() {
        return;
    }
    let (host, bot) = bot_com("ferramenta").await;
    host.call("send", json!({"botId": bot, "text": "oi"})).await;
    esperar(&host, &bot, Duration::from_secs(40), |e| e["data"]["text"].as_str().is_some_and(|t| t.contains("feito")))
        .await;
    esperar_ocioso(&host, &bot).await;
    let tudo = entradas(&host, &bot).await;
    assert!(avisos_de_motor_mudo(&tudo).is_empty(), "{tudo:?}");
}

#[tokio::test]
async fn cartao_de_permissao_pendente_nao_recebe_aviso() {
    if python_missing() {
        return;
    }
    let (host, bot) = bot_com("cartao").await;
    host.call("send", json!({"botId": bot, "text": "oi"})).await;
    esperar(&host, &bot, Duration::from_secs(30), |e| e["kind"] == "permission" && e["data"]["status"] == "pending")
        .await;
    // Mais de dois limites inteiros esperando a pessoa decidir.
    tokio::time::sleep(Duration::from_secs(7)).await;
    let tudo = entradas(&host, &bot).await;
    assert!(avisos_de_motor_mudo(&tudo).is_empty(), "{tudo:?}");
    host.call("stop", json!({"botId": bot})).await;
    esperar_ocioso(&host, &bot).await;
}

#[tokio::test]
async fn aviso_ativo_nao_sobra_depois_de_reiniciar_o_hub() {
    if python_missing() {
        return;
    }
    let (mut host, bot) = bot_com("mudo").await;
    host.call("send", json!({"botId": bot, "text": "oi"})).await;
    let aviso = esperar(&host, &bot, Duration::from_secs(30), |e| {
        e["kind"] == "notice" && e["data"]["motorMudo"]["estado"] == "mudo"
    })
    .await;
    host.restart(|_| {}).await;
    let depois = esperar(&host, &bot, Duration::from_secs(30), |e| {
        e["id"] == aviso["id"] && e["data"]["motorMudo"]["estado"] == "voltou"
    })
    .await;
    assert!(!depois["data"]["text"].as_str().unwrap().contains("sem dar sinal"), "{depois}");
}
