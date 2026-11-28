//! Provas da ponte com o programa falso `tests/fixtures/claude_oficial_falso.py`, que imita o Claude Code
//! oficial no modo sem tela. Cada teste cobre um item do que a conversa já fazia pelo adaptador anterior.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::agente::{Config, servir};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

struct Ambiente {
    _dir: tempfile::TempDir,
    perfil: PathBuf,
    exe: PathBuf,
    cwd: PathBuf,
}

fn python3() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("python3"))
        .find(|p| p.is_file())
        .expect("python3 no PATH do teste")
}

fn ambiente(logado: bool) -> Ambiente {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let perfil = dir.path().join("perfil");
    let cwd = dir.path().join("trabalho");
    std::fs::create_dir_all(&perfil).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    if logado {
        std::fs::write(perfil.join("logado"), "").unwrap();
        std::fs::write(perfil.join(".credentials.json"), "{\"segredo\":\"nao-deve-sair\"}").unwrap();
    }
    let falso = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_oficial_falso.py");
    let exe = dir.path().join("claude");
    let aspas = |p: &Path| p.display().to_string().replace('\'', "'\\''");
    std::fs::write(
        &exe,
        format!(
            "#!/bin/sh\nCLAUDE_CONFIG_DIR='{}' exec '{}' -u '{}' \"$@\"\n",
            aspas(&perfil),
            aspas(&python3()),
            aspas(&falso)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    Ambiente { _dir: dir, perfil, exe, cwd }
}

impl Ambiente {
    fn config(&self) -> Config {
        Config { exe: Some(self.exe.clone()), pasta_config: Some(self.perfil.clone()) }
    }

    fn lancamentos(&self) -> Vec<Value> {
        std::fs::read_to_string(self.perfil.join("lancamentos.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

/// O lado do Hub: pede, coleta notificações e responde pedidos de permissão conforme `aprovar`.
struct Cliente {
    escrita: DuplexStream,
    leitura: Lines<BufReader<DuplexStream>>,
    proximo: i64,
    notas: Vec<Value>,
    permissoes: Vec<Value>,
    aprovar: bool,
    /// Ao ver uma notificação com este texto, envia esta mensagem (cancelamento no meio do turno).
    gatilho: Option<(String, Value)>,
    _ponte: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Cliente {
    fn novo(cfg: Config) -> Self {
        let (escrita, ponte_entrada) = tokio::io::duplex(1 << 20);
        let (ponte_saida, leitura) = tokio::io::duplex(1 << 20);
        let ponte = tokio::spawn(servir(ponte_entrada, ponte_saida, cfg));
        Self {
            escrita,
            leitura: BufReader::new(leitura).lines(),
            proximo: 1,
            notas: Vec::new(),
            permissoes: Vec::new(),
            aprovar: true,
            gatilho: None,
            _ponte: ponte,
        }
    }

    async fn escrever(&mut self, msg: &Value) {
        self.escrita.write_all(format!("{msg}\n").as_bytes()).await.unwrap();
    }

    async fn pedir(&mut self, metodo: &str, params: Value) -> Value {
        let id = self.proximo;
        self.proximo += 1;
        self.escrever(&json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": params})).await;
        loop {
            let linha = tokio::time::timeout(std::time::Duration::from_secs(30), self.leitura.next_line())
                .await
                .expect("a ponte respondeu a tempo")
                .unwrap()
                .expect("a ponte segue aberta");
            let msg: Value = serde_json::from_str(&linha).unwrap();
            match (msg["method"].as_str(), msg.get("id")) {
                (None, Some(i)) if *i == id => return msg,
                (Some("session/request_permission"), Some(i)) => {
                    let ordem = self.notas.len();
                    self.permissoes.push(json!({"ordem": ordem, "params": msg["params"]}));
                    let escolha = if self.aprovar { "allow-once" } else { "reject" };
                    let i = i.clone();
                    self.escrever(&json!({"jsonrpc": "2.0", "id": i,
                        "result": {"outcome": {"outcome": "selected", "optionId": escolha}}}))
                        .await;
                }
                (Some(_), None) => {
                    let texto = msg.to_string();
                    self.notas.push(msg["params"]["update"].clone());
                    if let Some((trecho, envio)) = self.gatilho.take() {
                        if texto.contains(&trecho) {
                            self.escrever(&envio).await;
                        } else {
                            self.gatilho = Some((trecho, envio));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    async fn nova(&mut self, cwd: &Path, opcoes: Value) -> String {
        let r = self
            .pedir(
                "session/new",
                json!({"cwd": cwd, "mcpServers": [{"name": "chat", "command": "/bin/true", "args": ["mcp"],
                    "env": [{"name": "DONNA_CHAT", "value": "1"}]}],
                    "_meta": {"systemPrompt": {"type": "preset", "preset": "claude_code", "append": "Você é o Berga."},
                        "claudeCode": {"options": opcoes}}}),
            )
            .await;
        r["result"]["sessionId"].as_str().unwrap_or_else(|| panic!("sessão criada: {r}")).to_owned()
    }

    async fn falar(&mut self, sessao: &str, texto: &str) -> Value {
        self.pedir("session/prompt", json!({"sessionId": sessao, "prompt": [{"type": "text", "text": texto}]})).await
    }

    fn texto(&self, tipo: &str) -> String {
        self.notas.iter().filter(|n| n["sessionUpdate"] == tipo).filter_map(|n| n["content"]["text"].as_str()).collect()
    }

    fn de(&self, tipo: &str) -> Vec<&Value> {
        self.notas.iter().filter(|n| n["sessionUpdate"] == tipo).collect()
    }
}

/// Opções que o Hub manda hoje para a conversa (`claude_meta`).
fn opcoes_da_conversa() -> Value {
    json!({"settings": {"permissions": {"defaultMode": "default", "allow": ["Read(//docs/**)"], "deny": ["Edit(//mem/**)"]},
            "includeGitInstructions": false, "attribution": {"commit": "", "pr": ""}, "autoMemoryEnabled": false, "autoDreamEnabled": false},
        "settingSources": [], "strictMcpConfig": true,
        "disallowedTools": crate::fork::claude_tools::disallowed(),
        "permissionMode": "default", "allowDangerouslySkipPermissions": false})
}

/// Valor de uma opção, nas formas `--opcao valor` e `--opcao=valor`.
fn arg<'a>(args: &'a [Value], nome: &str) -> Option<&'a str> {
    let prefixo = format!("{nome}=");
    args.iter().enumerate().find_map(|(i, a)| {
        let a = a.as_str()?;
        if a == nome { args.get(i + 1)?.as_str() } else { a.strip_prefix(prefixo.as_str()) }
    })
}

#[tokio::test]
async fn handshake_identifica_o_claude_e_a_ponte() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let init = c.pedir("initialize", json!({"protocolVersion": 1})).await["result"].clone();
    assert!(init["agentCapabilities"]["_meta"]["claudeCode"].is_object(), "o Hub trata como Claude: {init}");
    assert_eq!(init["agentCapabilities"]["loadSession"], true);
    assert!(super::e_a_ponte(&init));
    assert!(super::legado() || crate::fork::background_adapter::matches(&init), "o fundo aceita a ponte");
}

#[tokio::test]
async fn sem_login_pede_entrar_e_nao_abre_o_programa() {
    let amb = ambiente(false);
    let mut c = Cliente::novo(amb.config());
    let r = c.pedir("session/new", json!({"cwd": amb.cwd, "mcpServers": []})).await;
    assert_eq!(r["error"]["code"], -32000, "{r}");
    assert!(amb.lancamentos().is_empty(), "nenhum turno sem login");
}

#[tokio::test]
async fn conversa_usa_o_programa_oficial_sem_sdk_com_as_mesmas_regras() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    let r = c.falar(&sessao, "oi").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    let lancamentos = amb.lancamentos();
    assert_eq!(lancamentos.len(), 1);
    let l = &lancamentos[0];
    assert_eq!(l["env_sdk"], json!({}), "nada de SDK de agentes no ambiente: {l}");
    let args = l["args"].as_array().unwrap();
    assert!(
        args.contains(&json!("-p"))
            && args.contains(&json!("--strict-mcp-config"))
            && args.contains(&json!("--verbose"))
    );
    assert!(args.contains(&json!("--include-partial-messages")));
    assert_eq!(arg(args, "--input-format"), Some("stream-json"));
    assert_eq!(arg(args, "--permission-mode"), Some("default"));
    assert_eq!(arg(args, "--permission-prompt-tool"), Some("mcp__mexerica_ponte__permissao"));
    assert_eq!(arg(args, "--session-id"), Some(sessao.as_str()));
    assert!(args.contains(&json!("--setting-sources=")), "nenhuma configuração da máquina: {args:?}");
    let negadas = args.iter().find_map(|a| a.as_str()?.strip_prefix("--disallowedTools=")).unwrap();
    for ferramenta in crate::fork::claude_tools::disallowed().into_iter().chain(["AskUserQuestion"]) {
        assert!(negadas.split(',').any(|n| n == ferramenta), "{ferramenta} continua negada");
    }
    assert!(!args.iter().any(|a| a.as_str().is_some_and(|a| a.contains("dangerously"))));
    assert_eq!(l["cwd"], amb.cwd.display().to_string());
    // Texto, pensamento e uso chegam no formato que o Hub consome, sem duplicar o texto consolidado.
    assert_eq!(c.texto("agent_message_chunk"), "olá de claude-falso-padrao, turno 1");
    assert_eq!(c.texto("agent_thought_chunk"), "pensando");
    let usage = &r["result"]["usage"];
    assert_eq!(
        usage,
        &json!({"inputTokens": 10, "outputTokens": 5, "cachedReadTokens": 100, "cachedWriteTokens": 20, "totalTokens": 135})
    );
    assert_eq!(r["result"]["_meta"]["quota"]["model_usage"][0]["model"], "claude-falso-padrao");
    assert_eq!(r["result"]["_meta"]["quota"]["token_count"]["cachedInputTokens"], 100);
}

#[tokio::test]
async fn custo_acumulado_e_ocupacao_de_contexto_como_antes() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    c.falar(&sessao, "oi").await;
    c.falar(&sessao, "de novo").await;
    let custos: Vec<f64> = c.de("usage_update").iter().filter_map(|u| u["cost"]["amount"].as_f64()).collect();
    assert_eq!(custos, vec![0.01, 0.02], "custo cumulativo da sessão, uma vez por turno");
    assert!(c.de("usage_update").iter().filter(|u| u["cost"].is_object()).all(|u| u["cost"]["currency"] == "USD"));
    let ultimo = c.de("usage_update").into_iter().rev().find(|u| u["used"].is_u64()).unwrap().clone();
    assert_eq!(ultimo["size"], 500_000, "janela do modelo informada pelo programa");
    assert!(c.de("usage_update").iter().any(|u| u["_meta"]["_claude/rateLimit"]["status"] == "allowed"));
    assert_eq!(amb.lancamentos().len(), 1, "um processo para a sessão inteira");
}

#[tokio::test]
async fn ferramenta_que_exige_aprovacao_e_negada_sem_cartao() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    // O cliente de prova aprovaria tudo: mesmo assim, nenhum pedido de permissão pode chegar a ele.
    c.aprovar = true;
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    let r = c.falar(&sessao, "use a [[ferramenta]]").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    assert!(c.permissoes.is_empty(), "nenhum session/request_permission chega ao cliente: {:?}", c.permissoes);
    let decisao: Value =
        serde_json::from_str(&std::fs::read_to_string(amb.perfil.join("ferramenta.json")).unwrap()).unwrap();
    assert_eq!(decisao["behavior"], "deny", "a negação chega ao programa falso: {decisao}");
    assert_eq!(decisao["message"], super::permissao::NEGACAO, "a mensagem é a negação padrão: {decisao}");
    // A chamada foi anunciada no histórico e fechou como falha, sem rodar.
    assert_eq!(c.texto("agent_message_chunk"), "não rodei");
    assert_eq!(c.de("tool_call").len(), 1, "a chamada segue anunciada no histórico");
    let fim = c.de("tool_call_update").into_iter().rev().find(|u| u["status"].is_string()).unwrap();
    assert_eq!(fim["status"], "failed", "a ferramenta não rodou: {fim}");
    // A configuração MCP (segredo da ponte e variáveis dos servidores do Hub) sai do disco depois da leitura.
    let mcp = amb.lancamentos()[0]["mcp_config"].as_str().unwrap().to_owned();
    assert!(!Path::new(&mcp).exists(), "mcp.json apagado depois do início");
}

#[tokio::test]
async fn retomada_usa_a_mesma_sessao_do_programa() {
    let amb = ambiente(true);
    let sessao = {
        let mut c = Cliente::novo(amb.config());
        let s = c.nova(&amb.cwd, opcoes_da_conversa()).await;
        c.falar(&s, "primeiro").await;
        s
    };
    let mut c = Cliente::novo(amb.config());
    let r = c
        .pedir(
            "session/load",
            json!({"sessionId": sessao, "cwd": amb.cwd, "mcpServers": [],
            "_meta": {"claudeCode": {"options": opcoes_da_conversa()}}}),
        )
        .await;
    assert!(r["result"]["configOptions"].is_array(), "{r}");
    c.falar(&sessao, "segundo").await;
    assert_eq!(c.texto("agent_message_chunk"), "olá de claude-falso-padrao, turno 2", "a conversa continuou");
    let l = amb.lancamentos();
    assert_eq!(arg(l[1]["args"].as_array().unwrap(), "--resume"), Some(sessao.as_str()));
    let inexistente = uuid::Uuid::new_v4().to_string();
    let r = c.pedir("session/load", json!({"sessionId": inexistente, "cwd": amb.cwd, "mcpServers": []})).await;
    assert!(r["error"].is_object(), "sessão sem transcrição não é retomada: {r}");
}

#[tokio::test]
async fn troca_de_modelo_e_esforco_reabre_com_resume() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    c.falar(&sessao, "oi").await;
    let r = c
        .pedir("session/set_config_option", json!({"sessionId": sessao, "configId": "model", "value": "sonnet"}))
        .await;
    let opcoes = r["result"]["configOptions"].as_array().unwrap();
    assert!(opcoes.iter().any(|o| o["category"] == "model" && o["currentValue"] == "sonnet"));
    assert!(opcoes.iter().any(|o| o["category"] == "mode" && o["currentValue"] == "default"));
    c.pedir("session/set_config_option", json!({"sessionId": sessao, "configId": "effort", "value": "high"})).await;
    let r = c
        .pedir(
            "session/set_config_option",
            json!({"sessionId": sessao, "configId": "mode", "value": "bypassPermissions"}),
        )
        .await;
    assert!(r["error"].is_object(), "nenhum modo além do padrão");
    c.falar(&sessao, "e agora").await;
    let l = amb.lancamentos();
    assert_eq!(l.len(), 2);
    let args = l[1]["args"].as_array().unwrap();
    assert_eq!(arg(args, "--model"), Some("sonnet"));
    assert_eq!(arg(args, "--effort"), Some("high"));
    assert_eq!(arg(args, "--resume"), Some(sessao.as_str()));
    assert!(c.texto("agent_message_chunk").ends_with("olá de claude-falso-sonnet, turno 2"));
}

#[tokio::test]
async fn cancelar_encerra_o_turno_pelo_sinal_documentado() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    c.gatilho = Some((
        "começando".into(),
        json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sessao}}),
    ));
    let r = c.falar(&sessao, "[[dormir]]").await;
    assert_eq!(r["result"]["stopReason"], "cancelled", "{r}");
    assert!(amb.perfil.join("sigint").is_file(), "o programa recebeu o SIGINT antes de sair");
    let r = c.falar(&sessao, "oi").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "a sessão segue depois do cancelamento: {r}");
    assert_eq!(arg(amb.lancamentos()[1]["args"].as_array().unwrap(), "--resume"), Some(sessao.as_str()));
}

#[tokio::test]
async fn compactacao_plano_e_limite_seguem_o_formato_do_hub() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    c.falar(&sessao, "/compact mantenha as regras").await;
    let compactacoes: Vec<_> = c.de("compaction_update").iter().map(|u| u["status"].clone()).collect();
    assert_eq!(compactacoes, vec![json!("in_progress"), json!("completed")]);
    assert!(c.de("usage_update").iter().any(|u| u["used"] == 1200));
    c.falar(&sessao, "faça um [[plano]]").await;
    let plano = c.de("plan");
    assert_eq!(
        plano.last().unwrap()["entries"][0],
        json!({"content": "Lendo", "status": "in_progress", "priority": "medium"})
    );
    assert!(c.de("tool_call").iter().all(|t| t["_meta"]["claudeCode"]["toolName"] != "TodoWrite"));
    let r = c.falar(&sessao, "[[limite]]").await;
    let erro = &r["error"];
    let msg = erro["message"].as_str().unwrap();
    assert!(msg.contains("hit your limit"), "o texto do limite chega ao Hub: {r}");
    let kind = erro["data"]["errorKind"].as_str();
    assert_eq!(kind, Some("rate_limit"));
    let limite =
        crate::agent::subscription::detect("claude", kind, msg, &erro["data"], 0).expect("o Hub reconhece o limite");
    assert_eq!(limite.resets_at, Some(4_102_444_800_000), "e o horário de volta");
    let r = c.falar(&sessao, "oi").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "o limite encerra só o turno: {r}");
}

