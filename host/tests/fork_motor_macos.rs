#[path = "../src/fork/fork-motor-macos/admissao.rs"]
mod admissao;
#[path = "../src/fork/fork-motor-macos/observacao.rs"]
mod observacao;

use std::{num::NonZeroU32, path::Path};

use observacao::{Censo, Geracao, Instancia, Observacao, ParAutenticado, Processo, TokenAuditado};
use rusqlite::{Connection, OpenFlags};

const UID_MOTOR: NonZeroU32 = NonZeroU32::new(901).unwrap();
const TITULAR: NonZeroU32 = NonZeroU32::new(502).unwrap();
const SUPERVISOR: Processo = Processo {
    pid: NonZeroU32::new(42).unwrap(),
    versao: Some(NonZeroU32::MIN),
    inicio_micros: 101,
    uid_real: UID_MOTOR.get(),
    uid_efetivo: UID_MOTOR.get(),
};

fn abrir(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).expect("reabrir banco da fixture")
}

fn db_sintetico() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().expect("criar diretório da fixture");
    let db = Connection::open(dir.path().join("geracao.sqlite3")).expect("abrir banco da fixture");
    (dir, db)
}

fn par(g: &Geracao) -> ParAutenticado {
    ParAutenticado {
        instancia: Instancia {
            token: TokenAuditado {
                pid: SUPERVISOR.pid,
                versao: NonZeroU32::MIN,
                uid_real: UID_MOTOR,
                uid_efetivo: UID_MOTOR,
            },
            inicio_micros: SUPERVISOR.inicio_micros,
        },
        titular: TITULAR,
        geracao: g.valor(),
    }
}

fn censo(real: Vec<Processo>, efetivo: Vec<Processo>) -> Censo {
    Censo {
        uid_real_observador: TITULAR.get(),
        uid_efetivo_observador: TITULAR.get(),
        por_uid_real: real,
        por_uid_efetivo: efetivo,
    }
}

#[test]
fn darwin_retorna_codigo_estavel_sem_capacidade() {
    assert_eq!(
        admissao::indisponivel().unwrap_err().to_string().split_once(':').unwrap().0,
        "motor_macos_indisponivel"
    );
}

#[test]
fn uniao_deduplica_supervisor_e_conta_trabalhadores_por_ambos_os_uids() {
    let (_dir, mut db) = db_sintetico();
    Geracao::inicializar(&db).unwrap();
    let geracao = Geracao::recuperar(&mut db).unwrap();
    let supervisor = par(&geracao);
    let apenas_real = Processo { pid: NonZeroU32::new(43).unwrap(), uid_efetivo: 999, ..SUPERVISOR };
    let apenas_efetivo = Processo { pid: NonZeroU32::new(44).unwrap(), uid_real: 999, ..SUPERVISOR };
    let ambos = Processo { pid: NonZeroU32::new(45).unwrap(), ..SUPERVISOR };
    let observacao = geracao
        .observar(&mut db, UID_MOTOR, TITULAR, Some(supervisor), || {
            Ok((censo(vec![SUPERVISOR, apenas_real, ambos], vec![ambos, apenas_efetivo, SUPERVISOR]), Some(supervisor)))
        })
        .unwrap();
    assert_eq!(observacao, Observacao::Trabalhadores(3));
}

#[test]
fn perda_bloqueia_e_reinicio_invalida_provas_antigas_ate_novo_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("geracao.sqlite3");
    Geracao::inicializar(&Connection::open(&path).unwrap()).unwrap();

    let mut db = abrir(&path);
    let geracao = Geracao::recuperar(&mut db).unwrap();
    let perdido = par(&geracao);
    let trabalhador = Processo { pid: NonZeroU32::new(43).unwrap(), ..SUPERVISOR };
    assert_eq!(
        geracao
            .observar(&mut db, UID_MOTOR, TITULAR, Some(perdido), || {
                Ok((censo(vec![trabalhador], vec![trabalhador]), None))
            })
            .unwrap(),
        Observacao::Bloqueada
    );
    drop(db);

    let mut db = abrir(&path);
    let geracao = Geracao::recuperar(&mut db).unwrap();
    assert_eq!(geracao.valor(), perdido.geracao + 2);
    assert_eq!(
        geracao
            .observar(&mut db, UID_MOTOR, TITULAR, Some(perdido), || {
                Ok((censo(vec![SUPERVISOR], vec![SUPERVISOR]), Some(perdido)))
            })
            .unwrap(),
        Observacao::Bloqueada
    );
    let geracao = Geracao::recuperar(&mut db).unwrap();
    let atual = par(&geracao);
    assert_eq!(
        geracao
            .observar(&mut db, UID_MOTOR, TITULAR, Some(atual), || {
                Ok((censo(vec![SUPERVISOR], vec![SUPERVISOR]), Some(atual)))
            })
            .unwrap(),
        Observacao::ZeroObservado
    );
    assert!(admissao::indisponivel().is_err());
}

