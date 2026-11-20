//! O Heartbeat dentro das Rotinas: quem declara, quando liga e o que nunca roda. Só rotinas e dados fictícios.
use super::tests::routine;
use super::{Routine, Routines, Run, State, Status, Trigger, bound_save, new_run};
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
    assert!(ok(&estado_com(true, true), &renomeada).is_err(), "renomear é do dono");
    let mut chave = estado_com(true, true);
    chave.routines[0].webhook_key = "outra".into();
    assert!(ok(&estado_com(true, true), &chave).is_err(), "girar a chave é do dono");
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

// --- Agendador: a consulta à fonte, o estado e os avisos ---

use crate::fork::heartbeat::testes::Ficticia;
use crate::fork::heartbeat::{Fonte, Item, Motivo};
use std::sync::atomic::Ordering;

const PASSO: i64 = 300_000;

fn com_fonte() -> (Arc<Hub>, Arc<Ficticia>, String) {
    let hub = hub();
    let fonte = Arc::new(Ficticia::default());
    hub.routines.registrar_fonte("bot", "pedidos.ficticio", fonte.clone());
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    (hub, fonte, id)
}

/// Uma checagem no instante `volta * 5 min`, como o agendador a chamaria.
fn checa(hub: &Hub, id: &str, volta: i64) {
    hub.routines.check_due(&hub.store, &[id.to_owned()], volta * PASSO);
}

fn avisos(hub: &Hub) -> Vec<String> {
    hub.routines.publish(hub).unwrap();
    hub.store
        .entries_since(0, 1000)
        .unwrap()
        .into_iter()
        .filter(|e| e.id.starts_with("routine-result-"))
        .map(|e| e.data["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn execucoes(hub: &Hub) -> Vec<Run> {
    hub.routines.state.locked().runs.clone()
}

fn estado(hub: &Hub, id: &str) -> crate::fork::heartbeat::Estado {
    hub.routines.state.locked().routines.iter().find(|r| r.id == id).unwrap().heartbeat_state.clone().unwrap()
}

#[test]
fn sem_mudanca_nao_gera_mensagem_nem_execucao() {
    let (hub, fonte, id) = com_fonte();
    *fonte.marcador.lock().unwrap() = Some("m1".into());
    fonte.define(&[("1", "v1", "arquivado")]);
    for volta in 1..=10 {
        checa(&hub, &id, volta);
    }
    assert_eq!(avisos(&hub), Vec::<String>::new());
    assert_eq!(execucoes(&hub).len(), 1, "só a primeira leitura, que estabelece a base, fica no histórico");
    assert!(!execucoes(&hub)[0].report_pending);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), 1, "marcador igual: o conteúdo não é lido de novo");
    assert_eq!(estado(&hub, &id).ultima.unwrap().em, 10 * PASSO, "a última checagem é registrada mesmo em silêncio");
}

#[test]
fn a_mesma_pendencia_em_quatro_checagens_gera_um_aviso_so() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    for volta in 1..=4 {
        checa(&hub, &id, volta);
    }
    let avisos = avisos(&hub);
    assert_eq!(avisos.len(), 1);
    assert!(avisos[0].contains("Pedido 7 aguarda resposta") && avisos[0].contains("Aprovar ou recusar"), "{avisos:?}");
    assert_eq!(estado(&hub, &id).ultima.unwrap().abertas, 1);
}

#[test]
fn decisao_nova_avisa_de_novo_so_a_nova() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    checa(&hub, &id, 1);
    fonte.define(&[("7", "v1", "aguardando"), ("8", "v1", "aguardando")]);
    checa(&hub, &id, 2);
    let avisos = avisos(&hub);
    assert_eq!(avisos.len(), 2);
    assert!(avisos[1].contains("Pedido 8") && !avisos[1].contains("Pedido 7"), "{avisos:?}");
}

#[test]
fn historico_e_estado_nao_guardam_o_conteudo_da_fonte() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    checa(&hub, &id, 1);
    assert_eq!(avisos(&hub).len(), 1);
    let execucao = execucoes(&hub).pop().unwrap();
    assert!(!execucao.report_pending);
    let guardado = hub.store.kv_read("routines.v1").unwrap().unwrap();
    assert!(
        !guardado.contains("Pedido 7") && !guardado.contains("Aprovar"),
        "o conteúdo saiu do histórico ao publicar"
    );
    assert!(guardado.contains("needsOwner"), "mas o resultado fica");
}

