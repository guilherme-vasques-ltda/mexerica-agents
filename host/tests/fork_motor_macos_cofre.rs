#[path = "../src/fork/fork-motor-macos/cofre.rs"]
#[allow(dead_code)]
mod cofre;
#[path = "../src/fork/fork-motor-macos/partida.rs"]
#[allow(dead_code)]
mod partida;
#[path = "../src/fork/fork-motor-macos/porta.rs"]
#[allow(dead_code)]
mod porta;
#[path = "../src/fork/fork-motor-macos/recusas.rs"]
#[allow(dead_code)]
mod recusas;

use std::num::NonZeroU64;

use cofre::{Autorizacao, Item, Pedido, Recusa, RecusaComposicao, Vinculo};
use partida::Capacidade;
use porta::{Acao, Alvo, Decisao, Escopo, Evidencia, Instancia, Parada, Porta, Recusa as RecusaPorta};
use rusqlite::Connection;

const GRUPO_DO_APP: &str = "invalid.example.mexerica";

const ITEM_PROPRIO: Item<'_> = Item { grupo: GRUPO_DO_APP, identificador: "invalid.example.inferencia" };

const ITEM_OUTRO_APP: Item<'_> =
    Item { grupo: "invalid.example.outro", identificador: "invalid.example.segredo-alheio" };

fn id(valor: u64) -> NonZeroU64 {
    NonZeroU64::new(valor).expect("ID da fixture deve ser diferente de zero")
}

fn escopo() -> Escopo {
    Escopo { titular: id(1), dispositivo: id(2), bot: id(3), sessao: id(4), geracao: id(5) }
}

fn acao() -> Acao {
    Acao::Apagar(Alvo::Autorizavel(id(6)))
}

fn vinculo() -> Vinculo<'static> {
    Vinculo::compor(GRUPO_DO_APP, escopo(), id(7), "invalid.example.inferencia").expect("compor vínculo da fixture")
}

fn inferencia(item: Item<'_>) -> cofre::Inferencia<'_> {
    cofre::Inferencia { escopo: escopo(), provedor: id(7), item }
}

#[test]
fn chamada_generica_de_leitura_ou_exportacao_nao_existe() {
    let mut cofre = cofre::Cofre::abrir(vinculo());
    // A chamada genérica nega antes de tudo — inclusive com o cofre fechado.
    cofre.parar();
    for item in [ITEM_PROPRIO, ITEM_OUTRO_APP] {
        assert_eq!(cofre.autorizar(Pedido::Leitura(item)), Err(Recusa::OperacaoGenerica));
        assert_eq!(cofre.autorizar(Pedido::Exportacao(item)), Err(Recusa::OperacaoGenerica));
    }
}

#[test]
fn vinculo_divergente_recusa_antes_do_item() {
    let cofre = cofre::Cofre::abrir(vinculo());
    let escopo_de_outro_par = Escopo { geracao: id(50), ..escopo() };
    let mut pedidos = vec![];
    for item in [ITEM_PROPRIO, ITEM_OUTRO_APP] {
        pedidos.push(cofre::Inferencia { escopo: escopo_de_outro_par, provedor: id(7), item });
        pedidos.push(cofre::Inferencia { escopo: escopo(), provedor: id(70), item });
    }
    for inferencia in pedidos {
        // Escopo errado, provedor errado ou os dois: a inferência não atravessa,
        // antes de olhar o item, próprio ou alheio.
        assert_eq!(cofre.autorizar(Pedido::Inferencia(inferencia)), Err(Recusa::Vinculo));
    }
}

#[test]
fn autorizacao_nao_atravessa_geracoes() {
    let cofre = cofre::Cofre::abrir(vinculo());
    // Pedido idêntico em tudo, menos na geração: recusa antes do item.
    let outra_geracao =
        cofre::Inferencia { escopo: Escopo { geracao: id(51), ..escopo() }, provedor: id(7), item: ITEM_PROPRIO };
    assert_eq!(cofre.autorizar(Pedido::Inferencia(outra_geracao)), Err(Recusa::Vinculo));
    // A autorização que sai é ligada à geração do vínculo, não à do pedido alheio.
    let autorizacao = cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))).expect("autorizar");
    assert_eq!(cofre.redimir(autorizacao).expect("redimir pela geração do vínculo"), id(5));
}

#[test]
fn item_de_outro_app_nunca_e_atendido() {
    let cofre = cofre::Cofre::abrir(vinculo());
    assert_eq!(cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_OUTRO_APP))), Err(Recusa::OutroApp));
}

#[test]
fn item_do_proprio_app_com_identidade_divergente_recusa() {
    let cofre = cofre::Cofre::abrir(vinculo());
    // Mesmo grupo, identificador diferente: a identidade do item é exata.
    let item_divergente = Item { grupo: GRUPO_DO_APP, identificador: "invalid.example.outro-item" };
    assert_eq!(cofre.autorizar(Pedido::Inferencia(inferencia(item_divergente))), Err(Recusa::ItemDivergente));
}

#[test]
fn parar_fecha_o_cofre_da_geracao_sem_reabrir() {
    let mut cofre = cofre::Cofre::abrir(vinculo());
    assert!(cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))).is_ok());
    // "Parar tudo" aplicado ao cofre: fecha o acesso da geração.
    cofre.parar();
    assert_eq!(cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))), Err(Recusa::Fechada));
    // Idempotente: parar de novo não reabre nem muda o estado.
    cofre.parar();
    assert_eq!(cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))), Err(Recusa::Fechada));
}