#[test]
fn prova_incompleta_ou_instancia_diferente_nunca_subtrai_supervisor() {
    let (_dir, mut db) = db_sintetico();
    Geracao::inicializar(&db).unwrap();
    for caso in 0..12 {
        let geracao = Geracao::recuperar(&mut db).unwrap();
        let antes = par(&geracao);
        let mut depois = antes;
        let mut censo = censo(vec![SUPERVISOR], vec![SUPERVISOR]);
        match caso {
            0 => depois.instancia.token.versao = NonZeroU32::new(2).unwrap(),
            1 => depois.titular = NonZeroU32::new(999).unwrap(),
            2 => censo.uid_real_observador = UID_MOTOR.get(),
            3 => censo.uid_efetivo_observador = UID_MOTOR.get(),
            4 => censo.por_uid_real.clear(),
            5 => censo.por_uid_efetivo[0].inicio_micros += 1,
            6 => censo.por_uid_real.push(SUPERVISOR),
            7 => {
                censo.por_uid_real[0].inicio_micros += 1;
                censo.por_uid_efetivo[0].inicio_micros += 1;
            }
            8 => censo.uid_real_observador = 0,
            9 => censo.uid_efetivo_observador = 999,
            10 => {
                censo.por_uid_real[0].versao = None;
                censo.por_uid_efetivo[0].versao = None;
            }
            11 => {
                censo.por_uid_real[0].versao = NonZeroU32::new(2);
                censo.por_uid_efetivo[0].versao = NonZeroU32::new(2);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            geracao.observar(&mut db, UID_MOTOR, TITULAR, Some(antes), || Ok((censo, Some(depois)))).unwrap(),
            Observacao::Bloqueada
        );
    }
}

#[test]
fn geracao_nao_recria_registro_ausente_nem_publica_apos_erro_de_leitura() {
    let (_dir, mut db) = db_sintetico();
    assert!(Geracao::recuperar(&mut db).is_err());
    Geracao::inicializar(&db).unwrap();
    let geracao = Geracao::recuperar(&mut db).unwrap();
    let antes = par(&geracao);
    assert!(
        geracao
            .observar(&mut db, UID_MOTOR, TITULAR, Some(antes), || anyhow::bail!("leitura sintética falhou"))
            .is_err()
    );
    assert_eq!(Geracao::recuperar(&mut db).unwrap().valor(), antes.geracao + 2);
    db.execute("DELETE FROM cerca_macos", []).unwrap();
    assert!(Geracao::recuperar(&mut db).is_err());
}

#[cfg(unix)]
#[test]
fn observador_nao_interrompe_o_processo_inerte_da_fixture() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let (_dir, mut db) = db_sintetico();
    Geracao::inicializar(&db).unwrap();
    let geracao = Geracao::recuperar(&mut db).unwrap();
    let mut supervisor = par(&geracao);
    supervisor.instancia.token.pid = NonZeroU32::new(std::process::id()).unwrap();
    let linha_supervisor = Processo { pid: supervisor.instancia.token.pid, ..SUPERVISOR };
    let mut inerte = Command::new("/bin/cat").env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let trabalhador = Processo { pid: NonZeroU32::new(inerte.id()).unwrap(), ..SUPERVISOR };
    let resultado = geracao.observar(&mut db, UID_MOTOR, TITULAR, Some(supervisor), || {
        Ok((censo(vec![linha_supervisor, trabalhador], vec![linha_supervisor, trabalhador]), Some(supervisor)))
    });
    let escrita = inerte.stdin.take().unwrap().write_all(b"sentinela sintetica");
    let saida = inerte.wait_with_output().unwrap();
    assert_eq!(resultado.unwrap(), Observacao::Trabalhadores(1));
    assert!(escrita.is_ok());
    assert!(saida.status.success());
    assert_eq!(saida.stdout, b"sentinela sintetica");
}
