//! Provas do repasse local dos planos: servidores de mentira, nenhuma rede, nenhuma chave real.
use super::tests::{bot, com_chave, hub_de_prova};
use super::*;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const CHAVE: &str = "chave-ficticia-do-repasse-456";
const CHAVE_DO_COFRE: &str = "chave-ficticia-do-plano-123";

struct Upstream {
    base: String,
    pedidos: Arc<std::sync::Mutex<Vec<String>>>,
}

/// Lê o pedido inteiro: cabeçalho e corpo pelo `content-length`.
async fn ler(conexao: &mut TcpStream) -> String {
    let mut dados = Vec::new();
    let mut parte = [0u8; 16384];
    loop {
        let n = conexao.read(&mut parte).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        dados.extend_from_slice(&parte[..n]);
        let Some(fim) = dados.windows(4).position(|janela| janela == b"\r\n\r\n") else { continue };
        let cabecalho = String::from_utf8_lossy(&dados[..fim]).to_ascii_lowercase();
        let tamanho = cabecalho
            .lines()
            .find_map(|linha| linha.strip_prefix("content-length:"))
            .and_then(|valor| valor.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if dados.len() >= fim + 4 + tamanho {
            break;
        }
    }
    String::from_utf8_lossy(&dados).into_owned()
}

/// Um fornecedor de mentira: `comportamento` recebe a conexão e o pedido já lido, e responde como quiser.
async fn upstream<F, Fut>(comportamento: F) -> Upstream
where
    F: Fn(TcpStream, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let escuta = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", escuta.local_addr().unwrap());
    let pedidos = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (comportamento, guardados) = (Arc::new(comportamento), pedidos.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut conexao, _)) = escuta.accept().await else { return };
            let (comportamento, guardados) = (comportamento.clone(), guardados.clone());
            tokio::spawn(async move {
                let pedido = Box::pin(ler(&mut conexao)).await;
                guardados.locked().push(pedido.clone());
                comportamento(conexao, pedido).await;
            });
        }
    });
    Upstream { base, pedidos }
}

fn json_ok(corpo: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}",
        corpo.len()
    )
    .into_bytes()
}

async fn pedaco(conexao: &mut TcpStream, dados: &str) {
    let _ = conexao.write_all(format!("{:x}\r\n{dados}\r\n", dados.len()).as_bytes()).await;
    let _ = conexao.flush().await;
}

const CABECALHO_SSE: &str =
    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n";

struct Montagem {
    _hub: Arc<Hub>,
    repasse: Arc<Repasse>,
    concessao: Concessao,
    cliente: reqwest::Client,
}

async fn montar(up: &Upstream) -> Montagem {
    let hub = hub_de_prova();
    let repasse = repasse_do_hub(&hub).await.unwrap();
    *repasse.base_de_prova.locked() = Some(up.base.clone());
    let geracao = repasse.geracao(&PLANOS[0]);
    let concessao = repasse.conceder(&PLANOS[0], Zeroizing::new(CHAVE.into()), geracao).unwrap();
    let cliente = reqwest::Client::builder().no_proxy().build().unwrap();
    Montagem { _hub: hub, repasse, concessao, cliente }
}

impl Montagem {
    fn pedido(&self, caminho: &str, cabecalho_do_codigo: &str) -> reqwest::RequestBuilder {
        let valor = if cabecalho_do_codigo == "authorization" {
            format!("Bearer {}", self.concessao.codigo())
        } else {
            self.concessao.codigo().to_owned()
        };
        self.cliente.post(format!("{}/{caminho}", self.concessao.base())).header(cabecalho_do_codigo, valor)
    }
}

