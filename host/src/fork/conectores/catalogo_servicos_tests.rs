//! Marketplace 1a: o catálogo é dado sem segredo, as fichas têm conferência em dia e as liberações vêm só da ficha.
use super::catalogo_fichas::servicos;
use super::catalogo_json::{FICHA_DONO_A, FICHAS, ler};
use super::catalogo_servicos::{
    Auth, Curada, Grupo, Imposicao, PRAZO_CONFERENCIA_DIAS, Tipo, TipoConexao, dias_desde_conferencia,
    filtrar_ferramentas, hoje, servico,
};
use chrono::NaiveDate;
use serde_json::json;

fn dia(texto: &str) -> NaiveDate {
    NaiveDate::parse_from_str(texto, "%Y-%m-%d").unwrap()
}

/// Falha no CI quando alguma ficha passa de 90 dias sem conferência (decisão do dono, 16/11/2026).
#[test]
fn nenhuma_ficha_esta_com_a_conferencia_vencida() {
    let hoje = hoje();
    for ficha in servicos() {
        let dias = dias_desde_conferencia(ficha.conferida_em, hoje);
        assert!(
            (0..=PRAZO_CONFERENCIA_DIAS).contains(&dias),
            "ficha `{}` conferida em {} ({dias} dias): reconfira as fontes e atualize `conferida_em`",
            ficha.id,
            ficha.conferida_em
        );
        assert!(ficha.conferencia_vigente(hoje));
    }
}

#[test]
fn a_regra_dos_90_dias_reprova_no_91o_dia_e_data_ilegivel_ou_futura() {
    let linear = servico("linear").unwrap();
    let conferida = dia(linear.conferida_em);
    let nos_90 = conferida + chrono::Days::new(90);
    assert!(linear.conferencia_vigente(nos_90));
    assert!(!linear.conferencia_vigente(nos_90 + chrono::Days::new(1)));
    assert!(!linear.conferencia_vigente(conferida - chrono::Days::new(1)), "data futura não vale");
    assert_eq!(dias_desde_conferencia("16/11/2026", conferida), i64::MAX, "data ilegível conta como vencida");
    let vencida = nos_90 + chrono::Days::new(1);
    let erro = linear.exigir_conectavel("linear.oficial_oauth", vencida).unwrap_err().to_string();
    assert!(erro.starts_with("ficha_vencida"), "{erro}");
    assert!(linear.exigir_conectavel("linear.oficial_oauth", conferida).is_ok());
}

#[test]
fn o_catalogo_nao_tem_segredo_nem_duplicata_e_as_fichas_tem_fonte_oficial() {
    // `"prefixo":"Bearer "` é só o rótulo do cabeçalho (o app junta a chave da pessoa); qualquer outro `Bearer ` seria token.
    let json =
        serde_json::to_string(&super::catalogo_servicos::publico()).unwrap().replace("\"prefixo\":\"Bearer \"", "");
    for proibido in ["client_secret", "access_token", "refresh_token", "Bearer ", "password", "api_key=", "sk_live"] {
        assert!(!json.contains(proibido), "catálogo não pode citar `{proibido}`");
    }
    let mut ids = servicos().iter().map(|s| s.id).collect::<Vec<_>>();
    ids.sort_unstable();
    assert!(ids.windows(2).all(|par| par[0] != par[1]), "ids de serviço repetidos");
    for ficha in servicos() {
        assert!(!ficha.fontes.is_empty() && !ficha.exemplos.is_empty() && !ficha.descricao.is_empty(), "{}", ficha.id);
        assert!(ficha.links.privacidade.starts_with("https://") && ficha.links.termos.starts_with("https://"));
        assert!(ficha.fontes.iter().all(|f| f.starts_with("https://") || f.starts_with("docs/")));
        for tipo in ficha.tipos {
            assert!(tipo.id.starts_with(ficha.id) && tipo.url.starts_with("https://"), "{}", tipo.id);
            assert!(tipo.grupos.iter().any(|g| g.padrao), "{} sem grupo padrão", tipo.id);
            let ids: Vec<_> = tipo.grupos.iter().map(|g| g.id).collect();
            assert_eq!(ids.iter().collect::<std::collections::BTreeSet<_>>().len(), ids.len());
            if let Auth::Oauth { registro } = &tipo.auth {
                assert_eq!(*registro, "dcr", "o Hub só se cadastra por DCR hoje");
            }
        }
        // Tipo conectável precisa de um grupo padrão realmente disponível: a ficha nunca nasce sem liberação.
        for tipo in ficha.tipos.iter().filter(|t| t.conectavel()) {
            assert!(tipo.grupos.iter().any(|g| g.padrao && g.disponivel), "{}", tipo.id);
        }
        assert_eq!(ficha.conectavel(), ficha.tipos.iter().any(TipoConexao::conectavel));
        assert!(ficha.conectavel() || ficha.em_breve.is_some(), "{} não conecta e não diz por quê", ficha.id);
    }
}

