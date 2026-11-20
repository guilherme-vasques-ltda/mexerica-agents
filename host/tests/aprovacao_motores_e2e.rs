//! Kilo e Cline passam pelo porteiro: o pedido de permissão do motor chega ao Hub e vira cartão, em vez de o motor
//! executar sozinho. Motor de mentira (`fixtures/engine_approval_agent.py`) que se comporta como cada um deles; sem
//! conta, rede ou modelo reais.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::host::{Host, typed_origin};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Falsos {
    raiz: PathBuf,
}

impl Falsos {
    fn nova() -> Self {
        assert!(std::hint::black_box(cfg!(debug_assertions)), "exige build com debug_assertions");
        let raiz = std::env::temp_dir().join(format!("mexerica-aprovacao-{}", uuid::Uuid::new_v4()));
        for sub in ["bin", "home", "config", "data", "cache", "state", "tmp", "npm-cache"] {
            std::fs::create_dir_all(raiz.join(sub)).unwrap();
        }
        let falsos = Self { raiz };
        let quoted = |p: &Path| p.display().to_string().replace('\'', "'\\''");
        falsos.escrever("shell-falso", "#!/bin/sh\nexec /bin/sh -c \"$2\"\n".to_owned());
        let cache = quoted(&falsos.raiz.join("npm-cache"));
        falsos.escrever(
            "bin/npm",
            format!("#!/bin/sh\n[ \"$1 $2 $3\" = 'config get cache' ] && printf '%s\\n' '{cache}'\nexit 0\n"),
        );
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/engine_approval_agent.py");
        for motor in ["kilo", "cline"] {
            let python = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|d| d.join("python3"))
                .find(|p| p.is_file())
                .expect("python3 no PATH do teste");
            falsos.escrever(
                &format!("bin/{motor}"),
                format!(
                    "#!/bin/sh\n[ \"$1\" = '--version' ] && echo '{motor} 1.0.0' && exit 0\nexec '{}' -X utf8 -u '{}' {motor} \"$@\"\n",
                    quoted(&python),
                    quoted(&fixture)
                ),
            );
        }
        falsos
    }

    fn escrever(&self, relativo: &str, conteudo: String) {
        let caminho = self.raiz.join(relativo);
        std::fs::write(&caminho, conteudo).unwrap();
        std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn binario(&self, motor: &str) -> PathBuf {
        self.raiz.join("bin").join(motor)
    }

    async fn hub(&self) -> Host {
        let em = |sub: &str| self.raiz.join(sub).into_os_string();
        let env: Vec<(&str, OsString)> = vec![
            ("HOME", em("home")),
            ("PATH", em("bin")),
            ("SHELL", em("shell-falso")),
            ("XDG_CONFIG_HOME", em("config")),
            ("XDG_DATA_HOME", em("data")),
            ("XDG_CACHE_HOME", em("cache")),
            ("XDG_STATE_HOME", em("state")),
            ("TMPDIR", em("tmp")),
            ("MEXERICA_TESTE_ISOLADO", "1".into()),
            // Se o Hub herdasse isto do ambiente, o teste do Kilo passaria sem o Hub impor nada.
            ("KILO_PERMISSION", "{}".into()),
        ];
        Host::start_isolado(&env).await
    }
}

impl Drop for Falsos {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.raiz);
    }
}

fn pasta(host: &Host, bot: &str) -> PathBuf {
    host.home.join("bots").join(bot).join("workspace")
}

fn executou(host: &Host, bot: &str, ferramenta: &str) -> bool {
    pasta(host, bot).join(format!("executou-{ferramenta}")).exists()
}

async fn criar_bot(host: &Host, motor: &str, extra: Value) -> String {
    let mut bot = json!({"name": motor, "backend": motor, "connectors": [], "notify": false, "permission": "ask"});
    bot.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    host.call("createBot", bot).await["bot"]["id"].as_str().unwrap().to_owned()
}

async fn enviar(host: &Host, bot: &str, texto: &str) {
    let body = json!({"botId": bot, "text": texto, "clientNonce": uuid::Uuid::new_v4().to_string(),
        "contentOrigin": typed_origin(texto)});
    host.call("send", body).await;
}

