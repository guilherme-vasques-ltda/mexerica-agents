#[path = "../src/fork/fork-motor-macos/cofre.rs"]
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

use cofre::Item;
use partida::Capacidade;
use porta::{Acao, Alvo, Decisao, Evidencia, Instancia, Parada, Porta, Recusa};
use rusqlite::Connection;

const GRUPO_DO_APP: &str = "invalid.example.mexerica";

const ITEM_PROPRIO: Item<'_> = Item { grupo: GRUPO_DO_APP, identificador: "invalid.example.inferencia" };

const ITEM_OUTRO_APP: Item<'_> =
    Item { grupo: "invalid.example.outro", identificador: "invalid.example.segredo-alheio" };

fn id(valor: u64) -> NonZeroU64 {
    NonZeroU64::new(valor).expect("ID da fixture deve ser diferente de zero")
}

fn escopo() -> porta::Escopo {
    porta::Escopo { titular: id(1), dispositivo: id(2), bot: id(3), sessao: id(4), geracao: id(5) }
}

fn acao() -> Acao {
    Acao::Apagar(Alvo::Autorizavel(id(6)))
}

fn inferencia(item: Item<'_>) -> cofre::Inferencia<'_> {
    cofre::Inferencia { escopo: escopo(), provedor: id(7), item }
}

fn vinculo() -> cofre::Vinculo {
    cofre::Vinculo { escopo: escopo(), provedor: id(7) }
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
fn chamada_generica_de_leitura_ou_exportacao_nao_existe() {
    // A chamada genérica nega antes mesmo de olhar vínculo ou item.
    for item in [ITEM_PROPRIO, ITEM_OUTRO_APP] {
        assert_eq!(
            cofre::autorizar(GRUPO_DO_APP, vinculo(), cofre::Pedido::Leitura(item)),
            Err(cofre::Recusa::OperacaoGenerica)
        );
        assert_eq!(
            cofre::autorizar(GRUPO_DO_APP, vinculo(), cofre::Pedido::Exportacao(item)),
            Err(cofre::Recusa::OperacaoGenerica)
        );
    }
}

#[test]
fn vinculo_divergente_recusa_antes_do_item() {
    let escopo_de_outro_par = porta::Escopo { geracao: id(50), ..escopo() };
    let outro_vinculo = cofre::Vinculo { escopo: escopo_de_outro_par, provedor: id(7) };
    let outro_provedor = cofre::Vinculo { escopo: escopo(), provedor: id(70) };
    let ambos_divergentes = cofre::Vinculo { escopo: escopo_de_outro_par, provedor: id(70) };
    // Escopo errado, provedor errado ou os dois: a inferência não atravessa.
    for vinculo in [outro_vinculo, outro_provedor, ambos_divergentes] {
        assert_eq!(
            cofre::autorizar(GRUPO_DO_APP, vinculo, cofre::Pedido::Inferencia(inferencia(ITEM_PROPRIO))),
            Err(cofre::Recusa::Vinculo)
        );
    }
    // Autorização não é reaproveitável em outro par: nem com item do próprio app.
    assert_eq!(
        cofre::autorizar(GRUPO_DO_APP, outro_vinculo, cofre::Pedido::Inferencia(inferencia(ITEM_PROPRIO))),
        Err(cofre::Recusa::Vinculo)
    );
    // O vínculo nega antes de olhar o item, próprio ou alheio.
    assert_eq!(
        cofre::autorizar(GRUPO_DO_APP, ambos_divergentes, cofre::Pedido::Inferencia(inferencia(ITEM_OUTRO_APP))),
        Err(cofre::Recusa::Vinculo)
    );
}

#[test]
fn item_de_outro_app_nunca_e_atendido() {
    assert_eq!(
        cofre::autorizar(GRUPO_DO_APP, vinculo(), cofre::Pedido::Inferencia(inferencia(ITEM_OUTRO_APP))),
        Err(cofre::Recusa::OutroApp)
    );
}

#[test]
fn sempre_permitir_nunca_existira_para_shell() {
    // Shell é proibição categórica; as outras capacidades nem têm regra de escopo hoje.
    assert_eq!(cofre::regra_permanente(Capacidade::Shell), Err(cofre::RecusaRegra::SempreShell));
    for capacidade in [Capacidade::Mediada, Capacidade::Sudo, Capacidade::Tela] {
        assert_eq!(cofre::regra_permanente(capacidade), Err(cofre::RecusaRegra::SempreIndisponivel));
    }
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
    recusas::registrar(&db, pedido, recusas::Motivo::Fechada, 10).expect("persistir recusa do fechamento");
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 11), Err(Recusa::Fechada));
    assert_eq!(porta.pedir(acao(), 12), Err(Recusa::Fechada));
    assert!(!porta.registrar(Instancia { pid: id(43), inicio: id(52) }));
    // Sobrevivente visível com identidade; sem evidência, nunca sucesso falso.
    assert!(porta.confirmar_saida(lider));
    assert_eq!(porta.estado(1_000, Evidencia::Viva(filho)), Parada::Sobrevivente(filho));
    assert_eq!(porta.estado(1_000, Evidencia::SemCenso), Parada::EmCurso);
    assert_eq!(porta.estado(2_011, Evidencia::SemCenso), Parada::NaoConfirmada);
    // Com unidade fechada e saídas dentro do prazo de 2 s, a parada é confirmada.
    assert!(porta.confirmar_saida(filho));
    assert_eq!(porta.estado(2_000, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaNoPrazo);
    // A recusa do fechamento sobrevive ao reinício do banco.
    drop(db);
    let db = Connection::open(dir.path().join("recusas.sqlite3")).expect("reabrir banco da fixture");
    assert_eq!(linhas(&db), vec![(1, "fechada".to_owned(), 10)]);
}

#[test]
fn inferencia_vinculada_do_proprio_app_autoriza_sem_devolver_nada() {
    assert_eq!(cofre::autorizar(GRUPO_DO_APP, vinculo(), cofre::Pedido::Inferencia(inferencia(ITEM_PROPRIO))), Ok(()));
}
