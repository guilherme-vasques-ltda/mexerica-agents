#[path = "../src/fork/fork-motor-macos/porta.rs"]
#[allow(dead_code)]
mod porta;

use std::{cell::Cell, num::NonZeroU64};

use porta::{Acao, Alvo, Decisao, Escopo, Evidencia, Instancia, Parada, Porta, Recusa};

fn id(valor: u64) -> NonZeroU64 {
    NonZeroU64::new(valor).expect("ID da fixture deve ser diferente de zero")
}

fn escopo() -> Escopo {
    Escopo { titular: id(1), dispositivo: id(2), bot: id(3), sessao: id(4), geracao: id(5) }
}

fn acao() -> Acao {
    Acao::Apagar(Alvo::Autorizavel(id(6)))
}

#[test]
fn aviso_nao_concede_e_so_aceite_do_escopo_abre_pedido() {
    let mut porta = Porta::nova(escopo());
    let outro = Escopo { bot: id(9), ..escopo() };
    assert_eq!(porta.pedir(acao(), 0), Err(Recusa::SemAceite));
    assert_eq!(porta.aceitar_aviso(outro), Err(Recusa::Escopo));
    porta.aceitar_aviso(escopo()).unwrap();
    assert_eq!(porta.pedir(Acao::Apagar(Alvo::Proibido), 0), Err(Recusa::AlvoProibido));
    let pedido = porta.pedir(acao(), 0).unwrap();
    // A interface do pedido é somente leitura: escopo e série saem da porta, nunca entram.
    assert_eq!(pedido.escopo(), escopo());
    assert_eq!(pedido.serie(), 1);
    assert_eq!(porta.resolver(pedido, outro, Decisao::UmaVez, 1), Err(Recusa::Escopo));
    assert_eq!(porta.efetuar(pedido, acao(), || 1, |_, _, _, _| true, |_| panic!()), Err(Recusa::SemDecisao));
}

#[test]
fn autoriza_uma_vez_a_acao_exata_e_audita_antes_do_efeito() {
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let a = Acao::Gravar { alvo: Alvo::Autorizavel(id(6)), conteudo: id(7) };
    let pedido = porta.pedir(a, 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).unwrap();
    let alterada = Acao::Gravar { alvo: Alvo::Autorizavel(id(6)), conteudo: id(8) };
    assert_eq!(porta.efetuar(pedido, alterada, || 2, |_, _, _, _| true, |_| panic!()), Err(Recusa::AcaoDiferente));
    let audita = Cell::new(false);
    let efeitos = Cell::new(0);
    assert_eq!(
        porta.efetuar(
            pedido,
            a,
            || 2,
            |pedido_auditado, auditada, evento, _| {
                assert_eq!(pedido_auditado, pedido);
                assert_eq!(auditada, a);
                assert_eq!(evento, porta::EventoAuditoria::Intencao);
                audita.set(true);
                true
            },
            |efetivada| {
                assert_eq!(efetivada, a);
                assert!(audita.get());
                efeitos.set(efeitos.get() + 1);
            },
        ),
        Ok(())
    );
    assert_eq!(efeitos.get(), 1);
    assert_eq!(porta.efetuar(pedido, a, || 3, |_, _, _, _| true, |_| panic!()), Err(Recusa::PedidoInvalido));
    assert_eq!(efeitos.get(), 1);

    let pedido = porta.pedir(a, 4).unwrap();
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 5).unwrap();
    let relogio = Cell::new(6);
    assert_eq!(
        porta.efetuar(
            pedido,
            a,
            || relogio.get(),
            |_, _, _, _| {
                relogio.set(3_006);
                false
            },
            |_| panic!(),
        ),
        Err(Recusa::Auditoria)
    );
    assert_eq!(porta.efetuar(pedido, a, || 3_007, |_, _, _, _| true, |_| panic!()), Err(Recusa::Fechada));
    assert_eq!(porta.estado(5_005, Evidencia::SemCenso), Parada::EmCurso);
    assert_eq!(porta.estado(5_006, Evidencia::SemCenso), Parada::NaoConfirmada);
}

#[test]
fn nega_por_expiracao_no_commit_pedido_recusado_e_reinicio() {
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let recusado = porta.pedir(acao(), 0).unwrap();
    porta.resolver(recusado, escopo(), Decisao::Negar, 2).unwrap();
    assert_eq!(porta.resolver(recusado, escopo(), Decisao::UmaVez, 3), Err(Recusa::PedidoInvalido));
    let vencido = porta.pedir(acao(), 10).unwrap();
    porta.resolver(vencido, escopo(), Decisao::UmaVez, 300_009).unwrap();
    assert_eq!(porta.efetuar(vencido, acao(), || 300_010, |_, _, _, _| true, |_| panic!()), Err(Recusa::Expirada));

    let antigo = porta.pedir(acao(), 300_011).unwrap();
    let novo = Escopo { geracao: id(10), ..escopo() };
    let mut reiniciada = Porta::nova(novo);
    reiniciada.aceitar_aviso(novo).unwrap();
    reiniciada.pedir(acao(), 0).unwrap();
    assert_eq!(reiniciada.resolver(antigo, novo, Decisao::UmaVez, 1), Err(Recusa::Escopo));
}

