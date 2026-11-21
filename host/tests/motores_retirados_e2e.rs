//! Motores que a Mexerica deixou de oferecer: o Hub recusa criar e editar para eles, não os lista
//! e nunca abre o motor de um bot antigo que ainda os tem gravados.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::host::{Host, typed_origin};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A lista fechada do enunciado, como o Hub a vê.
const RETIRADOS: [&str; 10] = ["kilo", "cline", "goose", "amp", "minimax", "pi", "qwen", "omp", "junie", "vibe"];

const MSG_CRIAR: &str = "Este motor não está disponível nesta versão da Mexerica. Escolha outro motor.";

/// Pasta com o PATH isolado do Hub e CLIs falsos: qualquer motor aberto deixa marca.
struct Falsos {
    raiz: PathBuf,
    pasta: PathBuf,
}

impl Falsos {
    fn nova() -> Self {
        let raiz = std::env::temp_dir().join(format!("mexerica-retirados-{}", uuid::Uuid::new_v4()));
        for sub in ["bin", "home", "config", "data", "cache", "state", "tmp"] {
            std::fs::create_dir_all(raiz.join(sub)).unwrap();
        }
        Self { pasta: raiz.join("bin"), raiz }
    }

    /// Um CLI falso que grava `marca` assim que é executado e espera o handshake; só usa comandos internos do shell,
    /// porque o PATH do Hub de teste contém apenas esta pasta.
    fn instalar(&self, binario: &str, marca: &Path) {
        let script = format!("#!/bin/sh\n: > '{}'\nIFS= read -r _\n", marca.display());
        let caminho = self.pasta.join(binario);
        std::fs::write(&caminho, script).unwrap();
        std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Ambiente limpo do Hub; `chave` devolve a oferta antiga (só para criar o bot antigo do cenário).
    fn env(&self, chave: bool) -> Vec<(&'static str, OsString)> {
        let em = |sub: &str| self.raiz.join(sub).into_os_string();
        let mut env = vec![
            ("HOME", em("home")),
            ("PATH", self.pasta.clone().into_os_string()),
            ("XDG_CONFIG_HOME", em("config")),
            ("XDG_DATA_HOME", em("data")),
            ("XDG_CACHE_HOME", em("cache")),
            ("XDG_STATE_HOME", em("state")),
            ("TMPDIR", em("tmp")),
            ("MEXERICA_TESTE_ISOLADO", "1".into()),
        ];
        if chave {
            env.push(("MEXERICA_DEV_MOTORES_RETIRADOS", "1".into()));
        }
        env
    }
}

impl Drop for Falsos {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.raiz);
    }
}

async fn criar_bot(host: &Host, nome: &str, backend: &str) -> Value {
    host.call("createBot", json!({"name": nome, "backend": backend, "connectors": [], "notify": false})).await
}

fn backends(resposta: &Value) -> &Vec<Value> {
    resposta["backends"].as_array().unwrap()
}

#[tokio::test]
async fn criar_bot_com_motor_retirado_e_recusado_com_mensagem_clara() {
    let falsos = Falsos::nova();
    let host = Host::start_isolado(&falsos.env(false)).await;
    for id in RETIRADOS {
        let erro =
            host.refused("createBot", json!({"name": id, "backend": id, "connectors": [], "notify": false})).await;
        assert_eq!(erro, MSG_CRIAR, "{id}");
    }
}

#[tokio::test]
async fn update_bot_para_motor_retirado_e_recusado() {
    let falsos = Falsos::nova();
    let host = Host::start_isolado(&falsos.env(false)).await;
    let bot = host.bot("Mantido", "ask").await;
    for id in RETIRADOS {
        let erro = host.refused("updateBot", json!({"id": bot, "backend": id})).await;
        assert_eq!(erro, MSG_CRIAR, "{id}");
    }
}

#[tokio::test]
async fn nome_do_catalogo_ou_em_outra_caixa_tambem_e_recusado() {
    let falsos = Falsos::nova();
    let host = Host::start_isolado(&falsos.env(false)).await;
    let bot = host.bot("Mantido", "ask").await;
    for id in ["qwen-code", "pi-acp", "amp-acp", "minimax-code", "mistral-vibe", "KILO", "Qwen-Code", "GOOSE"] {
        let erro =
            host.refused("createBot", json!({"name": id, "backend": id, "connectors": [], "notify": false})).await;
        assert_eq!(erro, MSG_CRIAR, "createBot {id}");
        let erro = host.refused("updateBot", json!({"id": bot, "backend": id})).await;
        assert_eq!(erro, MSG_CRIAR, "updateBot {id}");
    }
}