/// As fichas conectáveis além da do dono A, na ordem do catálogo. Entrar aqui é decisão consciente.
const CONECTAVEIS: &[&str] = &[
    "linear",
    "context7",
    "neon",
    "wispr-flow",
    "github",
    "better-stack",
    "clerk",
    "coderabbit",
    "posthog",
    "revenuecat",
    "render",
    "tavily",
    "apify",
    "heygen",
    "trello",
];

/// Id da ficha do dono A, lido do próprio arquivo JSON (nenhum nome de produto fora dele).
fn id_dono_a() -> String {
    serde_json::from_str::<serde_json::Value>(FICHA_DONO_A).unwrap()["id"].as_str().unwrap().to_owned()
}

#[test]
fn so_conectam_as_fichas_listadas() {
    let conectaveis: Vec<_> = servicos().iter().filter(|s| s.conectavel()).map(|s| s.id.to_owned()).collect();
    let mut esperadas = vec![id_dono_a()];
    esperadas.extend(CONECTAVEIS.iter().map(|s| (*s).to_owned()));
    // O Google direto (app OAuth da própria pessoa) vem depois das fichas em JSON; a leitura é imposta pelo escopo.
    esperadas.extend(GOOGLE.iter().map(|s| (*s).to_owned()));
    esperadas.extend(GOOGLE_NOVOS.iter().map(|(id, ..)| (*id).to_owned()));
    assert_eq!(conectaveis, esperadas, "ficha nova só conecta de propósito: inclua o id em CONECTAVEIS");
    for id in ["outlook", "slack", "instagram"] {
        let ficha = servico(id).unwrap();
        assert!(ficha.tipos.is_empty() && ficha.em_breve.is_some(), "{id} deve ser só \"em breve\"");
    }
    // Composio: ficha comum, mas sem modo só leitura no fornecedor, então indisponível.
    let composio = servico("composio").unwrap();
    assert_eq!(composio.categoria, "Agregadores");
    assert!(!composio.conectavel());
    assert_eq!(composio.tipos[0].url, "https://connect.composio.dev/mcp");
    assert!(composio.exigir_conectavel("composio.oficial_oauth", dia(composio.conferida_em)).is_err());
    assert!(composio.aviso.is_some_and(|a| a.contains("passam pelo Composio")));
    let linear = servico("linear").unwrap().tipo("linear.oficial_oauth").unwrap();
    assert_eq!(linear.tipo, Tipo::OficialOauth);
    assert_eq!(linear.url, "https://mcp.linear.app/mcp/readonly");
    assert!(!linear.grupos.iter().find(|g| g.id == "criar_editar").unwrap().disponivel);
}

#[test]
fn liberacoes_vem_so_da_ficha_e_o_padrao_do_linear_e_ler() {
    let linear = servico("linear").unwrap().tipo("linear.oficial_oauth").unwrap();
    assert_eq!(linear.liberacoes_validas(None).unwrap(), ["ler"]);
    assert_eq!(linear.liberacoes_validas(Some(&["ler".into(), "ler".into()])).unwrap(), ["ler"]);
    for ruim in
        [vec!["criar_editar".to_owned()], vec!["ler".into(), "criar_editar".into()], vec!["admin".into()], vec![]]
    {
        assert!(linear.liberacoes_validas(Some(&ruim)).is_err(), "{ruim:?}");
    }
    let ler = vec!["ler".to_owned()];
    assert_eq!(linear.url_efetiva(&ler), "https://mcp.linear.app/mcp/readonly");
    assert_eq!(linear.escopos_efetivos(&ler), ["read"], "só o escopo do grupo, nunca os do fornecedor inteiro");
    assert!(linear.ferramentas_permitidas(&ler).is_none(), "leitura imposta pelo fornecedor, sem lista do Hub");
}