#[test]
fn revogar_fecha_porta_e_cancela_efeito_pendente_sem_resposta() {
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let aprovado = porta.pedir(acao(), 0).unwrap();
    porta.resolver(aprovado, escopo(), Decisao::UmaVez, 1).unwrap();
    porta.parar(2);
    assert_eq!(porta.efetuar(aprovado, acao(), || 2, |_, _, _, _| true, |_| panic!()), Err(Recusa::Fechada));
    assert_eq!(porta.pedir(acao(), 3), Err(Recusa::Fechada));
    assert_eq!(porta.aceitar_aviso(escopo()), Err(Recusa::Fechada));
}

#[test]
fn observacao_vazia_nao_prova_fim_e_sobrevivente_tem_identidade() {
    let lider = Instancia { pid: id(41), inicio: id(50) };
    let filho = Instancia { pid: id(42), inicio: id(51) };
    let reutilizado = Instancia { pid: filho.pid, inicio: id(52) };
    let mut s = Porta::nova(escopo());
    s.aceitar_aviso(escopo()).unwrap();
    let pendente = s.pedir(acao(), 0).unwrap();
    assert!(s.registrar(lider));
    assert!(s.registrar(filho));
    s.parar(10);
    s.parar(11);
    assert_eq!(s.resolver(pendente, escopo(), Decisao::UmaVez, 11), Err(Recusa::Fechada));
    assert!(!s.registrar(reutilizado));
    assert_eq!(s.estado(10, Evidencia::SemCenso), Parada::EmCurso);
    assert!(s.confirmar_saida(lider));
    assert!(!s.confirmar_saida(reutilizado));
    assert_eq!(s.estado(2_009, Evidencia::SemCenso), Parada::EmCurso);
    assert_eq!(s.estado(2_010, Evidencia::SemCenso), Parada::NaoConfirmada);
    assert_eq!(s.estado(2_010, Evidencia::Viva(filho)), Parada::Sobrevivente(filho));
    assert!(s.confirmar_saida(filho));
    assert_eq!(s.estado(2_011, Evidencia::SemCenso), Parada::NaoConfirmada);
    assert_eq!(s.estado(2_011, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaAtrasada);
}

#[test]
fn segundo_pedido_nao_substitui_o_que_esta_na_tela() {
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 1).unwrap();
    assert_eq!(porta.pedir(acao(), 2), Err(Recusa::Ocupada));
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 3).unwrap();
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 4), Err(Recusa::JaDecidida));
    porta.efetuar(pedido, acao(), || 4, |_, _, _, _| true, |_| {}).unwrap();
}

#[test]
fn prazo_e_rechecado_depois_da_auditoria() {
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).unwrap();
    let relogio = Cell::new(299_999);
    let registros = std::cell::RefCell::new(Vec::new());
    assert_eq!(
        porta.efetuar(
            pedido,
            acao(),
            || relogio.get(),
            |pedido_auditado, acao_auditada, evento, _| {
                assert_eq!(pedido_auditado, pedido);
                assert_eq!(acao_auditada, acao());
                registros.borrow_mut().push(evento);
                if evento == porta::EventoAuditoria::Intencao {
                    relogio.set(300_000);
                }
                true
            },
            |_| panic!(),
        ),
        Err(Recusa::Expirada)
    );
    assert_eq!(*registros.borrow(), [porta::EventoAuditoria::Intencao, porta::EventoAuditoria::ExpiradaNaoExecutada]);
}

#[test]
fn falha_no_registro_terminal_fecha_a_porta_sem_efeito() {
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).unwrap();
    let relogio = Cell::new(299_999);
    assert_eq!(
        porta.efetuar(
            pedido,
            acao(),
            || relogio.get(),
            |_, _, evento, _| {
                if evento == porta::EventoAuditoria::Intencao {
                    relogio.set(300_000);
                    true
                } else {
                    relogio.set(303_000);
                    false
                }
            },
            |_| panic!(),
        ),
        Err(Recusa::Auditoria)
    );
    assert_eq!(porta.pedir(acao(), 303_000), Err(Recusa::Fechada));
    assert_eq!(porta.estado(304_999, Evidencia::SemCenso), Parada::EmCurso);
    assert_eq!(porta.estado(305_000, Evidencia::SemCenso), Parada::NaoConfirmada);
}

#[test]
fn parada_confirmada_no_limite_exige_evidencia_e_saidas() {
    let mut porta = Porta::nova(escopo());
    assert_eq!(porta.estado(0, Evidencia::SemCenso), Parada::NaoSolicitada);
    porta.aceitar_aviso(escopo()).unwrap();
    let filho = Instancia { pid: id(70), inicio: id(71) };
    assert!(porta.registrar(filho));
    porta.parar(1);
    assert_eq!(porta.alvos_da_parada().unwrap().iter().copied().collect::<Vec<_>>(), vec![filho]);
    assert_eq!(porta.estado(2_001, Evidencia::UnidadeFechada(escopo())), Parada::NaoConfirmada);
    assert!(porta.confirmar_saida(filho));
    let anterior = Escopo { geracao: id(4), ..escopo() };
    assert_eq!(porta.estado(2_001, Evidencia::UnidadeFechada(anterior)), Parada::NaoConfirmada);
    assert_eq!(porta.estado(2_001, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaNoPrazo);
}
