//! Rotina «só quando eu mandar»: a execução automática já reservada não chega à IA depois de a rotina virar manual.
use super::{Status, new_run};
use crate::{LockExt, hub::Hub, store::Store};
use serde_json::{Value, json};
use std::sync::Arc;

fn hub() -> Arc<Hub> {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.save_bot(&serde_json::from_value(json!({"id":"bot","name":"Bot"})).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!("rotina-manual-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    Hub::new(store, "fixture".into(), crate::remote::identity::Identity::load_or_create(&dir).unwrap(), "f".into(), 0)
}

fn agendada(hub: &Hub) -> String {
    let body =
        json!({"name":"Relatório","instruction":"Montar o relatório","triggers":[{"type":"interval","seconds":300}]});
    hub.routines.save(hub, "bot", &body).unwrap()["routine"]["id"].as_str().unwrap().to_owned()
}

fn reserva(hub: &Hub, id: &str, event: Value) -> String {
    hub.routines
        .change(&hub.store, |s| {
            let r = s.routines.iter().find(|r| r.id == id).unwrap().clone();
            let mut run = new_run(&r, event, None);
            run.status = Status::Starting;
            s.runs.push(run);
            Ok(())
        })
        .unwrap();
    hub.routines.state.locked().runs.last().unwrap().id.clone()
}

fn vira_manual(hub: &Hub, id: &str) {
    let body = json!({"id":id,"name":"Relatório","instruction":"Montar o relatório","manual":true,"triggers":[]});
    hub.routines.save(hub, "bot", &body).unwrap();
}

fn status(hub: &Hub, run: &str) -> Status {
    hub.routines.state.locked().runs.iter().find(|r| r.id == run).unwrap().status
}

/// Reserva como o agendador entrega ao ator: recibo de admissão em mãos e run em Starting.
fn reserva_com_recibo(hub: &Hub, id: &str, event: Value, test: bool) -> String {
    let result = hub.routines.enqueue(&hub.store, "bot", id, event, None, test).unwrap();
    let run_id = result["run"]["id"].as_str().unwrap().to_owned();
    hub.routines
        .change(&hub.store, |s| {
            s.runs.iter_mut().find(|r| r.id == run_id).unwrap().status = Status::Starting;
            Ok(())
        })
        .unwrap();
    run_id
}

#[test]
fn execucao_automatica_reservada_nao_chega_a_ia_depois_de_virar_manual() {
    let hub = hub();
    let id = agendada(&hub);
    let run = reserva(&hub, &id, json!({"source":"schedule"}));
    vira_manual(&hub, &id);
    assert!(hub.routines.begin(&hub, "bot", &run).unwrap().is_none(), "nada chega ao modelo");
    assert!(status(&hub, &run) == Status::Cancelled);
}

#[test]
fn preparo_da_tentativa_tambem_descarta_a_execucao_automatica_de_rotina_manual() {
    let hub = hub();
    let id = agendada(&hub);
    let run = reserva(&hub, &id, json!({"source":"webhook"}));
    vira_manual(&hub, &id);
    assert!(hub.routines.prepare_attempt(&hub.store, "bot", &run).is_err());
    assert!(status(&hub, &run) == Status::Cancelled, "descartada, não interrompida nem enviada");
}

#[test]
fn pedido_manual_do_dono_ou_do_bot_continua_rodando_em_rotina_manual() {
    let hub = hub();
    let id = agendada(&hub);
    vira_manual(&hub, &id);
    let run = reserva(&hub, &id, json!({"source":"test"}));
    assert!(hub.routines.begin(&hub, "bot", &run).unwrap().is_some(), "Rodar agora e run_routine seguem valendo");
}

#[test]
fn rotina_com_horario_nao_muda_o_despacho_da_execucao_reservada() {
    let hub = hub();
    let id = agendada(&hub);
    let run = reserva(&hub, &id, json!({"source":"schedule"}));
    assert!(hub.routines.begin(&hub, "bot", &run).unwrap().is_some());
}

#[test]
fn confirmacao_de_inicio_cancela_a_execucao_automatica_de_rotina_virada_manual() {
    let hub = hub();
    let id = agendada(&hub);
    let run = reserva_com_recibo(&hub, &id, json!({"source":"schedule"}), false);
    let receipt = hub.routines.prepare_attempt(&hub.store, "bot", &run).unwrap();
    vira_manual(&hub, &id);
    let started = hub.routines.mark_started(&hub.store, "bot", &run, &receipt).unwrap();
    assert!(matches!(started, crate::routines::Inicio::Cancelada), "decisão explícita, não erro");
    assert!(status(&hub, &run) == Status::Cancelled, "cancelada na mesma passada da conferência do recibo");
    // O despacho não roda: a reserva da cadeia continua como estava.
    assert_eq!(
        crate::fork::cadeias::admissao::fase(&hub.store, &receipt).unwrap(),
        crate::fork::cadeias::modelo::Fase::Reservada
    );
}

#[test]
fn pedido_manual_confirma_o_inicio_mesmo_depois_de_virar_manual() {
    let hub = hub();
    let id = agendada(&hub);
    let run = reserva_com_recibo(&hub, &id, json!({"source":"test"}), true);
    let receipt = hub.routines.prepare_attempt(&hub.store, "bot", &run).unwrap();
    vira_manual(&hub, &id);
    let started = hub.routines.mark_started(&hub.store, "bot", &run, &receipt).unwrap();
    assert!(matches!(started, crate::routines::Inicio::Iniciada), "«Rodar agora» segue valendo");
    assert!(status(&hub, &run) == Status::Running);
    assert_eq!(
        crate::fork::cadeias::admissao::fase(&hub.store, &receipt).unwrap(),
        crate::fork::cadeias::modelo::Fase::EnvioPossivel,
        "o recibo manual é despachado normalmente"
    );
}

#[test]
fn rotina_automatica_comum_confirma_o_inicio_normalmente() {
    let hub = hub();
    let id = agendada(&hub);
    let run = reserva_com_recibo(&hub, &id, json!({"source":"schedule"}), false);
    let receipt = hub.routines.prepare_attempt(&hub.store, "bot", &run).unwrap();
    let started = hub.routines.mark_started(&hub.store, "bot", &run, &receipt).unwrap();
    assert!(matches!(started, crate::routines::Inicio::Iniciada));
    assert!(status(&hub, &run) == Status::Running);
}