static GRUPOS: &[Grupo] = &[
    Grupo {
        id: "ler",
        rotulo: "Ler",
        escopos: &["a:read"],
        url: None,
        ferramentas: &["listar", "ver"],
        curadas: &[],
        padrao: true,
        disponivel: true,
        imposicao: None,
    },
    Grupo {
        id: "cobrar",
        rotulo: "Cobrar",
        escopos: &["a:read", "a:write"],
        url: Some("https://mcp.exemplo.invalid/escrita"),
        ferramentas: &["cobrar"],
        curadas: &[],
        padrao: false,
        disponivel: true,
        imposicao: None,
    },
    Grupo {
        id: "sem_lista",
        rotulo: "Sem lista",
        escopos: &["b:read"],
        url: None,
        ferramentas: &[],
        curadas: &[],
        padrao: false,
        disponivel: true,
        imposicao: None,
    },
];
static TIPO_TESTE: TipoConexao = TipoConexao {
    id: "teste.oficial_chave",
    tipo: Tipo::OficialChave,
    rotulo: "Teste",
    mantido_por: "Teste",
    url: "https://mcp.exemplo.invalid/mcp",
    auth: Auth::Chave { destino: "header", nome: "Authorization", prefixo: None, como_criar_leitura: "texto" },
    grupos: GRUPOS,
    perigosas: &[],
    recomendado: false,
    disponivel: true,
    motivo_indisponivel: None,
};

#[test]
fn escopos_url_e_ferramentas_seguem_exatamente_os_grupos_ligados() {
    let ids = |lista: &[&str]| lista.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let ler = ids(&["ler"]);
    assert_eq!(TIPO_TESTE.escopos_efetivos(&ler), ["a:read"]);
    assert_eq!(TIPO_TESTE.url_efetiva(&ler), "https://mcp.exemplo.invalid/mcp");
    let ler_e_cobrar = ids(&["ler", "cobrar"]);
    assert_eq!(TIPO_TESTE.escopos_efetivos(&ler_e_cobrar), ["a:read", "a:write"], "união sem repetir");
    assert_eq!(TIPO_TESTE.url_efetiva(&ler_e_cobrar), "https://mcp.exemplo.invalid/escrita");
    assert_eq!(TIPO_TESTE.ferramentas_permitidas(&ler).unwrap().into_iter().collect::<Vec<_>>(), ["listar", "ver"]);
    assert_eq!(
        TIPO_TESTE.liberacoes_validas(Some(&ids(&["cobrar", "ler"]))).unwrap(),
        ["ler", "cobrar"],
        "ordem da ficha"
    );
    // Grupo sem lista misturado a grupo com lista não abre nada sozinho (fail-closed).
    let mista = ids(&["ler", "sem_lista"]);
    assert_eq!(TIPO_TESTE.ferramentas_permitidas(&mista).unwrap().into_iter().collect::<Vec<_>>(), ["listar", "ver"]);
    assert!(TIPO_TESTE.ferramentas_permitidas(&ids(&["sem_lista"])).is_none());
}

#[test]
fn o_filtro_deixa_passar_so_as_ferramentas_dos_grupos_ligados() {
    let tools = vec![json!({"name":"listar"}), json!({"name":"cobrar"}), json!({"name":"ver"}), json!({"semNome":1})];
    let ler = vec!["ler".to_owned()];
    let permitidas = TIPO_TESTE.ferramentas_permitidas(&ler).unwrap();
    let nomes: Vec<_> = filtrar_ferramentas(&tools, Some(&permitidas)).iter().map(|t| t["name"].clone()).collect();
    assert_eq!(nomes, [json!("listar"), json!("ver")]);
    assert_eq!(filtrar_ferramentas(&tools, None).len(), 4, "sem lista, o fornecedor já impôs o limite");
}

