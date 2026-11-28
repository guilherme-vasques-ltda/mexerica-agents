use super::*;
use crate::api::devices::permit;
use crate::fork::donna_fixa::metodos::{METODOS, coletor};
use crate::fork::ponte_credencial::Origem;
use std::collections::{BTreeMap, BTreeSet};

/// Erro com os métodos roteados sem classe, os nomes repetidos e as classes sem método no código.
fn verificar(inventario: &BTreeMap<String, BTreeSet<String>>) -> Result<(), String> {
    let mut vistos = BTreeSet::new();
    let repetidos: Vec<&str> =
        TABELA.iter().flat_map(|(_, metodos)| metodos.iter().copied()).filter(|m| !vistos.insert(*m)).collect();
    let faltam: Vec<String> = inventario
        .iter()
        .filter(|(m, _)| acesso(m).is_none())
        .map(|(m, arquivos)| format!("{m} ({})", arquivos.iter().cloned().collect::<Vec<_>>().join(", ")))
        .collect();
    let sobram: Vec<&str> =
        vistos.iter().copied().filter(|m| !inventario.contains_key(*m) && !DO_CANAL.contains(m)).collect();
    if repetidos.is_empty() && faltam.is_empty() && sobram.is_empty() {
        Ok(())
    } else {
        Err(format!("sem acesso em acesso_rpc: {faltam:?}; repetidos: {repetidos:?}; sem método no código: {sobram:?}"))
    }
}

/// Todo método que os roteadores atendem (o mesmo coletor da tabela de `donna_fixa::metodos`) declara uma
/// classe de acesso, e a tabela não guarda nome que o código não atende.
#[test]
fn toda_rpc_roteada_declara_acesso() {
    let inventario = coletor::coletar(&coletor::fontes_do_host());
    assert!(inventario.len() > 200, "o coletor deixou de ver os métodos: {}", inventario.len());
    if let Err(erro) = verificar(&inventario) {
        panic!("{erro}");
    }
    let classificados: BTreeSet<&str> = METODOS.iter().map(|(m, _)| *m).collect();
    let com_acesso: BTreeSet<&str> =
        TABELA.iter().flat_map(|(_, metodos)| metodos.iter().copied()).filter(|m| !DO_CANAL.contains(m)).collect();
    assert_eq!(classificados, com_acesso, "as duas tabelas cobrem os mesmos métodos");
    for m in DO_CANAL {
        assert!(!inventario.contains_key(*m), "{m} é do canal, não do dispatch");
    }
}

#[test]
fn metodo_novo_sem_acesso_reprova() {
    let fonte = "fn call(method: &str) { match method {\n    \"metodoNovoSemClasse\" => x,\n    _ => y,\n} }";
    let inventario = coletor::coletar(&[("api/novo.rs".to_owned(), fonte.to_owned())]);
    let erro = verificar(&inventario).unwrap_err();
    assert!(erro.contains("metodoNovoSemClasse (api/novo.rs)"), "{erro}");
    assert_eq!(acesso("metodoNovoSemClasse"), None);
}

fn device(scopes: &[Scope]) -> Caller {
    Caller::Device { key: "aparelho".into(), scopes: scopes.to_vec(), autoridade: None }
}

/// Cada chamador com um nome, para a matriz esperada não depender de `permite`.
fn chamadores() -> Vec<(&'static str, Caller)> {
    use Scope::{Control, Executor, OwnerDevices, Screen};
    vec![
        ("local", Caller::Local),
        ("ponte", Caller::Ponte { bot: "bot".into(), sessao: "binding".into(), origem: Origem::ficticia() }),
        ("pairing", Caller::Pairing { key: "aparelho".into() }),
        ("bootstrap", Caller::Bootstrap { key: "aparelho".into(), bootstrap_id: "fixture".into() }),
        ("sem_escopo", device(&[])),
        ("control", device(&[Control])),
        ("screen", device(&[Screen])),
        ("control_screen", device(&[Control, Screen])),
        ("owner", device(&[OwnerDevices])),
        ("control_owner", device(&[Control, OwnerDevices])),
        ("executor", device(&[Executor])),
        ("executor_control", device(&[Executor, Control])),
    ]
}