#[tokio::test]
async fn opcoes_do_fundo_e_do_catalogo_viram_flags_documentadas() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let r = c
        .pedir(
            "session/new",
            json!({"cwd": amb.cwd, "mcpServers": [], "_meta": {"systemPrompt": "Só resuma.", "claudeCode": {"options": {
                "tools": [], "disallowedTools": ["*"], "strictMcpConfig": true, "persistSession": false, "settingSources": [],
                "maxTurns": 1, "model": "haiku", "permissionMode": "default", "allowDangerouslySkipPermissions": false,
                "settings": {"autoMemoryEnabled": false, "autoDreamEnabled": false, "disableAllHooks": true}}}}}),
        )
        .await;
    let sessao = r["result"]["sessionId"].as_str().unwrap().to_owned();
    let catalogo = crate::agent::auth::model_catalog(&r["result"], &Value::Null);
    assert_eq!(catalogo["currentModelId"], "haiku");
    assert!(catalogo["models"].as_array().unwrap().iter().any(|m| m["id"] == "opus"));
    assert!(amb.lancamentos().is_empty(), "abrir sessão (catálogo) não abre o programa nem gasta");
    let r = c.falar(&sessao, "oi").await;
    assert_eq!(r["result"]["_meta"]["quota"]["model_usage"][0]["model"], "claude-falso-haiku");
    let args = amb.lancamentos()[0]["args"].clone();
    let args = args.as_array().unwrap();
    for esperado in ["--tools=", "--disallowedTools=*", "--no-session-persistence"] {
        assert!(args.contains(&json!(esperado)), "{esperado}: {args:?}");
    }
    assert_eq!(arg(args, "--max-turns"), Some("1"));
    assert_eq!(arg(args, "--model"), Some("haiku"));
    assert!(arg(args, "--system-prompt-file").is_some());
    for proibida in [
        json!({"permissionMode": "bypassPermissions"}),
        json!({"allowDangerouslySkipPermissions": true}),
        json!({"hooks": {}}),
        json!({"strictMcpConfig": false}),
    ] {
        let r = c
            .pedir(
                "session/new",
                json!({"cwd": amb.cwd, "mcpServers": [], "_meta": {"claudeCode": {"options": proibida}}}),
            )
            .await;
        assert!(r["error"].is_object(), "opção recusada, não ignorada: {proibida}");
    }
}

