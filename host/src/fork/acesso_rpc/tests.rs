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
        ("subscribe", Acesso::Control),
        ("screenStatus", Acesso::Control),
        ("teamSettings", Acesso::Control),
        ("donnaProjects", Acesso::Control),
        ("donnaTaskProposalDecide", Acesso::Control),
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
/// desconhecida ou `Pairing`, recusada a todos.
#[test]
fn fronteiras_fora_da_tabela_sao_nomeadas() {
    for (nome, arquivo) in FORA_DA_TABELA {
        let caminho = format!("{}/src/{arquivo}", env!("CARGO_MANIFEST_DIR"));
        let fonte = std::fs::read_to_string(&caminho).unwrap();
        assert!(fonte.contains(&format!("\"{nome}\"")), "{nome} não aparece em {arquivo}");
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

#[test]
fn desconhecido_e_classificado_antes_dos_argumentos() {
    assert!(desconhecido("metodoInexistente"));
    assert!(desconhecido("bootstrap.prove"));
    assert!(!desconhecido("send"));
    assert!(!desconhecido("pair"));
    assert!(!desconhecido("donnaWorkplaceUnknown"));
}