#[test]
fn queda_da_fonte_e_falha_visivel_e_nao_sem_mudanca() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    checa(&hub, &id, 1);
    *fonte.falha.lock().unwrap() = Some(Motivo::SemAcesso);
    checa(&hub, &id, 2);
    let avisos = avisos(&hub);
    assert_eq!(avisos.len(), 2, "decisão e falha que exige ação: {avisos:?}");
    assert!(avisos[1].contains("sem acesso"), "{avisos:?}");
    let ultima = estado(&hub, &id).ultima.unwrap();
    assert_eq!(ultima.resultado, crate::fork::heartbeat::Resultado::FonteIndisponivel);
    assert_eq!(ultima.abertas, 1, "a falha não apaga o que se sabia");
    let rotina = hub.routines.list("bot")["routines"][0].clone();
    assert_eq!(rotina["heartbeatStatus"]["ultimaChecagem"]["resultado"], "fonte_indisponivel");
    assert_eq!(rotina["heartbeatStatus"]["falhasSeguidas"], 1);
}

#[test]
fn falha_repetida_recua_e_nao_repete_o_alerta_e_a_volta_nao_reavisa() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    checa(&hub, &id, 1);
    *fonte.falha.lock().unwrap() = Some(Motivo::SemAcesso);
    checa(&hub, &id, 2);
    let leituras = fonte.leituras.load(Ordering::SeqCst);
    let ate = estado(&hub, &id).nao_antes;
    assert!(ate > 2 * PASSO, "recuo depois da falha");
    hub.routines.check_due(&hub.store, std::slice::from_ref(&id), ate - 1);
    assert_eq!(estado(&hub, &id).falhas_seguidas, 1, "em recuo a fonte nem é consultada");
    hub.routines.check_due(&hub.store, std::slice::from_ref(&id), ate);
    assert_eq!(estado(&hub, &id).falhas_seguidas, 2);
    *fonte.falha.lock().unwrap() = None;
    hub.routines.check_due(&hub.store, std::slice::from_ref(&id), ate + 3_600_000 * 7);
    assert!(fonte.leituras.load(Ordering::SeqCst) > leituras);
    assert_eq!(avisos(&hub).len(), 2, "uma decisão e um alerta de falha; a volta não repete a pendência já avisada");
    assert_eq!(estado(&hub, &id).falhas_seguidas, 0);
}

#[test]
fn fonte_nao_concedida_ao_bot_e_indisponivel_e_nunca_e_lida() {
    let hub = hub();
    let fonte = Arc::new(Ficticia::default());
    hub.routines.registrar_fonte("outro-bot", "pedidos.ficticio", fonte.clone());
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    checa(&hub, &id, 1);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), 0);
    assert_eq!(avisos(&hub).len(), 1);
    assert_eq!(estado(&hub, &id).falhas_seguidas, 1);
}

#[test]
fn rotina_pausada_removida_ou_com_fonte_revogada_nao_consulta_mais() {
    let (hub, fonte, id) = com_fonte();
    checa(&hub, &id, 1);
    let leituras = fonte.leituras.load(Ordering::SeqCst);
    hub.routines.set_enabled(&hub.store, "bot", &id, false).unwrap();
    checa(&hub, &id, 2);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), leituras, "rotina pausada");
    hub.routines.set_enabled(&hub.store, "bot", &id, true).unwrap();
    hub.routines
        .change(&hub.store, |s| {
            s.paused = true;
            Ok(())
        })
        .unwrap();
    checa(&hub, &id, 3);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), leituras, "pausa global");
    hub.routines
        .change(&hub.store, |s| {
            s.paused = false;
            Ok(())
        })
        .unwrap();
    hub.routines.retirar_fonte("bot", "pedidos.ficticio");
    checa(&hub, &id, 4);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), leituras, "concessão revogada");
    hub.routines.remove(&hub.store, "bot", &id).unwrap();
    hub.routines.registrar_fonte("bot", "pedidos.ficticio", fonte.clone());
    checa(&hub, &id, 5);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), leituras, "rotina apagada");
}