#[tokio::test]
async fn nenhuma_credencial_do_perfil_sai_pela_ponte() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    c.falar(&sessao, "use a [[ferramenta]]").await;
    let tudo = format!("{:?}{:?}", c.notas, c.permissoes);
    assert!(!tudo.contains("nao-deve-sair"));
    assert!(!amb.lancamentos().iter().any(|l| l.to_string().contains("nao-deve-sair")));
}

#[tokio::test]
async fn argumentos_longos_da_ferramenta_tambem_sao_negados_sem_cartao() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    let r = c.falar(&sessao, "[[mcp-grande]]").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    assert!(c.permissoes.is_empty(), "nenhum cartão, nem para argumentos longos: {:?}", c.permissoes);
    let decisao: Value =
        serde_json::from_str(&std::fs::read_to_string(amb.perfil.join("mcp-grande.json")).unwrap()).unwrap();
    assert_eq!(decisao["behavior"], "deny", "{decisao}");
    assert_eq!(decisao["message"], super::permissao::NEGACAO, "{decisao}");
    let fim = c.de("tool_call_update").into_iter().rev().find(|u| u["status"].is_string()).unwrap();
    assert_eq!(fim["status"], "failed", "a chamada fecha como falha sem rodar: {fim}");
    // A projeção sanitizada da chamada segue no histórico, dentro do limite do cartão.
    let projecao = c
        .de("tool_call_update")
        .into_iter()
        .find(|u| u["toolCallId"] == "toolu_g" && u["rawInput"]["resumo"].is_string())
        .expect("a projeção da chamada genérica no histórico");
    let texto = projecao["content"][0]["content"]["text"].as_str().unwrap();
    assert!(texto.chars().count() <= 1800, "cabe na tela: {}", texto.chars().count());
    assert!(texto.contains("argumentos longos demais para conferir"), "corte visível no fim do texto: {texto}");
}

