//! Regras puras do estado do fallback (MEX-1391): ordem, descanso, bloqueio de 1 h, uso extra, sobreposição e limpeza.
use super::estado::{self, BLOQUEIO_MS, Descanso, Estado, Motivo, Sobreposicao};
use crate::LockExt;
use crate::store::Store;

fn store() -> Store {
    Store::open(std::path::Path::new(":memory:")).unwrap()
}

fn sobre(origem: &str, atual: &str) -> Sobreposicao {
    Sobreposicao {
        space: "pessoal".into(),
        bot: "bot".into(),
        origem: origem.into(),
        atual: atual.into(),
        desde: 1,
        motivo: Motivo::PertoDoLimite,
    }
}

#[test]
fn ordem_do_bot_vale_no_lugar_da_do_space_e_nunca_passa_de_tres() {
    let mut e = Estado::default();
    e.ordens.insert(estado::chave_ordem("pessoal", "claude"), vec!["b".into(), "c".into(), "d".into(), "e".into()]);
    assert_eq!(e.ordem("pessoal", "bot", "claude"), ["b", "c", "d"], "até 3 reservas");
    assert!(e.ordem("pessoal", "bot", "codex").is_empty(), "sem lista, nada muda");
    e.ordens_bot.insert(estado::chave_ordem_bot("bot", "claude"), vec!["z".into()]);
    assert_eq!(e.ordem("pessoal", "bot", "claude"), ["z"], "a do bot vale no lugar da global");
    assert_eq!(e.ordem("pessoal", "outro", "claude"), ["b", "c", "d"]);
}

#[test]
fn descanso_bloqueio_e_uso_extra_vencidos_nao_contam_e_o_registro_se_limpa() {
    let store = store();
    let agora = crate::store::now_ms();
    estado::alterar(&store, |e| {
        e.descanso.insert("a".into(), Descanso { ate: agora + 10_000, motivo: Motivo::Esgotada });
        e.descanso.insert("velha".into(), Descanso { ate: agora - 1, motivo: Motivo::Esgotada });
        e.bloqueio.insert("p".into(), agora + BLOQUEIO_MS);
        e.uso_extra.insert("a".into(), agora - 1);
        Ok(())
    })
    .unwrap();
    let e = estado::ler(&store).unwrap();
    assert!(e.descansando("a", agora).is_some());
    assert!(!e.descanso.contains_key("velha"), "o vencido sai do registro");
    assert!(e.bloqueada("p", agora));
    assert!(!e.bloqueada("p", agora + BLOQUEIO_MS + 1), "a hora de bloqueio acaba: o destino volta a valer");
    assert!(!e.uso_extra_vigente("a", agora), "uso extra vale só até o reset informado");
}

#[test]
fn sobreposicao_grava_le_e_some_e_a_conta_em_uso_impede_a_remocao() {
    let store = store();
    {
        let c = store.db.locked();
        estado::gravar_sobreposicao(&c, "s1", Some(&sobre("a", "b"))).unwrap();
        assert_eq!(estado::sobreposicao(&c, "s1").unwrap().unwrap().atual, "b");
        assert_eq!(estado::sessoes_em(&c, "b").unwrap(), ["s1"]);
        assert!(
            estado::gravar_sobreposicao(&c, "s2", Some(&sobre("a", "a"))).is_err(),
            "origem igual ao destino não é troca"
        );
        estado::gravar_sobreposicao(&c, "s1", None).unwrap();
        assert!(estado::sobreposicao(&c, "s1").unwrap().is_none());
        assert_eq!(estado::sessoes_em(&c, "b").unwrap(), Vec::<String>::new());
    }
}

#[test]
fn esquecer_conta_tira_a_conta_das_listas_e_dos_descansos() {
    let store = store();
    estado::alterar(&store, |e| {
        e.ordens.insert(estado::chave_ordem("pessoal", "claude"), vec!["b".into()]);
        e.ordens_bot.insert(estado::chave_ordem_bot("bot", "claude"), vec!["b".into(), "c".into()]);
        e.descanso.insert("b".into(), Descanso { ate: crate::store::now_ms() + 5_000, motivo: Motivo::PertoDoLimite });
        Ok(())
    })
    .unwrap();
    super::esquecer_conta(&store, "b").unwrap();
    let e = estado::ler(&store).unwrap();
    assert!(e.ordens.is_empty(), "lista vazia some");
    assert_eq!(e.ordens_bot.values().next().unwrap(), &["c".to_string()]);
    assert!(e.descanso.is_empty());
}

#[test]
fn estado_ilegivel_falha_fechado() {
    let store = store();
    store.kv_set("fork.fallback-contas.v1", "{nao e json").unwrap();
    assert!(estado::ler(&store).is_err());
    assert!(estado::alterar(&store, |_| Ok(())).is_err(), "nunca recomeça por cima de um estado que não leu");
}

/// Regra (k): a prova dos vínculos é refeita a cada envio; revogar entre a prova e o envio cancela a troca.
mod prova_dos_vinculos {
    use super::super::{estado, prova};
    use crate::LockExt;
    use crate::hub::Hub;
    use crate::remote::identity::Identity;
    use crate::store::{BotConfig, Store};
    use serde_json::{Value, json};

