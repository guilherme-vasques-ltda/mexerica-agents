//! As skills que acompanham as fichas do Marketplace são pacotes comuns do catálogo curado, com a licença junto.
use super::registry;
use std::path::Path;

fn pacotes() -> Vec<registry::Package> {
    registry::packages().unwrap().into_iter().filter(|p| p.metadata.category == "Conectores").collect()
}

#[test]
fn skills_de_conectores_sao_pacotes_validos_com_licenca_e_descricao_de_uso() {
    let pacotes = pacotes();
    assert_eq!(pacotes.len(), 4);
    let raiz = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../LICENSE")).unwrap();
    for p in &pacotes {
        p.validate().unwrap();
        let m = &p.metadata;
        assert!(m.description.starts_with("Use ao"), "{}: a descrição diz quando usar", m.id);
        assert!(m.description.contains("não use"), "{}: a descrição diz quando não usar", m.id);
        let licenca = p.assets.iter().find(|a| a.path == "LICENSE").unwrap();
        if m.license == "Apache-2.0" {
            assert_eq!(licenca.content, raiz, "{}: o aviso Apache é o LICENSE do repositório", m.id);
        } else {
            assert!(licenca.content.contains("MIT License") && licenca.content.contains("Copyright"), "{}", m.id);
        }
        let texto = p
            .assets
            .iter()
            .find(|a| a.path == "SKILL.md")
            .unwrap()
            .content
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(texto.contains("não concede"), "{}: a skill diz que não concede acesso", m.id);
        assert!(!texto.contains("http://") && !texto.contains("https://"), "{}: sem endereço no texto", m.id);
    }
}

#[test]
fn skill_do_supabase_so_consulta_e_a_da_exa_nao_inicia_pesquisa_cobrada() {
    let texto =
        |id: &str| registry::find(id).unwrap().assets.iter().find(|a| a.path == "SKILL.md").unwrap().content.clone();
    let supabase = texto("supabase-revisar-seguranca");
    assert!(supabase.contains("get_advisors") && supabase.contains("SELECT") && supabase.contains("Nunca"));
    let exa = texto("exa-pesquisar-na-web");
    assert!(exa.contains("web_search_exa") && exa.contains("web_fetch_exa") && exa.contains("Não use `agent_run`"));
}

/// Origem das skills próprias: o commit tem, nestes caminhos, os bytes empacotados (digest conferido contra `git show` ao fixar).
#[test]
fn origem_das_skills_proprias_e_o_commit_que_tem_os_bytes_empacotados() {
    const REVISAO: &str = "006c72e754a4b6af6904d866903a5ca705d24603";
    for (id, pasta, digest) in [
        (
            "linear-triar-issues-novas",
            "host/catalogo/skills/linear/linear-triar-issues-novas",
            "ecd1e8f0b258f0b50717592aa8706aa606dec0a2cf9bc70ad935236b314fe41d",
        ),
        (
            "e-commerce-pedidos-e-anuncios-do-dia",
            "host/catalogo/skills/e-commerce/e-commerce-pedidos-e-anuncios-do-dia",
            "6cb4748cb4fe0b65cea42f74adea6cb93e961546052ce97252ecafe8c6b00b8b",
        ),
    ] {
        let p = registry::find(id).unwrap();
        let origem = &p.metadata.origin;
        assert_eq!(origem.repo, "example-org/mexerica", "{id}");
        assert_eq!(origem.revision, REVISAO, "{id}");
        assert_eq!(origem.path, pasta, "{id}");
        assert_eq!(origem.source_digest, digest, "{id}: o digest é o do SKILL.md empacotado");
        let skill = p.assets.iter().find(|a| a.path == "SKILL.md").unwrap();
        assert_eq!(registry::hash(skill.content.as_bytes()), digest, "{id}");
    }
}

/// Varredura offline (sem rede, sem executar nada) do que as skills de conectores carregam: só texto de três arquivos,
/// sem código, endereço, comando, segredo ou frase de injeção; só ferramentas de leitura conhecidas, e as de escrita
/// só aparecem proibidas. Roda sobre os bytes empacotados, que são os distribuídos.
#[test]
fn varredura_offline_das_skills_de_conectores_nao_acha_nada_perigoso() {
    const FERRAMENTAS_CONHECIDAS: &[&str] = &[
        "web_search_exa",
        "web_fetch_exa",
        "get_advisors",
        "list_tables",
        "search_docs",
        "execute_sql",
        "list_issues",
        "get_issue",
        "list_comments",
        "list_teams",
        "list_issue_statuses",
        "LICENSE",
        "THIRD_PARTY.md",
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "agent_run",
        "ADAPTATION.md",
        "SKILL.md",
    ];
    const SO_PROIBIDAS: &[&str] = &["agent_run", "INSERT", "UPDATE", "DELETE"];
    const PADROES_PROIBIDOS: &[&str] = &[
        "```",
        "://",
        "www.",
        "@",
        "/home/",
        "~/",
        "curl ",
        "wget ",
        "sudo ",
        "bash ",
        "npx ",
        "pip install",
        "eval(",
        "<script",
        "sk-",
        "ghp_",
        "eyJ",
        "-----BEGIN",
        "Bearer ",
        "allowed-tools",
        "ignore as instru",
        "ignore previous",
        "desconsidere as instru",
        "esqueça as instru",
        "system prompt",
        "prompt do sistema",
    ];
    let pacotes = pacotes();
    assert_eq!(pacotes.len(), 4);
    for p in &pacotes {
        let id = &p.metadata.id;
        for asset in &p.assets {
            assert!(
                ["SKILL.md", "LICENSE", "ADAPTATION.md"].contains(&asset.path.as_str()),
                "{id}: arquivo `{}` fora do recorte",
                asset.path
            );
            if asset.path == "LICENSE" {
                continue;
            }
            let texto = &asset.content;
            let minusculo = texto.to_lowercase();
            for padrao in PADROES_PROIBIDOS {
                assert!(!minusculo.contains(&padrao.to_lowercase()), "{id}/{}: padrão proibido `{padrao}`", asset.path);
            }
            for (i, trecho) in texto.split('`').enumerate() {
                if i % 2 == 0 || asset.path != "SKILL.md" {
                    continue;
                }
                assert!(
                    FERRAMENTAS_CONHECIDAS.contains(&trecho),
                    "{id}/{}: `{trecho}` não é uma ferramenta conhecida",
                    asset.path
                );
            }
            for proibida in SO_PROIBIDAS {
                for (posicao, _) in texto.match_indices(proibida) {
                    let antes: String =
                        texto[..posicao].chars().rev().take(120).collect::<Vec<_>>().into_iter().rev().collect();
                    let antes = antes.to_lowercase();
                    assert!(
                        antes.contains("não ") || antes.contains("nunca"),
                        "{id}/{}: `{proibida}` só pode aparecer proibida",
                        asset.path
                    );
                }
            }
        }
    }
}