#[tokio::test]
async fn o_repasse_troca_o_codigo_pela_chave_e_passa_o_resto_sem_alteracao() {
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(&json_ok(r#"{"ok":true}"#)).await;
    })
    .await;
    let m = montar(&up).await;
    let resposta = m
        .pedido("chat/completions", "authorization")
        .header("user-agent", "opencode-teste/1.0")
        .header("x-opencode-session", "s1")
        .header("x-api-key", m.concessao.codigo())
        .header("cookie", "a=b")
        .body(r#"{"model":"glm"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resposta.status(), 200);
    assert_eq!(resposta.text().await.unwrap(), r#"{"ok":true}"#);
    let pedido = up.pedidos.locked()[0].to_ascii_lowercase();
    assert!(pedido.starts_with("post /inference/go/openai/v1/chat/completions http/1.1"), "{pedido}");
    assert!(pedido.contains(&format!("authorization: bearer {}", CHAVE.to_ascii_lowercase())), "{pedido}");
    assert!(pedido.contains("user-agent: opencode-teste/1.0"), "a identificação do cliente passa sem alteração");
    assert!(pedido.contains("x-opencode-session: s1"));
    assert!(pedido.contains("accept-encoding: identity"));
    assert!(pedido.ends_with(r#"{"model":"glm"}"#));
    for proibido in ["x-api-key", "cookie", &m.concessao.codigo().to_ascii_lowercase()] {
        assert!(!pedido.contains(proibido), "{proibido} não pode seguir ao fornecedor: {pedido}");
    }
}

#[tokio::test]
async fn cada_formato_vai_ao_seu_endereco_fixo_com_o_codigo_no_cabecalho_do_seu_sdk() {
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(&json_ok("{}")).await;
    })
    .await;
    let m = montar(&up).await;
    for (caminho, cabecalho) in [
        ("chat/completions", "authorization"),
        ("responses", "authorization"),
        ("messages", "x-api-key"),
        ("models/gemini-4-argon:generateContent", "x-goog-api-key"),
        ("models/gemini-4-argon:streamGenerateContent?alt=sse", "x-goog-api-key"),
    ] {
        let resposta = m.pedido(caminho, cabecalho).body("{}").send().await.unwrap();
        assert_eq!(resposta.status(), 200, "{caminho}");
    }
    let pedidos = up.pedidos.locked().clone();
    assert_eq!(pedidos.len(), 5);
    for (pedido, esperado) in pedidos.iter().zip([
        "POST /inference/go/openai/v1/chat/completions ",
        "POST /inference/go/openai/v1/responses ",
        "POST /inference/go/anthropic/v1/messages ",
        "POST /inference/go/google/v1beta/models/gemini-4-argon:generateContent ",
        "POST /inference/go/google/v1beta/models/gemini-4-argon:streamGenerateContent?alt=sse ",
    ]) {
        assert!(pedido.starts_with(esperado), "{pedido}");
        assert!(pedido.to_ascii_lowercase().contains(&format!("authorization: bearer {}", CHAVE.to_ascii_lowercase())));
    }
}

#[tokio::test]
async fn o_repasse_recusa_codigo_errado_revogado_metodo_caminho_e_consulta_fora_da_tabela() {
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(&json_ok("{}")).await;
    })
    .await;
    let m = montar(&up).await;
    let base = m.concessao.base();
    let bearer = format!("Bearer {}", m.concessao.codigo());
    let casos: Vec<(reqwest::Method, String, String)> = vec![
        (reqwest::Method::POST, format!("{base}/chat/completions"), "Bearer mxs_errado".into()),
        (reqwest::Method::POST, format!("{base}/chat/completions"), String::new()),
        (reqwest::Method::GET, format!("{base}/chat/completions"), bearer.clone()),
        (reqwest::Method::POST, format!("{base}/models"), bearer.clone()),
        (reqwest::Method::POST, format!("{base}/chat/completions/extra"), bearer.clone()),
        (reqwest::Method::POST, format!("{base}/../x"), bearer.clone()),
        (reqwest::Method::POST, format!("{base}/chat/completions?x=1"), bearer.clone()),
        (reqwest::Method::POST, format!("{base}/models/a/b:generateContent"), bearer.clone()),
        (reqwest::Method::POST, format!("{}/outro/chat/completions", base.trim_end_matches("/r")), bearer.clone()),
    ];
    for (metodo, url, autorizacao) in casos {
        let mut pedido = m.cliente.request(metodo.clone(), &url).body("{}");
        if !autorizacao.is_empty() {
            pedido = pedido.header("authorization", autorizacao);
        }
        let resposta = pedido.send().await.unwrap();
        assert_eq!(resposta.status(), 404, "{metodo} {url}");
        assert_eq!(resposta.text().await.unwrap(), "");
    }
    assert!(up.pedidos.locked().is_empty(), "nada fora da tabela chega ao fornecedor");

    drop(m.concessao);
    let resposta = m
        .cliente
        .post(format!("{base}/chat/completions"))
        .header("authorization", bearer)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resposta.status(), 404, "o código de uma sessão encerrada morre");
    assert!(up.pedidos.locked().is_empty());
}

