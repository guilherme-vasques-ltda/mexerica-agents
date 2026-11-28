//! M1: revisões MCP legadas por ponta. Parser exato, sem rede, cofre, Authority ou retry.
//! Não promete a era 2026-07-28 nem conformidade além do perfil de cada ponta.
use anyhow::{Result, anyhow};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Versao {
    V2025_06_18,
    V2025_03_26,
    V2024_11_05,
}

/// Preferência implementada; também é a única versão dos servidores internos.
pub(crate) const PREFERIDA: Versao = Versao::V2025_06_18;

impl Versao {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::V2025_06_18 => "2025-06-18",
            Self::V2025_03_26 => "2025-03-26",
            Self::V2024_11_05 => "2024-11-05",
        }
    }

    /// Igualdade exata: sem trim, caixa ou coerção.
    fn parse(text: &str) -> Option<Self> {
        [Self::V2025_06_18, Self::V2025_03_26, Self::V2024_11_05].into_iter().find(|v| v.as_str() == text)
    }
}

/// Ponta da negociação. `Builtin`: servidores internos de `mcp.rs`.
/// `BridgeLegacy`: ponte Guard/curada e verify, só o perfil já mediado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Perfil {
    Builtin,
    BridgeLegacy,
}

/// Erros técnicos de vocabulário fixo; nunca carregam o valor remoto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Erro {
    PedidoInvalido,
    RespostaInvalida,
    NaoSuportada,
}

impl Erro {
    pub(crate) const fn code(self) -> i64 {
        match self {
            Self::PedidoInvalido => -32602,
            Self::RespostaInvalida | Self::NaoSuportada => -32603,
        }
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::PedidoInvalido => "initialize MCP exige protocolVersion em texto",
            Self::RespostaInvalida => "Resposta initialize MCP inválida",
            Self::NaoSuportada => "Versão MCP não suportada nesta conexão",
        }
    }

    pub(crate) fn json(self) -> Value {
        serde_json::json!({"code":self.code(),"message":self.message()})
    }
}

impl Perfil {
    fn suporta(self, versao: Versao) -> bool {
        match self {
            Self::Builtin => versao == PREFERIDA,
            Self::BridgeLegacy => true,
        }
    }

    /// Pedido: versão suportada é conservada; outra string recebe a preferência
    /// implementada; ausência ou outro tipo não vira versão.
    pub(crate) fn oferta(self, pedido: &Value) -> Result<Versao, Erro> {
        let texto = pedido.as_str().ok_or(Erro::PedidoInvalido)?;
        Ok(Versao::parse(texto).filter(|v| self.suporta(*v)).unwrap_or(PREFERIDA))
    }

    /// Resultado de initialize: a versão efetiva é preservada; nada é coagido.
    pub(crate) fn resultado(self, result: &Value) -> Result<Versao, Erro> {
        let texto = result.get("protocolVersion").and_then(Value::as_str).ok_or(Erro::RespostaInvalida)?;
        Versao::parse(texto).filter(|v| self.suporta(*v)).ok_or(Erro::NaoSuportada)
    }
}

const RESERVADOS: [&str; 2] = ["mcp-protocol-version", "mcp-session-id"];

/// Nome de header MCP reservado ao estado da sessão, em qualquer caixa.
pub(crate) fn reservado(nome: &str) -> bool {
    RESERVADOS.iter().any(|r| r.eq_ignore_ascii_case(nome))
}