fn esperado(acesso: Acesso) -> &'static [&'static str] {
    match acesso {
        Acesso::LocalOnly => &["local"],
        Acesso::Ponte => &["local", "ponte"],
        Acesso::OwnerDevices => &["local", "owner", "control_owner"],
        Acesso::Control => &["local", "control", "control_screen", "control_owner"],
        Acesso::Screen => &["local", "screen", "control_screen"],
        Acesso::Proprio => &["local", "sem_escopo", "control", "screen", "control_screen", "owner", "control_owner"],
        Acesso::Executor => &["executor"],
        Acesso::Pairing => &[],
    }
}

/// Fronteira por chamador: para cada método da tabela, só os chamadores da sua classe passam. Pairing e
/// Bootstrap nunca passam; executor não usa outra classe; este computador não fala o protocolo do executor.
#[test]
fn cada_chamador_so_alcanca_a_sua_classe() {
    for (classe, metodos) in TABELA {
        for metodo in *metodos {
            for (nome, caller) in chamadores() {
                let pode = esperado(*classe).contains(&nome);
                assert_eq!(permit(&caller, metodo).is_ok(), pode, "{nome} em {metodo} ({classe:?})");
            }
        }
    }
}

#[test]
fn navegador_so_usa_a_lista_de_chat_sem_misturar_escopos() {
    let web = device(&[Scope::WebChat]);
    let misto = device(&[Scope::WebChat, Scope::Control, Scope::Screen]);
    assert!(permite_assinatura(&web, "events"));
    assert!(!permite_assinatura(&web, "term"));
    assert!(!permite_assinatura(&misto, "events"));
    for metodo in WEB_CHAT {
        assert!(permit(&web, metodo).is_ok(), "{metodo}");
    }
    for metodo in TABELA.iter().flat_map(|(_, metodos)| *metodos) {
        assert_eq!(permit(&web, metodo).is_ok(), WEB_CHAT.contains(metodo), "{metodo}");
        assert!(permit(&misto, metodo).is_err(), "{metodo}");
    }
}

#[test]
fn metodo_sem_classe_nega_a_todos_inclusive_local() {
    for metodo in ["", "metodoInexistente", "donnaWorkplaceUnknown", "PAIRING", "send ", "bootstrap.prove"] {
        assert_eq!(acesso(metodo), None, "{metodo}");
        for (nome, caller) in chamadores() {
            assert!(permit(&caller, metodo).is_err(), "{nome} em {metodo:?}");
        }
    }
}

/// Classes de métodos conhecidos, para que mover um nome de classe seja uma mudança visível.
#[test]
fn metodos_conhecidos_mantem_a_classe() {
    for (metodo, classe) in [
        ("send", Acesso::Control),
        ("hello", Acesso::Control),
        ("workspaceContext", Acesso::Control),
        ("subscribe", Acesso::Control),
        ("screenStatus", Acesso::Control),
        ("teamSettings", Acesso::Control),
        ("donnaProjects", Acesso::Control),
        ("donnaGovernance", Acesso::Control),
        ("donnaGovernanceSave", Acesso::Control),
        ("donnaTaskProposalDecide", Acesso::Control),
        ("donnaTowerView", Acesso::Control),
        ("donnaTeamRequests", Acesso::Control),
        ("engineGoKeySave", Acesso::Control),
        ("screenOffer", Acesso::Screen),
        ("screenTakeover", Acesso::Screen),
        ("devices", Acesso::OwnerDevices),
        ("decideAccessRequest", Acesso::OwnerDevices),
        ("donnaAppLanguageSave", Acesso::Proprio),
        ("executorPoll", Acesso::Executor),
        ("chatCall", Acesso::Ponte),
        ("computerCall", Acesso::Ponte),
        ("donnaGuardRuntime", Acesso::Ponte),
        ("pairing", Acesso::LocalOnly),
        ("setTeamSettings", Acesso::LocalOnly),
        ("computersList", Acesso::LocalOnly),
        ("donnaWorkplaceList", Acesso::LocalOnly),
        ("motorChaveSalvar", Acesso::LocalOnly),
        ("installHostUpdate", Acesso::LocalOnly),
        ("pair", Acesso::Pairing),
    ] {
        assert_eq!(acesso(metodo), Some(classe), "{metodo}");
    }
}

