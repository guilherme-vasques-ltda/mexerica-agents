//! Fichas do Google direto (MEX-1447): Gmail, Agenda, Drive, Docs e Planilhas, todos com o app OAuth da PRÓPRIA pessoa
//! (`Auth::AppProprio`). Nada aqui é segredo: o ID e a chave do app moram no cofre, por Space (`market::google_app`).
//! Fontes lidas em 18/11/2026: os guias oficiais de MCP de cada serviço e os metadados `oauth-protected-resource` de cada
//! servidor. Escopos de cada grupo são os mínimos do serviço e todos constam em `scopes_supported` do respectivo servidor
//! (o Hub reconfere isso a cada login). Um login por serviço: o token de cada um só carrega os escopos dele.
use super::catalogo_servicos::{Auth, Grupo, Imposicao, Links, PassoDoGuia, Servico, Tipo, TipoConexao};

pub(super) const CONFERIDA: &str = "2026-11-18";
const PESQUISA: &str = "docs/propostas/marketplace-20261113/google-caminho-proprio.md";
const PREVIA: &str = "https://developers.google.com/workspace/preview";
/// O servidor de login do Google. O PRM de cada MCP o anuncia com uma barra final; o Hub compara sem ela.
const EMISSOR: &str = "https://accounts.google.com";
const AVISO: &str = "Os conectores do Google para IA estão em prévia e podem mudar. Enquanto o seu app do Google estiver em modo de teste, o Google pede o login de novo a cada 7 dias.";

/// O guia de 6 passos é o mesmo para os cinco serviços; os textos ficam no app, os links aqui.
const GUIA: &[PassoDoGuia] = &[
    PassoDoGuia { id: "projeto", url: Some("https://console.cloud.google.com/projectcreate") },
    PassoDoGuia {
        id: "apis",
        url: Some(
            "https://console.cloud.google.com/flows/enableapi?apiid=gmail.googleapis.com,calendar-json.googleapis.com,drive.googleapis.com,docs.googleapis.com,sheets.googleapis.com,gmailmcp.googleapis.com,calendarmcp.googleapis.com,drivemcp.googleapis.com,docsmcp.googleapis.com,sheetsmcp.googleapis.com",
        ),
    },
    PassoDoGuia { id: "previa", url: Some(PREVIA) },
    PassoDoGuia { id: "tela_login", url: Some("https://console.cloud.google.com/auth/overview") },
    PassoDoGuia { id: "cliente", url: Some("https://console.cloud.google.com/auth/clients/create") },
    PassoDoGuia { id: "campos", url: None },
];

const LINKS: Links =
    Links { privacidade: "https://policies.google.com/privacy", termos: "https://policies.google.com/terms" };

const AUTH: Auth = Auth::AppProprio { provedor: "google", emissor: EMISSOR, guia: GUIA };

/// Escopo do Google por extenso, montado em tempo de compilação (`concat!` não aceita `const`).
macro_rules! escopo {
    ($nome:literal) => {
        concat!("https://www.googleapis.com/auth/", $nome)
    };
}

