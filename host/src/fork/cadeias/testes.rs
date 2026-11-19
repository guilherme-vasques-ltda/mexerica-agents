#![allow(clippy::unwrap_used)]
use super::{
    admissao,
    modelo::{Escopo, Fase, Origem, Pedido, Recibo, Recusa, Tipo},
};
use crate::{
    LockExt,
    fork::atividades::{modelo::CoreError, store::with_store_tx},
    store::Store,
};
use std::{
    path::Path,
    sync::{Arc, Barrier},
    time::Duration,
};

fn raiz(evento: &str) -> Pedido {
    let mut p = admissao::pedido(
        Tipo::Raiz,
        evento.into(),
        admissao::digest(&"intenção sintética").unwrap(),
        "bot",
        "run",
        None,
    );
    p.origem = Origem::Manual;
    p.rotina = Some("rotina".into());
    p
}
fn proxima(r: &Recibo, tipo: Tipo, evento: &str) -> Pedido {
    let mut pedido = admissao::pedido(
        tipo,
        evento.into(),
        admissao::digest(&evento).unwrap(),
        &r.bot_id,
        &r.run_id,
        Some(r.clone()),
    );
    pedido.rotina = Some("rotina".into());
    pedido
}
fn total(store: &Store, chain: &str) -> i64 {
    store.db.locked().query_row("SELECT used FROM donna_chain_roots WHERE id=?1", [chain], |r| r.get(0)).unwrap()
}
fn novo_store() -> Store {
    Store::open(Path::new(":memory:")).unwrap()
}

#[test]
fn raiz_repetida_devolve_run_original_e_conflita_identidade_divergente() {
    let store = novo_store();
    let p = raiz("ocorrência");
    let r = admissao::admitir(&store, &p).unwrap();
    let mut replay = raiz("ocorrência");
    replay.candidato.run_id = "novo-id-candidato-descartado".into();
    assert_eq!(admissao::admitir(&store, &replay).unwrap(), r);
    assert_eq!(total(&store, &r.chain_id), 1);
    for alteracao in 0..4 {
        let mut conflito = raiz("ocorrência");
        match alteracao {
            0 => conflito.rotina = Some("outra-rotina".into()),
            1 => conflito.origem = Origem::Agenda,
            2 => {
                conflito.escopo =
                    Escopo::ConversaDoBot { bot_id: "bot".into(), space_id: Some("outro-contexto".into()) }
            }
            _ => conflito.intencao = admissao::digest(&"outra intenção").unwrap(),
        }
        assert_eq!(admissao::admitir(&store, &conflito).unwrap_err().downcast_ref(), Some(&Recusa::Conflito));
    }
    assert_eq!(total(&store, &r.chain_id), 1);
    assert_eq!(
        admissao::digest(&serde_json::json!({"b":2,"a":{"d":4,"c":3}})).unwrap(),
        admissao::digest(&serde_json::json!({"a":{"c":3,"d":4},"b":2})).unwrap()
    );
}

#[test]
fn niveis_cinco_e_seis_e_continuacao_no_mesmo_nivel() {
    let store = novo_store();
    let mut r = admissao::admitir(&store, &raiz("raiz")).unwrap();
    assert_eq!((r.level, r.ordinal), (1, 1));
    for level in 2..=5 {
        r = admissao::admitir(&store, &proxima(&r, Tipo::Delegacao, &format!("filha-{level}"))).unwrap();
        assert_eq!(r.level, level);
    }
    assert_eq!(
        admissao::admitir(&store, &proxima(&r, Tipo::Delegacao, "sexta")).unwrap_err().downcast_ref(),
        Some(&Recusa::Nivel)
    );
    let c = admissao::admitir(&store, &proxima(&r, Tipo::Continuacao, "continuação")).unwrap();
    assert_eq!((c.level, c.ordinal), (5, 6));
}