/// Cada fronteira fora da tabela é nomeada, vive no arquivo da sua guarda e não abre a porta do `dispatch`: lá ela é
/// desconhecida ou `Pairing`, recusada a todos, salvo nome que o `dispatch` também roteia com a sua classe.
#[test]
fn fronteiras_fora_da_tabela_sao_nomeadas() {
    let roteados = coletor::coletar(&coletor::fontes_do_host());
    for (nome, arquivo) in FORA_DA_TABELA {
        let caminho = format!("{}/src/{arquivo}", env!("CARGO_MANIFEST_DIR"));
        let fonte = std::fs::read_to_string(&caminho).unwrap();
        assert!(fonte.contains(&format!("\"{nome}\"")), "{nome} não aparece em {arquivo}");
        if roteados.contains_key(*nome) {
            assert!(acesso(nome).is_some(), "{nome}");
            continue;
        }
        assert!(matches!(acesso(nome), None | Some(Acesso::Pairing)), "{nome}");
        for (quem, caller) in chamadores() {
            assert!(permit(&caller, nome).is_err(), "{quem} em {nome}");
        }
    }
    let pessoais: BTreeSet<&str> =
        crate::fork::donna_fixa::metodos::METODOS_VINCULO_PESSOAL.iter().map(|(nome, _)| *nome).collect();
    let nomeados: BTreeSet<&str> = FORA_DA_TABELA
        .iter()
        .filter(|(_, arquivo)| arquivo.ends_with("personal_channel.rs"))
        .map(|(nome, _)| *nome)
        .collect();
    assert_eq!(pessoais, nomeados);
}

