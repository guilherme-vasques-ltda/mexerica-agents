//! Cenários de ponta a ponta da checagem silenciosa, com fonte fictícia e pelo agendador de verdade: o relógio é
//! simulado, mas cada passo passa pelo agendador (`schedule_due_com`, o mesmo caminho do `schedule_due` com o relógio injetado) e por `publish`, como no Hub em funcionamento. Nenhum dado real,
//! nenhuma rede, nenhuma IA.
use super::heartbeat_rotina_tests::{avisos, avisos_de, corpo, estado, execucoes, hub, salva};
use super::{Routines, Status};
use crate::LockExt;
use crate::fork::heartbeat::testes::Ficticia;
use crate::fork::heartbeat::{Motivo, Resultado};
use crate::hub::Hub;
use serde_json::json;
use std::sync::{Arc, atomic::Ordering};

const CINCO_MIN: i64 = 300_000;

/// Uma rotina de checagem a cada cinco minutos sobre uma fonte fictícia de pedidos de compra.
struct Cena {
    hub: Arc<Hub>,
    fonte: Arc<Ficticia>,
    id: String,
    agora: i64,
}

impl Cena {
    fn nova() -> Self {
        let hub = hub();
        let fonte = Arc::new(Ficticia::default());
        hub.routines.registrar_fonte("bot", "pedidos.ficticio", "conta-1", fonte.clone());
        let id = salva(&hub, &corpo(&json!({"enabled": true})))["id"].as_str().unwrap().to_owned();
        hub.routines
            .change(&hub.store, |s| {
                s.routines.iter_mut().find(|r| r.id == id).unwrap().next_runs = vec![Some(CINCO_MIN)];
                Ok(())
            })
            .unwrap();
        Self { hub, fonte, id, agora: 0 }
    }

    /// Anda `n` passos de cinco minutos, um tick do agendador por passo.
    fn anda(&mut self, passos: i64) {
        for _ in 0..passos {
            self.agora += CINCO_MIN;
            let agora = self.agora;
            self.hub.routines.schedule_due_com(&self.hub.store, agora, || agora).unwrap();
        }
    }

    /// Põe a próxima ocorrência no relógio simulado (religar e despausar recalculam a agenda pelo relógio real).
    fn rearma(&self) {
        let proxima = self.agora + CINCO_MIN;
        self.hub
            .routines
            .change(&self.hub.store, |s| {
                s.routines.iter_mut().find(|r| r.id == self.id).unwrap().next_runs = vec![Some(proxima)];
                Ok(())
            })
            .unwrap();
    }

    fn mensagens(&self) -> Vec<String> {
        avisos(&self.hub)
    }

    fn leituras(&self) -> usize {
        self.fonte.leituras.load(Ordering::SeqCst)
    }
}

#[test]
fn um_dia_sem_novidade_nao_manda_nenhuma_mensagem() {
    let mut cena = Cena::nova();
    *cena.fonte.marcador.lock().unwrap() = Some("v1".into());
    cena.fonte.define(&[("1", "v1", "arquivado"), ("2", "v1", "em andamento")]);
    cena.anda(288);
    assert_eq!(cena.mensagens(), Vec::<String>::new());
    assert_eq!(cena.leituras(), 1, "marcador igual: o conteúdo foi lido uma vez só");
    assert_eq!(estado(&cena.hub, &cena.id).ultima.unwrap().em, 288 * CINCO_MIN, "a última checagem fica registrada");
    assert_eq!(execucoes(&cena.hub).len(), 1, "só a leitura inicial, sem aviso, fica no histórico");
}

#[test]
fn pedido_aberto_por_quatro_checagens_gera_um_aviso() {
    let mut cena = Cena::nova();
    cena.fonte.define(&[("7", "v1", "aguardando")]);
    cena.anda(4);
    let mensagens = cena.mensagens();
    assert_eq!(mensagens.len(), 1, "{mensagens:?}");
    assert!(mensagens[0].contains("Pedido 7 aguarda resposta"), "o aviso diz o fato e a escolha: {mensagens:?}");
    assert!(mensagens[0].contains("Aprovar ou recusar"), "{mensagens:?}");
    assert_eq!(cena.leituras(), 4);
}