    const CONTA: &str = "ac_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn conta(billing: &str, enabled: bool) -> Value {
        let launch = json!({"kind":"cli_profile","executable":"/fixture/claude","directory":"/fixture/b"});
        let proof = json!({"method":"claude_auth_status","plan":"max"});
        json!({"id":CONTA,"backend":"claude","billing":billing,"state":"connected","launch":launch,"scope":"owner",
            "quotaIdentity":CONTA,"verifiedAt":1,"subscriptionProof":proof,"models":[],
            "managed":{"label":"Conta b","revision":1,"generation":0,"enabled":enabled,"requestId":"f","principal":"f",
                "profile":null,"setupRequest":null,
                "catalog":{"accountId":CONTA,"principal":"f","launch":launch,"revision":1,"subscription":proof}}})
    }

    fn registrar(store: &Store, conta: &Value) {
        let registro = json!({"salt": uuid::Uuid::new_v4().to_string(), "accounts": [conta]});
        store.kv_set("fork.engine-accounts.v1", &registro.to_string()).unwrap();
    }

    fn bot() -> BotConfig {
        serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"claude"})).unwrap()
    }

    fn hub() -> std::sync::Arc<Hub> {
        let dir =
            std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("fallback-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        store.save_bot(&bot()).unwrap();
        Hub::new(
            store,
            "00000000-0000-4000-8000-0000000000f6".into(),
            Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            0,
        )
    }

    #[test]
    fn uso_extra_nao_busca_origem_de_outro_space() {
        let hub = hub();
        let pessoal = crate::fork::spaces::PERSONAL;
        let ate = crate::store::now_ms() + 60_000;
        estado::alterar(&hub.store, |e| {
            e.ordens.insert(estado::chave_ordem(pessoal, "claude"), vec![CONTA.into()]);
            for origem in ["origem-pessoal", "origem-outro"] {
                e.descanso.insert(origem.into(), super::Descanso { ate, motivo: super::Motivo::Esgotada });
            }
            Ok(())
        })
        .unwrap();
        let sobre = |space: &str, origem: &str| super::Sobreposicao {
            space: space.into(),
            bot: "bot".into(),
            origem: origem.into(),
            atual: CONTA.into(),
            desde: 1,
            motivo: super::Motivo::Esgotada,
        };
        {
            let db = hub.store.db.locked();
            estado::gravar_sobreposicao(&db, "sessao-outro", Some(&sobre("outro", "origem-outro"))).unwrap();
        }
        let executar = || {
            super::super::api::call(
                &hub,
                &crate::fork::engine_setup::access::Authority::Local,
                "donnaEngineFallbackAction",
                json!({"action":"allowExtra", "engineAccountId":CONTA}),
            )
        };
        assert!(executar().unwrap_err().to_string().contains("fallback_extra_not_needed"));
        assert!(estado::ler(&hub.store).unwrap().uso_extra.is_empty());
        {
            let db = hub.store.db.locked();
            estado::gravar_sobreposicao(&db, "sessao-pessoal", Some(&sobre(pessoal, "origem-pessoal"))).unwrap();
        }
        executar().unwrap();
        let estado = estado::ler(&hub.store).unwrap();
        assert_eq!(estado.uso_extra.get("origem-pessoal"), Some(&ate));
        assert!(!estado.uso_extra.contains_key("origem-outro"));
    }

    #[test]
    fn cada_vinculo_revogado_cancela_a_troca() {
        let hub = hub();
        let cfg = bot();
        registrar(&hub.store, &conta("subscription", true));
        // Sem constar da ordem do Space nem de um bot dele: sem prova de acesso.
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Err(prova::Falha::SemAcessoAoSpace));
        estado::alterar(&hub.store, |e| {
            e.ordens.insert(estado::chave_ordem(&cfg.space_id, "claude"), vec![CONTA.into()]);
            Ok(())
        })
        .unwrap();
        // Constar da lista não basta: sem um bot do Space usando a conta, a prova do Space falha (não é tautológica).
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Err(prova::Falha::SemAcessoAoSpace));
        let usa: BotConfig = serde_json::from_value(
            json!({"id":"usa","name":"Usa","backend":"claude","forkEditor":{"engineAccountId":CONTA}}),
        )
        .unwrap();
        hub.store.save_bot(&usa).unwrap();
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Ok(()));
        // Revogação no meio: a ordem do Space perde a conta, a conta é desligada, vira chave de API ou o bot some.
        estado::alterar(&hub.store, |e| {
            e.ordens.clear();
            Ok(())
        })
        .unwrap();
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Err(prova::Falha::SemAcessoAoSpace));
        estado::alterar(&hub.store, |e| {
            e.ordens.insert(estado::chave_ordem(&cfg.space_id, "claude"), vec![CONTA.into()]);
            Ok(())
        })
        .unwrap();
        registrar(&hub.store, &conta("subscription", false));
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Err(prova::Falha::Revogada));
        registrar(&hub.store, &conta("paid_api", true));
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Err(prova::Falha::Revogada), "chave de API nunca");
        registrar(&hub.store, &conta("subscription", true));
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Ok(()));
        hub.store.delete_bot("bot").unwrap();
        assert_eq!(prova::conferir_vinculos(&hub, &cfg, CONTA), Err(prova::Falha::SemAutoridade));
        let _ = hub.store.db.locked();
    }
}