#[test]
fn ficha_do_dono_a_vem_do_arquivo_e_so_libera_leitura_por_escopo() {
    let ficha = ler(FICHA_DONO_A).expect("o arquivo da ficha do dono A precisa ser uma ficha válida");
    let no_catalogo = servico(ficha.id).expect("a ficha do dono A está no catálogo");
    assert_eq!(servicos()[0].id, ficha.id, "a ficha do dono A vai primeiro");
    assert_eq!(no_catalogo.nome, ficha.nome);
    let tipo = &ficha.tipos[0];
    assert!(matches!(tipo.auth, Auth::Oauth { registro: "dcr" }));
    assert!(tipo.url.starts_with("https://") && tipo.url.ends_with("/api/mcp"), "endereço oficial fixo");
    let ler_g = tipo.grupos.iter().find(|g| g.id == "ler").unwrap();
    let escrever = tipo.grupos.iter().find(|g| g.id == "escrever").unwrap();
    assert!(ler_g.padrao && ler_g.disponivel && !escrever.padrao && !escrever.disponivel);
    // Ler: só escopos `:read`. Escrever (indisponível até a fatia 2): só `:write` e `:invoke`. Nenhum escopo nos dois.
    assert!(!ler_g.escopos.is_empty() && ler_g.escopos.iter().all(|e| e.ends_with(":read")));
    assert!(
        !escrever.escopos.is_empty()
            && escrever.escopos.iter().all(|e| e.ends_with(":write") || e.ends_with(":invoke"))
    );
    assert!(ler_g.escopos.iter().all(|e| !escrever.escopos.contains(e)));
    // O padrão e o login pedem só leitura; escrita é recusada.
    assert_eq!(tipo.liberacoes_validas(None).unwrap(), ["ler"]);
    assert!(tipo.liberacoes_validas(Some(&["escrever".to_owned()])).is_err());
    assert!(tipo.liberacoes_validas(Some(&["ler".to_owned(), "escrever".to_owned()])).is_err());
    let ler_ids = vec!["ler".to_owned()];
    assert_eq!(tipo.escopos_efetivos(&ler_ids), ler_g.escopos);
    assert_eq!(tipo.url_efetiva(&ler_ids), tipo.url);
    assert!(tipo.ferramentas_permitidas(&ler_ids).is_none(), "o limite é imposto pelo fornecedor, por escopo");
    // Conecta pela ficha vigente.
    assert!(no_catalogo.exigir_conectavel(tipo.id, dia(no_catalogo.conferida_em)).is_ok());
}

#[test]
fn json_de_ficha_invalido_ou_com_campo_desconhecido_nao_entra() {
    let mut v: serde_json::Value = serde_json::from_str(FICHA_DONO_A).unwrap();
    assert!(ler(&v.to_string()).is_ok());
    v["campoNovo"] = json!(1);
    assert!(ler(&v.to_string()).is_err(), "campo desconhecido é recusado");
    v.as_object_mut().unwrap().remove("campoNovo");
    v["tipos"][0]["tipo"] = json!("inventado");
    assert!(ler(&v.to_string()).is_err());
    // Campo desconhecido dentro de `auth` e de `links` também tira a ficha (ex.: um segredo colado por engano).
    let mut base: serde_json::Value = serde_json::from_str(FICHA_DONO_A).unwrap();
    base["tipos"][0]["auth"]["clientSecret"] = json!("x");
    assert!(ler(&base.to_string()).is_err(), "campo desconhecido em auth");
    let mut base: serde_json::Value = serde_json::from_str(FICHA_DONO_A).unwrap();
    base["links"]["extra"] = json!("https://x.invalid");
    assert!(ler(&base.to_string()).is_err(), "campo desconhecido em links");
    // O modo só aceita os seus campos: oauth com campo de chave, ou chave sem os seus, recusa.
    let mut base: serde_json::Value = serde_json::from_str(FICHA_DONO_A).unwrap();
    base["tipos"][0]["auth"]["destino"] = json!("header");
    assert!(ler(&base.to_string()).is_err(), "oauth com campo de chave");
    let mut base: serde_json::Value = serde_json::from_str(FICHA_DONO_A).unwrap();
    base["tipos"][0]["auth"] = json!({"modo": "chave", "registro": "dcr"});
    assert!(ler(&base.to_string()).is_err(), "chave sem os seus campos");
    assert!(ler("{").is_err());
}

/// A logo de cada serviço é a chave do arquivo no app: o id nas fichas fixas; na ficha em JSON, a chave própria do
/// arquivo (o nome do produto só pode aparecer dentro do JSON). Nenhum serviço fica sem marca.
#[test]
fn cada_servico_traz_a_chave_da_sua_logo() {
    for ficha in servicos() {
        assert!(!ficha.marca.is_empty(), "{}", ficha.id);
        assert!(
            ficha.marca.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{}",
            ficha.marca
        );
    }
    assert_eq!(servico("linear").unwrap().marca, "linear");
    assert_eq!(servico("google-drive").unwrap().marca, "google-drive");
    assert_eq!(servicos()[0].marca, "dono-a", "a ficha em JSON vem primeiro e usa a chave do próprio arquivo");
    let json = serde_json::to_value(super::catalogo_servicos::publico()).unwrap();
    assert!(json["servicos"].as_array().unwrap().iter().all(|s| s["marca"].is_string()));
}