#[test]
fn imagem_so_passa_com_tipo_e_bytes_conferidos_e_dentro_do_limite() {
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let png = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[0; 64]].concat();
    let jpeg = [&[0xFF, 0xD8, 0xFF, 0xE0][..], &[0; 64]].concat();
    let gif = [&b"GIF89a"[..], &[0; 64]].concat();
    let webp = [&b"RIFF"[..], &[0; 4], &b"WEBP"[..], &[0; 64]].concat();
    let imagem = |tipo: &str, dados: &str| json!([{"type": "image", "mimeType": tipo, "data": dados}]);
    let recurso = |tipo: &str, dados: &str| json!([{"type": "resource", "resource": {"uri": "file:///x", "mimeType": tipo, "blob": dados}}]);
    for (tipo, bytes) in [("image/png", &png), ("image/jpeg", &jpeg), ("image/gif", &gif), ("image/webp", &webp)] {
        assert!(super::agente::blocos(&imagem(tipo, &b64(bytes))).is_ok(), "{tipo} válida");
        assert!(super::agente::blocos(&recurso(tipo, &b64(bytes))).is_ok(), "{tipo} válida como recurso");
    }
    let grande = [&png[..], &vec![0; 6 * 1024 * 1024]].concat();
    for (tipo, dados) in [
        ("image/png", b64(b"isto nao e uma imagem")),
        ("image/jpeg", b64(&png)),
        ("image/png", "@@@@não é base64@@@@".to_owned()),
        ("image/svg+xml", b64(b"<svg xmlns='http://www.w3.org/2000/svg'/>")),
        ("image/png", b64(&grande)),
    ] {
        assert!(super::agente::blocos(&imagem(tipo, &dados)).is_err(), "bloco image recusado: {tipo}");
        assert!(super::agente::blocos(&recurso(tipo, &dados)).is_err(), "recurso recusado: {tipo}");
    }
    let varias: Vec<Value> =
        (0..5).map(|_| imagem("image/png", &b64(&[&png[..], &vec![0; 4 * 1024 * 1024]].concat()))[0].clone()).collect();
    assert!(super::agente::blocos(&Value::Array(varias)).is_err(), "o total de imagens do turno tem teto");
}

