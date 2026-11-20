//! O Heartbeat dentro das Rotinas: quem declara, quando liga e o que nunca roda. Só rotinas e dados fictícios.
use super::tests::routine;
use super::{Routine, State, Status, Trigger, bound_save, new_run};
use crate::fork::heartbeat::{INSTRUCAO, SO_NO_HORARIO, SO_O_DONO};
use crate::{LockExt, hub::Hub, store::Store};
use serde_json::{Value, json};
use std::sync::Arc;

fn hub() -> Arc<Hub> {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.save_bot(&serde_json::from_value(json!({"id":"bot","name":"Bot"})).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!("heartbeat-rotina-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    Hub::new(store, "fixture".into(), crate::remote::identity::Identity::load_or_create(&dir).unwrap(), "f".into(), 0)
}

fn declaracao() -> Value {
    json!({"fonte":"pedidos.ficticio","escopo":"fila de compras","criterio":{"estados":["aguardando"]}})
}

fn corpo(extra: &Value) -> Value {
    let mut corpo = json!({"name":"Checar pedidos","instruction":"Procure o pedido e envie um e-mail","triggers":[{"type":"interval","seconds":300}],"heartbeat":declaracao()});
    for (chave, valor) in extra.as_object().unwrap() {
        corpo[chave] = valor.clone();
    }
    corpo
}

fn salva(hub: &Hub, body: &Value) -> Value {
    hub.routines.save(hub, "bot", body).unwrap()["routine"].clone()
}

fn heartbeat_pausado_ou_nao(enabled: Option<bool>) -> Value {
    let hub = hub();
    let extra = enabled.map_or_else(|| json!({}), |e| json!({"enabled": e}));
    salva(&hub, &corpo(&extra))
}

#[test]
fn checagem_nasce_pausada_e_so_liga_com_pedido_expresso_do_dono() {
    assert_eq!(heartbeat_pausado_ou_nao(None)["enabled"], false);
    assert_eq!(heartbeat_pausado_ou_nao(Some(false))["enabled"], false);
    assert_eq!(heartbeat_pausado_ou_nao(Some(true))["enabled"], true);
}

#[test]
fn rotina_comum_continua_nascendo_ligada() {
    let hub = hub();
    let comum =
        salva(&hub, &json!({"name":"Café","instruction":"Avisar","triggers":[{"type":"interval","seconds":300}]}));
    assert_eq!(comum["enabled"], true);
    assert!(comum["heartbeat"].is_null());
}

#[test]
fn instrucao_vira_texto_fixo_e_a_declaracao_aparece_na_rotina() {
    let hub = hub();
    let salva_ = salva(&hub, &corpo(&json!({"enabled": true})));
    assert_eq!(salva_["instruction"], INSTRUCAO);
    assert_eq!(salva_["heartbeat"]["fonte"], "pedidos.ficticio");
    assert_eq!(salva_["heartbeat"]["criterio"]["estados"], json!(["aguardando"]));
    assert_eq!(salva_["heartbeat"]["destinatario"], "dono");
    let listada = hub.routines.list("bot");
    assert_eq!(listada["routines"][0]["instruction"], INSTRUCAO);
}

#[test]
fn declaracao_incompleta_ou_com_permissao_escondida_recusa_o_corpo_inteiro() {
    let hub = hub();
    for ruim in [
        json!({"fonte":"a","escopo":"x"}),
        json!({"fonte":"a","escopo":"x","criterio":{"estados":["a"]},"enviar":true}),
        json!({"fonte":"a","escopo":"x","criterio":{"estados":["a"],"excluir":true}}),
        json!("texto"),
    ] {
        assert!(hub.routines.save(&hub, "bot", &corpo(&json!({"heartbeat": ruim}))).is_err());
    }
    assert_eq!(hub.routines.list("bot")["routines"].as_array().unwrap().len(), 0);
}

#[test]
fn checagem_so_aceita_horario_e_nao_webhook_nem_evento() {
    let hub = hub();
    for gatilho in [json!({"type":"webhook"}), json!({"type":"event","source":"github","event":"*"})] {
        let erro = hub.routines.save(&hub, "bot", &corpo(&json!({"triggers":[gatilho]}))).unwrap_err();
        assert_eq!(erro.to_string(), SO_NO_HORARIO);
    }
}

#[test]
fn editar_sem_o_campo_mantem_a_declaracao_e_null_a_remove() {
    let hub = hub();
    let criada = salva(&hub, &corpo(&json!({"enabled": true})));
    let id = criada["id"].clone();
    let renomeada = salva(
        &hub,
        &json!({"id":id,"name":"Outro nome","instruction":"x","triggers":[{"type":"interval","seconds":300}]}),
    );
    assert_eq!(renomeada["heartbeat"]["fonte"], "pedidos.ficticio");
    assert_eq!(renomeada["enabled"], true);
    let comum = salva(
        &hub,
        &json!({"id":id,"name":"Outro nome","instruction":"Avisar","triggers":[{"type":"interval","seconds":300}],"heartbeat":null}),
    );
    assert!(comum["heartbeat"].is_null());
    assert_eq!(comum["instruction"], "Avisar");
}

#[test]
fn o_modelo_nao_declara_nem_edita_checagem_pelo_mcp() {
    let hub = hub();
    for nome in ["save_routine", "duplicate_routine"] {
        let erro = crate::routines::call(&hub, "bot", nome, &corpo(&json!({"id":"x"}))).unwrap_err();
        assert_eq!(erro.to_string(), SO_O_DONO, "{nome}");
    }
    let erro = crate::routines::call(&hub, "bot", "save_routine", &corpo(&json!({}))).unwrap_err();
    assert_eq!(erro.to_string(), SO_O_DONO);
}

#[test]
fn o_modelo_nao_declara_checagem_pelo_time() {
    let hub = hub();
    let erro = hub.routines.save_team("bot", &[corpo(&json!({}))], |_, _| Ok(())).unwrap_err();
    assert_eq!(erro.to_string(), SO_O_DONO);
    assert_eq!(hub.routines.list("bot")["routines"].as_array().unwrap().len(), 0);
}

fn estado_com(heartbeat: bool, enabled: bool) -> State {
    let mut r: Routine = routine();
    r.enabled = enabled;
    if heartbeat {
        r.heartbeat = Some(crate::fork::heartbeat::Declaracao::parse(&declaracao()).unwrap());
    }
    State { routines: vec![r], ..State::default() }
}

#[test]
fn guarda_do_modelo_recusa_declarar_alterar_e_ligar_mas_deixa_pausar() {
    let ok = |antes: &State, depois: &State| bound_save::ensure_no_model_heartbeat(antes, depois);
    assert!(ok(&estado_com(false, true), &estado_com(false, true)).is_ok(), "rotina comum não muda");
    assert!(ok(&estado_com(true, true), &estado_com(true, true)).is_ok());
    assert!(ok(&estado_com(true, true), &estado_com(true, false)).is_ok(), "pausar é permitido");
    assert!(ok(&estado_com(true, false), &estado_com(true, true)).is_err(), "ligar é do dono");
    assert!(ok(&estado_com(false, true), &estado_com(true, false)).is_err(), "declarar é do dono");
    assert!(ok(&estado_com(true, false), &estado_com(false, false)).is_err(), "remover é do dono");
    let mut horario = estado_com(true, true);
    horario.routines[0].triggers = vec![Trigger::Interval { seconds: 1 }];
    assert!(ok(&estado_com(true, true), &horario).is_err(), "mudar o horário é do dono");
    let mut tempo = estado_com(true, true);
    tempo.routines[0].timeout_seconds += 1;
    assert!(ok(&estado_com(true, true), &tempo).is_err(), "mudar o tempo limite é do dono");
    let mut apagada = estado_com(true, true);
    apagada.routines[0].deleted = true;
    assert!(ok(&estado_com(true, true), &apagada).is_err(), "apagar é do dono");
    assert!(ok(&estado_com(true, true), &State::default()).is_err(), "sumir da lista também");
    let mut renomeada = estado_com(true, true);
    renomeada.routines[0].name = "Outro".into();
    assert!(ok(&estado_com(true, true), &renomeada).is_ok(), "o nome não importa");
    let mut outra = estado_com(true, true);
    outra.routines[0].heartbeat.as_mut().unwrap().escopo = "outro".into();
    assert!(ok(&estado_com(true, true), &outra).is_err(), "alterar a declaração é do dono");
}

#[test]
fn execucao_pendente_de_checagem_nunca_chega_a_ia() {
    let hub = hub();
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    hub.routines
        .change(&hub.store, |s| {
            let r = s.routines.iter().find(|r| r.id == id).unwrap().clone();
            s.runs.push(new_run(&r, Value::Null, None));
            Ok(())
        })
        .unwrap();
    let run = hub.routines.state.locked().runs[0].id.clone();
    assert!(hub.routines.begin(&hub, "bot", &run).unwrap().is_none());
    assert!(hub.routines.state.locked().runs[0].status == Status::Cancelled);
}

#[test]
fn disparo_manual_ou_por_webhook_de_checagem_e_recusado() {
    let hub = hub();
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    let erro = hub.routines.enqueue(&hub.store, "bot", &id, json!({"source":"test"}), None, true).unwrap_err();
    assert_eq!(erro.to_string(), SO_NO_HORARIO);
    assert!(hub.routines.state.locked().runs.is_empty());
}

#[test]
fn agendador_ainda_nao_cria_execucao_com_ia_para_checagem() {
    let hub = hub();
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    hub.routines
        .change(&hub.store, |s| {
            s.routines.iter_mut().find(|r| r.id == id).unwrap().next_runs = vec![Some(1)];
            Ok(())
        })
        .unwrap();
    hub.routines.schedule_due(&hub.store, 10_000).unwrap();
    assert!(hub.routines.state.locked().runs.is_empty());
}

#[test]
fn dono_duplica_a_checagem_com_a_declaracao_e_pausada() {
    let hub = hub();
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    let copia = hub.routines.duplicate(&hub, "bot", &id).unwrap()["routine"].clone();
    assert_eq!(copia["heartbeat"]["fonte"], "pedidos.ficticio");
    assert_eq!(copia["enabled"], false);
    assert_eq!(copia["instruction"], INSTRUCAO);
}

#[test]
fn converter_rotina_comum_cancela_pendentes_e_recusa_execucao_ja_reservada() {
    let hub = hub();
    let comum =
        salva(&hub, &json!({"name":"Café","instruction":"Avisar","triggers":[{"type":"interval","seconds":300}]}));
    let id = comum["id"].as_str().unwrap().to_owned();
    let empilha = |status: Status| {
        hub.routines
            .change(&hub.store, |s| {
                let r = s.routines.iter().find(|r| r.id == id).unwrap().clone();
                let mut run = new_run(&r, Value::Null, None);
                run.status = status;
                s.runs.push(run);
                Ok(())
            })
            .unwrap();
    };
    let converte = || hub.routines.save(&hub, "bot", &corpo(&json!({"id": id, "enabled": true})));
    empilha(Status::Starting);
    assert!(converte().is_err(), "execução reservada ainda poderia chegar à IA");
    hub.routines
        .change(&hub.store, |s| {
            s.runs.clear();
            Ok(())
        })
        .unwrap();
    empilha(Status::Pending);
    assert!(converte().is_ok());
    assert!(hub.routines.state.locked().runs.iter().all(|r| r.status == Status::Cancelled));
}

#[test]
fn rotina_criada_pelo_bot_nao_vira_checagem() {
    let hub = hub();
    let id = salva(&hub, &json!({"name":"Café","instruction":"Avisar","triggers":[{"type":"interval","seconds":300}]}))
        ["id"]
        .as_str()
        .unwrap()
        .to_owned();
    hub.routines
        .change(&hub.store, |s| {
            s.routines.iter_mut().find(|r| r.id == id).unwrap().creation_origin =
                Some(crate::fork::cadeias::descendencia_rotina::fixture_origin(
                    crate::fork::cadeias::modelo::Recibo {
                        chain_id: "c".into(),
                        admission_id: "a".into(),
                        run_id: "r".into(),
                        bot_id: "bot".into(),
                        level: 1,
                        ordinal: 1,
                    },
                    "personal",
                ));
            Ok(())
        })
        .unwrap();
    assert!(hub.routines.save(&hub, "bot", &corpo(&json!({"id": id, "enabled": true}))).is_err());
}

#[test]
fn webhook_recebido_nao_enfileira_execucao_de_checagem() {
    let hub = hub();
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    let chave = hub.routines.state.locked().routines[0].webhook_key.clone();
    let cabecalhos = std::collections::BTreeMap::from([("authorization".to_owned(), format!("Bearer {chave}"))]);
    let _ = hub.routines.receive(&hub, &id, &cabecalhos, br#"{"text":"oi"}"#);
    assert!(hub.routines.state.locked().runs.iter().all(|r| r.status == Status::Cancelled));
}