#[test]
fn todo_grupo_disponivel_declara_a_contencao() {
    const ESCRITA: &[&str] = &["write", "delete", "invoke", "manage", "admin", "send", "create", "edit", "update"];
    // "Por natureza" é uma afirmação sobre o que a conta recebe depois do login, que o Hub não filtra. Só entra quem tem a
    // documentação do fornecedor conferida (Context7: duas ferramentas de consulta; Wispr Flow: só lê reuniões). O Exa
    // ficou de fora: o login libera também uma ferramenta que inicia trabalho cobrado.
    const NATUREZA_CONFERIDA: &[&str] = &["context7", "wispr-flow"];
    for ficha in servicos() {
        assert!(super::catalogo_servicos::CATEGORIAS.contains(&ficha.categoria), "{}: {}", ficha.id, ficha.categoria);
        for tipo in ficha.tipos.iter().filter(|t| t.disponivel) {
            for grupo in tipo.grupos.iter().filter(|g| g.disponivel) {
                let imposicao = grupo.imposicao.unwrap_or_else(|| panic!("{}/{} sem imposição", tipo.id, grupo.id));
                if imposicao == Imposicao::Filtro {
                    assert!(grupo.curadas.is_empty(), "{}/{}: o filtro não admite curadoria", tipo.id, grupo.id);
                    continue;
                }
                if imposicao == Imposicao::Escopo {
                    assert!(!grupo.escopos.is_empty(), "{}/{}: imposição por escopo sem escopos", tipo.id, grupo.id);
                }
                if imposicao == Imposicao::Natureza {
                    assert!(
                        NATUREZA_CONFERIDA.contains(&ficha.id),
                        "{}: leitura \"por natureza\" sem conferência da documentação",
                        ficha.id
                    );
                }
                if imposicao == Imposicao::Url {
                    let url = grupo.url.unwrap_or(tipo.url).to_ascii_lowercase();
                    assert!(url.contains("read"), "{}/{}: a URL não diz que é só de leitura", tipo.id, grupo.id);
                }
                for escopo in grupo.escopos {
                    let escopo = escopo.to_ascii_lowercase();
                    assert!(
                        !escopo.split([':', '.', '_', '/', '-']).any(|parte| ESCRITA.contains(&parte)),
                        "{}/{}: escopo de escrita `{escopo}` em grupo disponível",
                        tipo.id,
                        grupo.id
                    );
                }
            }
        }
    }
}

/// Cada arquivo de `fichas/` está em `FICHAS` (senão a ficha some do catálogo sem ninguém ver) e é uma ficha válida.
#[test]
fn todo_arquivo_de_fichas_esta_na_lista_e_e_valido() {
    let pasta = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fork/conectores/fichas");
    let mut arquivos = 0;
    for entrada in std::fs::read_dir(pasta).unwrap() {
        let caminho = entrada.unwrap().path();
        if caminho.extension().is_none_or(|e| e != "json") {
            continue;
        }
        arquivos += 1;
        let conteudo = std::fs::read_to_string(&caminho).unwrap();
        assert!(FICHAS.contains(&conteudo.as_str()), "{} não está em FICHAS", caminho.display());
        ler(&conteudo).unwrap_or_else(|e| panic!("{}: {e}", caminho.display()));
    }
    assert_eq!(arquivos, FICHAS.len());
    let google = super::catalogo_google::FICHAS.len();
    assert_eq!(servicos().len(), FICHAS.len() + google, "alguma ficha foi recusada ao carregar (veja o log)");
}

#[test]
fn ficha_fora_das_regras_do_catalogo_nao_entra() {
    let base: serde_json::Value = serde_json::from_str(FICHA_DONO_A).unwrap();
    let recusa = |ajuste: &dyn Fn(&mut serde_json::Value), motivo: &str| {
        let mut v = base.clone();
        ajuste(&mut v);
        let erro = ler(&v.to_string()).err().unwrap_or_else(|| panic!("{motivo}: deveria recusar")).to_string();
        assert!(!erro.is_empty(), "{motivo}");
    };
    recusa(&|v| v["categoria"] = json!("Qualquer coisa"), "categoria fora da lista");
    recusa(&|v| v["descricao"] = json!("x".repeat(141)), "descrição longa");
    recusa(&|v| v["site"] = json!("http://x.invalid"), "site sem https");
    recusa(&|v| v["docs"] = json!("ftp://x.invalid"), "docs sem https");
    recusa(
        &|v| {
            v["skillsOficiais"] =
                json!([{"url": "http://x.invalid", "origem": "fornecedor", "licenca": "MIT", "aproveitavel": true}]);
        },
        "skill sem https",
    );
    recusa(
        &|v| {
            v["tipos"][0]["grupos"][0].as_object_mut().unwrap().remove("imposicao");
        },
        "grupo disponível sem imposição",
    );
    recusa(&|v| v["tipos"][0]["id"] = json!("outra.oficial_oauth"), "tipo de outra ficha");
    let mut v = base.clone();
    v["tipos"][0]["grupos"][0]["disponivel"] = json!(false);
    v["tipos"][0]["grupos"][0].as_object_mut().unwrap().remove("imposicao");
    assert!(ler(&v.to_string()).is_ok());
}

