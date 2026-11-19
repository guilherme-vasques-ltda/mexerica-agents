//! D70 do R0: fronteiras D220 do ramo entre bots, sem motor. O caminho real com
//! `ExecutionRef`, avisos e fila fica em `chat/team/tests.rs`.
#![allow(clippy::unwrap_used)]
use super::{
    admissao,
    equipe::{Entrada, RecusaEquipe, admitir_na},
    modelo::{Escopo, Origem, Recibo, Recusa, Tipo},
};
use crate::{LockExt, store::Store};
use anyhow::Result;
use std::{
    path::Path,
    sync::{Arc, Barrier},
};

/// Run de Rotina real do produtor (fonte manual), predecessor do primeiro repasse.
fn rotina(store: &Store) -> Recibo {
    let mut p =
        admissao::pedido(Tipo::Raiz, "ocorrência".into(), admissao::digest(&"intenção").unwrap(), "a", "run", None);
    p.origem = Origem::Manual;
    p.rotina = Some("rotina".into());
    p.escopo = Escopo::ConversaDoBot { bot_id: "a".into(), space_id: Some("space".into()) };
    admissao::admitir(store, &p).unwrap()
}

fn repassar(store: &Store, de: &str, para: &str, anterior: Option<&Recibo>) -> Result<Option<Recibo>> {
    let id = uuid::Uuid::new_v4().to_string();
    let entrada = Entrada::nova(de, para, &id, "ask_bot", "pedido", None, anterior.cloned())?;
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let receipt = admitir_na(&tx, &entrada, Some("space"), None)?;
    tx.commit()?;
    Ok(receipt)
}

fn recusa(result: Result<Option<Recibo>>) -> Recusa {
    result.unwrap_err().downcast_ref::<RecusaEquipe>().unwrap().0
}

fn contar(store: &Store, chain: &str) -> (i64, i64) {
    let db = store.db.locked();
    let used = db.query_row("SELECT used FROM donna_chain_roots WHERE id=?1", [chain], |r| r.get(0)).unwrap();
    let rows =
        db.query_row("SELECT COUNT(*) FROM donna_chain_admissions WHERE chain_id=?1", [chain], |r| r.get(0)).unwrap();
    (used, rows)
}

#[test]
fn heranca_entre_bots_vai_ate_cinco_niveis_e_recusa_o_sexto_sem_gravar() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let root = rotina(&store);
    let bots = ["a", "b", "c", "d", "e", "f"];
    let mut atual = root.clone();
    for (n, par) in (2_i64..).zip(bots.windows(2).take(4)) {
        atual = repassar(&store, par[0], par[1], Some(&atual)).unwrap().unwrap();
        // Outro bot herda a cadeia: nada de raiz nova nem contador zerado.
        assert_eq!((atual.chain_id.as_str(), atual.bot_id.as_str()), (root.chain_id.as_str(), par[1]));
        assert_eq!((atual.level, atual.ordinal), (n, n));
    }
    assert_eq!(atual.level, 5);
    assert_eq!(recusa(repassar(&store, "e", "f", Some(&atual))), Recusa::Nivel);
    assert_eq!(contar(&store, &root.chain_id), (5, 5));
    // A linha do repasse conserva a fonte real da Rotina que originou a cadeia.
    let fontes: Vec<String> = store
        .db
        .locked()
        .prepare("SELECT DISTINCT source FROM donna_chain_admissions WHERE chain_id=?1")
        .unwrap()
        .query_map([&root.chain_id], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(fontes, ["manual"]);
}

#[test]
fn ramos_irmaos_dividem_cinquenta_execucoes_e_a_corrida_em_49_admite_um() {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let root = rotina(&store);
    // Raiz + 47 ramos do mesmo predecessor = 48; o 49º fica por conta da corrida abaixo.
    for _ in 0..47 {
        repassar(&store, "a", "b", Some(&root)).unwrap().unwrap();
    }
    let segundo = repassar(&store, "a", "c", Some(&root)).unwrap().unwrap();
    assert_eq!(contar(&store, &root.chain_id), (49, 49));
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|n| {
            let (store, barrier, anterior) = (store.clone(), barrier.clone(), segundo.clone());
            std::thread::spawn(move || {
                barrier.wait();
                repassar(&store, "c", if n == 0 { "d" } else { "e" }, Some(&anterior))
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(contar(&store, &root.chain_id), (50, 50));
    let derrotado = results.into_iter().find(Result::is_err).unwrap();
    assert_eq!(recusa(derrotado), Recusa::Execucoes);
    // 51ª por outro ramo também recusa, sem gravar.
    assert_eq!(recusa(repassar(&store, "a", "b", Some(&root))), Recusa::Execucoes);
    assert_eq!(contar(&store, &root.chain_id), (50, 50));
}

#[test]
fn recibo_alheio_forjado_ou_de_outro_remetente_nao_abre_ramo() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let root = rotina(&store);
    let b = repassar(&store, "a", "b", Some(&root)).unwrap().unwrap();
    // O recibo de "b" não serve a "c"; nem o de Rotina de "a" a outro bot.
    assert_eq!(recusa(repassar(&store, "c", "d", Some(&b))), Recusa::ReciboInvalido);
    assert_eq!(recusa(repassar(&store, "b", "c", Some(&root))), Recusa::ReciboInvalido);
    let mut forjado = b.clone();
    forjado.level = 1;
    assert_eq!(recusa(repassar(&store, "b", "c", Some(&forjado))), Recusa::ReciboInvalido);
    assert_eq!(recusa(repassar(&store, "b", "b", Some(&b))), Recusa::ReciboInvalido);
    assert_eq!(contar(&store, &root.chain_id), (2, 2));
    // Guardas de Rotinas intactas: o recibo MCP não continua como Run nem cruza contexto.
    assert!(admissao::contexto(&store, &b).is_err());
    let mut rotinas = admissao::pedido(
        Tipo::Continuacao,
        "resume".into(),
        admissao::digest(&"r").unwrap(),
        "b",
        &b.run_id,
        Some(b.clone()),
    );
    rotinas.escopo = Escopo::ConversaDoBot { bot_id: "b".into(), space_id: Some("space".into()) };
    assert_eq!(admissao::admitir(&store, &rotinas).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    assert_eq!(contar(&store, &root.chain_id), (2, 2));
}

#[test]
fn legado_sem_execucao_nem_cadeia_nao_grava_e_contador_sobrevive_a_reabertura() {
    let dir = std::env::temp_dir().join(format!("donna-equipe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let store = Store::open(&path).unwrap();
    assert!(repassar(&store, "a", "b", None).unwrap().is_none());
    let roots: i64 = store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
    assert_eq!(roots, 0);
    let root = rotina(&store);
    let b = repassar(&store, "a", "b", Some(&root)).unwrap().unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let c = repassar(&store, "b", "c", Some(&b)).unwrap().unwrap();
    assert_eq!((c.chain_id.as_str(), c.level, c.ordinal), (root.chain_id.as_str(), 3, 3));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
