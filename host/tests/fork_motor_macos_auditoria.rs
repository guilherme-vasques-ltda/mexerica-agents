#[path = "../src/fork/fork-motor-macos/auditoria.rs"]
mod auditoria;
#[path = "../src/fork/fork-motor-macos/porta.rs"]
#[allow(dead_code)]
mod porta;
#[path = "../src/fork/fork-motor-macos/recusas.rs"]
#[allow(dead_code)]
mod recusas;

use std::num::NonZeroU64;

use auditoria::{Falha, PortaAuditada};
use porta::{Acao, Alvo, Decisao, Escopo, Recusa};
use recusas::Registro;
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

/// Registro durável real, aberto pela própria fronteira, em arquivo sintético.
fn registro() -> (tempfile::TempDir, Registro) {
    let dir = tempfile::tempdir().expect("criar diretório da fixture");
    let registro = Registro::abrir(&dir.path().join("hub.sqlite3")).expect("abrir registro durável");
    (dir, registro)
}

/// Segunda conexão só de leitura, para conferir o que ficou gravado.
fn leitor(dir: &tempfile::TempDir) -> Connection {
    Connection::open(dir.path().join("hub.sqlite3")).expect("abrir leitor da fixture")
}

fn linhas(db: &Connection) -> Vec<(i64, String, String, i64, i64)> {
    db.prepare("SELECT serie, motivo, caminho, instancia, instante_ms FROM recusas_motor_macos ORDER BY id")
        .expect("preparar consulta de recusas")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .expect("consultar recusas")
        .collect::<rusqlite::Result<_>>()
        .expect("juntar recusas")
}

fn porta(registro: &Registro, instancia: u64) -> PortaAuditada<'_> {
    PortaAuditada::nova(escopo(), id(instancia), registro)
}

#[test]
fn pedir_com_pendencia_vencida_e_gravacao_falha_nega_fecha_e_sem_retry() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let _pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    leitor(&dir).execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    // A expiração no caminho de pedir não pode ficar silenciosa: sem gravação,
    // o pedido novo não nasce e a porta fecha, sem retry.
    assert_eq!(porta.pedir(acao(), 300_000), Err(Falha::Recusa(Recusa::Auditoria)));
    assert_eq!(porta.pedir(acao(), 300_001), Err(Falha::Recusa(Recusa::Fechada)));
    assert_eq!(porta.aceitar_aviso(escopo()), Err(Falha::Recusa(Recusa::Fechada)));
    // Reaberto o registro, nenhuma recusa fantasma: nada foi gravado.
    drop(registro);
    Registro::abrir(&dir.path().join("hub.sqlite3")).expect("reabrir registro durável");
    assert_eq!(linhas(&leitor(&dir)), vec![]);
}

#[test]
fn pedir_com_pendencia_vencida_grava_expirada_uma_vez_e_cria_novo_pedido() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let vencido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    // No instante exato do prazo a pendência já venceu no caminho de pedir.
    let novo = porta.pedir(acao(), 300_000).expect("criar pedido novo após expiração auditada");
    assert_ne!(novo, vencido);
    assert_eq!(linhas(&leitor(&dir)), vec![(1, "expirada".to_owned(), "pedir".to_owned(), 9, 300_000)]);
    // O pedido novo segue o fluxo: negado, grava `negada` no caminho de resolver.
    porta.resolver(novo, escopo(), Decisao::Negar, 300_001).expect("negar pedido novo");
    assert_eq!(
        linhas(&leitor(&dir)),
        vec![
            (1, "expirada".to_owned(), "pedir".to_owned(), 9, 300_000),
            (2, "negada".to_owned(), "resolver".to_owned(), 9, 300_001),
        ]
    );
}

#[test]
fn pedir_com_alvo_proibido_nao_expira_nem_grava_falso_expirada() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let _pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    // A recusa de alvo vem antes da expiração: a pendência segue intacta.
    assert_eq!(porta.pedir(Acao::Apagar(Alvo::Proibido), 300_000), Err(Falha::Recusa(Recusa::AlvoProibido)));
    assert_eq!(linhas(&leitor(&dir)), vec![]);
    // A pendência não foi consumida pela recusa de alvo: expira agora, auditada.
    let _novo = porta.pedir(acao(), 300_000).expect("expirar pendência no caminho de pedir");
    assert_eq!(linhas(&leitor(&dir)), vec![(1, "expirada".to_owned(), "pedir".to_owned(), 9, 300_000)]);
}

