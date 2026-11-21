//! Verificações do coordenador sem privilégio. Nunca abrir backup como root.
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Transacao {
    pub id: String,
    pub sequencia_anterior: Option<u64>,
    pub sequencia_nova: u64,
    pub snapshot_sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Comprovante {
    pub transacao: Transacao,
    pub banco_sha256: String,
    pub schema_sha256: String,
}
/// Só pode ser produzida pela leitura do arquivo root-owned validado.
pub(crate) struct Certidao(Transacao);
impl Certidao {
    pub(crate) fn transacao(&self) -> Transacao {
        self.0.clone()
    }
}

pub(crate) fn ler_certidao(uid_hub: u32) -> Result<Certidao> {
    // SAFETY: consultas de identidade sem efeitos colaterais.
    let (real, efetivo) = unsafe { (libc::getuid(), libc::geteuid()) };
    ensure!(uid_hub != 0 && real == uid_hub && efetivo == uid_hub, "certidão deve ser lida pelo Hub");
    let bytes = super::publicacao_atualizacao::ler(super::publicacao_atualizacao::Nome::Certidao)?;
    let certidao: Transacao = serde_json::from_slice(&bytes)?;
    ensure!(
        hash_valido(&certidao.id)
            && hash_valido(&certidao.snapshot_sha256)
            && certidao.sequencia_nova > certidao.sequencia_anterior.unwrap_or(0),
        "certidão inválida"
    );
    Ok(Certidao(certidao))
}

fn hash_valido(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn conferir_vinculo(certidao: &Transacao, comprovante: &Comprovante) -> Result<()> {
    ensure!(
        certidao == &comprovante.transacao
            && hash_valido(&certidao.id)
            && hash_valido(&certidao.snapshot_sha256)
            && hash_valido(&comprovante.banco_sha256)
            && hash_valido(&comprovante.schema_sha256)
            && certidao.sequencia_nova > certidao.sequencia_anterior.unwrap_or(0),
        "comprovante não corresponde à transação"
    );
    Ok(())
}
fn texto_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}
fn parte(hash: &mut Sha256, texto: &[u8]) -> Result<()> {
    ensure!(texto.len() <= 1024 * 1024, "schema grande demais");
    hash.update(u64::try_from(texto.len())?.to_be_bytes());
    hash.update(texto);
    Ok(())
}

/// Inclui kv.schema e objetos persistentes, inclusive migrações de outros módulos.
pub(crate) fn schema(db: &Connection) -> Result<String> {
    let versao: String =
        db.query_row("SELECT v FROM kv WHERE k='schema'", [], |row| row.get(0)).context("schema ausente")?;
    let numero: u64 = versao.parse().context("schema desconhecido")?;
    // A comparação com o par anterior continua sendo um gate separado; esta
    // lista somente impede que uma versão nunca reconhecida seja tratada como
    // conhecida. Atualize-a após revisar as migrações em store/schema.rs.
    ensure!((3..=7).contains(&numero) && numero.to_string() == versao, "schema desconhecido");
    let mut hash = Sha256::new();
    hash.update(b"mexerica/backup/schema/v1\0");
    parte(&mut hash, versao.as_bytes())?;
    let mut consulta = db.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type COLLATE BINARY, name COLLATE BINARY",
    )?;
    let mut linhas = consulta.query([])?;
    let mut contador = 0usize;
    while let Some(linha) = linhas.next()? {
        contador += 1;
        ensure!(contador <= 10000, "objetos de schema demais");
        for indice in 0..4 {
            let campo: Option<String> = linha.get(indice)?;
            match campo {
                Some(valor) => {
                    hash.update([1]);
                    parte(&mut hash, valor.as_bytes())?;
                }
                None => hash.update([0]),
            }
        }
    }
    ensure!(contador > 0, "schema vazio");
    Ok(texto_hex(&hash.finalize()))
}

fn sem_wal(path: &Path) -> Result<()> {
    for sufixo in ["-wal", "-shm", "-journal"] {
        let mut nome = path.as_os_str().to_os_string();
        nome.push(sufixo);
        match fs::symlink_metadata(nome) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => anyhow::bail!("backup possui journal SQLite ou não pode ser verificado"),
        }
    }
    Ok(())
}