/// O cartão de permissão pendente, ou `None` se o turno terminou antes de aparecer um.
async fn cartao_pendente(host: &Host, bot: &str) -> Option<Value> {
    let fim = Instant::now() + Duration::from_secs(20);
    while Instant::now() < fim {
        let historico = host.call("history", json!({"botId": bot})).await;
        let entradas = historico["entries"].as_array().unwrap();
        if let Some(cartao) = entradas.iter().find(|e| e["kind"] == "permission" && e["data"]["status"] == "pending") {
            return Some(cartao.clone());
        }
        if entradas.iter().any(|e| e["kind"] == "agent" && e["data"]["final"] == true) {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("nem cartão nem resposta final em 20 s");
}

async fn responder(host: &Host, cartao: &Value, opcao: &str) {
    let body = json!({"botId": cartao["botId"], "entryRev": cartao["rev"], "entryId": cartao["id"], "optionId": opcao});
    host.call("respondPermission", body).await;
}

fn decisoes(host: &Host, bot: &str) -> Vec<Value> {
    let texto = std::fs::read_to_string(pasta(host, bot).join("decisions.jsonl")).unwrap_or_default();
    texto.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// Quem pede cartão em cada motor, na ordem: o Kilo lê sem pedir; o Cline pergunta por tudo.
const KILO: [&str; 3] = ["edit", "bash", "manage_update_bot"];
const CLINE: [&str; 4] = ["read", "edit", "bash", "manage_update_bot"];

/// Roda um turno: toda ferramenta precisa virar cartão antes de executar. Aprova tudo menos o comando, que recusa.
/// Devolve as opções do primeiro cartão.
async fn turno_com_cartoes(host: &Host, bot: &str, com_cartao: &[&str], aprovar: &str, recusar: &str) -> Vec<String> {
    enviar(host, bot, "faça as alterações").await;
    let mut opcoes = Vec::new();
    for ferramenta in com_cartao {
        let cartao = cartao_pendente(host, bot)
            .await
            .unwrap_or_else(|| panic!("{ferramenta}: o motor executou sem pedir cartão: {:?}", decisoes(host, bot)));
        assert!(!executou(host, bot, ferramenta), "{ferramenta}: executou antes de o dono decidir");
        if opcoes.is_empty() {
            opcoes = cartao["data"]["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o["optionId"].as_str().unwrap().to_owned())
                .collect();
        }
        responder(host, &cartao, if *ferramenta == "bash" { recusar } else { aprovar }).await;
    }
    assert!(cartao_pendente(host, bot).await.is_none(), "sobrou cartão além dos esperados");
    opcoes
}

fn confere_efeitos(host: &Host, bot: &str, com_cartao: &[&str]) {
    assert!(executou(host, bot, "read"), "a leitura executa (sem cartão no Kilo, aprovada no Cline)");
    assert!(executou(host, bot, "edit"), "edição aprovada deve executar");
    assert!(!executou(host, bot, "bash"), "comando recusado não pode executar");
    assert!(executou(host, bot, "manage_update_bot"), "ferramenta do Hub aprovada deve executar");
    let todas = decisoes(host, bot);
    let pediram: Vec<_> =
        todas.iter().filter(|d| d["decision"] == "ask").map(|d| d["tool"].as_str().unwrap()).collect();
    assert_eq!(pediram, com_cartao, "{todas:?}");
}

#[tokio::test]
async fn kilo_pede_cartao_para_editar_comandar_e_usar_ferramenta_do_hub() {
    let falsos = Falsos::nova();
    let host = falsos.hub().await;
    let bot = criar_bot(&host, "kilo", json!({})).await;
    let opcoes = turno_com_cartoes(&host, &bot, &KILO, "once", "reject").await;
    assert_eq!(opcoes, ["once", "reject"], "«sempre permitir» do Kilo grava regra permanente fora do porteiro");
    confere_efeitos(&host, &bot, &KILO);
}

#[tokio::test]
async fn cline_pede_cartao_por_padrao() {
    let falsos = Falsos::nova();
    let host = falsos.hub().await;
    let bot = criar_bot(&host, "cline", json!({})).await;
    turno_com_cartoes(&host, &bot, &CLINE, "allow_once", "reject_once").await;
    confere_efeitos(&host, &bot, &CLINE);
}

#[tokio::test]
async fn cline_nao_liga_aprovacao_automatica_pelas_opcoes_do_bot() {
    let falsos = Falsos::nova();
    let host = falsos.hub().await;
    let extra = json!({"model": "m1", "modelOptions": {"auto_approve": "true"}});
    let bot = criar_bot(&host, "cline", extra).await;
    turno_com_cartoes(&host, &bot, &CLINE, "allow_once", "reject_once").await;
    confere_efeitos(&host, &bot, &CLINE);
    let pedidos = std::fs::read_to_string(pasta(&host, &bot).join("requests.jsonl")).unwrap();
    assert!(!pedidos.contains("\"configId\": \"auto_approve\", \"value\": \"true\""), "o Hub repassou auto_approve");
}

#[tokio::test]
async fn cline_lancado_com_aprovacao_automatica_volta_a_perguntar() {
    let falsos = Falsos::nova();
    let host = falsos.hub().await;
    let comando = format!("{} --acp --auto-approve true", falsos.binario("cline").display());
    let bot = criar_bot(&host, "cline", json!({"command": comando})).await;
    turno_com_cartoes(&host, &bot, &CLINE, "allow_once", "reject_once").await;
    confere_efeitos(&host, &bot, &CLINE);
    let pedidos = std::fs::read_to_string(pasta(&host, &bot).join("requests.jsonl")).unwrap();
    assert!(
        pedidos.contains("\"configId\": \"auto_approve\", \"value\": false"),
        "o Hub não religou o pedido: {pedidos}"
    );
}

#[tokio::test]
async fn cline_que_nao_desliga_a_aprovacao_automatica_nao_executa_nada() {
    let falsos = Falsos::nova();
    let host = falsos.hub().await;
    let comando = format!("{} --acp --auto-approve true --stubborn", falsos.binario("cline").display());
    let bot = criar_bot(&host, "cline", json!({"command": comando})).await;
    enviar(&host, &bot, "faça as alterações").await;
    let fim = Instant::now() + Duration::from_secs(15);
    while Instant::now() < fim && !pasta(&host, &bot).join("requests.jsonl").exists() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let pedidos = std::fs::read_to_string(pasta(&host, &bot).join("requests.jsonl")).unwrap_or_default();
    assert!(pedidos.contains("auto_approve"), "o Hub devia tentar desligar: {pedidos}");
    assert!(!pedidos.contains("session/prompt"), "o turno não pode começar com a aprovação automática ligada");
    for ferramenta in ["read", "edit", "bash", "manage_update_bot"] {
        assert!(!executou(&host, &bot, ferramenta), "{ferramenta} executou");
    }
}