#[tokio::test]
async fn parar_no_meio_do_turno_cancela_e_pedido_depois_dele_continua_negado() {
    let amb = ambiente(true);
    let mut c = Cliente::novo(amb.config());
    let sessao = c.nova(&amb.cwd, opcoes_da_conversa()).await;
    c.gatilho = Some((
        "pedindo".into(),
        json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sessao}}),
    ));
    let r = c.falar(&sessao, "[[parar-e-pedir]]").await;
    assert_eq!(r["result"]["stopReason"], "cancelled", "{r}");
    let decisao: Value =
        serde_json::from_str(&std::fs::read_to_string(amb.perfil.join("apos-parar.json")).unwrap()).unwrap();
    assert_eq!(decisao["behavior"], "deny", "o pedido depois de Parar também é negado: {decisao}");
    assert_eq!(decisao["message"], super::permissao::NEGACAO, "{decisao}");
    assert!(c.permissoes.is_empty(), "nenhum cartão depois de Parar: {:?}", c.permissoes);
}

#[tokio::test]
async fn servidor_de_permissao_recusa_quem_nao_tem_o_segredo() {
    let servidor = super::permissao::iniciar().await.unwrap();
    assert!(servidor.url.starts_with("http://127.0.0.1:"));
    let cliente = reqwest::Client::new();
    let corpo =
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "permissao", "arguments": {}}});
    let sem = cliente.post(&servidor.url).json(&corpo).send().await.unwrap();
    assert_eq!(sem.status(), 401);
    let errado = cliente.post(&servidor.url).bearer_auth("x".repeat(64)).json(&corpo).send().await.unwrap();
    assert_eq!(errado.status(), 401);
    let get = cliente.get(&servidor.url).bearer_auth(&servidor.segredo).send().await.unwrap();
    assert_eq!(get.status(), 405);
    let pagina = cliente
        .post(&servidor.url)
        .bearer_auth(&servidor.segredo)
        .header("Origin", "https://exemplo.invalid")
        .json(&corpo)
        .send()
        .await
        .unwrap();
    assert_eq!(pagina.status(), 403);
    // Com o segredo certo, a ferramenta existe e a resposta é sempre a negação padrão.
    let lista = cliente
        .post(&servidor.url)
        .bearer_auth(&servidor.segredo)
        .json(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}))
        .send()
        .await
        .unwrap();
    let ferramentas: Value = lista.json().await.unwrap();
    assert_eq!(ferramentas["result"]["tools"][0]["name"], super::opcoes::FERRAMENTA_PERMISSAO);
    let certo = cliente.post(&servidor.url).bearer_auth(&servidor.segredo).json(&corpo).send().await.unwrap();
    assert_eq!(certo.status(), 200);
    let msg: Value = certo.json().await.unwrap();
    let decisao: Value = serde_json::from_str(msg["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(decisao["behavior"], "deny", "{decisao}");
    assert_eq!(decisao["message"], super::permissao::NEGACAO, "{decisao}");
}

#[test]
fn texto_nao_some_quando_o_pensamento_ja_o_continha_e_compactacao_interrompida_fecha() {
    let mut t = super::traducao::Traducao::new("/tmp");
    let parcial = |evento: Value| json!({"type": "stream_event", "parent_tool_use_id": null, "event": evento});
    t.evento(&parcial(json!({"type": "message_start", "message": {"id": "m1"}})));
    t.evento(&parcial(json!({"type": "content_block_delta", "index": 0,
        "delta": {"type": "thinking_delta", "thinking": "a resposta é 42"}})));
    let saida = t.evento(&json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": [
        {"type": "thinking", "thinking": "a resposta é 42"}, {"type": "text", "text": "42"}]}}));
    let textos: Vec<_> = saida.iter().filter(|u| u["sessionUpdate"] == "agent_message_chunk").collect();
    assert_eq!(textos.len(), 1, "{saida:?}");
    assert_eq!(textos[0]["content"]["text"], "42");
    assert!(!saida.iter().any(|u| u["sessionUpdate"] == "agent_thought_chunk"), "pensamento já enviado: {saida:?}");

    let inicio = t.evento(&json!({"type": "system", "subtype": "status", "status": "compacting"}));
    let id = inicio[0]["compactionId"].clone();
    let fim = t.fechar_abertas();
    t.encerrar_turno();
    assert!(
        fim.iter()
            .any(|u| u["sessionUpdate"] == "compaction_update" && u["compactionId"] == id && u["status"] == "failed"),
        "{fim:?}"
    );
    let outra = t.evento(&json!({"type": "system", "subtype": "status", "status": "compacting"}));
    assert_eq!(outra.len(), 1, "a próxima compactação é anunciada: {outra:?}");

    // Ferramenta do servidor: o resultado vem na própria mensagem do assistente.
    let web = t.evento(&json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m2", "content": [
        {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_fetch", "input": {"url": "https://exemplo.invalid"}},
        {"type": "web_fetch_tool_result", "tool_use_id": "srvtoolu_1", "content": {"type": "web_fetch_result", "url": "https://exemplo.invalid"}},
        {"type": "server_tool_use", "id": "srvtoolu_2", "name": "web_search", "input": {"query": "x"}},
        {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_2",
            "content": {"type": "web_search_tool_result_error", "error_code": "unavailable"}}]}}));
    let status = |id: &str| {
        web.iter().rev().find(|u| u["toolCallId"] == id && u["status"].is_string()).map(|u| u["status"].clone())
    };
    assert_eq!(status("srvtoolu_1"), Some(json!("completed")), "{web:?}");
    assert_eq!(status("srvtoolu_2"), Some(json!("failed")), "{web:?}");
    assert!(
        !t.fechar_abertas().iter().any(|u| u["sessionUpdate"] == "tool_call_update"),
        "nenhuma chamada fica aberta para virar falha no fim do turno"
    );
}