#[test]
fn corrida_em_49_nunca_chega_a_51() {
    let store = Arc::new(novo_store());
    let root = admissao::admitir(&store, &raiz("raiz")).unwrap();
    for n in 2..=49 {
        admissao::admitir(&store, &proxima(&root, Tipo::Continuacao, &format!("tentativa-{n}"))).unwrap();
    }
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|n| {
            let (store, barrier, root) = (store.clone(), barrier.clone(), root.clone());
            std::thread::spawn(move || {
                let request = proxima(&root, Tipo::Continuacao, &format!("concorrente-{n}"));
                barrier.wait();
                for _ in 0..1000 {
                    match admissao::admitir(&store, &request) {
                        Err(e) if e.downcast_ref() == Some(&CoreError::StoreBusy) => {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        result => return result,
                    }
                }
                panic!("admissão permaneceu ocupada")
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().filter(|r| r.as_ref().is_err_and(|e| e.downcast_ref() == Some(&Recusa::Execucoes))).count(),
        1
    );
    assert_eq!(total(&store, &root.chain_id), 50);
}

#[test]
fn falha_no_insert_reverte_contador_e_recibo_e_busy_nao_admite() {
    let store = novo_store();
    let r = admissao::admitir(&store, &raiz("raiz")).unwrap();
    store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER recusar_tentativa BEFORE INSERT ON donna_chain_admissions
        WHEN NEW.kind='continuation' BEGIN SELECT RAISE(ABORT,'falha sintética'); END;",
        )
        .unwrap();
    assert!(admissao::admitir(&store, &proxima(&r, Tipo::Continuacao, "falha")).is_err());
    assert_eq!(total(&store, &r.chain_id), 1);
    let lock = store.db.locked();
    assert_eq!(admissao::admitir(&store, &raiz("ocupado")).unwrap_err().downcast_ref(), Some(&CoreError::StoreBusy));
    drop(lock);
    assert_eq!(total(&store, &r.chain_id), 1);
}

#[test]
fn seam_cobra_cinco_por_admissao_e_duas_por_fase_sem_ultrapassar_64() {
    let store = novo_store();
    let p = raiz("orçamento");
    let result = with_store_tx(&store, |tx| {
        // 12 * 5 = 60. Replay pode usar menos SQL, mas nunca recebe crédito de volta.
        let mut receipt = None;
        for _ in 0..12 {
            receipt = Some(tx.chain_admit(&p)?.unwrap());
        }
        let receipt = receipt.unwrap();
        tx.chain_finish(&receipt)?; // 62
        tx.chain_finish(&receipt)?; // 64
        tx.chain_dispatch(&receipt).map(|_| ()) // excede: toda a Tx volta
    });
    assert_eq!(result, Err(CoreError::TransactionLimit));
    let count: i64 = store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn despacho_exige_run_ligada_e_um_unico_cas_publicacao_nao_gasta() {
    let store = novo_store();
    let r = admissao::admitir(&store, &raiz("raiz")).unwrap();
    assert!(admissao::despachar(&store, &r).is_err());
    assert_eq!(admissao::fase(&store, &r).unwrap(), Fase::Reservada);
    let raw = serde_json::json!({"runs":[{"id":r.run_id,"_chain":r}]}).to_string();
    store.guard_routines_commit(&raw, std::iter::empty()).unwrap();
    admissao::despachar(&store, &r).unwrap();
    assert!(admissao::despachar(&store, &r).is_err());
    assert_eq!(admissao::fase(&store, &r).unwrap(), Fase::EnvioPossivel);
    admissao::concluir(&store, &r).unwrap();
    admissao::concluir(&store, &r).unwrap();
    assert_eq!(total(&store, &r.chain_id), 1);
}

#[test]
fn migracao_segunda_abertura_conserva_contadores() {
    let dir = std::env::temp_dir().join(format!("donna-cadeias-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let store = Store::open(&path).unwrap();
    let r = admissao::admitir(&store, &raiz("raiz")).unwrap();
    admissao::admitir(&store, &proxima(&r, Tipo::Continuacao, "segunda")).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(total(&store, &r.chain_id), 2);
    assert_eq!(admissao::admitir(&store, &raiz("raiz")).unwrap(), r);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn continuacao_recusa_escopo_alterado_e_predecessor_divergente_da_raiz() {
    let store = novo_store();
    let mut root = raiz("raiz-space");
    root.escopo = Escopo::ConversaDoBot { bot_id: "bot".into(), space_id: Some("space-original".into()) };
    let first = admissao::admitir(&store, &root).unwrap();
    let missing = proxima(&first, Tipo::Continuacao, "scope-default");
    assert_eq!(admissao::admitir(&store, &missing).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    assert_eq!(total(&store, &first.chain_id), 1);

    let mut valid = proxima(&first, Tipo::Continuacao, "retomada-valida");
    valid.escopo = root.escopo.clone();
    let resumed = admissao::admitir(&store, &valid).unwrap();
    let mut changed = proxima(&resumed, Tipo::Continuacao, "outro-space");
    changed.escopo = Escopo::ConversaDoBot { bot_id: "bot".into(), space_id: Some("space-alterado".into()) };
    assert_eq!(admissao::admitir(&store, &changed).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    // A variante futura de delegação não concede a travessia de contexto.
    changed.tipo = Tipo::Delegacao;
    assert_eq!(admissao::admitir(&store, &changed).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    changed.tipo = Tipo::Continuacao;
    changed.candidato.bot_id = "outro-bot".into();
    changed.escopo = Escopo::ConversaDoBot { bot_id: "outro-bot".into(), space_id: Some("space-original".into()) };
    assert_eq!(admissao::admitir(&store, &changed).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));

    // Mesmo herdando o predecessor, sua divergência da raiz deve ser recusada pelo JOIN SQL.
    changed.candidato.bot_id = "bot".into();
    changed.escopo = Escopo::ConversaDoBot { bot_id: "bot".into(), space_id: Some("space-alterado".into()) };
    store
        .db
        .locked()
        .execute(
            "UPDATE donna_chain_admissions SET scope=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&changed.escopo).unwrap(), resumed.admission_id],
        )
        .unwrap();
    assert!(admissao::contexto(&store, &resumed).is_err());
    assert_eq!(admissao::despachar(&store, &resumed).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    assert_eq!(admissao::admitir(&store, &changed).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    assert_eq!(total(&store, &first.chain_id), 2);
    let admissions: i64 = store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_chain_admissions WHERE chain_id=?1", [&first.chain_id], |row| row.get(0))
        .unwrap();
    assert_eq!(admissions, 2);
}