/// Controle positivo da prova de não execução: com a chave ligada, o mesmo CLI falso, no mesmo ambiente isolado, é
/// aberto e deixa a marca. Sem isto, a ausência da marca nos outros testes não provaria nada.
#[tokio::test]
async fn controle_positivo_o_cli_falso_deixa_marca_quando_o_motor_e_aberto() {
    let falsos = Falsos::nova();
    let marca = falsos.raiz.join("abriu-kilo");
    falsos.instalar("kilo", &marca);
    let host = Host::start_isolado(&falsos.env(true)).await;
    let bot = criar_bot(&host, "Antigo", "kilo").await["bot"]["id"].as_str().unwrap().to_owned();
    let texto = "Responda ok";
    host.call(
        "send",
        json!({"botId": bot, "text": texto, "clientNonce": uuid::Uuid::new_v4().to_string(),
            "contentOrigin": typed_origin(texto)}),
    )
    .await;
    let prazo = Instant::now() + Duration::from_secs(60);
    while Instant::now() < prazo && !marca.exists() {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(marca.exists(), "o CLI falso não deixou marca nem com a chave ligada: a prova de não execução não vale");
}

#[tokio::test]
async fn motores_mantidos_continuam_criaveis() {
    let falsos = Falsos::nova();
    let host = Host::start_isolado(&falsos.env(false)).await;
    for id in ["claude", "codex", "opencode"] {
        let feito = criar_bot(&host, id, id).await;
        assert!(feito["bot"]["id"].as_str().is_some(), "{id}: {feito}");
    }
}

#[tokio::test]
async fn hello_e_refresh_nao_listam_motores_retirados() {
    let falsos = Falsos::nova();
    let host = Host::start_isolado(&falsos.env(false)).await;
    for lista in [host.call("hello", json!({})).await, host.call("refreshBackends", json!({})).await] {
        let ids: Vec<&str> = backends(&lista).iter().filter_map(|b| b["id"].as_str()).collect();
        for id in RETIRADOS {
            assert!(!ids.contains(&id), "{id} não pode ser oferecido: {ids:?}");
        }
        for id in ["claude", "codex", "opencode"] {
            assert!(ids.contains(&id), "{id} tinha de estar na oferta: {ids:?}");
        }
    }
}

/// Um bot criado antes da retirada (chave ligada), com o motor instalado de mentira: depois do
/// reinício sem a chave, ele aparece indisponível e uma mensagem nunca abre o motor.
#[tokio::test]
async fn bot_antigo_com_motor_retirado_aparece_indisponivel_e_nunca_abre_o_motor() {
    let falsos = Falsos::nova();
    let marca = falsos.raiz.join("abriu-kilo");
    falsos.instalar("kilo", &marca);
    let mut host = Host::start_isolado(&falsos.env(true)).await;
    let bot = criar_bot(&host, "Antigo", "kilo").await["bot"]["id"].as_str().unwrap().to_owned();
    assert!(!marca.exists(), "o falso só pode rodar se o Hub abrir o motor");

    host.restart_isolado(&falsos.env(false)).await;
    let lista = host.call("refreshBackends", json!({})).await;
    let ids: Vec<&str> = backends(&lista).iter().filter_map(|b| b["id"].as_str()).collect();
    assert!(!ids.contains(&"kilo"), "o motor retirado não pode voltar na oferta: {ids:?}");

    // Editar outro campo do bot antigo segue permitido; só trocar para motor retirado é recusado.
    host.call("updateBot", json!({"id": bot, "name": "Nome novo"})).await;
    let erro = host.refused("updateBot", json!({"id": bot, "backend": "goose"})).await;
    assert_eq!(erro, MSG_CRIAR);

    let texto = "Responda ok";
    host.call(
        "send",
        json!({"botId": bot, "text": texto, "clientNonce": uuid::Uuid::new_v4().to_string(),
            "contentOrigin": typed_origin(texto)}),
    )
    .await;
    let prazo = Instant::now() + Duration::from_secs(60);
    let mut apareceu = false;
    while Instant::now() < prazo {
        assert!(!marca.exists(), "o motor retirado foi aberto pelo bot antigo");
        let historico = host.call("history", json!({"botId": bot})).await;
        let motivo = historico["entries"].as_array().unwrap().iter().any(|e| {
            let motivo = &e["data"]["reason"];
            (motivo["code"] == "engine_not_ready" && motivo["backend"] == "kilo")
                || e["data"]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("O motor Kilo não está disponível nesta versão da Mexerica"))
        });
        if motivo {
            apareceu = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(apareceu, "o bot antigo não apareceu indisponível: {}", host.call("history", json!({"botId": bot})).await);
    assert!(!marca.exists(), "nenhum turno abriu o motor retirado");
}