#[test]
fn decisao_nova_chama_de_novo_e_so_pela_nova() {
    let mut cena = Cena::nova();
    cena.fonte.define(&[("7", "v1", "aguardando")]);
    cena.anda(3);
    cena.fonte.define(&[("7", "v1", "aguardando"), ("8", "v1", "aguardando")]);
    cena.anda(3);
    let mensagens = cena.mensagens();
    assert_eq!(mensagens.len(), 2, "{mensagens:?}");
    assert!(mensagens[1].contains("Pedido 8") && !mensagens[1].contains("Pedido 7"), "{mensagens:?}");
}

#[test]
fn pedido_respondido_some_em_silencio_e_aberto_de_novo_e_decisao_nova() {
    let mut cena = Cena::nova();
    cena.fonte.define(&[("7", "v1", "aguardando")]);
    cena.anda(2);
    cena.fonte.define(&[("7", "v2", "aprovado")]);
    cena.anda(2);
    assert_eq!(cena.mensagens().len(), 1, "responder não gera aviso");
    assert_eq!(estado(&cena.hub, &cena.id).ultima.unwrap().abertas, 0);
    cena.fonte.define(&[("7", "v3", "aguardando")]);
    cena.anda(1);
    assert_eq!(cena.mensagens().len(), 2, "reaberto é decisão nova");
}

#[test]
fn queda_da_fonte_aparece_como_falha_e_a_volta_nao_repete_o_que_ja_foi_avisado() {
    let mut cena = Cena::nova();
    cena.fonte.define(&[("7", "v1", "aguardando")]);
    cena.anda(1);
    *cena.fonte.falha.lock().unwrap() = Some(Motivo::Fora);
    cena.anda(40);
    let mensagens = cena.mensagens();
    assert_eq!(mensagens.len(), 2, "o aviso da decisão e um alerta único de fonte fora do ar: {mensagens:?}");
    assert!(mensagens[1].contains("fora do ar"), "{mensagens:?}");
    let ultima = estado(&cena.hub, &cena.id).ultima.unwrap();
    assert_eq!(ultima.resultado, Resultado::FonteIndisponivel, "a falha nunca vira «sem mudança»");
    assert!(
        cena.hub.routines.list("bot")["routines"][0]["heartbeatStatus"]["falhasSeguidas"].as_u64().unwrap() >= 3,
        "a falha também aparece na rotina"
    );
    *cena.fonte.falha.lock().unwrap() = None;
    cena.anda(200);
    assert_eq!(cena.mensagens().len(), 2, "a fonte voltou: nada de aviso repetido");
    assert_eq!(estado(&cena.hub, &cena.id).falhas_seguidas, 0);
}

#[test]
fn perda_de_acesso_alerta_na_hora_uma_vez() {
    let mut cena = Cena::nova();
    *cena.fonte.falha.lock().unwrap() = Some(Motivo::SemAcesso);
    cena.anda(60);
    let mensagens = cena.mensagens();
    assert_eq!(mensagens.len(), 1, "{mensagens:?}");
    assert!(mensagens[0].contains("sem acesso"), "{mensagens:?}");
}

#[test]
fn pausa_apagar_e_concessao_retirada_param_de_consultar() {
    let mut cena = Cena::nova();
    cena.anda(2);
    let mut lidas = cena.leituras();
    assert!(lidas > 0);

    cena.hub.routines.set_enabled(&cena.hub.store, "bot", &cena.id, false).unwrap();
    cena.anda(5);
    assert_eq!(cena.leituras(), lidas, "rotina pausada");

    // Religar recalcula a agenda pelo relógio real; o cenário a rearma no relógio simulado e confere que voltou a ler.
    cena.hub.routines.set_enabled(&cena.hub.store, "bot", &cena.id, true).unwrap();
    cena.rearma();
    cena.anda(2);
    assert!(cena.leituras() > lidas, "religada, a rotina volta a consultar");
    lidas = cena.leituras();

    cena.hub.routines.set_paused(&cena.hub.store, true).unwrap();
    cena.anda(5);
    assert_eq!(cena.leituras(), lidas, "pausa geral das rotinas");

    cena.hub.routines.set_paused(&cena.hub.store, false).unwrap();
    cena.rearma();
    cena.anda(2);
    assert!(cena.leituras() > lidas, "sem a pausa geral, volta a consultar");
    lidas = cena.leituras();

    cena.hub.routines.retirar_fonte("bot", "pedidos.ficticio");
    cena.anda(5);
    assert_eq!(cena.leituras(), lidas, "concessão da fonte retirada");

    cena.hub.routines.registrar_fonte("bot", "pedidos.ficticio", "conta-1", cena.fonte.clone());
    cena.hub.routines.remove(&cena.hub.store, "bot", &cena.id).unwrap();
    cena.anda(5);
    assert_eq!(cena.leituras(), lidas, "rotina apagada");
}