/// Cópia dos headers do alvo sem valores MCP configurados/duplicados, mais
/// somente o estado canônico da Session (nenhum no initialize/GET inicial).
pub(crate) fn headers(base: &HeaderMap, versao: Option<Versao>, sessao: Option<&str>) -> Result<HeaderMap> {
    let mut headers = base.clone();
    for nome in RESERVADOS {
        // HeaderMap normaliza a caixa; remove todas as entradas do nome.
        headers.remove(nome);
    }
    if let Some(versao) = versao {
        headers.insert(RESERVADOS[0], HeaderValue::from_static(versao.as_str()));
    }
    if let Some(sessao) = sessao {
        let valor = HeaderValue::from_str(sessao).map_err(|_| anyhow!("Identificador de sessão MCP inválido"))?;
        headers.insert(RESERVADOS[1], valor);
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn oferta_conserva_suportada_contrapropoe_texto_e_recusa_tipo() {
        assert_eq!(Perfil::Builtin.oferta(&json!("2025-06-18")), Ok(Versao::V2025_06_18));
        // Builtin implementa só a preferência: as legadas recebem contraproposta.
        for pedido in ["2025-03-26", "2024-11-05", "2026-07-28", " 2025-06-18", "2025-06-18\n", ""] {
            assert_eq!(Perfil::Builtin.oferta(&json!(pedido)), Ok(PREFERIDA), "{pedido:?}");
        }
        assert_eq!(Perfil::BridgeLegacy.oferta(&json!("2025-03-26")), Ok(Versao::V2025_03_26));
        assert_eq!(Perfil::BridgeLegacy.oferta(&json!("2024-11-05")), Ok(Versao::V2024_11_05));
        assert_eq!(Perfil::BridgeLegacy.oferta(&json!("2099-01-01")), Ok(PREFERIDA));
        for perfil in [Perfil::Builtin, Perfil::BridgeLegacy] {
            for pedido in [Value::Null, json!(20_250_618), json!(["2025-06-18"]), json!({})] {
                assert_eq!(perfil.oferta(&pedido), Err(Erro::PedidoInvalido));
            }
            assert_eq!(perfil.oferta(&json!({})["protocolVersion"]), Err(Erro::PedidoInvalido));
        }
        assert_eq!(Erro::PedidoInvalido.code(), -32602);
    }

    #[test]
    fn resultado_preserva_alternativa_e_recusa_desconhecida_sem_eco() {
        assert_eq!(Perfil::BridgeLegacy.resultado(&json!({"protocolVersion":"2025-03-26"})), Ok(Versao::V2025_03_26));
        assert_eq!(Perfil::BridgeLegacy.resultado(&json!({"protocolVersion":"2024-11-05"})), Ok(Versao::V2024_11_05));
        assert_eq!(Perfil::Builtin.resultado(&json!({"protocolVersion":"2025-03-26"})), Err(Erro::NaoSuportada));
        for versao in ["1999-01-01", "2025-06-18 ", "2025-06-18\r\nx-injetado: 1", "<donnabot-external>"] {
            let erro = Perfil::BridgeLegacy.resultado(&json!({"protocolVersion":versao})).unwrap_err();
            assert_eq!(erro, Erro::NaoSuportada);
            assert!(!erro.json().to_string().contains(versao.trim()));
        }
        for result in [json!({}), json!({"protocolVersion":null}), json!({"protocolVersion":7}), json!(null)] {
            assert_eq!(Perfil::BridgeLegacy.resultado(&result), Err(Erro::RespostaInvalida));
        }
        assert_eq!(
            Erro::NaoSuportada.json(),
            json!({"code":-32603,"message":"Versão MCP não suportada nesta conexão"})
        );
    }

    #[test]
    fn headers_removem_configurados_duplicados_e_inserem_um_valor_da_sessao() {
        let mut base = HeaderMap::new();
        base.insert("authorization", HeaderValue::from_static("Bearer fixture"));
        for (nome, valor) in [("MCP-Protocol-Version", "1999-01-01"), ("mcp-protocol-version", "2024-11-05")] {
            base.append(
                reqwest::header::HeaderName::from_bytes(nome.as_bytes()).unwrap(),
                HeaderValue::from_static(valor),
            );
        }
        base.append(
            reqwest::header::HeaderName::from_bytes(b"Mcp-Session-Id").unwrap(),
            HeaderValue::from_static("sessao-alheia"),
        );
        let inicial = headers(&base, None, None).unwrap();
        assert!(inicial.get_all("mcp-protocol-version").iter().next().is_none());
        assert!(inicial.get("mcp-session-id").is_none());
        assert_eq!(inicial["authorization"], "Bearer fixture");
        let sessao = headers(&base, Some(Versao::V2025_03_26), Some("sessao-propria")).unwrap();
        let versoes: Vec<_> = sessao.get_all("mcp-protocol-version").iter().collect();
        assert_eq!(versoes, ["2025-03-26"]);
        let ids: Vec<_> = sessao.get_all("mcp-session-id").iter().collect();
        assert_eq!(ids, ["sessao-propria"]);
        assert!(headers(&base, None, Some("quebra\nde linha")).is_err());
        assert!(reservado("MCP-PROTOCOL-VERSION") && reservado("Mcp-Session-Id") && !reservado("authorization"));
    }

    /// verify: ping reverso com o mesmo ID do pedido não vira resultado; versão
    /// alternativa chega à descoberta; inválida para antes de initialized/tools/list.
    #[tokio::test]
    async fn verify_valida_versao_antes_de_initialized_e_responde_ping_local() {
        const SCRIPT: &str = r#"
import json, sys
log = open(sys.argv[1], "a")
for line in sys.stdin:
    msg = json.loads(line)
    if "method" not in msg:
        log.write("pong:" + json.dumps(msg.get("result")) + "\n"); log.flush(); continue
    log.write(msg["method"] + "\n"); log.flush()
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping"}), flush=True)
        log.write("pong:" + json.dumps(json.loads(sys.stdin.readline()).get("result")) + "\n"); log.flush()
        result = {"capabilities": {"tools": {}}}
        if sys.argv[2] != "omit":
            result["protocolVersion"] = sys.argv[2]
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
    elif msg["method"] == "tools/list":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"tools": []}}), flush=True)
"#;
        let python = if cfg!(windows) { "python" } else { "python3" };
        let dir = std::env::temp_dir().join(format!("mcp-versao-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for (version, outcome) in
            [("2025-03-26", Ok(())), ("1999-01-01", Err(Erro::NaoSuportada)), ("omit", Err(Erro::RespostaInvalida))]
        {
            let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
            let log = dir.join(format!("{version}.log"));
            let connector = serde_json::from_value(json!({"id":"versao","name":"Versão","command":python,
                "args":["-c",SCRIPT,log.to_string_lossy(),version]}))
            .unwrap();
            crate::market::save_connectors(&store, &[connector]).unwrap();
            let result = crate::market::verify::verify(&store, "versao", (1, None)).await;
            let seen = std::fs::read_to_string(&log).unwrap();
            assert!(seen.starts_with("initialize\npong:{}\n"), "{seen}");
            match outcome {
                Ok(()) => {
                    assert_eq!(result.unwrap()["status"], "ready");
                    assert!(seen.ends_with("notifications/initialized\ntools/list\n"), "{seen}");
                }
                Err(error) => {
                    assert_eq!(result.unwrap_err().to_string(), error.message());
                    assert!(!seen.contains("notifications/initialized") && !seen.contains("tools/list"), "{seen}");
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