#[test]
fn opcoes_perigosas_ou_malformadas_sao_recusadas() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().display().to_string();
    let pedido = |opcoes: Value| json!({"cwd": cwd, "mcpServers": [], "_meta": {"claudeCode": {"options": opcoes}}});
    for ruim in [
        json!({"model": "--dangerously-skip-permissions"}),
        json!({"model": "-p"}),
        json!({"settings": {"permissions": {"defaultMode": "bypassPermissions"}}}),
        json!({"settings": {"hooks": {}}}),
        json!({"settings": {"apiKeyHelper": "/bin/true"}}),
        json!({"disallowedTools": ["Bash,Edit"]}),
        json!({"tools": ["--model=x"]}),
        json!({"settingSources": ["policy"]}),
        json!({"persistSession": "no"}),
    ] {
        assert!(super::opcoes::ler(&pedido(ruim.clone())).is_err(), "{ruim}");
    }
    let tipos = [
        json!({"cwd": cwd, "mcpServers": {}}),
        json!({"cwd": cwd, "mcpServers": [{"name": "chat", "command": "/bin/true", "env": {"A": "1"}}]}),
        json!({"cwd": cwd, "mcpServers": [], "systemPrompt": {"mode": "append", "content": "texto"}}),
        json!({"cwd": cwd, "mcpServers": [], "_meta": {"systemPrompt": {"append": 3}}}),
        json!({"cwd": cwd, "mcpServers": [], "env": {}}),
        json!({"cwd": cwd, "mcpServers": [], "_meta": {"claudeCode": {"options": []}}}),
        json!({"cwd": cwd, "mcpServers": [{"name": "web", "type": "http", "url": "http://127.0.0.1:1/mcp", "headers": [],
            "timeout": 5}]}),
        json!({"cwd": cwd, "mcpServers": [{"name": "chat", "command": "/bin/true", "args": [], "env": [], "cwd": "/"}]}),
    ];
    for ruim in tipos {
        assert!(super::opcoes::ler(&ruim).is_err(), "{ruim}");
    }
    assert!(super::opcoes::ler(&pedido(opcoes_da_conversa())).is_ok());
}