#[test]
fn pausa_no_meio_da_leitura_descarta_o_resultado() {
    struct Gancho(Arc<Hub>, String);
    impl Fonte for Gancho {
        fn marcador(&self, _: &str) -> Result<Option<String>, Motivo> {
            self.0.routines.set_enabled(&self.0.store, "bot", &self.1, false).unwrap();
            Ok(None)
        }
        fn ler(&self, _: &str) -> Result<Vec<Item>, Motivo> {
            Ok(vec![Item {
                id: "1".into(),
                versao: "v".into(),
                estado: "aguardando".into(),
                fato: "x".into(),
                escolha: "y".into(),
            }])
        }
    }
    let hub = hub();
    let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
    hub.routines.registrar_fonte("bot", "pedidos.ficticio", Arc::new(Gancho(hub.clone(), id.clone())));
    checa(&hub, &id, 1);
    assert!(execucoes(&hub).is_empty());
    assert!(hub.routines.state.locked().routines[0].heartbeat_state.is_none());
    assert_eq!(avisos(&hub).len(), 0);
}

struct Quebrada;
impl Fonte for Quebrada {
    fn marcador(&self, _: &str) -> Result<Option<String>, Motivo> {
        panic!("fonte fictícia quebrada")
    }
    fn ler(&self, _: &str) -> Result<Vec<Item>, Motivo> {
        unreachable!()
    }
}

#[test]
fn rodada_sobreposta_e_recusada_e_a_vaga_volta_mesmo_se_a_fonte_der_panico() {
    let (hub, fonte, id) = com_fonte();
    hub.routines.checando.locked().insert(id.clone());
    checa(&hub, &id, 1);
    assert_eq!(fonte.leituras.load(Ordering::SeqCst), 0, "já há uma rodada desta rotina");
    hub.routines.checando.locked().remove(&id);
    hub.routines.registrar_fonte("bot", "pedidos.ficticio", Arc::new(Quebrada));
    checa(&hub, &id, 2);
    assert_eq!(estado(&hub, &id).falhas_seguidas, 1, "pânico da fonte vira falha visível");
    assert!(hub.routines.checando.locked().is_empty(), "a vaga é devolvida");
}

#[test]
fn dedup_e_ultima_checagem_sobrevivem_ao_reinicio() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    checa(&hub, &id, 1);
    assert_eq!(avisos(&hub).len(), 1);
    let reiniciado = Routines::default();
    reiniciado.restore(&hub.store).unwrap();
    reiniciado.registrar_fonte("bot", "pedidos.ficticio", fonte.clone());
    for volta in 2..=4 {
        reiniciado.check_due(&hub.store, std::slice::from_ref(&id), volta * PASSO);
    }
    let rotina = reiniciado.state.locked().routines[0].clone();
    assert_eq!(rotina.heartbeat_state.unwrap().ultima.unwrap().em, 4 * PASSO);
    assert_eq!(avisos(&hub).len(), 1, "depois do reinício a mesma pendência não é avisada de novo");
}

#[test]
fn o_agendador_dispara_a_consulta_sem_criar_execucao_com_ia() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    hub.routines
        .change(&hub.store, |s| {
            s.routines.iter_mut().find(|r| r.id == id).unwrap().next_runs = vec![Some(1)];
            Ok(())
        })
        .unwrap();
    hub.routines.schedule_due(&hub.store, 10_000_000).unwrap();
    let execucoes = execucoes(&hub);
    assert_eq!(execucoes.len(), 1);
    assert!(execucoes[0].heartbeat && execucoes[0].status == Status::Failed && execucoes[0].chain.is_none());
    assert_eq!(avisos(&hub).len(), 1);
}

#[test]
fn editar_a_declaracao_recomeca_o_estado_e_renomear_nao() {
    let (hub, fonte, id) = com_fonte();
    fonte.define(&[("7", "v1", "aguardando")]);
    checa(&hub, &id, 1);
    let mut corpo_igual = corpo(&json!({"id": id, "name": "Outro nome", "enabled": true}));
    corpo_igual.as_object_mut().unwrap().remove("heartbeat");
    hub.routines.save(&hub, "bot", &corpo_igual).unwrap();
    assert!(!estado(&hub, &id).avisadas.is_empty(), "renomear mantém o que a checagem sabe");
    let mut outra = declaracao();
    outra["escopo"] = json!("outra fila");
    hub.routines.save(&hub, "bot", &corpo(&json!({"id": id, "heartbeat": outra, "enabled": true}))).unwrap();
    assert!(hub.routines.state.locked().routines[0].heartbeat_state.is_none(), "outro escopo, outra base");
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
    let resposta = hub.routines.receive(&hub, &id, &cabecalhos, br#"{"text":"oi"}"#).unwrap();
    assert!(resposta["ignored"].is_string(), "a entrega autenticada é ignorada: {resposta}");
    assert!(hub.routines.state.locked().runs.is_empty());
}
