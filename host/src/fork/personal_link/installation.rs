//! Instalação D430 isolada (P2 a). Seleção por `<dados>/personal-link/`, decidida sob host.lock
//! ANTES de qualquer prepare PL1. Nascimento = host.lock + criação exclusiva + marca própria
//! D430 (nunca `personal_local_birth`). Reabertura valida marca, identidade H exata, keyId do
//! master e journal; ausência/divergência fecha e preserva, sem `load_or_create` nem fallback.
use super::{
    canon,
    procedure::{self, Procedimento},
    time::Relogio,
    wire::{self, Contexto},
    writer,
};
use crate::{LockExt, hub::Hub, remote::identity::Identity, store::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::TransactionBehavior;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const PASTA: &str = "personal-link";
const MARCA: &str = "birth";
const IDENTIDADE: &str = "identity";
const MASTER: &str = "master.key";
const SOCKET: &str = "procedure.sock";
const DOMINIO_IDENTIDADE: &str = "donna.personal-link/hub-identity/v1";
const DOMINIO_MASTER: &[u8] = b"donna.personal-link/master-key-id/v1";

#[derive(clap::Subcommand)]
pub(crate) enum Acao {
    /// Cria uma instalação D430 isolada e vazia nesta pasta de dados (procedimento administrativo).
    Init,
    /// Ponte stdin/stdout para o procedimento local desta instância (sessão SSH já ancorada).
    Helper,
}

/// A pasta existir (mesmo inválida/symlink) seleciona o perfil D430: dali em diante só fecha.
pub(crate) fn selecionado(dir: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(dir.join(PASTA)) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Estado aberto e conferido. Só `reabrir` constrói.
pub(crate) struct Aberta {
    pub(crate) store: Store,
    pub(crate) identidade: Identity,
    pub(crate) k: Contexto,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn key_id(master: &[u8]) -> String {
    hex(&crate::remote::crypto::sha256(&[DOMINIO_MASTER, master]))
}

fn identidade_bytes(id: &Identity) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    use crate::remote::crypto::b64;
    Ok(zeroize::Zeroizing::new(canon::j(&serde_json::json!([
        DOMINIO_IDENTIDADE,
        1,
        b64(id.sign.as_bytes()),
        b64(id.box_secret.as_bytes())
    ]))?))
}

fn identidade_de(bytes: &[u8]) -> Result<Identity> {
    let v = canon::ler(bytes)?;
    let a = v.as_array().context("not_allowed: identidade D430")?;
    ensure!(a.len() == 4 && a[0] == DOMINIO_IDENTIDADE && a[1] == 1, "not_allowed: identidade D430");
    let sign = zeroize::Zeroizing::new(canon::b64n::<32>(&a[2])?);
    let caixa = zeroize::Zeroizing::new(canon::b64n::<32>(&a[3])?);
    Ok(Identity {
        sign: ed25519_dalek::SigningKey::from_bytes(&sign),
        box_secret: x25519_dalek::StaticSecret::from(*caixa),
    })
}

fn full(store: &Store) -> Result<()> {
    let db = store.db.locked();
    db.pragma_update(None, "synchronous", "FULL")?;
    ensure!(
        db.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))? == 2,
        "not_allowed: FULL indisponível antes da escrita D430"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{Context, Result, ensure};
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Write},
        os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        path::{Path, PathBuf},
    };

    pub(super) fn euid() -> u32 {
        // SAFETY: geteuid não recebe ponteiros nem altera o processo.
        unsafe { libc::geteuid() }
    }

    pub(super) fn sincronizar(pasta: &Path) -> Result<()> {
        File::open(pasta)?.sync_all()?;
        Ok(())
    }

    pub(super) fn criar_pasta(pasta: &Path) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(pasta).context("not_allowed: pasta D430 já existe")?;
        std::fs::set_permissions(pasta, std::fs::Permissions::from_mode(0o700))?;
        sincronizar(pasta.parent().context("pasta D430 sem pai")?)
    }

    /// Pasta da instalação: diretório real do UID do serviço, 0700.
    pub(super) fn pasta(dir: &Path) -> Result<PathBuf> {
        let pasta = dir.join(super::PASTA);
        let m = std::fs::symlink_metadata(&pasta)?;
        ensure!(
            m.is_dir() && !m.file_type().is_symlink() && m.uid() == euid() && m.mode() & 0o7777 == 0o700,
            "not_allowed: pasta D430 sem proteção; preservada"
        );
        Ok(pasta)
    }

    /// Pais do socket sem escrita de grupo/outros e de root ou do serviço.
    pub(super) fn pais(pasta: &Path) -> Result<()> {
        for p in pasta.ancestors() {
            let m = std::fs::symlink_metadata(p)?;
            ensure!(
                m.is_dir()
                    && !m.file_type().is_symlink()
                    && (m.uid() == 0 || m.uid() == euid())
                    && m.mode() & 0o022 == 0,
                "not_allowed: pais do procedimento D430 desprotegidos"
            );
        }
        Ok(())
    }

    pub(super) fn criar_exclusivo(pasta: &Path, nome: &str, bytes: &[u8]) -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(pasta.join(nome))
            .with_context(|| format!("not_allowed: {nome} D430 já existe"))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        sincronizar(pasta)
    }

    pub(super) fn ler_privado(pasta: &Path, nome: &str, limite: u64) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
        let caminho = pasta.join(nome);
        let mut f = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&caminho)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let m = f.metadata()?;
        ensure!(
            m.is_file() && m.uid() == euid() && m.mode() & 0o7777 == 0o600 && m.nlink() == 1 && m.len() <= limite,
            "not_allowed: arquivo D430 {nome} sem proteção; preservado"
        );
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        f.read_to_end(&mut bytes)?;
        let l = std::fs::symlink_metadata(&caminho)?;
        ensure!(
            l.dev() == m.dev() && l.ino() == m.ino() && u64::try_from(bytes.len())? == m.len(),
            "not_allowed: arquivo D430 {nome} mudou"
        );
        Ok(Some(bytes))
    }

    pub(super) fn escutar(pasta: &Path) -> Result<tokio::net::UnixListener> {
        pais(pasta)?;
        let caminho = pasta.join(super::SOCKET);
        match std::fs::symlink_metadata(&caminho) {
            // Sob host.lock nenhuma outra instância vive: socket velho do mesmo UID sai.
            Ok(m) if m.file_type().is_socket() && m.uid() == euid() => std::fs::remove_file(&caminho)?,
            Ok(_) => anyhow::bail!("not_allowed: caminho do procedimento D430 ocupado"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let l = std::os::unix::net::UnixListener::bind(&caminho)?;
        std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(0o600))?;
        socket(pasta)?;
        l.set_nonblocking(true)?;
        Ok(tokio::net::UnixListener::from_std(l)?)
    }

    pub(super) fn socket(pasta: &Path) -> Result<PathBuf> {
        let caminho = pasta.join(super::SOCKET);
        let m = std::fs::symlink_metadata(&caminho)?;
        ensure!(
            m.file_type().is_socket() && m.uid() == euid() && m.mode() & 0o7777 == 0o600 && m.nlink() == 1,
            "not_allowed: socket D430 sem proteção"
        );
        Ok(caminho)
    }
}