#[test]
fn composicao_do_vinculo_recusa_grupo_ou_identificador_vazios() {
    assert_eq!(Vinculo::compor("", escopo(), id(7), "invalid.example.inferencia"), Err(RecusaComposicao::GrupoVazio));
    assert_eq!(Vinculo::compor(GRUPO_DO_APP, escopo(), id(7), ""), Err(RecusaComposicao::IdentificadorVazio));
}

/// Ponto do efeito: redime a autorização por valor, sempre pelo cofre da
/// geração ainda aberto; reuso não compila (sem `Clone`/`Copy`).
fn ponto_do_efeito(cofre: &cofre::Cofre<'_>, autorizacao: Autorizacao) -> NonZeroU64 {
    cofre.redimir(autorizacao).expect("redimir no ponto do efeito")
}

#[test]
fn autorizacao_move_only_e_redimida_pelo_cofre_aberto() {
    let cofre = cofre::Cofre::abrir(vinculo());
    let autorizacao = cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))).expect("autorizar");
    assert_eq!(ponto_do_efeito(&cofre, autorizacao), id(5));
}

#[test]
fn autorizacao_emitida_antes_da_parada_morre_com_o_cofre() {
    let mut cofre = cofre::Cofre::abrir(vinculo());
    let autorizacao = cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))).expect("autorizar");
    cofre.parar();
    // Redenção só pelo cofre ainda aberto: o token emitido antes morre com a
    // parada, consumido por valor mesmo no fracasso — nunca pelo portador.
    assert_eq!(cofre.redimir(autorizacao), Err(Recusa::Fechada));
    assert_eq!(cofre.autorizar(Pedido::Inferencia(inferencia(ITEM_PROPRIO))), Err(Recusa::Fechada));
}

#[test]
fn sempre_permitir_nunca_existira_para_shell() {
    // Shell é proibição categórica; as outras capacidades nem têm regra de escopo hoje.
    assert_eq!(cofre::regra_permanente(Capacidade::Shell), Err(cofre::RecusaRegra::SempreShell));
    for capacidade in [Capacidade::Mediada, Capacidade::Sudo, Capacidade::Tela] {
        assert_eq!(cofre::regra_permanente(capacidade), Err(cofre::RecusaRegra::SempreIndisponivel));
    }
}

fn instancia() -> NonZeroU64 {
    NonZeroU64::new(9).expect("ID da fixture deve ser diferente de zero")
}

fn db_sintetico() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().expect("criar diretório da fixture");
    let db = Connection::open(dir.path().join("recusas.sqlite3")).expect("abrir banco da fixture");
    let modo: String =
        db.pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0)).expect("ligar WAL da fixture");
    assert_eq!(modo, "wal");
    recusas::preparar(&db).expect("preparar registro de recusas");
    (dir, db)
}

fn linhas(db: &Connection) -> Vec<(i64, String, i64)> {
    db.prepare("SELECT serie, motivo, instante_ms FROM recusas_motor_macos ORDER BY id")
        .expect("preparar consulta de recusas")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("consultar recusas")
        .collect::<rusqlite::Result<_>>()
        .expect("juntar recusas")
}

#[test]
fn parada_cancela_pendente_grava_fechada_e_mostra_sobrevivente() {
    let (dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    let lider = Instancia { pid: id(41), inicio: id(50) };
    let filho = Instancia { pid: id(42), inicio: id(51) };
    assert!(porta.registrar(lider));
    assert!(porta.registrar(filho));
    // "Parar tudo" não bloqueia: pendência cancelada na hora e porta fechada.
    porta.parar(10);
    recusas::registrar(&db, pedido, recusas::Caminho::Parar, recusas::Motivo::Fechada, instancia(), 10)
        .expect("persistir recusa do fechamento");
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 11), Err(RecusaPorta::Fechada));
    assert_eq!(porta.pedir(acao(), 12), Err(RecusaPorta::Fechada));
    assert!(!porta.registrar(Instancia { pid: id(43), inicio: id(52) }));
    // Sobrevivente visível com identidade; sem evidência, nunca sucesso falso.
    assert!(porta.confirmar_saida(lider));
    assert_eq!(porta.estado(1_000, Evidencia::Viva(filho)), Parada::Sobrevivente(filho));
    assert_eq!(porta.estado(1_500, Evidencia::SemCenso), Parada::EmCurso);
    // Com unidade fechada e saídas dentro do prazo de 2 s, a parada é confirmada.
    assert!(porta.confirmar_saida(filho));
    assert_eq!(porta.estado(2_000, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaNoPrazo);
    // Instantes crescentes: após o prazo, sem prova é incerteza e fechamento tardio é atraso.
    assert_eq!(porta.estado(2_011, Evidencia::SemCenso), Parada::NaoConfirmada);
    assert_eq!(porta.estado(2_011, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaAtrasada);
    // A recusa do fechamento sobrevive ao reinício do banco.
    drop(db);
    let db = Connection::open(dir.path().join("recusas.sqlite3")).expect("reabrir banco da fixture");
    assert_eq!(linhas(&db), vec![(1, "fechada".to_owned(), 10)]);
}
