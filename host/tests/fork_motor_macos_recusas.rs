#[path = "../src/fork/fork-motor-macos/porta.rs"]
#[allow(dead_code)]
mod porta;
#[path = "../src/fork/fork-motor-macos/recusas.rs"]
mod recusas;

use std::{cell::Cell, num::NonZeroU64};

use porta::{Acao, Alvo, Decisao, Escopo, Porta, Recusa};
use recusas::Motivo;
use rusqlite::Connection;

fn id(valor: u64) -> NonZeroU64 {
    NonZeroU64::new(valor).expect("ID da fixture deve ser diferente de zero")
}

fn escopo() -> Escopo {
    Escopo { titular: id(1), dispositivo: id(2), bot: id(3), sessao: id(4), geracao: id(5) }
}

fn acao() -> Acao {
    Acao::Apagar(Alvo::Autorizavel(id(6)))
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
fn pendencia_sem_resposta_expira_no_prazo_e_grava_duravel() {
    let (dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    // Sem resposta: no instante exato de 5 minutos a pendência já está expirada.
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 300_000), Err(Recusa::Expirada));
    recusas::registrar(&db, pedido, Motivo::Expirada, 300_000).expect("persistir recusa de expiração");
    // O pedido expirado não volta: sem retry e sem reabrir.
    assert_eq!(
        porta.efetuar(pedido, acao(), || 300_001, |_, _, _| true, |_| panic!("efeito de pedido expirado")),
        Err(Recusa::PedidoInvalido)
    );
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 300_002), Err(Recusa::PedidoInvalido));
    let novo = porta.pedir(acao(), 300_003).unwrap();
    assert_ne!(novo, pedido);
    // A recusa sobrevive ao reinício do banco.
    drop(db);
    let db = Connection::open(dir.path().join("recusas.sqlite3")).expect("reabrir banco da fixture");
    recusas::preparar(&db).expect("repreparar registro de recusas");
    assert_eq!(linhas(&db), vec![(1, "expirada".to_owned(), 300_000)]);
}

#[test]
fn auditoria_fora_do_prazo_nega_sem_efeito_sem_retry_e_sem_reabrir() {
    let (_dir, db) = db_sintetico();
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
            |auditado, _, evento| {
                assert_eq!(auditado, pedido);
                match evento {
                    porta::EventoAuditoria::Intencao => {
                        // A auditoria passou do prazo antes de retornar.
                        relogio.set(300_010);
                        true
                    }
                    porta::EventoAuditoria::ExpiradaNaoExecutada => {
                        recusas::registrar(&db, pedido, Motivo::Expirada, relogio.get()).is_ok()
                    }
                }
            },
            |_| panic!("efeito executado com auditoria fora do prazo"),
        ),
        Err(Recusa::Expirada)
    );
    assert_eq!(
        porta.efetuar(pedido, acao(), || 300_011, |_, _, _| true, |_| panic!("retry do pedido negado")),
        Err(Recusa::PedidoInvalido)
    );
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 300_012), Err(Recusa::PedidoInvalido));
    // A porta segue útil para um pedido novo; o antigo não é reaberto.
    let novo = porta.pedir(acao(), 300_013).unwrap();
    assert_ne!(novo, pedido);
    porta.resolver(novo, escopo(), Decisao::Negar, 300_014).unwrap();
    assert_eq!(linhas(&db), vec![(1, "expirada".to_owned(), 300_010)]);
}

#[test]
fn falha_ao_gravar_expiracao_fecha_a_porta_sem_efeito() {
    let (_dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).unwrap();
    db.execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    let relogio = Cell::new(299_999);
    assert_eq!(
        porta.efetuar(
            pedido,
            acao(),
            || relogio.get(),
            |_, _, evento| {
                if evento == porta::EventoAuditoria::Intencao {
                    relogio.set(300_010);
                    return true;
                }
                recusas::registrar(&db, pedido, Motivo::Expirada, relogio.get()).is_ok()
            },
            |_| panic!("efeito executado com registro falho"),
        ),
        Err(Recusa::Auditoria)
    );
    assert_eq!(porta.pedir(acao(), 300_010), Err(Recusa::Fechada));
    assert_eq!(porta.aceitar_aviso(escopo()), Err(Recusa::Fechada));
}

#[test]
fn recusa_explicita_grava_e_o_pedido_morre() {
    let (_dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    // Interface somente leitura do pedido: a série só sai da porta, nunca entra.
    assert_eq!(pedido.escopo(), escopo());
    assert_eq!(pedido.serie(), 1);
    porta.resolver(pedido, escopo(), Decisao::Negar, 2).unwrap();
    recusas::registrar(&db, pedido, Motivo::Negada, 2).expect("persistir recusa explícita");
    // Segundo clique na mesma decisão não grava duas vezes nem revive o pedido.
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 3), Err(Recusa::PedidoInvalido));
    assert_eq!(
        porta.efetuar(pedido, acao(), || 3, |_, _, _| true, |_| panic!("efeito de pedido negado")),
        Err(Recusa::PedidoInvalido)
    );
    assert_eq!(linhas(&db), vec![(1, "negada".to_owned(), 2)]);
}