/// Procedimento administrativo de nascimento, sob host.lock já retido pelo chamador.
#[cfg(target_os = "linux")]
pub(crate) fn nascer(dir: &Path, _trava: &std::fs::File) -> Result<Contexto> {
    ensure!(
        !crate::fork::personal_bootstrap::provisioning::raiz_pessoal_presente()?,
        "not_allowed: raiz PL1/ACS4 presente; D430 exige instalação isolada"
    );
    for legado in ["identity.json", "codync.db"] {
        ensure!(
            !selecionado_arquivo(&dir.join(legado))?,
            "not_allowed: pasta de dados já usada; D430 não migra a instalação atual"
        );
    }
    let pasta = dir.join(PASTA);
    linux::criar_pasta(&pasta)?;
    let pasta = linux::pasta(dir)?;
    let id = Identity {
        sign: ed25519_dalek::SigningKey::from_bytes(&crate::remote::crypto::random()),
        box_secret: x25519_dalek::StaticSecret::from(crate::remote::crypto::random::<32>()),
    };
    linux::criar_exclusivo(&pasta, IDENTIDADE, &identidade_bytes(&id)?)?;
    let master = zeroize::Zeroizing::new(crate::remote::crypto::random::<32>());
    linux::criar_exclusivo(&pasta, MASTER, &master[..])?;
    let chave = key_id(&master[..]);
    drop(master);
    let k = Contexto {
        h: id.sign_pub_b64(),
        g: canon::aleatorio::<32>(),
        instalacao: canon::aleatorio::<16>(),
        revisao_instalacao: 1,
    };
    let marca = wire::nascimento(&id, &k, &chave)?;
    let store = Store::open_d430_created(crate::store::personal_database::create_exclusive(&dir.join("codync.db"))?)?;
    full(&store)?;
    {
        let mut db = store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        writer::nascer(&tx, &id, &k, &marca, &chave)?;
        tx.commit()?;
    }
    drop(store);
    // Marca por último: sem ela a pasta é nascimento incompleto, preservado e fechado.
    linux::criar_exclusivo(&pasta, MARCA, &marca.bytes()?)?;
    Ok(k)
}