#[test]
fn pasta_adicional_e_reconferida_ao_montar_o_proximo_inicio() {
    let dir = tempfile::tempdir().unwrap();
    let extra = dir.path().join("extra");
    std::fs::create_dir(&extra).unwrap();
    let o =
        super::opcoes::ler(&json!({"cwd": dir.path(), "mcpServers": [], "additionalDirectories": [&extra]})).unwrap();
    let arg = format!("--add-dir={}", extra.canonicalize().unwrap().display());
    let arquivos = super::opcoes::Arquivos { mcp: dir.path().join("mcp.json"), settings: None, prompt: None };
    assert!(super::opcoes::argumentos(&o, "s", false, "default", "default", &arquivos).unwrap().contains(&arg));
    std::fs::rename(&extra, dir.path().join("anterior")).unwrap();
    assert!(super::opcoes::argumentos(&o, "s", true, "default", "default", &arquivos).is_err());
    #[cfg(unix)]
    {
        std::fs::create_dir(&extra).unwrap();
        assert!(super::opcoes::argumentos(&o, "s", true, "default", "default", &arquivos).is_err());
    }
}

/// O diagnóstico do programa só sai mascarado: o segredo da ponte e os valores das variáveis dos
/// servidores MCP da sessão. Valores curtos ficam de fora para não destruir a linha.
#[test]
fn diagnostico_mascara_o_segredo_da_ponte_e_as_variaveis_mcp() {
    // Sem literal no formato de chave: os valores de exemplo são montados em tempo de execução.
    let ponte = ["segredo-da-ponte-", "0123456789"].concat();
    let valor = ["valor-exemplo-", "0123456789"].concat();
    let codigo = ["a", "1b2c3"].concat();
    let servidores = json!({
        "chat": {"type": "stdio", "command": "/bin/true", "env": {"TOKEN_EXEMPLO": valor, "CODIGO": codigo, "NIVEL": "1"}},
        "web": {"type": "http", "url": "http://127.0.0.1:1/mcp"},
    });
    let mapa = servidores.as_object().unwrap();
    let segredos = super::agente::mascaraveis(&ponte, mapa);
    assert!(
        segredos.contains(&valor) && segredos.contains(&codigo),
        "valores a partir de 6 caracteres entram na máscara"
    );
    assert!(!segredos.contains(&"1".into()), "valor de até 5 caracteres não entra");
    let linha = format!("falha ao ligar com {ponte} e TOKEN_EXEMPLO={valor} codigo {codigo} nivel 1");
    let texto = super::agente::mascarar(std::borrow::Cow::from(linha.as_str()), &segredos);
    assert_eq!(texto, "falha ao ligar com *** e TOKEN_EXEMPLO=*** codigo *** nivel 1");
    assert!(!texto.contains(ponte.as_str()) && !texto.contains("valor-exemplo"), "{texto}");
}