#[test]
fn acesso_de_escrita_pelo_filtro_exige_declaracao_e_nao_admite_curadoria() {
    let ficha = super::catalogo_servicos::fixture_filtro();
    let tipo = &ficha.tipos[0];
    let acoes = vec!["acoes".to_owned()];
    assert!(tipo.conectavel());
    assert_eq!(tipo.escopos_efetivos(&acoes), ["items:write"]);
    assert_eq!(serde_json::to_value(tipo).unwrap()["grupos"][0]["imposicao"], "filtro");

    let ler_so = vec!["ler".to_owned()];
    assert!(!tipo.ferramentas_curadas(&ler_so).is_empty());
    assert!(tipo.ferramentas_curadas(&[acoes, ler_so].concat()).is_empty());

    let mut v: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/catalogo-filtro.json")).unwrap();
    for imposicao in [serde_json::Value::Null, json!("desconhecida")] {
        v["tipos"][0]["grupos"][0]["imposicao"] = imposicao;
        assert!(ler(&v.to_string()).is_err());
    }
    v["tipos"][0]["grupos"][0]["imposicao"] = json!("filtro");
    let digest = "b".repeat(64);
    for ferramenta in ["list_items", "create_item"] {
        v["tipos"][0]["grupos"][0]["curadas"] = json!([{"ferramenta": ferramenta, "digest": digest}]);
        assert!(ler(&v.to_string()).unwrap_err().to_string().contains("não admite curadoria"));
    }
}

// MARK: Google direto (app OAuth da própria pessoa)

const GOOGLE: [&str; 5] = ["gmail", "google-agenda", "google-drive", "google-docs", "google-planilhas"];

#[test]
fn as_cinco_fichas_do_google_usam_o_app_da_pessoa_e_so_pedem_escopos_do_proprio_servico() {
    for id in GOOGLE {
        let ficha = servico(id).unwrap();
        assert!(ficha.conectavel() && ficha.em_breve.is_none(), "{id}");
        assert!(ficha.aviso.is_some_and(|a| a.contains("teste externo") && a.contains("7 dias")), "{id}");
        assert!(ficha.fontes.iter().any(|f| f.contains("configure-mcp-server")), "{id} cita o guia oficial");
        let [tipo, anterior] = ficha.tipos else { panic!("{id}: modo compatível e modo anterior") };
        assert_eq!(tipo.id, format!("{id}.oficial_app_self"));
        assert!(tipo.recomendado);
        assert_eq!(anterior.id, format!("{id}.oficial_app"));
        assert!(!anterior.recomendado);
        assert_eq!(tipo.url.strip_suffix("self"), Some(anterior.url));
        let padroes = anterior.liberacoes_validas(None).unwrap();
        assert_eq!(tipo.escopos_efetivos(&padroes), anterior.escopos_efetivos(&padroes));
        assert_eq!(tipo.tipo, Tipo::OficialApp);
        assert_eq!(tipo.mantido_por, "Google");
        let Auth::AppProprio { provedor, emissor, guia, configuracao: None } = &tipo.auth else {
            panic!("{id}: app próprio")
        };
        assert_eq!((*provedor, *emissor), ("google", "https://accounts.google.com"));
        let passos: Vec<_> = guia.iter().map(|p| p.id).collect();
        assert_eq!(passos, ["projeto", "apis", "tela_login", "escopos", "cliente", "campos"]);
        assert!(guia.iter().filter_map(|p| p.url).all(|u| u.starts_with("https://")));
        // Padrão seguro: só ler. Tudo que muda algo fica desligado até a pessoa ligar.
        let padrao: Vec<_> = tipo.grupos.iter().filter(|g| g.padrao).map(|g| g.id).collect();
        assert_eq!(padrao, ["ler"], "{id}");
        assert!(
            tipo.grupos.iter().all(|g| g.escopos.iter().all(|e| e.starts_with("https://www.googleapis.com/auth/")))
        );
    }
}

