//! Fichas que moram em arquivo JSON (`fichas/*.json`), lidas em tempo de compilação com `include_str!`.
//! O arquivo tem o mesmo formato do que o catálogo entrega ao app. Ficha malformada nunca entra no catálogo: o
//! erro é registrado e o serviço fica de fora (fail-closed); o teste do catálogo reprova esse caso.
use super::catalogo_servicos::{
    AppProprioConfig, Auth, CATEGORIAS, Curada, Grupo, Imposicao, Links, Perigosa, Servico, SkillOficial, SkillRef,
    Tipo, TipoConexao,
};
use serde::Deserialize;

/// Ficha do dono A (decisão do dono, 17/11/2026). O nome do produto só pode aparecer dentro deste arquivo.
pub(super) const FICHA_DONO_A: &str = include_str!("fichas/ficha-dono-a.json");

/// Todas as fichas, na ordem em que o catálogo as mostra (a do dono A vai primeiro). Cada arquivo novo de `fichas/`
/// entra aqui; o teste `todo_arquivo_de_fichas_esta_na_lista` reprova o esquecimento.
pub(super) const FICHAS: &[&str] = &[
    FICHA_DONO_A,
    include_str!("fichas/linear.json"),
    include_str!("fichas/context7.json"),
    include_str!("fichas/exa.json"),
    include_str!("fichas/neon.json"),
    include_str!("fichas/wispr-flow.json"),
    include_str!("fichas/github.json"),
    include_str!("fichas/better-stack.json"),
    include_str!("fichas/clerk.json"),
    include_str!("fichas/coderabbit.json"),
    include_str!("fichas/posthog.json"),
    include_str!("fichas/revenuecat.json"),
    include_str!("fichas/supabase.json"),
    include_str!("fichas/todoist.json"),
    include_str!("fichas/clickup.json"),
    include_str!("fichas/metricool.json"),
    include_str!("fichas/lovable.json"),
    include_str!("fichas/firecrawl.json"),
    include_str!("fichas/render.json"),
    include_str!("fichas/short-io.json"),
    include_str!("fichas/composio.json"),
    include_str!("fichas/outlook.json"),
    include_str!("fichas/slack.json"),
    include_str!("fichas/instagram.json"),
    include_str!("fichas/airtable.json"),
    include_str!("fichas/atlassian.json"),
    include_str!("fichas/attio.json"),
    include_str!("fichas/brevo.json"),
    include_str!("fichas/browser-use.json"),
    include_str!("fichas/cal-com.json"),
    include_str!("fichas/calendly.json"),
    include_str!("fichas/camtasia.json"),
    include_str!("fichas/cloudflare.json"),
    include_str!("fichas/coda.json"),
    include_str!("fichas/descript.json"),
    include_str!("fichas/expo.json"),
    include_str!("fichas/fal.json"),
    include_str!("fichas/granola.json"),
    include_str!("fichas/higgsfield.json"),
    include_str!("fichas/jungle-scout.json"),
    include_str!("fichas/mem0.json"),
    include_str!("fichas/monday.json"),
    include_str!("fichas/notion.json"),
    include_str!("fichas/openrouter.json"),
    include_str!("fichas/paypal.json"),
    include_str!("fichas/pipedrive.json"),
    include_str!("fichas/plaud.json"),
    include_str!("fichas/railway.json"),
    include_str!("fichas/resend.json"),
    include_str!("fichas/semrush.json"),
    include_str!("fichas/sentry.json"),
    include_str!("fichas/stripe.json"),
    include_str!("fichas/tally.json"),
    include_str!("fichas/tiktok-ads.json"),
    include_str!("fichas/tinyfish.json"),
    include_str!("fichas/vidiq.json"),
    include_str!("fichas/wix.json"),
    include_str!("fichas/tavily.json"),
    include_str!("fichas/apify.json"),
    include_str!("fichas/heygen.json"),
    include_str!("fichas/trello.json"),
    include_str!("fichas/wrike.json"),
    include_str!("fichas/agentmail.json"),
    include_str!("fichas/bright-data.json"),
    include_str!("fichas/prisma.json"),
];

/// Cada string é vazada uma única vez na partida: o catálogo inteiro vive até o fim do processo.
fn fixa(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
fn fixos(v: Vec<String>) -> &'static [&'static str] {
    Box::leak(v.into_iter().map(fixa).collect::<Vec<_>>().into_boxed_slice())
}
fn lista<T>(v: Vec<T>) -> &'static [T] {
    Box::leak(v.into_boxed_slice())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServicoJson {
    id: String,
    nome: String,
    categoria: String,
    descricao: String,
    exemplos: Vec<String>,
    site: String,
    #[serde(default)]
    docs: Option<String>,
    links: LinksJson,
    conferida_em: String,
    fontes: Vec<String>,
    #[serde(default)]
    aviso: Option<String>,
    tipos: Vec<TipoJson>,
    #[serde(default)]
    skills: Vec<SkillJson>,
    #[serde(default)]
    skills_oficiais: Vec<SkillOficialJson>,
    #[serde(default)]
    em_breve: Option<String>,
}