/// Código de produção: o filtro do coletor e, além dele, todo bloco `#[cfg(test)] mod … { … }` em linha.
fn sem_testes(codigo: &str) -> String {
    let inicio = regex::Regex::new(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{").expect("regex");
    let mut codigo = coletor::producao(codigo);
    while let Some(m) = inicio.find(&codigo) {
        let mut nivel = 1;
        let mut fim = codigo.len();
        for (i, c) in codigo[m.end()..].char_indices() {
            match c {
                '{' => nivel += 1,
                '}' => {
                    nivel -= 1;
                    if nivel == 0 {
                        fim = m.end() + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        codigo.replace_range(m.start()..fim, "");
    }
    codigo
}

/// Listas citadas numa leitura do nome do pedido, pelo caminho escrito no código.
fn listas_do_fio() -> Vec<(&'static str, &'static [&'static str])> {
    vec![("browser_canal::METODOS", &crate::fork::browser::stream::canal::METODOS)]
}

/// Erro com cada porta de produção que lê o nome do pedido (`["m"]`) sem estar declarada: arquivo ou contagem de
/// leituras fora de `LEITURAS_DO_NOME`, leitura de forma desconhecida e nome atendido fora de `FORA_DA_TABELA` ou
/// `PELA_TABELA`, ou declarado e não mais atendido.
fn verificar_portas(fontes: &[(String, String)]) -> Result<(), String> {
    use regex::Regex;
    let re = |padrao: &str| Regex::new(padrao).expect("regex");
    let literal = re(r#"\w+\["m"\]\s*(?:==|!=)\s*"([^"]+)""#);
    let variavel = re(r#"\w+\["m"\]\s*(?:==|!=)\s*([a-z_][a-z0-9_]*)\b"#);
    let partida = re(r#"match\s+\w+\["m"\]\.as_str\(\)\s*\{"#);
    let braco = re(r#"Some\("([^"]+)"\)"#);
    let lista = re(r#"\w+\["m"\]\.as_str\(\)\.filter\(\|\w+\|\s*((?:\w+::)*[A-Z][A-Z0-9_]*)\.contains\("#);
    let repasse = re(r#"let\s+Some\(\w+\)\s*=\s*\w+\["m"\]\.as_str\(\)\s*\{"#);
    let quadro = re(r#"\w+\["m"\]\.(?:is_object|clone)\(\)"#);
    let aspas = re(r#""([^"]+)""#);
    let mut erros = Vec::new();
    let mut leituras = BTreeMap::new();
    let mut atendidos = BTreeSet::new();
    for (arquivo, codigo) in fontes {
        let codigo = sem_testes(codigo);
        let total = codigo.matches(r#"["m"]"#).count();
        if total == 0 {
            continue;
        }
        leituras.insert(arquivo.as_str(), total);
        let mut nomes = BTreeSet::new();
        let mut formas = 0;
        for c in literal.captures_iter(&codigo) {
            formas += 1;
            nomes.insert(c[1].to_owned());
        }
        for c in variavel.captures_iter(&codigo) {
            formas += 1;
            let laco = re(&format!(r"for\s+{}\s+in\s+\[([^\]]*)\]", &c[1]));
            match laco.captures(&codigo) {
                Some(l) => nomes.extend(aspas.captures_iter(&l[1]).map(|a| a[1].to_owned())),
                None => erros.push(format!("{arquivo}: nome comparado com `{}` de origem desconhecida", &c[1])),
            }
        }
        for m in partida.find_iter(&codigo) {
            formas += 1;
            let topo = nivel_um(&codigo[m.end()..]);
            nomes.extend(braco.captures_iter(&topo).map(|b| b[1].to_owned()));
        }
        for c in lista.captures_iter(&codigo) {
            formas += 1;
            match listas_do_fio().into_iter().find(|(caminho, _)| *caminho == &c[1]) {
                Some((_, valores)) => nomes.extend(valores.iter().map(|v| (*v).to_owned())),
                None => erros.push(format!("{arquivo}: lista `{}` não declarada em listas_do_fio", &c[1])),
            }
        }
        formas += repasse.find_iter(&codigo).count() + quadro.find_iter(&codigo).count();
        if formas != total {
            erros.push(format!("{arquivo}: {total} leituras de [\"m\"], {formas} de forma reconhecida"));
        }
        atendidos.extend(nomes.into_iter().map(|n| (arquivo.clone(), n)));
    }
    let declaradas: BTreeMap<&str, usize> = LEITURAS_DO_NOME.iter().copied().collect();
    if leituras != declaradas {
        erros.push(format!("leituras do nome: código {leituras:?}, declaradas {declaradas:?}"));
    }
    let nomeados: BTreeSet<(String, String)> = FORA_DA_TABELA
        .iter()
        .chain(PELA_TABELA)
        .filter(|(_, arquivo)| leituras.contains_key(arquivo))
        .map(|(nome, arquivo)| ((*arquivo).to_owned(), (*nome).to_owned()))
        .collect();
    for (arquivo, nome) in atendidos.difference(&nomeados) {
        erros.push(format!("{nome} ({arquivo}) é atendido fora do dispatch sem declaração"));
    }
    for (arquivo, nome) in nomeados.difference(&atendidos) {
        erros.push(format!("{nome} ({arquivo}) está declarado e o código não o atende"));
    }
    if erros.is_empty() { Ok(()) } else { Err(erros.join("; ")) }
}

/// O texto no nível 1 de um bloco que começa logo depois de `{`.
fn nivel_um(bloco: &str) -> String {
    let mut nivel = 1;
    let mut out = String::new();
    for c in bloco.chars() {
        match c {
            '{' => nivel += 1,
            '}' => {
                nivel -= 1;
                if nivel == 0 {
                    break;
                }
            }
            _ if nivel == 1 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Inventário fechado das portas de produção fora do `dispatch`: canal E2E (navegador, pareamento, bootstrap), probe
/// pessoal e caixa de mensagens do relay. Cada nome atendido ou passa pela tabela (`PELA_TABELA`) ou é exceção nomeada
/// (`FORA_DA_TABELA`), e os nomes que passam pela tabela têm classe.
#[test]
fn toda_porta_de_producao_e_declarada() {
    if let Err(erro) = verificar_portas(&coletor::fontes_do_host()) {
        panic!("{erro}");
    }
    for (nome, arquivo) in PELA_TABELA {
        assert!(!matches!(acesso(nome), None | Some(Acesso::Pairing)), "{nome} ({arquivo}) sem classe que passe");
    }
}

#[test]
fn porta_nova_sem_declaracao_reprova() {
    let canal = std::fs::read_to_string(format!("{}/src/remote/channel.rs", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let com_porta = canal.replacen(
        "        if let Some(kind) = v[\"sub\"].as_str() {",
        "        if v[\"m\"] == \"portaNova\" {\n            return Ok(());\n        }\n        if let Some(kind) = v[\"sub\"].as_str() {",
        1,
    );
    assert_ne!(com_porta, canal, "o ponto de inserção mudou");
    let erro = verificar_portas(&[("remote/channel.rs".to_owned(), com_porta)]).unwrap_err();
    assert!(erro.contains("portaNova (remote/channel.rs) é atendido fora do dispatch sem declaração"), "{erro}");
    assert!(erro.contains("remote/channel.rs"), "{erro}");
    let novo = "fn f(v: &Value) { if v[\"m\"] == \"outraPorta\" {} }".to_owned();
    let erro = verificar_portas(&[("api/novo.rs".to_owned(), novo)]).unwrap_err();
    assert!(erro.contains("outraPorta (api/novo.rs)") && erro.contains("leituras do nome"), "{erro}");
}

/// O texto entre o `(` em `inicio` e o `)` que o fecha.
fn ate_fechar(codigo: &str, inicio: usize) -> &str {
    let mut nivel = 0;
    for (i, c) in codigo[inicio..].char_indices() {
        match c {
            '(' => nivel += 1,
            ')' => {
                nivel -= 1;
                if nivel == 0 {
                    return &codigo[inicio..inicio + i];
                }
            }
            _ => {}
        }
    }
    &codigo[inicio..]
}

/// O corpo da função `caminho` (`nome`, `modulo::nome` ou `crate::…::nome`): no arquivo da rota ou no do módulo.
fn corpo_do_handler(fontes: &[(String, String)], arquivo: &str, caminho: &str) -> Option<String> {
    let partes: Vec<&str> = caminho.trim_start_matches("crate::").split("::").collect();
    let (nome, modulo) = partes.split_last()?;
    let candidatos: Vec<String> = if modulo.is_empty() {
        vec![arquivo.to_owned()]
    } else {
        let base = modulo.join("/");
        vec![format!("{base}.rs"), format!("{base}/mod.rs")]
    };
    let definicao = regex::Regex::new(&format!(r"\bfn\s+{nome}\s*[(<]")).expect("regex");
    let (_, codigo) = fontes.iter().find(|(f, _)| candidatos.contains(f))?;
    let codigo = sem_testes(codigo);
    let inicio = definicao.find(&codigo)?.end();
    let abre = inicio + codigo[inicio..].find('{')?;
    let mut nivel = 0;
    for (i, c) in codigo[abre..].char_indices() {
        match c {
            '{' => nivel += 1,
            '}' => {
                nivel -= 1;
                if nivel == 0 {
                    return Some(codigo[abre + 1..abre + i].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

/// Toda rota HTTP de produção está em `ROTAS_HTTP`, e o handler de cada uma tem a guarda declarada: a rota pública não
/// confere token, a com token confere, a do `dispatch` autentica e chama o `dispatch`, e a de guarda própria faz a
/// chamada declarada (um handler extra só pode devolver um status). Rota nova reprova até ser declarada.
#[test]
fn toda_rota_http_e_declarada() {
    use regex::Regex;
    let rota = Regex::new(r#"\.route\(\s*"([^"]+)""#).unwrap();
    let metodo = Regex::new(r"\b(?:get|post|put|patch|delete|any)\(\s*([\w:]+)\s*\)").unwrap();
    let sobra =
        Regex::new(r"\.(fallback|method_not_allowed_fallback|nest|nest_service|route_service|merge)\(").unwrap();
    let fontes = coletor::fontes_do_host();
    let mut no_codigo: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (arquivo, codigo) in &fontes {
        let codigo = sem_testes(codigo);
        if !codigo.contains("Router::new()") && !codigo.contains("Router<") {
            continue;
        }
        for c in rota.captures_iter(&codigo) {
            let inicio = c.get(0).unwrap().start() + ".route".len();
            let handlers = metodo.captures_iter(ate_fechar(&codigo, inicio)).map(|h| h[1].to_owned()).collect();
            no_codigo.insert((arquivo.clone(), c[1].to_owned()), handlers);
        }
        for c in sobra.captures_iter(&codigo) {
            let inicio = c.get(0).unwrap().end() - 1;
            let handler = ate_fechar(&codigo, inicio).trim_start_matches('(').trim().to_owned();
            no_codigo.insert((arquivo.clone(), c[1].to_owned()), vec![handler]);
        }
    }
    let declaradas: BTreeSet<(String, String)> =
        ROTAS_HTTP.iter().map(|(arquivo, caminho, _)| ((*arquivo).to_owned(), (*caminho).to_owned())).collect();
    assert_eq!(no_codigo.keys().cloned().collect::<BTreeSet<_>>(), declaradas);
    let com_token = |corpo: &str| corpo.contains("autenticar") || corpo.contains("authorized(");
    for (arquivo, caminho, guarda) in ROTAS_HTTP {
        let handlers = &no_codigo[&((*arquivo).to_owned(), (*caminho).to_owned())];
        assert!(!handlers.is_empty(), "{caminho}: handler não encontrado");
        assert_eq!(caminho.starts_with("/api/"), matches!(guarda, Guarda::Dispatch), "{caminho}");
        for handler in handlers {
            let corpo = corpo_do_handler(&fontes, arquivo, handler).unwrap_or_else(|| panic!("{caminho}: {handler}"));
            let ok = match guarda {
                Guarda::Publica => !com_token(&corpo),
                Guarda::PublicaMinima(chamada) => corpo.contains(chamada),
                Guarda::Bearer => com_token(&corpo),
                Guarda::Dispatch => (com_token(&corpo) && corpo.contains("dispatch(")) || corpo.contains("command("),
                Guarda::Propria(chamada) => {
                    corpo.contains(chamada) || (corpo.trim().starts_with("StatusCode::") && !corpo.contains(';'))
                }
            };
            assert!(ok, "{caminho} ({handler}) não tem a guarda {guarda:?}");
        }
    }
    assert!(acesso("donnaGuardRuntime").is_some());
}

#[test]
fn desconhecido_e_classificado_antes_dos_argumentos() {
    assert!(desconhecido("metodoInexistente"));
    assert!(desconhecido("bootstrap.prove"));
    assert!(desconhecido("donnaWorkplaceUnknown"), "o prefixo não muda a classificação");
    assert!(!desconhecido("donnaWorkplaceList"));
    assert!(!desconhecido("send"));
    assert!(!desconhecido("pair"));
}
