use super::*;
use std::time::Duration;

fn porta() -> Porta<'static> {
    Porta::fixture("bot-fixture", "concessao-fixture", "modelo-fixture", Duration::from_secs(5))
}

fn admitir<'a>(porta: &'a Porta<'static>) -> Result<Vaga<'a, 'static>, Recusa> {
    porta.admitir("bot-fixture", "concessao-fixture", "modelo-fixture")
}

#[test]
fn sem_ativacao_nao_admite() {
    assert!(matches!(admitir(&Porta::fechada()), Err(Recusa::Fechada)));
}

#[tokio::test(start_paused = true)]
async fn janela_expirada_nao_admite() {
    let porta = porta();
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(matches!(admitir(&porta), Err(Recusa::Expirada)));
    assert!(matches!(admitir(&porta), Err(Recusa::Fechada)));
}

#[test]
fn bot_divergente_nao_admite() {
    assert!(matches!(porta().admitir("outro-bot", "concessao-fixture", "modelo-fixture"), Err(Recusa::Escopo)));
}

#[test]
fn concessao_divergente_nao_admite() {
    assert!(matches!(porta().admitir("bot-fixture", "outra-concessao", "modelo-fixture"), Err(Recusa::Escopo)));
}

#[test]
fn modelo_divergente_nao_admite() {
    assert!(matches!(porta().admitir("bot-fixture", "concessao-fixture", "outro-modelo"), Err(Recusa::Escopo)));
}

#[test]
fn segundo_turno_concorrente_nao_ocupa_a_vaga() {
    let porta = porta();
    let primeira = admitir(&porta).unwrap();
    std::thread::scope(|s| {
        s.spawn(|| assert!(matches!(admitir(&porta), Err(Recusa::Ocupada)))).join().unwrap();
    });
    drop(primeira);
    assert!(admitir(&porta).is_ok());
}

#[tokio::test]
async fn revogacao_antes_do_inicio_impede_a_tentativa() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    porta.revogar().unwrap();
    assert_eq!(vaga.conferir(), Err(Recusa::Fechada));
    let mut tentativas = 0;
    assert_eq!(vaga.enviar(|| async { tentativas += 1 }).await, Err(Recusa::Fechada));
    assert_eq!(tentativas, 0);
    drop(vaga);
    assert!(matches!(admitir(&porta), Err(Recusa::Fechada)));
}

#[tokio::test(start_paused = true)]
async fn expirar_entre_admissao_e_inicio_impede_a_tentativa() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    tokio::time::advance(Duration::from_secs(5)).await;
    let mut tentativas = 0;
    assert_eq!(vaga.enviar(|| async { tentativas += 1 }).await, Err(Recusa::Expirada));
    assert_eq!(tentativas, 0);
    drop(vaga);
    assert!(matches!(admitir(&porta), Err(Recusa::Fechada)));
}

#[tokio::test]
async fn revogacao_depois_do_inicio_nega_conclusao() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    let mut tentativas = 0;
    vaga.enviar(|| async { tentativas += 1 }).await.unwrap();
    porta.revogar().unwrap();
    assert_eq!(vaga.concluir(), Err(Recusa::Fechada));
    assert_eq!(tentativas, 1);
    assert!(matches!(admitir(&porta), Err(Recusa::Fechada)));
}

#[tokio::test]
async fn nao_repete_tentativa_na_mesma_vaga() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    let mut tentativas = 0;
    vaga.enviar(|| async { tentativas += 1 }).await.unwrap();
    assert_eq!(vaga.enviar(|| async { tentativas += 1 }).await, Err(Recusa::JaIniciada));
    assert_eq!(tentativas, 1);
    assert_eq!(vaga.concluir(), Ok(()));
    assert!(admitir(&porta).is_ok());
}

#[test]
fn vaga_sem_tentativa_nao_pode_concluir() {
    assert_eq!(admitir(&porta()).unwrap().concluir(), Err(Recusa::NaoIniciada));
}

#[tokio::test]
async fn tentativa_abandonada_fecha_a_porta() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    vaga.enviar(|| async {}).await.unwrap();
    drop(vaga);
    assert!(matches!(admitir(&porta), Err(Recusa::Fechada)));
}

#[tokio::test]
async fn revogacao_cancela_tentativa_pendente_sem_entregar_resposta() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    let iniciou = std::cell::Cell::new(false);
    let mut envio = std::pin::pin!(vaga.enviar(|| async {
        iniciou.set(true);
        std::future::pending::<()>().await;
    }));
    assert!(futures::poll!(&mut envio).is_pending());
    porta.revogar().unwrap();
    assert_eq!(envio.await, Err(Recusa::Fechada));
    assert!(iniciou.get());
}

#[tokio::test]
async fn revogacao_impede_novo_poll_de_tentativa_pendente() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    let polls = std::cell::Cell::new(0);
    let mut envio = std::pin::pin!(vaga.enviar(|| std::future::poll_fn(|_| {
        polls.set(polls.get() + 1);
        Poll::<()>::Pending
    })));
    assert!(futures::poll!(&mut envio).is_pending());
    porta.revogar().unwrap();
    assert_eq!(envio.await, Err(Recusa::Fechada));
    assert_eq!(polls.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn prazo_cancela_tentativa_pendente() {
    let porta = porta();
    let mut vaga = admitir(&porta).unwrap();
    let mut envio = std::pin::pin!(vaga.enviar(std::future::pending::<()>));
    assert!(futures::poll!(&mut envio).is_pending());
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(envio.await, Err(Recusa::Expirada));
    assert!(matches!(admitir(&porta), Err(Recusa::Fechada)));
}