#[tokio::test]
async fn o_repasse_nao_segue_redirecionamento_do_fornecedor() {
    let alvo = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(&json_ok("{}")).await;
    })
    .await;
    let destino = alvo.base.clone();
    let up = upstream(move |mut conexao, _| {
        let destino = destino.clone();
        async move {
            let _ = conexao
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nlocation: {destino}/x\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await;
        }
    })
    .await;
    let m = montar(&up).await;
    let resposta = m.pedido("chat/completions", "authorization").body("{}").send().await.unwrap();
    assert_eq!(resposta.status(), 502);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(alvo.pedidos.locked().is_empty(), "o destino do redirecionamento não pode receber nem a chave");
}

#[tokio::test]
async fn resposta_com_a_chave_e_cortada_inclusive_partida_em_dois_pedacos_e_cabecalho_e_recusado() {
    // A chave inteira no corpo.
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(CABECALHO_SSE.as_bytes()).await;
        pedaco(&mut conexao, "data: inicio\n\n").await;
        pedaco(&mut conexao, &format!("data: eco {CHAVE}\n\n")).await;
        pedaco(&mut conexao, "data: fim\n\n").await;
        let _ = conexao.write_all(b"0\r\n\r\n").await;
    })
    .await;
    let m = montar(&up).await;
    let mut resposta = m.pedido("chat/completions", "authorization").body("{}").send().await.unwrap();
    let mut recebido = Vec::new();
    let mut falhou = false;
    loop {
        match resposta.chunk().await {
            Ok(Some(pedaco)) => recebido.extend_from_slice(&pedaco),
            Ok(None) => break,
            Err(_) => {
                falhou = true;
                break;
            }
        }
    }
    assert!(falhou, "o fluxo com a chave é cortado, não termina bem");
    assert!(!recebido.windows(CHAVE.len()).any(|janela| janela == CHAVE.as_bytes()));
    assert!(!String::from_utf8_lossy(&recebido).contains("data: fim"), "nada depois da chave segue");

    // A chave partida entre dois pedaços: nem a primeira metade sai.
    let metade = CHAVE.len() / 2;
    let up = upstream(move |mut conexao, _| async move {
        let _ = conexao.write_all(CABECALHO_SSE.as_bytes()).await;
        pedaco(&mut conexao, &format!("data: partida {}", &CHAVE[..metade])).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        pedaco(&mut conexao, &format!("{}\n\n", &CHAVE[metade..])).await;
        let _ = conexao.write_all(b"0\r\n\r\n").await;
    })
    .await;
    let m = montar(&up).await;
    let mut resposta = m.pedido("chat/completions", "authorization").body("{}").send().await.unwrap();
    let mut recebido = Vec::new();
    while let Ok(Some(pedaco)) = resposta.chunk().await {
        recebido.extend_from_slice(&pedaco);
    }
    let texto = String::from_utf8_lossy(&recebido);
    assert!(!texto.contains(&CHAVE[..metade]), "nem a primeira metade da chave sai: {texto}");
    assert!(texto.contains("data: partida "), "o que vem antes da chave segue");

    // A chave num cabeçalho da resposta.
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao
            .write_all(
                format!("HTTP/1.1 200 OK\r\nx-eco: {CHAVE}\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}")
                    .as_bytes(),
            )
            .await;
    })
    .await;
    let m = montar(&up).await;
    let resposta = m.pedido("chat/completions", "authorization").body("{}").send().await.unwrap();
    assert_eq!(resposta.status(), 502);
    assert!(
        !resposta
            .headers()
            .iter()
            .any(|(_, valor)| valor.as_bytes().windows(CHAVE.len()).any(|j| j == CHAVE.as_bytes()))
    );
}