/// Chamador fornece a certidão root autenticada e mantém host.lock até terminar.
/// A abertura/validação da certidão root e a restauração não são feitas aqui.
pub(crate) fn conferir(
    db: &Connection,
    banco: &mut File,
    trava: &File,
    data: &Path,
    uid_hub: u32,
    certidao: &Certidao,
    comprovante: &Comprovante,
) -> Result<()> {
    // SAFETY: consultas de identidade sem efeitos colaterais.
    let (real, efetivo) = unsafe { (libc::getuid(), libc::geteuid()) };
    ensure!(
        uid_hub != 0 && real == uid_hub && efetivo == uid_hub,
        "verificação do backup exige UID Hub sem privilégio"
    );
    conferir_vinculo(&certidao.0, comprovante)?;
    let lock_atual = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(data.join("host.lock"))?;
    let (a, b) = (trava.metadata()?, lock_atual.metadata()?);
    ensure!(
        a.is_file() && a.uid() == uid_hub && a.nlink() == 1 && a.dev() == b.dev() && a.ino() == b.ino(),
        "host.lock divergente"
    );
    // SAFETY: descritor válido; mantém/obtém exclusividade durante a verificação.
    ensure!(unsafe { libc::flock(trava.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0, "host.lock ocupada");
    let nome = db.path().context("banco sem arquivo")?;
    let path = Path::new(nome);
    let meta = fs::symlink_metadata(path)?;
    let aberto = banco.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == uid_hub
            && meta.nlink() == 1
            && meta.mode() & 0o7777 == 0o600
            && meta.dev() == aberto.dev()
            && meta.ino() == aberto.ino(),
        "arquivo de backup inseguro ou divergente"
    );
    sem_wal(path)?;
    ensure!(db.is_autocommit(), "conexão já possui transação");
    db.execute_batch("BEGIN")?;
    let resultado = (|| -> Result<()> {
        let mut check = db.prepare("PRAGMA integrity_check")?;
        let mut linhas = check.query([])?;
        let primeira: String = linhas.next()?.context("integridade não comprovada")?.get(0)?;
        ensure!(primeira == "ok" && linhas.next()?.is_none(), "integridade SQLite não comprovada");
        ensure!(schema(db)? == comprovante.schema_sha256, "schema do backup divergente");
        banco.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let quantidade = banco.read(&mut buffer)?;
            if quantidade == 0 {
                break;
            }
            hash.update(&buffer[..quantidade]);
        }
        ensure!(texto_hex(&hash.finalize()) == comprovante.banco_sha256, "hash do backup divergente");
        sem_wal(path)?;
        Ok(())
    })();
    let rollback = db.execute_batch("ROLLBACK");
    rollback?;
    resultado
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comprovante_nao_serve_para_outro_par() {
        let certidao = Transacao {
            id: "a".repeat(64),
            sequencia_anterior: Some(5),
            sequencia_nova: 6,
            snapshot_sha256: "b".repeat(64),
        };
        let mut comprovante = Comprovante {
            transacao: Transacao {
                id: "a".repeat(64),
                sequencia_anterior: Some(5),
                sequencia_nova: 6,
                snapshot_sha256: "b".repeat(64),
            },
            banco_sha256: "c".repeat(64),
            schema_sha256: "d".repeat(64),
        };
        assert!(conferir_vinculo(&certidao, &comprovante).is_ok());
        comprovante.transacao.sequencia_anterior = Some(4);
        assert!(conferir_vinculo(&certidao, &comprovante).is_err());
        comprovante.transacao.sequencia_anterior = Some(5);
        comprovante.schema_sha256.clear();
        assert!(conferir_vinculo(&certidao, &comprovante).is_err());
    }

    #[test]
    fn schema_desconhecido_e_alterado_nao_coincidem() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT); INSERT INTO kv VALUES('schema','6');").unwrap();
        let original = schema(&db).unwrap();
        db.execute_batch("CREATE TABLE adicional(id INTEGER);").unwrap();
        assert_ne!(schema(&db).unwrap(), original);
        db.execute_batch("UPDATE kv SET v='8' WHERE k='schema';").unwrap();
        assert!(schema(&db).is_err());
        db.execute_batch("UPDATE kv SET v='2' WHERE k='schema';").unwrap();
        assert!(schema(&db).is_err());
        db.execute_batch("DELETE FROM kv WHERE k='schema';").unwrap();
        assert!(schema(&db).is_err());
    }
}
