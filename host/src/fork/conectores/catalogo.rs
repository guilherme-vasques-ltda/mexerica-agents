//! Catálogo preparatório, sem instância, login, seleção ou autoridade de execução.
//! Linear: docs/mcp e PRMs públicos conferidos por GET em 06/11/2026.
//! Transporte previsto: Streamable HTTP. Escopos abaixo não são concessões.
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Catalogo {
    versao: u8,
    servicos: [Servico; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Servico {
    id: &'static str,
    nome: &'static str,
    estado: &'static str,
    conectar_disponivel: bool,
    acessos_disponiveis: [&'static str; 0],
    perfis: [Perfil; 2],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Perfil {
    id: &'static str,
    nome: &'static str,
    url: &'static str,
    resource: &'static str,
    issuer: &'static str,
    scopes_previstos: &'static [&'static str],
}

pub(super) fn publico() -> Catalogo {
    Catalogo {
        versao: 1,
        servicos: [Servico {
            id: "linear",
            nome: "Linear",
            estado: "preparatorio",
            conectar_disponivel: false,
            acessos_disponiveis: [],
            perfis: [
                Perfil {
                    id: "leitura",
                    nome: "Leitura",
                    url: "https://mcp.linear.app/mcp/readonly",
                    resource: "https://mcp.linear.app/mcp/readonly",
                    issuer: "https://mcp.linear.app",
                    scopes_previstos: &["read"],
                },
                Perfil {
                    id: "leitura_criacao",
                    nome: "Leitura e criação",
                    url: "https://mcp.linear.app/mcp",
                    resource: "https://mcp.linear.app/mcp",
                    issuer: "https://mcp.linear.app",
                    scopes_previstos: &["read", "write"],
                },
            ],
        }],
    }
}