#[tokio::test]
async fn o_streaming_chega_aos_pedacos_sem_esperar_o_fim() {
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(CABECALHO_SSE.as_bytes()).await;
        pedaco(&mut conexao, "data: um\n\n").await;
        tokio::time::sleep(Duration::from_millis(900)).await;
        pedaco(&mut conexao, "data: dois\n\n").await;
        let _ = conexao.write_all(b"0\r\n\r\n").await;
    })
    .await;
    let m = montar(&up).await;
    let inicio = Instant::now();
    let mut resposta = m.pedido("chat/completions", "authorization").body("{}").send().await.unwrap();
    let primeiro = resposta.chunk().await.unwrap().unwrap();
    assert_eq!(&primeiro[..], b"data: um\n\n");
    assert!(
        inicio.elapsed() < Duration::from_millis(600),
        "o primeiro pedaço não espera o segundo: {:?}",
        inicio.elapsed()
    );
    let segundo = resposta.chunk().await.unwrap().unwrap();
    assert_eq!(&segundo[..], b"data: dois\n\n");
}

#[tokio::test]
async fn revogar_corta_o_pedido_em_andamento_antes_e_depois_dos_cabecalhos() {
    for responde_cabecalho in [false, true] {
        let up = upstream(move |mut conexao, _| async move {
            if responde_cabecalho {
                let _ = conexao.write_all(CABECALHO_SSE.as_bytes()).await;
                pedaco(&mut conexao, "data: um\n\n").await;
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        })
        .await;
        let m = montar(&up).await;
        let cliente = m.cliente.clone();
        let (url, codigo) =
            (format!("{}/chat/completions", m.concessao.base()), format!("Bearer {}", m.concessao.codigo()));
        let inicio = Instant::now();
        let tarefa = tokio::spawn(async move {
            let resposta = cliente.post(url).header("authorization", codigo).body("{}").send().await?;
            resposta.bytes().await
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        m.repasse.revogar_plano(&PLANOS[0]);
        let desfecho =
            tokio::time::timeout(Duration::from_secs(3), tarefa).await.expect("o pedido foi cortado").unwrap();
        assert!(inicio.elapsed() < Duration::from_secs(3));
        if let Ok(corpo) = desfecho {
            assert!(!String::from_utf8_lossy(&corpo).contains("fim"));
        }
    }
}

#[tokio::test]
async fn pedido_grande_demais_e_recusado_sem_chegar_ao_fornecedor() {
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(&json_ok("{}")).await;
    })
    .await;
    let m = montar(&up).await;
    let grande = vec![b'x'; (32 << 20) + 1024];
    let resposta = m.pedido("chat/completions", "authorization").body(grande).send().await.unwrap();
    assert_eq!(resposta.status(), 413);
    assert!(up.pedidos.locked().is_empty());
}

#[test]
fn a_tabela_de_rotas_so_aponta_para_o_fornecedor_do_plano() {
    for plano in PLANOS {
        assert!(!plano.rotas.is_empty(), "{}", plano.id);
        for (local, url) in plano.rotas {
            assert!(!local.starts_with('/') && !local.contains(".."), "{local}");
            let endereco = reqwest::Url::parse(&url.replace('*', "modelo")).unwrap();
            assert_eq!(endereco.scheme(), "https", "{url}");
            assert_eq!(endereco.host_str(), Some("opencode.ai"), "{url}");
            assert!(endereco.username().is_empty() && endereco.query().is_none(), "{url}");
        }
    }
}

#[test]
fn o_destino_exige_o_prefixo_a_rota_e_o_segmento_do_modelo() {
    let go = &PLANOS[0];
    let d = |caminho: &str, consulta: Option<&str>| repasse::destino(go, caminho, consulta);
    assert_eq!(d("/r/responses", None).as_deref(), Some("https://opencode.ai/inference/go/openai/v1/responses"));
    assert_eq!(
        d("/r/models/gemini-4-argon:streamGenerateContent", Some("alt=sse")).as_deref(),
        Some("https://opencode.ai/inference/go/google/v1beta/models/gemini-4-argon:streamGenerateContent?alt=sse")
    );
    for ruim in [
        "/responses",
        "/r/",
        "/r/responses/",
        "/r/models/:generateContent",
        "/r/models/a b:generateContent",
        "/r/models/a/b:generateContent",
        "/r/x",
    ] {
        assert_eq!(d(ruim, None), None, "{ruim}");
    }
    assert_eq!(d("/r/responses", Some("outra=1")), None);
}

#[test]
fn a_parte_retida_so_e_o_comeco_da_chave() {
    use super::repasse::retido;
    let chave = b"chave-secreta";
    assert_eq!(retido(b"texto qualquer", chave), 0);
    assert_eq!(retido(b"texto chave-se", chave), 8);
    assert_eq!(retido(b"texto c", chave), 1);
    assert_eq!(retido(b"", chave), 0);
}

#[tokio::test]
async fn o_codigo_de_sessao_e_aleatorio_e_so_o_hash_fica_no_estado() {
    let up = upstream(|mut conexao, _| async move {
        let _ = conexao.write_all(&json_ok("{}")).await;
    })
    .await;
    let m = montar(&up).await;
    let geracao = m.repasse.geracao(&PLANOS[0]);
    let outra = m.repasse.conceder(&PLANOS[0], Zeroizing::new(CHAVE.into()), geracao).unwrap();
    assert_ne!(m.concessao.codigo(), outra.codigo());
    assert!(m.concessao.codigo().len() >= 68 && m.concessao.codigo().starts_with("mxs_"));
    assert!(!m.concessao.codigo().contains(CHAVE));
    drop(outra);
    assert_eq!(m.repasse.sessoes_vivas(), 1);
}

#[tokio::test]
async fn o_repasse_do_hub_e_um_so_e_escuta_so_em_loopback() {
    let hub = hub_de_prova();
    let a = repasse_do_hub(&hub).await.unwrap();
    let b = repasse_do_hub(&hub).await.unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    com_chave(&hub).await;
    let cfg = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&cfg).unwrap();
    let ambiente = ambiente_do_bot(&hub, &cfg).await.unwrap().unwrap();
    assert!(ambiente.concessao.base().starts_with("http://127.0.0.1:"));
}