pub(super) static FICHAS: &[Servico] = &[
    Servico {
        id: "gmail",
        marca: "gmail",
        nome: "Gmail",
        categoria: "Comunicação",
        descricao: "Resuma a caixa de entrada e rascunhe respostas, sem enviar nada sozinho.",
        exemplos: &["Resuma os e-mails de hoje", "Rascunhe uma resposta ao último e-mail do João"],
        site: "https://workspace.google.com",
        docs: Some("https://developers.google.com/workspace/gmail/api/guides/configure-mcp-server"),
        links: LINKS,
        conferida_em: CONFERIDA,
        fontes: &[
            PESQUISA,
            "https://developers.google.com/workspace/gmail/api/guides/configure-mcp-server",
            "https://gmailmcp.googleapis.com/.well-known/oauth-protected-resource/mcp/v1",
            PREVIA,
        ],
        aviso: Some(AVISO),
        tipos: &[TipoConexao {
            id: "gmail.oficial_app",
            tipo: Tipo::OficialApp,
            rotulo: "Oficial — com o seu app do Google",
            mantido_por: "Google",
            url: "https://gmailmcp.googleapis.com/mcp/v1",
            auth: AUTH,
            grupos: &[
                Grupo {
                    id: "ler",
                    rotulo: "Ler e-mails",
                    escopos: &[escopo!("gmail.readonly")],
                    url: None,
                    ferramentas: &[],
                    padrao: true,
                    disponivel: true,
                    imposicao: Some(Imposicao::Escopo),
                },
                // Criar rascunhos pede `gmail.compose`, que no Google também permite enviar: fica indisponível até o filtro de
                // ações perigosas (fatia 2) ou até a prova com conta real mostrar que o `gmail.drafts.create` basta.
                Grupo {
                    id: "rascunhos",
                    rotulo: "Criar rascunhos",
                    escopos: &[escopo!("gmail.compose")],
                    url: None,
                    ferramentas: &[],
                    padrao: false,
                    disponivel: false,
                    imposicao: None,
                },
            ],
            perigosas: &[],
            recomendado: true,
            disponivel: true,
            motivo_indisponivel: None,
        }],
        skills: &[],
        skills_oficiais: &[],
        em_breve: None,
    },
    Servico {
        id: "google-agenda",
        marca: "google-agenda",
        nome: "Google Agenda",
        categoria: "Produtividade",
        descricao: "Veja seus compromissos e ache horários livres na Agenda do Google.",
        exemplos: &["O que tenho na agenda amanhã?", "Ache um horário livre na quinta à tarde"],
        site: "https://workspace.google.com",
        docs: Some("https://developers.google.com/workspace/calendar/api/guides/configure-mcp-server"),
        links: LINKS,
        conferida_em: CONFERIDA,
        fontes: &[
            PESQUISA,
            "https://developers.google.com/workspace/calendar/api/guides/configure-mcp-server",
            "https://calendarmcp.googleapis.com/.well-known/oauth-protected-resource/mcp/v1",
            PREVIA,
        ],
        aviso: Some(AVISO),
        tipos: &[TipoConexao {
            id: "google-agenda.oficial_app",
            tipo: Tipo::OficialApp,
            rotulo: "Oficial — com o seu app do Google",
            mantido_por: "Google",
            url: "https://calendarmcp.googleapis.com/mcp/v1",
            auth: AUTH,
            grupos: &[
                Grupo {
                    id: "ler",
                    rotulo: "Ler a agenda",
                    escopos: &[
                        escopo!("calendar.calendarlist.readonly"),
                        escopo!("calendar.events.freebusy"),
                        escopo!("calendar.events.readonly"),
                    ],
                    url: None,
                    ferramentas: &[],
                    padrao: true,
                    disponivel: true,
                    imposicao: Some(Imposicao::Escopo),
                },
                // Criar e editar compromissos pode apagar também: só depois do filtro de ações perigosas.
                Grupo {
                    id: "criar_editar",
                    rotulo: "Criar e editar compromissos",
                    escopos: &[escopo!("calendar.events")],
                    url: None,
                    ferramentas: &[],
                    padrao: false,
                    disponivel: false,
                    imposicao: None,
                },
            ],
            perigosas: &[],
            recomendado: true,
            disponivel: true,
            motivo_indisponivel: None,
        }],
        skills: &[],
        skills_oficiais: &[],
        em_breve: None,
    },
    Servico {
        id: "google-drive",
        marca: "google-drive",
        nome: "Google Drive",
        categoria: "Produtividade",
        descricao: "Encontre e leia os arquivos que você guarda no Google Drive.",
        exemplos: &["Ache a planilha de orçamento de outubro", "Resuma o documento Plano 2027"],
        site: "https://workspace.google.com",
        docs: Some("https://developers.google.com/workspace/drive/api/guides/configure-mcp-server"),
        links: LINKS,
        conferida_em: CONFERIDA,
        fontes: &[
            PESQUISA,
            "https://developers.google.com/workspace/drive/api/guides/configure-mcp-server",
            "https://drivemcp.googleapis.com/.well-known/oauth-protected-resource/mcp/v1",
            PREVIA,
        ],
        aviso: Some(AVISO),
        tipos: &[TipoConexao {
            id: "google-drive.oficial_app",
            tipo: Tipo::OficialApp,
            rotulo: "Oficial — com o seu app do Google",
            mantido_por: "Google",
            url: "https://drivemcp.googleapis.com/mcp/v1",
            auth: AUTH,
            grupos: &[
                // `drive.file` sozinho só enxerga o que o próprio app criou ou abriu; para achar e ler os arquivos
                // que a pessoa já tem, o mínimo é `drive.readonly`.
                Grupo {
                    id: "ler",
                    rotulo: "Ler arquivos",
                    escopos: &[escopo!("drive.readonly")],
                    url: None,
                    ferramentas: &[],
                    padrao: true,
                    disponivel: true,
                    imposicao: Some(Imposicao::Escopo),
                },
                // Escrever (mesmo só arquivos do próprio app) fica para depois do filtro de ações perigosas (fatia 2).
                Grupo {
                    id: "criar",
                    rotulo: "Criar arquivos novos",
                    escopos: &[escopo!("drive.file")],
                    url: None,
                    ferramentas: &[],
                    padrao: false,
                    disponivel: false,
                    imposicao: None,
                },
            ],
            perigosas: &[],
            recomendado: true,
            disponivel: true,
            motivo_indisponivel: None,
        }],
        skills: &[],
        skills_oficiais: &[],
        em_breve: None,
    },
    Servico {
        id: "google-docs",
        marca: "google-docs",
        nome: "Google Docs",
        categoria: "Produtividade",
        descricao: "Leia e resuma os documentos de texto que você tem no Google Docs.",
        exemplos: &["Resuma o documento Plano 2027", "Quais pontos a ata da reunião de terça deixou em aberto?"],
        site: "https://workspace.google.com",
        docs: Some("https://developers.google.com/workspace/docs/api/guides/configure-mcp-server"),
        links: LINKS,
        conferida_em: CONFERIDA,
        fontes: &[
            PESQUISA,
            "https://developers.google.com/workspace/docs/api/guides/configure-mcp-server",
            "https://docsmcp.googleapis.com/.well-known/oauth-protected-resource/mcp/v1",
            PREVIA,
        ],
        aviso: Some(AVISO),
        tipos: &[TipoConexao {
            id: "google-docs.oficial_app",
            tipo: Tipo::OficialApp,
            rotulo: "Oficial — com o seu app do Google",
            mantido_por: "Google",
            url: "https://docsmcp.googleapis.com/mcp/v1",
            auth: AUTH,
            grupos: &[
                Grupo {
                    id: "ler",
                    rotulo: "Ler documentos",
                    escopos: &[escopo!("documents.readonly")],
                    url: None,
                    ferramentas: &[],
                    padrao: true,
                    disponivel: true,
                    imposicao: Some(Imposicao::Escopo),
                },
                Grupo {
                    id: "editar",
                    rotulo: "Criar e editar documentos",
                    escopos: &[escopo!("documents")],
                    url: None,
                    ferramentas: &[],
                    padrao: false,
                    disponivel: false,
                    imposicao: None,
                },
            ],
            perigosas: &[],
            recomendado: true,
            disponivel: true,
            motivo_indisponivel: None,
        }],
        skills: &[],
        skills_oficiais: &[],
        em_breve: None,
    },
    Servico {
        id: "google-planilhas",
        marca: "google-planilhas",
        nome: "Google Planilhas",
        categoria: "Produtividade",
        descricao: "Leia os dados das suas planilhas do Google Planilhas.",
        exemplos: &["Some a coluna de gastos da planilha de outubro", "Qual foi o maior valor de vendas do trimestre?"],
        site: "https://workspace.google.com",
        docs: Some("https://developers.google.com/workspace/sheets/api/guides/configure-mcp-server"),
        links: LINKS,
        conferida_em: CONFERIDA,
        fontes: &[
            PESQUISA,
            "https://developers.google.com/workspace/sheets/api/guides/configure-mcp-server",
            "https://sheetsmcp.googleapis.com/.well-known/oauth-protected-resource/mcp/v1",
            PREVIA,
        ],
        aviso: Some(AVISO),
        tipos: &[TipoConexao {
            id: "google-planilhas.oficial_app",
            tipo: Tipo::OficialApp,
            rotulo: "Oficial — com o seu app do Google",
            mantido_por: "Google",
            url: "https://sheetsmcp.googleapis.com/mcp/v1",
            auth: AUTH,
            grupos: &[
                Grupo {
                    id: "ler",
                    rotulo: "Ler planilhas",
                    escopos: &[escopo!("spreadsheets.readonly")],
                    url: None,
                    ferramentas: &[],
                    padrao: true,
                    disponivel: true,
                    imposicao: Some(Imposicao::Escopo),
                },
                Grupo {
                    id: "editar",
                    rotulo: "Criar e editar planilhas",
                    escopos: &[escopo!("spreadsheets")],
                    url: None,
                    ferramentas: &[],
                    padrao: false,
                    disponivel: false,
                    imposicao: None,
                },
            ],
            perigosas: &[],
            recomendado: true,
            disponivel: true,
            motivo_indisponivel: None,
        }],
        skills: &[],
        skills_oficiais: &[],
        em_breve: None,
    },
];