#[test]
fn escopos_minimos_por_servico_do_google() {
    let escopos = |id: &str, grupos: &[&str]| {
        let tipo = servico(id).unwrap().tipos.first().unwrap();
        tipo.escopos_efetivos(&grupos.iter().map(|g| (*g).to_owned()).collect::<Vec<_>>())
    };
    let base = "https://www.googleapis.com/auth/";
    assert_eq!(escopos("gmail", &["ler"]), [format!("{base}gmail.readonly")], "nunca gmail.modify nem gmail.send");
    assert_eq!(
        escopos("gmail", &["ler", "rascunhos"]),
        [format!("{base}gmail.readonly"), format!("{base}gmail.compose")]
    );
    assert_eq!(
        escopos("google-agenda", &["ler"]),
        ["calendar.calendarlist.readonly", "calendar.events.freebusy", "calendar.events.readonly"]
            .map(|e| format!("{base}{e}"))
    );
    assert_eq!(escopos("google-drive", &["ler"]), [format!("{base}drive.readonly")]);
    assert_eq!(escopos("google-drive", &["criar"]), [format!("{base}drive.file")]);
    assert_eq!(escopos("google-docs", &["ler"]), [format!("{base}documents.readonly")]);
    assert_eq!(escopos("google-planilhas", &["ler"]), [format!("{base}spreadsheets.readonly")]);
    // Nenhum escopo amplo (Gmail inteiro, Drive inteiro) em nenhuma ficha.
    for id in GOOGLE {
        let tipo = servico(id).unwrap().tipos.first().unwrap();
        for grupo in tipo.grupos {
            for escopo in grupo.escopos {
                for amplo in ["gmail.modify", "gmail.send", "mail.google.com", "/auth/drive\"", "/auth/calendar\""] {
                    assert!(
                        !escopo.contains(amplo)
                            && !escopo.ends_with("/auth/drive")
                            && !escopo.ends_with("/auth/calendar"),
                        "{escopo}"
                    );
                }
            }
        }
    }
}

#[test]
fn escrita_que_pode_apagar_fica_indisponivel_ate_o_filtro_de_acoes_perigosas() {
    for (id, grupo) in [
        ("gmail", "rascunhos"),
        ("google-agenda", "criar_editar"),
        ("google-drive", "criar"),
        ("google-docs", "editar"),
        ("google-planilhas", "editar"),
    ] {
        let tipo = servico(id).unwrap().tipos.first().unwrap();
        assert!(!tipo.grupos.iter().find(|g| g.id == grupo).unwrap().disponivel, "{id}/{grupo}");
        assert!(tipo.liberacoes_validas(Some(&[grupo.to_owned()])).is_err(), "{id}/{grupo} não liga");
    }
}

const GOOGLE_NOVOS: &[(&str, &str, &str, &[&str])] = &[
    ("google-contatos", "https://people.googleapis.com/mcp/v1", "contacts.readonly", &["search_contacts"]),
    (
        "google-slides",
        "https://slidesmcp.googleapis.com/mcp/v1self",
        "presentations.readonly",
        &["read_presentation", "read_slide_page", "read_slide_page_thumbnail"],
    ),
    (
        "google-analytics",
        "https://analyticsdata.googleapis.com/mcp/v1",
        "analytics.readonly",
        &["run_report", "get_metadata", "run_realtime_report", "check_compatibility"],
    ),
    (
        "google-tarefas",
        "https://tasksmcp.googleapis.com/mcp/v1self",
        "tasks.readonly",
        &["list_task_lists", "list_tasks", "get_task"],
    ),
];