#[test]
fn pedir_com_pendencia_nao_vencida_recusa_ocupada_sem_gravar() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let _pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    assert_eq!(porta.pedir(acao(), 1), Err(Falha::Recusa(Recusa::Ocupada)));
    assert_eq!(linhas(&leitor(&dir)), vec![]);
}

#[test]
fn resolver_negando_com_gravacao_falha_deixa_a_negacao_valendo_e_falha_visivel() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    leitor(&dir).execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::Negar, 1), Err(Falha::Auditoria));
    // A negação não é devolvida: o pedido morreu e nada é refeito.
    assert_eq!(
        porta.efetuar(pedido, acao(), || 2, |_| panic!("efeito de pedido negado")),
        Err(Falha::Recusa(Recusa::PedidoInvalido))
    );
    let novo = porta.pedir(acao(), 3).expect("porta segue útil para pedido novo");
    assert_ne!(novo, pedido);
}

#[test]
fn resolver_de_pedido_desconhecido_nao_grava() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let mut outra = PortaAuditada::nova(escopo(), id(10), &registro);
    outra.aceitar_aviso(escopo()).expect("aceitar aviso da outra fixture");
    let pedido = outra.pedir(acao(), 0).expect("abrir pedido da outra porta");
    // A série é da porta que emitiu o pedido: gravar aqui corromperia o registro.
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::Negar, 1), Err(Falha::Recusa(Recusa::PedidoInvalido)));
    assert_eq!(linhas(&leitor(&dir)), vec![]);
}

#[test]
fn resolver_com_pendencia_vencida_grava_expirada_no_resolver() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 300_000), Err(Falha::Recusa(Recusa::Expirada)));
    assert_eq!(linhas(&leitor(&dir)), vec![(1, "expirada".to_owned(), "resolver".to_owned(), 9, 300_000)]);
    // Sem reabrir e sem retry: o pedido expirado não volta.
    assert_eq!(
        porta.efetuar(pedido, acao(), || 300_001, |_| panic!("efeito de pedido expirado")),
        Err(Falha::Recusa(Recusa::PedidoInvalido))
    );
}

#[test]
fn efetuar_com_prazo_vencido_grava_expirada_com_instante_medido_pela_porta() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).expect("aprovar pedido da fixture");
    // O relógio avança entre a checagem de admissão e o efeito: o prazo vence.
    let chamadas = std::cell::Cell::new(0);
    let relogio = || {
        let atual = chamadas.get();
        chamadas.set(atual + 1);
        if atual == 0 { 299_999 } else { 300_010 }
    };
    assert_eq!(
        porta.efetuar(pedido, acao(), relogio, |_| panic!("efeito executado fora do prazo"),),
        Err(Falha::Recusa(Recusa::Expirada))
    );
    // O instante gravado é o medido pela porta no evento, não uma segunda leitura.
    assert_eq!(linhas(&leitor(&dir)), vec![(1, "expirada".to_owned(), "efetuar".to_owned(), 9, 300_010)]);
}

#[test]
fn efetuar_com_prazo_vencido_e_gravacao_falha_nega_fecha_e_sem_retry() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).expect("aprovar pedido da fixture");
    leitor(&dir).execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    let chamadas = std::cell::Cell::new(0);
    let relogio = || {
        let atual = chamadas.get();
        chamadas.set(atual + 1);
        if atual == 0 { 299_999 } else { 300_010 }
    };
    assert_eq!(
        porta.efetuar(pedido, acao(), relogio, |_| panic!("efeito executado com gravação falha"),),
        Err(Falha::Recusa(Recusa::Auditoria))
    );
    assert_eq!(porta.pedir(acao(), 300_010), Err(Falha::Recusa(Recusa::Fechada)));
    assert_eq!(porta.aceitar_aviso(escopo()), Err(Falha::Recusa(Recusa::Fechada)));
    // Reaberto o registro, nenhuma recusa fantasma: nada foi gravado.
    drop(registro);
    Registro::abrir(&dir.path().join("hub.sqlite3")).expect("reabrir registro durável");
    assert_eq!(linhas(&leitor(&dir)), vec![]);
}

#[test]
fn parar_com_pendencia_grava_fechada_e_nunca_bloqueia() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    porta.parar(10).expect("parar com pendência");
    assert_eq!(porta.resolver(pedido, escopo(), Decisao::UmaVez, 11), Err(Falha::Recusa(Recusa::Fechada)));
    assert_eq!(linhas(&leitor(&dir)), vec![(1, "fechada".to_owned(), "parar".to_owned(), 9, 10)]);
}

