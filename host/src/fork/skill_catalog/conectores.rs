//! Skills que acompanham as fichas do Marketplace: texto passivo do repositório (`host/catalogo/skills`), empacotado
//! como qualquer skill do catálogo curado. Instalar e ligar em cada bot continua sendo gesto da pessoa; a skill não
//! concede conta, ferramenta nem permissão.
use super::registry::{Asset, Metadata, Origin, Package, hash};
use crate::fork::agent_skills;
use anyhow::{Context, Result};

/// Commit da Mexerica que tem, nestes mesmos caminhos, exatamente os bytes empacotados (`conectores_tests` confere o digest).
const REVISAO_PROPRIA: &str = "006c72e754a4b6af6904d866903a5ca705d24603";
const LICENCA_PROPRIA: &str = include_str!("../../../catalogo/skills/LICENSE-Mexerica");

struct Entrada {
    skill: &'static str,
    licenca_texto: &'static str,
    licenca: &'static str,
    /// `None`: texto próprio da Mexerica (a origem é a pasta no repositório).
    origem_externa: Option<(&'static str, &'static str, &'static str, &'static str)>,
    motivo: &'static str,
    perdas: &'static str,
}

const ENTRADAS: &[Entrada] = &[
    Entrada {
        skill: include_str!("../../../catalogo/skills/exa/exa-pesquisar-na-web/SKILL.md"),
        licenca_texto: include_str!("../../../catalogo/skills/exa/exa-pesquisar-na-web/LICENSE"),
        licenca: "MIT",
        origem_externa: Some((
            "exa-labs/exa-mcp-server",
            "a02cd621534313b8f01dbb03b829dac448decb8b",
            "skills/search",
            // SHA-256 do `SKILL.md` de origem nesse commit.
            "0aacd119c70b57144e7358c5829061a0d25a734ea29afb668392861250c9e42f",
        )),
        motivo: "Skill do conector Exa: ensina o bot a pesquisar na web com fontes e datas, só com busca e leitura de página.",
        perdas: "Adaptação curta em pt-BR da skill oficial da Exa; não inclui a pesquisa longa (agent_run), de propósito.",
    },
    Entrada {
        skill: include_str!("../../../catalogo/skills/supabase/supabase-revisar-seguranca/SKILL.md"),
        licenca_texto: include_str!("../../../catalogo/skills/supabase/supabase-revisar-seguranca/LICENSE"),
        licenca: "MIT",
        origem_externa: Some((
            "supabase/agent-skills",
            "c9be0e931b7930f7d02126d04774d904c381e7d7",
            "skills/supabase",
            // SHA-256 do `SKILL.md` de origem nesse commit.
            "697e0e949f97621c6fc1784169eb3ccbf673747d6bb8299a52962d3d84d93dc6",
        )),
        motivo: "Skill do conector Supabase: ensina o bot a revisar a segurança do projeto só por leitura.",
        perdas: "Adaptação curta em pt-BR da skill oficial da Supabase; o texto longo e os exemplos de código ficam de fora.",
    },
    Entrada {
        skill: include_str!("../../../catalogo/skills/linear/linear-triar-issues-novas/SKILL.md"),
        licenca_texto: LICENCA_PROPRIA,
        licenca: "Apache-2.0",
        origem_externa: None,
        motivo: "Skill do conector Linear: ensina o bot a triar as issues novas só por leitura.",
        perdas: "",
    },
    Entrada {
        skill: include_str!("../../../catalogo/skills/e-commerce/e-commerce-pedidos-e-anuncios-do-dia/SKILL.md"),
        licenca_texto: LICENCA_PROPRIA,
        licenca: "Apache-2.0",
        origem_externa: None,
        motivo: "Skill do conector de e-commerce: ensina o bot a resumir pedidos e anúncios do dia só por leitura.",
        perdas: "",
    },
];

const PASTAS: &[&str] = &[
    "exa/exa-pesquisar-na-web",
    "supabase/supabase-revisar-seguranca",
    "linear/linear-triar-issues-novas",
    "e-commerce/e-commerce-pedidos-e-anuncios-do-dia",
];

fn asset(path: &str, content: &str) -> Asset {
    Asset {
        path: path.into(),
        bytes: content.len(),
        sha256: hash(content.as_bytes()),
        mode: "100644".into(),
        content: content.into(),
    }
}

pub(super) fn packages() -> Result<Vec<Package>> {
    ENTRADAS
        .iter()
        .zip(PASTAS)
        .map(|(entrada, pasta)| {
            let nome = pasta.rsplit('/').next().context("pasta sem nome")?;
            let documento = agent_skills::parse(entrada.skill, nome)?;
            let mut assets = vec![asset("SKILL.md", entrada.skill), asset("LICENSE", entrada.licenca_texto)];
            let (origem, adaptada) = match entrada.origem_externa {
                Some((repo, revisao, caminho, digest)) => (
                    Origin { repo: repo.into(), revision: revisao.into(), path: caminho.into(), source_digest: digest.into() },
                    true,
                ),
                None => (
                    Origin {
                        repo: "example-org/mexerica".into(),
                        revision: REVISAO_PROPRIA.into(),
                        path: format!("host/catalogo/skills/{pasta}"),
                        source_digest: hash(entrada.skill.as_bytes()),
                    },
                    false,
                ),
            };
            if let Some((repo, revisao, caminho, _)) = entrada.origem_externa {
                let texto = format!(
                    "# Adaptação\n\nTexto próprio em pt-BR, escrito a partir de `{repo}` (`{caminho}`, revisão `{revisao}`).\n\n{}\n\nA skill é só texto: não concede conta, ferramenta nem permissão.\n",
                    entrada.perdas
                );
                assets.push(asset("ADAPTATION.md", &texto));
            }
            let mut package = Package {
                metadata: Metadata {
                    id: nome.into(),
                    source: nome.into(),
                    name: documento.name,
                    description: documento.description,
                    category: "Conectores".into(),
                    aliases: Vec::new(),
                    origin: origem,
                    license: entrada.licenca.into(),
                    availability: if adaptada { "adapted" } else { "text" }.into(),
                    reason: entrada.motivo.into(),
                    requirements: vec![
                        "Precisa do conector do serviço conectado e ligado ao bot; a skill não concede conta, ferramenta nem permissão.".into(),
                    ],
                    losses: if adaptada { vec![entrada.perdas.into()] } else { Vec::new() },
                    adaptation: adaptada.then(|| "mexerica-conector-v1".into()),
                    readiness: None,
                    files: assets.iter().map(Asset::info).collect(),
                    digest: String::new(),
                },
                install_id: nome.into(),
                assets,
            };
            package.metadata.digest = package.envelope_digest()?;
            package.validate()?;
            Ok(package)
        })
        .collect()
}