#[test]
fn novos_servicos_google_restringem_endereco_escopo_e_ferramentas() {
    for &(id, url, escopo, ferramentas) in GOOGLE_NOVOS {
        let ficha = servico(id).unwrap();
        assert!(ficha.conectavel(), "{id}");
        let [tipo] = ficha.tipos else { panic!("{id}: um tipo oficial") };
        assert_eq!(tipo.url, url, "{id}");
        assert!(matches!(
            tipo.auth,
            Auth::AppProprio { provedor: "google", emissor: "https://accounts.google.com", .. }
        ));
        let liberacoes = tipo.liberacoes_validas(None).unwrap();
        assert_eq!(liberacoes, ["ler"]);
        assert_eq!(tipo.escopos_efetivos(&liberacoes), [format!("https://www.googleapis.com/auth/{escopo}")]);
        assert_eq!(
            tipo.ferramentas_permitidas(&liberacoes).unwrap(),
            ferramentas.iter().copied().collect::<std::collections::BTreeSet<_>>()
        );
        assert!(tipo.grupos.iter().all(|g| g.curadas.is_empty()));
        assert!(tipo.liberacoes_validas(Some(&["editar".to_owned()])).is_err());
    }
}

#[test]
fn perfil_salvo_so_vale_se_todo_grupo_continua_valido_na_ficha() {
    let ids = |lista: &[&str]| lista.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert!(TIPO_TESTE.perfil_disponivel(&ids(&["ler"])));
    assert!(TIPO_TESTE.perfil_disponivel(&ids(&["ler", "cobrar"])));
    // Um grupo que a ficha de agora não tem derruba o perfil inteiro, mesmo com outro grupo válido ao lado.
    assert!(!TIPO_TESTE.perfil_disponivel(&ids(&["ler", "sumiu"])));
    assert!(!TIPO_TESTE.perfil_disponivel(&ids(&["sumiu", "ler"])));
    assert!(!TIPO_TESTE.perfil_disponivel(&ids(&["sumiu"])));
    assert!(!TIPO_TESTE.perfil_disponivel(&[]));
}

static GRUPOS_CURADOS: &[Grupo] = &[
    Grupo {
        id: "a",
        rotulo: "A",
        escopos: &[],
        url: None,
        ferramentas: &["listar", "ver"],
        curadas: &[Curada { ferramenta: "listar", digest: "aa" }, Curada { ferramenta: "ver", digest: "vv" }],
        padrao: true,
        disponivel: true,
        imposicao: None,
    },
    Grupo {
        id: "b",
        rotulo: "B",
        escopos: &[],
        url: None,
        ferramentas: &["listar", "ver"],
        curadas: &[Curada { ferramenta: "listar", digest: "bb" }, Curada { ferramenta: "ver", digest: "vv" }],
        padrao: false,
        disponivel: true,
        imposicao: None,
    },
];
static TIPO_CURADO: TipoConexao = TipoConexao {
    id: "teste.curado",
    tipo: Tipo::OficialChave,
    rotulo: "Teste",
    mantido_por: "Teste",
    url: "https://mcp.exemplo.invalid/mcp",
    auth: Auth::Chave { destino: "header", nome: "Authorization", prefixo: None, como_criar_leitura: "texto" },
    grupos: GRUPOS_CURADOS,
    perigosas: &[],
    recomendado: false,
    disponivel: true,
    motivo_indisponivel: None,
};

#[test]
fn curadoria_vem_so_dos_grupos_ligados_e_digest_ambiguo_nao_passa() {
    let ids = |lista: &[&str]| lista.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(TIPO_CURADO.ferramentas_curadas(&ids(&["a"])).get("listar"), Some(&"aa"));
    assert_eq!(TIPO_CURADO.ferramentas_curadas(&ids(&["b"])).get("listar"), Some(&"bb"));
    let ambos = TIPO_CURADO.ferramentas_curadas(&ids(&["a", "b"]));
    assert_eq!(ambos.get("listar"), None, "dois digests curados para o mesmo nome: nenhum passa");
    assert_eq!(ambos.get("ver"), Some(&"vv"), "o mesmo digest nos dois grupos não é ambíguo");
    assert!(TIPO_TESTE.ferramentas_curadas(&ids(&["ler"])).is_empty(), "sem curadoria nada passa sozinho");
}

/// A ficha só sugere skill que existe no catálogo curado, sem repetir id.
#[test]
fn toda_skill_sugerida_pela_ficha_existe_no_catalogo_curado() {
    let mut vistas = std::collections::BTreeSet::new();
    for ficha in servicos() {
        for skill in ficha.skills {
            assert!(
                crate::fork::skill_catalog::curada(skill.id),
                "{}: a skill `{}` não está no catálogo curado",
                ficha.id,
                skill.id
            );
            assert!(vistas.insert(skill.id), "a skill `{}` aparece em mais de uma ficha", skill.id);
        }
    }
    assert!(servico("linear").unwrap().skills.iter().any(|s| s.id == "linear-triar-issues-novas"));
}