/// A prova com o OpenCode de verdade (ele não está instalado no teste automático: roda só onde há o programa, pelo
/// caminho em `MEXERICA_OPENCODE_BIN` ou no PATH). O fornecedor é de mentira. O OpenCode é aberto pelo mesmo ACP do Hub,
/// com o ambiente que o Hub monta, e um MCP filho e um comando de shell despejam o ambiente que enxergam.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn opencode_de_verdade_nem_ele_nem_o_mcp_filho_nem_o_shell_enxergam_a_chave() {
    let Some(binario) = std::env::var_os("MEXERICA_OPENCODE_BIN")
        .map(std::path::PathBuf::from)
        .or_else(|| crate::agent::backends::which("opencode"))
    else {
        eprintln!("pulado: o OpenCode não está instalado neste ambiente");
        return;
    };
    let dump = crate::store::private_fixture_dir("planos-opencode-real");
    let comando_do_shell = format!(
        "env > {d}/shell.env; tr '\\0' '\\n' < /proc/$PPID/environ > {d}/opencode.env 2>/dev/null; true",
        d = dump.display()
    );
    let up = upstream(move |mut conexao, pedido| {
        let comando = comando_do_shell.clone();
        async move {
            let _ = conexao.write_all(CABECALHO_SSE.as_bytes()).await;
            let chunk = |delta: Value, fim: Option<&str>| {
                json!({"id": "c1", "object": "chat.completion.chunk", "created": 1, "model": "glm-5.3",
                    "choices": [{"index": 0, "delta": delta, "finish_reason": fim}]})
                .to_string()
            };
            let uso =
                json!({"id": "c1", "object": "chat.completion.chunk", "created": 1, "model": "glm-5.3", "choices": [],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}})
                .to_string();
            let eventos = if pedido.contains("\"role\":\"tool\"") {
                vec![chunk(json!({"role": "assistant", "content": "ok"}), None), chunk(json!({}), Some("stop")), uso]
            } else {
                let chamada = json!({"role": "assistant", "content": null, "tool_calls": [{"index": 0, "id": "call_1",
                    "type": "function", "function": {"name": "bash",
                    "arguments": json!({"command": comando, "description": "ambiente"}).to_string()}}]});
                vec![chunk(chamada, None), chunk(json!({}), Some("tool_calls")), uso]
            };
            for evento in eventos {
                pedaco(&mut conexao, &format!("data: {evento}\n\n")).await;
            }
            pedaco(&mut conexao, "data: [DONE]\n\n").await;
            let _ = conexao.write_all(b"0\r\n\r\n").await;
        }
    })
    .await;

    let hub = hub_de_prova();
    com_chave(&hub).await;
    let repasse = repasse_do_hub(&hub).await.unwrap();
    *repasse.base_de_prova.locked() = Some(up.base.clone());
    let cfg = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&cfg).unwrap();
    let ambiente = ambiente_do_bot(&hub, &cfg).await.unwrap().unwrap();
    let mut env = Vec::new();
    aplicar_ambiente(&mut env, "opencode", "", Some(&ambiente));
    env.push(("OPENCODE_PERMISSION".into(), r#"{"bash":"allow"}"#.into()));

    let casa = std::path::PathBuf::from(&env.iter().find(|(k, _)| k == "HOME").unwrap().1);
    let cwd = casa.join("trabalho");
    std::fs::create_dir_all(&cwd).unwrap();
    let comando = format!("{} acp", crate::shell::quote(&binario.to_string_lossy()));
    let (acp, mut entrada) = crate::agent::acp::Acp::spawn(&comando, &cwd.to_string_lossy(), &env).unwrap();
    tokio::spawn(async move { while entrada.recv().await.is_some() {} });
    let passos = async {
        acp.request("initialize", json!({"protocolVersion": 1, "clientCapabilities": {}})).await.unwrap();
        let sonda = format!("env > {}/mcp.env; sleep 60", dump.display());
        let sessao = acp
            .request(
                "session/new",
                json!({"cwd": cwd, "mcpServers": [{"name": "sonda", "command": "/bin/sh", "args": ["-c", sonda], "env": []}]}),
            )
            .await
            .unwrap();
        let sid = sessao["sessionId"].as_str().unwrap().to_owned();
        acp.request(
            "session/set_config_option",
            json!({"sessionId": sid, "configId": "model", "value": "opencode-go/glm-5.3"}),
        )
        .await
        .unwrap();
        acp.request("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": "oi"}]}))
            .await
            .unwrap();
    };
    let desfecho = tokio::time::timeout(Duration::from_secs(120), passos).await;
    acp.kill().await;
    desfecho.expect("o OpenCode respondeu a tempo");

    let ler_dump = |nome: &str| std::fs::read_to_string(dump.join(nome)).ok();
    let shell = ler_dump("shell.env").expect("o comando de shell rodou");
    let mcp = ler_dump("mcp.env").expect("o MCP filho foi iniciado");
    let opencode = ler_dump("opencode.env");
    for (quem, texto) in [("shell", Some(&shell)), ("mcp", Some(&mcp)), ("opencode", opencode.as_ref())] {
        let Some(texto) = texto else { continue };
        assert!(!texto.contains(CHAVE_DO_COFRE), "o ambiente de {quem} contém a chave");
    }
    assert!(
        shell.contains("OPENCODE_API_KEY=mxs_") && mcp.contains("OPENCODE_API_KEY=mxs_"),
        "eles só têm o código de sessão"
    );
    let pedidos = up.pedidos.locked().join("\n").to_ascii_lowercase();
    assert!(
        pedidos.contains(&format!("authorization: bearer {}", CHAVE_DO_COFRE.to_ascii_lowercase())),
        "o repasse pôs a chave de verdade"
    );
    assert!(!pedidos.contains("mxs_"), "o código de sessão não segue ao fornecedor");
    eprintln!("prova com o OpenCode de verdade: ambiente do OpenCode lido={}", opencode.is_some());
}