/// Skill do catálogo curado que a ficha sugere depois de conectar (`id` é o da skill, conferido contra o catálogo).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillJson {
    id: String,
    titulo: String,
    resumo: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillOficialJson {
    url: String,
    origem: String,
    licenca: String,
    aproveitavel: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinksJson {
    privacidade: String,
    termos: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TipoJson {
    id: String,
    tipo: Tipo,
    rotulo: String,
    mantido_por: String,
    url: String,
    auth: AuthJson,
    grupos: Vec<GrupoJson>,
    #[serde(default)]
    perigosas: Vec<PerigosaJson>,
    recomendado: bool,
    disponivel: bool,
    #[serde(default)]
    motivo_indisponivel: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "modo", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
enum AuthJson {
    Oauth { registro: String },
    Chave { destino: String, nome: String, prefixo: Option<String>, como_criar_leitura: String },
    AppProprio { emissor: String, cadastro: String, recurso: String, autorizacao: String, token: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrupoJson {
    id: String,
    rotulo: String,
    escopos: Vec<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    ferramentas: Vec<String>,
    #[serde(default)]
    curadas: Vec<CuradaJson>,
    padrao: bool,
    disponivel: bool,
    #[serde(default)]
    imposicao: Option<Imposicao>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CuradaJson {
    ferramenta: String,
    digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PerigosaJson {
    ferramenta: String,
    rotulo: String,
}

impl ServicoJson {
    /// As regras que a ficha cumpre antes de entrar no catálogo (fail-closed: o motivo vai no erro, sem eco do arquivo).
    fn conferir(&self) -> Result<(), String> {
        if !CATEGORIAS.contains(&self.categoria.as_str()) {
            return Err(format!("categoria fora da lista fechada em `{}`", self.id));
        }
        let https = |url: &str| url.starts_with("https://");
        let links = [&self.site, &self.links.privacidade, &self.links.termos];
        let docs = self.docs.iter();
        let skills = self.skills_oficiais.iter().map(|s| &s.url);
        let tipos = self.tipos.iter().filter(|t| t.tipo != Tipo::Local).map(|t| &t.url);
        if !links.into_iter().chain(docs).chain(skills).chain(tipos).all(|url| https(url)) {
            return Err(format!("endereço sem https em `{}`", self.id));
        }
        if self.descricao.chars().count() > 140 {
            return Err(format!("descrição acima de 140 caracteres em `{}`", self.id));
        }
        let ids_das_skills: Vec<&str> = self.skills.iter().map(|s| s.id.as_str()).collect();
        let skills_ok =
            self.skills.iter().all(|s| !s.titulo.is_empty() && !s.resumo.is_empty() && s.resumo.chars().count() <= 160);
        let id_ok =
            |id: &&str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !skills_ok
            || !ids_das_skills.iter().all(id_ok)
            || ids_das_skills.iter().enumerate().any(|(i, id)| ids_das_skills[..i].contains(id))
        {
            return Err(format!("skill sugerida inválida em `{}`", self.id));
        }
        for tipo in &self.tipos {
            if tipo.tipo == Tipo::Local
                && (!tipo.url.is_empty()
                    || matches!(tipo.auth, AuthJson::Chave { .. })
                    || tipo.grupos.iter().any(|g| g.url.is_some() || g.ferramentas.is_empty())
                    || super::catalogo_local::da_ficha(&self.id, &tipo.id).is_none())
            {
                return Err("ficha MCP local sem artefato fixado ou com destino inválido".into());
            }
            if !tipo.id.starts_with(&self.id) {
                return Err(format!("tipo `{}` não pertence à ficha `{}`", tipo.id, self.id));
            }
            if let AuthJson::AppProprio { emissor, cadastro, recurso, autorizacao, token } = &tipo.auth {
                use super::oauth_urls;
                let seguro = |endereco: &String| {
                    endereco.starts_with("https://")
                        && !endereco.chars().any(|c| c.is_control() || c.is_whitespace())
                        && oauth_urls::conector(endereco).is_ok()
                };
                if self.id == "google"
                    || tipo.id.split_once('.').map(|(servico, _)| servico) != Some(self.id.as_str())
                    || self.tipos.iter().filter(|t| t.id == tipo.id).count() != 1
                    || ![emissor, cadastro, recurso, autorizacao, token].into_iter().all(seguro)
                    || emissor.contains('?')
                {
                    return Err("endereços inválidos no app OAuth próprio".into());
                }
            }
            let curada_invalida = |g: &GrupoJson| {
                g.curadas.iter().any(|c| {
                    !g.ferramentas.contains(&c.ferramenta)
                        || c.digest.len() != 64
                        || !c.digest.chars().all(|ch| matches!(ch, '0'..='9' | 'a'..='f'))
                })
            };
            if tipo.grupos.iter().any(curada_invalida) {
                return Err(format!(
                    "ferramenta curada fora da lista do grupo ou com digest inválido em `{}`",
                    tipo.id
                ));
            }
            for grupo in &tipo.grupos {
                if grupo.imposicao == Some(Imposicao::Filtro) && !grupo.curadas.is_empty() {
                    return Err(format!("grupo com imposição pelo filtro não admite curadoria em `{}`", tipo.id));
                }
                if tipo.disponivel && grupo.disponivel && grupo.imposicao.is_none() {
                    return Err(format!("grupo disponível sem imposição explícita em `{}`", tipo.id));
                }
            }
        }
        Ok(())
    }

    fn fixar(self) -> Servico {
        let id = fixa(self.id);
        Servico {
            id,
            marca: id,
            nome: fixa(self.nome),
            categoria: fixa(self.categoria),
            descricao: fixa(self.descricao),
            exemplos: fixos(self.exemplos),
            site: fixa(self.site),
            docs: self.docs.map(fixa),
            links: Links { privacidade: fixa(self.links.privacidade), termos: fixa(self.links.termos) },
            conferida_em: fixa(self.conferida_em),
            fontes: fixos(self.fontes),
            aviso: self.aviso.map(fixa),
            tipos: lista(self.tipos.into_iter().map(|tipo| tipo.fixar(id)).collect()),
            skills: lista(
                self.skills
                    .into_iter()
                    .map(|s| SkillRef { id: fixa(s.id), titulo: fixa(s.titulo), resumo: fixa(s.resumo) })
                    .collect(),
            ),
            skills_oficiais: lista(
                self.skills_oficiais
                    .into_iter()
                    .map(|s| SkillOficial {
                        url: fixa(s.url),
                        origem: fixa(s.origem),
                        licenca: fixa(s.licenca),
                        aproveitavel: s.aproveitavel,
                    })
                    .collect(),
            ),
            em_breve: self.em_breve.map(fixa),
        }
    }
}

impl TipoJson {
    fn fixar(self, servico: &'static str) -> TipoConexao {
        TipoConexao {
            id: fixa(self.id),
            tipo: self.tipo,
            rotulo: fixa(self.rotulo),
            mantido_por: fixa(self.mantido_por),
            url: fixa(self.url),
            auth: match self.auth {
                AuthJson::Oauth { registro } => Auth::Oauth { registro: fixa(registro) },
                AuthJson::AppProprio { emissor, cadastro, recurso, autorizacao, token } => Auth::AppProprio {
                    provedor: servico,
                    emissor: fixa(emissor),
                    guia: &[],
                    configuracao: Some(AppProprioConfig {
                        cadastro: fixa(cadastro),
                        recurso: fixa(recurso),
                        autorizacao: fixa(autorizacao),
                        token: fixa(token),
                    }),
                },
                AuthJson::Chave { destino, nome, prefixo, como_criar_leitura } => Auth::Chave {
                    destino: fixa(destino),
                    nome: fixa(nome),
                    prefixo: prefixo.map(fixa),
                    como_criar_leitura: fixa(como_criar_leitura),
                },
            },
            grupos: lista(
                self.grupos
                    .into_iter()
                    .map(|g| Grupo {
                        id: fixa(g.id),
                        rotulo: fixa(g.rotulo),
                        escopos: fixos(g.escopos),
                        url: g.url.map(fixa),
                        ferramentas: fixos(g.ferramentas),
                        curadas: lista(
                            g.curadas
                                .into_iter()
                                .map(|c| Curada { ferramenta: fixa(c.ferramenta), digest: fixa(c.digest) })
                                .collect(),
                        ),
                        padrao: g.padrao,
                        disponivel: g.disponivel,
                        imposicao: g.imposicao,
                    })
                    .collect(),
            ),
            perigosas: lista(
                self.perigosas
                    .into_iter()
                    .map(|p| Perigosa { ferramenta: fixa(p.ferramenta), rotulo: fixa(p.rotulo) })
                    .collect(),
            ),
            recomendado: self.recomendado,
            disponivel: self.disponivel,
            motivo_indisponivel: self.motivo_indisponivel.map(fixa),
        }
    }
}

/// `Err` com o motivo (sem eco do arquivo) se o JSON não for uma ficha válida.
pub(super) fn ler(json: &str) -> Result<Servico, serde_json::Error> {
    let ficha = serde_json::from_str::<ServicoJson>(json)?;
    ficha.conferir().map_err(<serde_json::Error as serde::de::Error>::custom)?;
    Ok(ficha.fixar())
}
