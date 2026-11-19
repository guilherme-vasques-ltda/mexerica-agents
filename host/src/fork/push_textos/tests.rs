#![allow(clippy::unwrap_used)]
//! MX-138: os seis idiomas têm todos os textos (com nome/emoji interpolados), e o idioma do push é o
//! do aparelho só com a geração atual da concessão — revogado/desconhecido cai no do dono, e sem
//! nada, pt-BR.
use super::{
    Corpo, Detalhe, IDIOMAS as TEXTOS_IDIOMAS, Motivo, Situacao, Titulo, corpo, generica, idioma_do_aparelho,
    subtitulo, titulo,
};
use crate::api::devices::Caller;
use crate::fork::idioma_app::{self, APARELHO, IDIOMAS};
use crate::hub::Hub;
use crate::remote::identity::Identity;
use crate::remote::push::AlertKind;
use crate::store::{BotConfig, Device, DeviceSource, Store};
use serde_json::json;
use std::path::Path;

fn bot(id: &str) -> BotConfig {
    serde_json::from_value(json!({"id":id,"name":id,"backend":"codex"})).unwrap()
}

fn hub() -> (std::sync::Arc<Hub>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("push-textos-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(Path::new(":memory:")).unwrap();
    store.save_bot(&bot("berga")).unwrap();
    (Hub::new(store, "push-textos".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0), dir)
}

fn aparelho(hub: &Hub, key: &str) -> Caller {
    hub.store
        .put_device(&Device {
            key: key.into(),
            name: key.into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![],
            lease_until: None,
            created_at: crate::store::now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    crate::fork::autoridade::autenticar(hub, key).unwrap().1
}

#[test]
fn seis_idiomas_tem_todos_os_textos() {
    for (codigo, _) in IDIOMAS {
        for t in [
            titulo(codigo, &Titulo::Nome("Berga")),
            titulo(codigo, &Titulo::Precisa("Berga")),
            titulo(codigo, &Titulo::NoGrupo { nome: "Berga", grupo: "Casa" }),
        ] {
            assert!(!t.trim().is_empty(), "{codigo}: título vazio");
        }
        assert!(titulo(codigo, &Titulo::Precisa("Berga")).contains("Berga"), "{codigo}");
        let no_grupo = titulo(codigo, &Titulo::NoGrupo { nome: "Berga", grupo: "Casa" });
        assert!(no_grupo.contains("Berga") && no_grupo.contains("Casa"), "{codigo}: {no_grupo}");
        for c in [
            corpo(codigo, &Corpo::Texto("resposta")),
            corpo(
                codigo,
                &Corpo::Rotina {
                    nome: "Diária", situacao: Situacao::Terminou, detalhe: Detalhe::Resumo("tudo certo")
                },
            ),
            corpo(
                codigo,
                &Corpo::Rotina {
                    nome: "Diária",
                    situacao: Situacao::Precisa,
                    detalhe: Detalhe::Resumo("falta acesso"),
                },
            ),
            corpo(
                codigo,
                &Corpo::Rotina { nome: "Diária", situacao: Situacao::Falhou, detalhe: Detalhe::Resumo("sem rede") },
            ),
            corpo(
                codigo,
                &Corpo::Rotina {
                    nome: "Diária",
                    situacao: Situacao::Interrompida,
                    detalhe: Detalhe::Resumo("reinício"),
                },
            ),
            corpo(codigo, &Corpo::Reagiu("👍")),
            corpo(codigo, &Corpo::Terminou),
            corpo(codigo, &Corpo::NaoTerminou),
        ] {
            assert!(!c.trim().is_empty(), "{codigo}: corpo vazio");
        }
        for s in [Situacao::Terminou, Situacao::Precisa, Situacao::Falhou, Situacao::Interrompida] {
            let rotina =
                corpo(codigo, &Corpo::Rotina { nome: "Diária", situacao: s, detalhe: Detalhe::Resumo("resumo") });
            assert!(rotina.contains("Diária") && rotina.contains("resumo"), "{codigo}/{s:?}: {rotina}");
        }
        // Motivo observado e texto padrão também saem no idioma, nunca em pt-BR fixo.
        for d in [
            Detalhe::Motivo(Motivo::Tempo),
            Detalhe::Motivo(Motivo::Limite),
            Detalhe::Motivo(Motivo::Entrar),
            Detalhe::Motivo(Motivo::NaoComecou),
            Detalhe::Motivo(Motivo::Parou),
            Detalhe::Padrao,
        ] {
            let rotina = corpo(codigo, &Corpo::Rotina { nome: "Diária", situacao: Situacao::Falhou, detalhe: d });
            assert!(!rotina.trim().is_empty() && rotina.contains("Diária"), "{codigo}: {rotina}");
        }
        assert!(corpo(codigo, &Corpo::Reagiu("👍")).contains('👍'), "{codigo}");
        for kind in [AlertKind::Done, AlertKind::NeedsInput, AlertKind::Failed] {
            assert!(!subtitulo(codigo, kind).trim().is_empty(), "{codigo}/{kind:?}: subtítulo vazio");
            assert!(!generica(codigo, kind).trim().is_empty(), "{codigo}/{kind:?}: genérica vazia");
        }
        // Idioma desconhecido cai no pt-BR, nunca em texto vazio.
        assert_eq!(generica("de", AlertKind::Done), generica("pt-BR", AlertKind::Done));
    }
    // Corpo::Texto passa como veio, em qualquer idioma.
    assert_eq!(corpo("en", &Corpo::Texto("oi")), "oi");
}

/// Todo idioma que o app sabe gravar tem textos de push: um código novo em
/// `idioma_app::IDIOMAS` sem template aqui cairia em pt-BR sem avisar.
#[test]
fn todo_idioma_do_app_tem_textos_de_push() {
    for (codigo, _) in IDIOMAS {
        assert!(TEXTOS_IDIOMAS.iter().any(|(c, _)| *c == codigo), "{codigo} sem textos de push");
    }
}

/// Marcador dentro do nome não vira template: "Ana {g}" continua literal no título.
#[test]
fn marcador_no_nome_nao_vira_template() {
    assert_eq!(titulo("pt-BR", &Titulo::NoGrupo { nome: "Ana {g}", grupo: "Casa" }), "Ana {g} em Casa");
    assert_eq!(
        corpo(
            "en",
            &Corpo::Rotina { nome: "Ana {d}", situacao: Situacao::Terminou, detalhe: Detalhe::Resumo("{n} ok") }
        ),
        "Routine “Ana {d}” finished: {n} ok"
    );
}

#[test]
fn idioma_do_aparelho_segue_a_geracao_e_cai_no_dono() {
    let (hub, dir) = hub();
    // Sem nada guardado: pt-BR.
    assert_eq!(idioma_do_aparelho(&hub.store, "fone"), "pt-BR");
    // O dono escolheu espanhol no app do computador.
    idioma_app::call(&hub, &Caller::Local, &json!({"language":"es"})).unwrap();
    assert_eq!(idioma_do_aparelho(&hub.store, "desconhecido"), "es");
    // O aparelho gravou francês na concessão atual.
    let fone = aparelho(&hub, "fone");
    idioma_app::call(&hub, &fone, &json!({"language":"fr"})).unwrap();
    assert_eq!(idioma_do_aparelho(&hub.store, "fone"), "fr");
    // Outro aparelho sem idioma gravado segue o dono.
    aparelho(&hub, "tablet");
    assert_eq!(idioma_do_aparelho(&hub.store, "tablet"), "es");
    // Revogado: nada de herdar o idioma antigo.
    let antigo = hub.store.kv_read(&format!("{APARELHO}fone")).unwrap().unwrap();
    assert!(hub.revoke_device("fone").unwrap());
    assert_eq!(idioma_do_aparelho(&hub.store, "fone"), "es");
    // Concessão nova na mesma chave com o valor antigo recriado: a geração não bate, cai no dono.
    let novo = aparelho(&hub, "fone");
    hub.store.kv_set(&format!("{APARELHO}fone"), &antigo).unwrap();
    assert_eq!(idioma_do_aparelho(&hub.store, "fone"), "es");
    // Gravando de novo na concessão nova, vale o idioma novo.
    idioma_app::call(&hub, &novo, &json!({"language":"zh-CN"})).unwrap();
    assert_eq!(idioma_do_aparelho(&hub.store, "fone"), "zh-CN");
    std::fs::remove_dir_all(dir).unwrap();
}

/// O que o celular passa a mostrar em cada idioma (saída real dos testes acima, para o PR).
#[test]
fn exemplo_do_que_o_celular_mostra() {
    let linhas: Vec<String> = IDIOMAS
        .iter()
        .map(|(codigo, _)| {
            format!(
                "{codigo}: título `{}` · corpo `{}` · legenda `{}`",
                titulo(codigo, &Titulo::Precisa("Berga")),
                corpo(codigo, &Corpo::Terminou),
                subtitulo(codigo, AlertKind::Done),
            )
        })
        .collect();
    println!("{}", linhas.join("\n"));
    assert_eq!(linhas.len(), 6);
}