#[test]
fn parar_sem_pendencia_fecha_sem_gravar() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    porta.parar(10).expect("parar sem pendência");
    assert_eq!(porta.pedir(acao(), 11), Err(Falha::Recusa(Recusa::Fechada)));
    assert_eq!(linhas(&leitor(&dir)), vec![]);
}

#[test]
fn parar_com_gravacao_falha_fecha_mesmo_assim() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let _pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    leitor(&dir).execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    // O fechamento nunca é bloqueado pela auditoria: a falha é devolvida sem reverter.
    assert_eq!(porta.parar(10), Err(Falha::Auditoria));
    assert_eq!(porta.pedir(acao(), 11), Err(Falha::Recusa(Recusa::Fechada)));
}

#[test]
fn instancia_distingue_series_iguais_e_nada_e_deduplicado() {
    let (dir, registro) = registro();
    let mut primeira = porta(&registro, 9);
    primeira.aceitar_aviso(escopo()).expect("aceitar aviso da primeira fixture");
    let mut segunda = porta(&registro, 10);
    segunda.aceitar_aviso(escopo()).expect("aceitar aviso da segunda fixture");
    let pedido_primeira = primeira.pedir(acao(), 0).expect("abrir pedido da primeira");
    let pedido_segunda = segunda.pedir(acao(), 0).expect("abrir pedido da segunda");
    assert_eq!(pedido_primeira.serie(), pedido_segunda.serie());
    primeira.resolver(pedido_primeira, escopo(), Decisao::Negar, 1).expect("negar da primeira");
    segunda.parar(2).expect("parar da segunda");
    // Mesma série, instâncias distintas: as linhas nunca se confundem.
    assert_eq!(
        linhas(&leitor(&dir)),
        vec![
            (1, "negada".to_owned(), "resolver".to_owned(), 9, 1),
            (1, "fechada".to_owned(), "parar".to_owned(), 10, 2),
        ]
    );
}

#[test]
fn aprovacao_no_prazo_executa_uma_vez_sem_recusa_registrada() {
    let (dir, registro) = registro();
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    porta.resolver(pedido, escopo(), Decisao::UmaVez, 1).expect("aprovar pedido da fixture");
    let executado = std::cell::Cell::new(false);
    porta
        .efetuar(
            pedido,
            acao(),
            || 2,
            |efetivada| {
                assert_eq!(efetivada, acao());
                executado.set(true);
            },
        )
        .expect("efetuar pedido aprovado");
    assert!(executado.get());
    assert_eq!(
        porta.efetuar(pedido, acao(), || 3, |_| panic!("efeito repetido")),
        Err(Falha::Recusa(Recusa::PedidoInvalido))
    );
    assert_eq!(linhas(&leitor(&dir)), vec![]);
}

#[test]
fn registro_aponta_conexao_full_ao_banco_aberto_pelo_store_legado() {
    let dir = tempfile::tempdir().expect("criar diretório da fixture");
    let caminho = dir.path().join("hub.sqlite3");
    // O Store legado abre em WAL+NORMAL: essa conexão não serve para recusa.
    let legado = Connection::open(&caminho).expect("abrir conexão legada da fixture");
    let modo: String = legado
        .pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))
        .expect("ligar WAL da conexão legada");
    assert_eq!(modo, "wal");
    legado.pragma_update(None, "synchronous", "NORMAL").expect("baixar sincronização da legada");
    let registro = Registro::abrir(&caminho).expect("apontar registro ao banco do Hub");
    let mut porta = porta(&registro, 9);
    porta.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = porta.pedir(acao(), 0).expect("abrir pedido da fixture");
    porta.resolver(pedido, escopo(), Decisao::Negar, 1).expect("negar pela conexão durável");
    // A conexão do registro é FULL com flush assegurado; a legada segue NORMAL,
    // sem ser promovida por efeito colateral.
    let sincronia = |db: &Connection| db.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0));
    assert_eq!(sincronia(&legado).expect("ler sincronização da legada"), 1);
    assert_eq!(
        linhas(&Connection::open(&caminho).expect("abrir leitor da fixture")),
        vec![(1, "negada".to_owned(), "resolver".to_owned(), 9, 1)]
    );
}