/// Blob de texto anexado vira contexto decodificado; o que não é texto nem imagem é recusado sem
/// derrubar o resto do contrato: imagem segue pelo caminho próprio.
#[test]
fn blob_de_texto_vira_contexto_e_tipo_desconhecido_e_recusado() {
    let texto = ["anexo de texto ", "simples"].concat();
    let dados = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(texto.as_bytes())
    };
    let pedido = json!([{"type": "resource",
        "resource": {"uri": "file:///docs/nota.txt", "blob": dados, "mimeType": "text/plain"}}]);
    let blocos = super::agente::blocos(&pedido).unwrap();
    let junto = blocos.iter().map(ToString::to_string).collect::<String>();
    assert!(junto.contains("anexo de texto simples"), "{junto}");
    assert!(junto.contains("file:///docs/nota.txt"), "a referência do anexo aparece");
    let ruim = json!([{"type": "resource",
        "resource": {"uri": "file:///docs/nao.txt", "blob": dados, "mimeType": "application/pdf"}}]);
    let erro = match super::agente::blocos(&ruim) {
        Err(e) => format!("{e:?}"),
        Ok(_) => panic!("devia recusar"),
    };
    assert!(erro.contains("só imagens"), "{erro}");
}

/// Vários anexos de texto somam no teto agregado do turno, igual às imagens: nenhum passa sozinho
/// do limite por item, mas juntos não podem inflar o pedido.
#[test]
fn varios_anexos_de_texto_somam_no_teto_agregado_do_turno() {
    use base64::Engine as _;
    // Cada um abaixo do teto por item (5 MB); cinco juntos passam do teto agregado do turno (20 MB).
    let dados = base64::engine::general_purpose::STANDARD.encode(vec![b'a'; 9 * 512 * 1024]);
    let anexo = |n: u32| json!({"type": "resource", "resource": {"uri": format!("file:///docs/nota{n}.txt"), "blob": dados, "mimeType": "text/plain"}});
    let pedido = json!([anexo(1), anexo(2), anexo(3), anexo(4), anexo(5)]);
    let erro = match super::agente::blocos(&pedido) {
        Err(e) => format!("{e:?}"),
        Ok(_) => panic!("devia recusar pelo teto agregado"),
    };
    assert!(erro.contains("anexo maior que o limite"), "{erro}");
}