#[cfg(target_os = "linux")]
fn selecionado_arquivo(p: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(p) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Reabertura sob host.lock. Nenhuma criação/reparo: qualquer falta fecha e preserva.
#[cfg(target_os = "linux")]
pub(crate) fn reabrir(dir: &Path, _trava: &std::fs::File) -> Result<Aberta> {
    let pasta = linux::pasta(dir)?;
    let marca_bytes =
        linux::ler_privado(&pasta, MARCA, 4096)?.context("not_allowed: nascimento D430 incompleto; preservado")?;
    let marca_valor = canon::ler(&marca_bytes)?;
    let h = marca_valor[0][2].as_str().context("not_allowed: marca D430")?.to_owned();
    let marca = canon::verificar(&marca_valor, wire::NASCIMENTO, 8, &h)?;
    let p = marca.payload();
    let identidade = identidade_de(
        &linux::ler_privado(&pasta, IDENTIDADE, 1024)?.context("not_allowed: identidade D430 ausente; preservado")?,
    )?;
    ensure!(
        identidade.sign_pub_b64() == h && p[3] == identidade.box_pub_b64(),
        "not_allowed: identidade D430 divergente"
    );
    let master = linux::ler_privado(&pasta, MASTER, 32)?.context("not_allowed: master D430 ausente; preservado")?;
    ensure!(master.len() == 32 && p[7] == key_id(&master), "not_allowed: master D430 divergente");
    drop(master);
    let texto = |i: usize| p[i].as_str().map(str::to_owned).context("not_allowed: marca D430");
    let k = Contexto { h, g: texto(4)?, instalacao: texto(5)?, revisao_instalacao: canon::inteiro(&p[6])? };
    let capturada =
        crate::store::personal_database::capture(&dir.join("codync.db"))?.context("not_allowed: base D430 ausente")?;
    let key_id = p[7].as_str().context("not_allowed: keyId")?;
    // SEG-A-R1-01: validação positiva na MESMA Connection, ainda query_only, ANTES de Store/migrations.
    let validada = capturada.validar_d430(|c| writer::validar(c, &k, &marca_bytes, key_id))?;
    let store = Store::open_d430(validada)?;
    full(&store)?;
    // Reconfere depois da inicialização (mesmo handle), sem substituir a validação anterior.
    {
        let mut db = store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        writer::validar(&tx, &k, &marca_bytes, key_id)?;
        tx.commit()?;
    }
    Ok(Aberta { store, identidade, k })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn nascer(_dir: &Path, _trava: &std::fs::File) -> Result<Contexto> {
    anyhow::bail!("not_allowed: instalação D430 requer Linux")
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn reabrir(_dir: &Path, _trava: &std::fs::File) -> Result<Aberta> {
    anyhow::bail!("not_allowed: instalação D430 requer Linux")
}

/// Perfil D430 selecionado: nada de prepare/listener PL1, API comum ou `hub.start`. Canal B próprio apenas.
pub(crate) async fn servir(trava: std::fs::File, bind: &str, port: u16, parar: impl Future<Output = ()>) -> Result<()> {
    let dir = crate::service::data_dir();
    let aberta = reabrir(&dir, &trava)?;
    let relogio = Arc::new(Relogio::produzir());
    let runtime = canon::aleatorio::<32>();
    let k = aberta.k.clone();
    let hub = Hub::new(aberta.store, k.instalacao.clone(), aberta.identidade, canon::aleatorio::<32>(), port);
    {
        let inicio = relogio.amostra().map_or(0, |a| a.boottime_ms);
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        writer::registrar_runtime(&tx, &runtime, &relogio, inicio)?;
        tx.commit()?;
    }
    // Início observado DEPOIS do commit; sem fonte qualificada o runtime fica fechado.
    if relogio.qualificada() {
        relogio.iniciar()?;
    }
    let viva = Arc::new(std::sync::atomic::AtomicBool::new(true));
    // servir_socket só retorna depois do drain de TODO worker; só então host.lock é solto (SEG-A-R1-02).
    let p = Arc::new(Procedimento {
        hub,
        k,
        relogio,
        runtime,
        viva,
        #[cfg(all(test, target_os = "linux"))]
        sincronia: super::test_sync::Sincronia::default(),
    });
    let (aviso, parada) = tokio::sync::watch::channel(false);
    let admin = servir_socket(&dir, &p, async {
        let mut s = parada.clone();
        let _ = s.changed().await;
    });
    let canal = super::channel::servir(p.clone(), bind, port, parada.clone());
    tokio::pin!(admin, canal, parar);
    let resultado = tokio::select! {
        ()=&mut parar => { let _=aviso.send(true); let (a,b)=tokio::join!(&mut admin,&mut canal); a.and(b) }
        a=&mut admin => { let _=aviso.send(true); let b=(&mut canal).await; a.and(b) }
        b=&mut canal => { let _=aviso.send(true); let a=(&mut admin).await; b.and(a) }
    };
    // Ambos os transportes drenados, inclusive capturas preparatórias, antes do host.lock.
    drop(trava);
    resultado
}

#[cfg(target_os = "linux")]
pub(super) async fn servir_socket(dir: &Path, p: &Arc<Procedimento>, parar: impl Future<Output = ()>) -> Result<()> {
    let pasta = linux::pasta(dir)?;
    let escuta = linux::escutar(&pasta)?;
    let (aviso, parada) = tokio::sync::watch::channel(false);
    let mut conexoes = tokio::task::JoinSet::new();
    tokio::pin!(parar);
    let resultado = loop {
        tokio::select! {
            () = &mut parar => break Ok(()),
            Some(_) = conexoes.join_next(), if !conexoes.is_empty() => {}
            aceito = escuta.accept() => {
                let stream = match aceito {
                    Ok((stream, _)) => stream,
                    Err(e) => break Err(e.into()),
                };
                // UID real do kernel, mesma instância/pasta. Necessário, não prova gesto nem âncora SSH.
                let ok = linux::pasta(dir).and_then(|p| linux::pais(&p)).is_ok()
                    && linux::socket(&pasta).is_ok()
                    && stream.peer_cred().is_ok_and(|c| c.uid() == linux::euid());
                if !ok {
                    continue;
                }
                let (p, parada) = (p.clone(), parada.clone());
                conexoes.spawn(async move {
                    let _ = procedure::sessao(&p, stream, parada).await;
                });
            }
        }
    };
    // Parada: instância e ingressos desligados; não aceita mais; aguarda TODA conexão, e cada uma aguarda seu
    // worker (spawn_blocking não é abortado). Só depois o chamador solta host.lock.
    p.viva.store(false, std::sync::atomic::Ordering::Release);
    let _ = aviso.send(true);
    drop(escuta);
    while conexoes.join_next().await.is_some() {}
    resultado
}

#[cfg(not(target_os = "linux"))]
pub(super) async fn servir_socket(
    _dir: &Path,
    _p: &Arc<Procedimento>,
    _parar: impl std::future::Future<Output = ()>,
) -> Result<()> {
    anyhow::bail!("not_allowed: instalação D430 requer Linux")
}

/// Ponte do helper: não abre DB, identidade ou semente; só copia bytes no contexto administrativo.
#[cfg(target_os = "linux")]
async fn ponte(dir: &Path) -> Result<()> {
    let pasta = linux::pasta(dir)?;
    linux::pais(&pasta)?;
    let stream = tokio::net::UnixStream::connect(linux::socket(&pasta)?).await?;
    ensure!(stream.peer_cred()?.uid() == linux::euid(), "not_allowed: procedimento D430 de outro UID");
    let (mut leitura, mut escrita) = stream.into_split();
    let (mut entrada, mut saida) = (tokio::io::stdin(), tokio::io::stdout());
    tokio::select! {
        r = tokio::io::copy(&mut entrada, &mut escrita) => { r?; }
        r = tokio::io::copy(&mut leitura, &mut saida) => { r?; }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
async fn ponte(_dir: &Path) -> Result<()> {
    anyhow::bail!("not_allowed: instalação D430 requer Linux")
}

pub(crate) async fn executar(acao: Acao) -> Result<()> {
    let dir: PathBuf = crate::service::data_dir();
    crate::environment::ensure_own_data_dir(&crate::service::home(), &dir)?;
    match acao {
        Acao::Init => {
            let trava = tokio::task::spawn_blocking(crate::service::lock_host).await??;
            let k = nascer(&dir, &trava)?;
            drop(trava);
            println!("Instalação D430 isolada criada (Mexerica). Hub: {}. Ainda sem aparelho raiz.", k.h);
            Ok(())
        }
        Acao::Helper => ponte(&dir).await,
    }
}