#[test]
fn registro_sem_deduplicacao_enquanto_serie_recomeca_por_porta() {
    let (_dir, db) = db_sintetico();
    let mut primeira = Porta::nova(escopo());
    primeira.aceitar_aviso(escopo()).unwrap();
    let mut segunda = Porta::nova(escopo());
    segunda.aceitar_aviso(escopo()).unwrap();
    let pedido_negado = primeira.pedir(acao(), 0).unwrap();
    let pedido_fechado = segunda.pedir(acao(), 0).unwrap();
    primeira.resolver(pedido_negado, escopo(), Decisao::Negar, 1).unwrap();
    segunda.parar(2);
    recusas::registrar(&db, pedido_negado, Motivo::Negada, 1).expect("persistir recusa explícita");
    recusas::registrar(&db, pedido_fechado, Motivo::Fechada, 2).expect("persistir recusa do fechamento");
    // Duas portas do mesmo escopo recomeçam a série: unicidade aqui corromperia o registro.
    assert_eq!(pedido_negado.serie(), pedido_fechado.serie());
    // Repetir a mesma gravação acumula: o registro é append-only, sem deduplicação.
    recusas::registrar(&db, pedido_negado, Motivo::Negada, 3).expect("persistir repetição da recusa");
    assert_eq!(
        linhas(&db),
        vec![(1, "negada".to_owned(), 1), (1, "fechada".to_owned(), 2), (1, "negada".to_owned(), 3)]
    );
}

#[test]
fn falha_ao_gravar_recusa_nao_devolve_a_permissao() {
    let (_dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::Negar, 2).unwrap();
    db.execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    assert!(recusas::registrar(&db, pedido, Motivo::Negada, 3).is_err());
    // A negação já vale; o registro falho não vira permissão nem retry.
    assert_eq!(
        porta.efetuar(pedido, acao(), || 3, |_, _, _| true, |_| panic!("efeito com registro falho")),
        Err(Recusa::PedidoInvalido)
    );
    let novo = porta.pedir(acao(), 4).unwrap();
    assert_ne!(novo, pedido);
}

#[test]
fn parar_cancela_pendencia_e_grava_fechada() {
    let (_dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.parar(5);
    recusas::registrar(&db, pedido, Motivo::Fechada, 5).expect("persistir recusa do fechamento");
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 6), Err(Recusa::Fechada));
    assert_eq!(linhas(&db), vec![(1, "fechada".to_owned(), 5)]);
}

#[test]
fn tabela_guarda_somente_ids_motivo_e_instante_tipados() {
    let (_dir, db) = db_sintetico();
    let colunas: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('recusas_motor_macos') ORDER BY cid")
        .expect("preparar consulta de colunas")
        .query_map([], |row| row.get(0))
        .expect("consultar colunas")
        .collect::<rusqlite::Result<_>>()
        .expect("juntar colunas");
    let esperadas = ["id", "titular", "dispositivo", "bot", "sessao", "geracao", "serie", "motivo", "instante_ms"];
    assert_eq!(colunas, esperadas.iter().map(ToString::to_string).collect::<Vec<_>>());
    // Motivo fora do conjunto tipado e id vazio não entram: nada de entrada bruta.
    assert!(db.execute("INSERT INTO recusas_motor_macos VALUES (NULL, 1, 2, 3, 4, 5, 1, 'conteudo', 0)", []).is_err());
    assert!(db.execute("INSERT INTO recusas_motor_macos VALUES (NULL, 0, 2, 3, 4, 5, 1, 'negada', 0)", []).is_err());
    assert_eq!(linhas(&db), []);
}

#[test]
fn gravacao_dentro_de_transacao_externa_e_recusada() {
    let (_dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::Negar, 2).unwrap();
    db.execute_batch("BEGIN").expect("abrir transação externa da fixture");
    assert!(recusas::registrar(&db, pedido, Motivo::Negada, 2).is_err());
    db.execute_batch("ROLLBACK").expect("desfazer transação externa da fixture");
    // Nenhum Ok ficou sujeito a rollback: nada foi gravado.
    assert_eq!(linhas(&db), []);
}

#[test]
fn journal_fora_do_wal_bloqueia_a_gravacao_duravel() {
    let dir = tempfile::tempdir().expect("criar diretório da fixture");
    let db = Connection::open(dir.path().join("recusas-rollback.sqlite3")).expect("abrir banco da fixture");
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::Negar, 2).unwrap();
    // Sem WAL o banco fica em journal de rollback: preparar e gravar recusam.
    assert!(recusas::preparar(&db).is_err());
    assert!(recusas::registrar(&db, pedido, Motivo::Negada, 2).is_err());
}

#[test]
fn aprovacao_no_prazo_executa_sem_recusa_registrada() {
    let (_dir, db) = db_sintetico();
    let mut porta = Porta::nova(escopo());
    porta.aceitar_aviso(escopo()).unwrap();
    let pedido = porta.pedir(acao(), 0).unwrap();
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).unwrap();
    let executado = Cell::new(false);
    porta
        .efetuar(
            pedido,
            acao(),
            || 2,
            |_, _, _| true,
            |efetivada| {
                assert_eq!(efetivada, acao());
                executado.set(true);
            },
        )
        .unwrap();
    assert!(executado.get());
    assert_eq!(linhas(&db), []);
}