#[test]
fn reiniciar_o_hub_no_meio_nao_repete_o_aviso_nem_perde_a_ultima_checagem() {
    let mut cena = Cena::nova();
    cena.fonte.define(&[("7", "v1", "aguardando")]);
    cena.anda(3);
    assert_eq!(cena.mensagens().len(), 1);
    let reiniciada = Routines::default();
    reiniciada.restore(&cena.hub.store).unwrap();
    reiniciada.registrar_fonte("bot", "pedidos.ficticio", "conta-1", cena.fonte.clone());
    for _ in 0..4 {
        cena.agora += CINCO_MIN;
        let agora = cena.agora;
        reiniciada.schedule_due_com(&cena.hub.store, agora, || agora).unwrap();
    }
    let rotina = reiniciada.state.locked().routines[0].clone();
    assert_eq!(
        rotina.heartbeat_state.unwrap().ultima.unwrap().em,
        cena.agora,
        "a checagem continuou depois do reinício"
    );
    assert_eq!(reiniciada.state.locked().runs.iter().filter(|r| r.report_pending).count(), 0);
    assert_eq!(avisos_de(&reiniciada, &cena.hub).len(), 1, "a mesma pendência não é avisada de novo");
}

#[test]
fn nada_nunca_chega_a_ia_nem_fica_pendente_e_o_conteudo_nao_fica_no_historico() {
    let mut cena = Cena::nova();
    cena.fonte.define(&[("7", "v1", "aguardando")]);
    cena.anda(2);
    *cena.fonte.falha.lock().unwrap() = Some(Motivo::SemAcesso);
    cena.anda(3);
    assert_eq!(cena.mensagens().len(), 2);
    let execucoes = execucoes(&cena.hub);
    assert!(!execucoes.is_empty());
    assert!(
        execucoes.iter().all(|r| r.heartbeat
            && !matches!(r.status, Status::Pending | Status::Starting | Status::Running | Status::Recovering)),
        "toda execução veio da checagem, já terminada, nenhuma entrou na fila de IA"
    );
    let guardado = cena.hub.store.kv_read("routines.v1").unwrap().unwrap();
    assert!(!guardado.contains("Pedido 7") && !guardado.contains("Aprovar"), "o histórico não guarda o conteúdo");
}

#[test]
fn um_dia_de_ponta_a_ponta() {
    // Uma manhã de pedidos de compra fictícios: tudo em silêncio, um pedido pede decisão, a fonte cai, volta, e
    // um segundo pedido aparece. O dono recebe exatamente três mensagens.
    let mut cena = Cena::nova();
    *cena.fonte.marcador.lock().unwrap() = Some("m1".into());
    cena.fonte.define(&[("1", "v1", "arquivado")]);
    cena.anda(24);
    assert_eq!(cena.mensagens().len(), 0, "duas horas sem novidade");
    *cena.fonte.marcador.lock().unwrap() = Some("m2".into());
    cena.fonte.define(&[("1", "v1", "arquivado"), ("2", "v1", "aguardando")]);
    cena.anda(12);
    assert_eq!(cena.mensagens().len(), 1, "um pedido novo pede decisão");
    *cena.fonte.falha.lock().unwrap() = Some(Motivo::SemAcesso);
    cena.anda(12);
    assert_eq!(cena.mensagens().len(), 2, "a perda de acesso é avisada uma vez");
    *cena.fonte.falha.lock().unwrap() = None;
    *cena.fonte.marcador.lock().unwrap() = Some("m3".into());
    cena.fonte.define(&[("1", "v1", "arquivado"), ("2", "v1", "aguardando"), ("3", "v1", "aguardando")]);
    cena.anda(60);
    let mensagens = cena.mensagens();
    assert_eq!(mensagens.len(), 3, "{mensagens:?}");
    assert!(mensagens[2].contains("Pedido 3") && !mensagens[2].contains("Pedido 2"), "{mensagens:?}");
}
