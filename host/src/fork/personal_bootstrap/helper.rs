//! Consumidor do listener entregue pelo PR4; não faz bind/unlink/chown nem lança helper.
use super::{ceremony, protocol};
use crate::{hub::Hub, remote::crypto};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
pub(super) const DOMAIN: &str = "donna/personal-linux/helper-hub/v1";
const SOCKET: &str = "/run/donnabot/bootstrap-hub-v1.sock";
pub(crate) struct Listener {
    #[cfg(target_os = "linux")]
    socket: tokio::net::UnixListener,
    #[cfg(target_os = "linux")]
    path_identity: (u64, u64),
}
#[cfg(target_os = "linux")]
mod linux {
    use super::{Listener, Result, SOCKET, ensure};
    use std::{
        ffi::CString,
        fs::File,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
    };
    fn option(fd: i32, name: i32) -> Result<i32> {
        let mut value = 0i32;
        let expected_len = libc::socklen_t::try_from(size_of_val(&value))?;
        let mut len = expected_len;
        // SAFETY: value/len são inicializados, alinhados e graváveis; len delimita i32 e a chamada não retém ponteiros.
        let status = unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, name, (&raw mut value).cast(), &raw mut len) };
        ensure!(status == 0 && len == expected_len, "invalid inherited socket");
        Ok(value)
    }
    fn path() -> Result<(u64, u64)> {
        let mut parent = File::open("/")?;
        for name in ["run", "donnabot"] {
            let m = parent.metadata()?;
            ensure!(m.is_dir() && m.uid() == 0 && m.mode() & 0o022 == 0, "unprotected helper parent");
            let name = CString::new(name)?;
            // SAFETY: dirfd mantido vivo, novo descritor próprio, nunca segue links.
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            ensure!(fd >= 0, "helper parent unavailable");
            // SAFETY: adota somente descritor recém-aberto.
            parent = unsafe { File::from_raw_fd(fd) };
        }
        let m = parent.metadata()?;
        ensure!(m.uid() == 0 && m.mode() & 0o7777 == 0o750, "unprotected helper directory");
        let name = c"bootstrap-hub-v1.sock";
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: parent está vivo, name é NUL-terminado e stat tem espaço/alinhamento de libc::stat, sem retenção.
        let status =
            unsafe { libc::fstatat(parent.as_raw_fd(), name.as_ptr(), stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
        ensure!(status == 0, "helper socket path unavailable");
        // SAFETY: fstatat retornou zero e inicializou o libc::stat inteiro; parent/name viveram durante a chamada.
        let s = unsafe { stat.assume_init() };
        ensure!(
            s.st_uid == 0
                && s.st_mode & libc::S_IFMT == libc::S_IFSOCK
                && s.st_mode & 0o7777 == 0o660
                && s.st_nlink == 1,
            "helper pathname protection mismatch"
        );
        Ok((s.st_dev, s.st_ino))
    }
    pub(super) fn validate(fd: i32) -> Result<(i32, (u64, u64))> {
        // SAFETY: F_GETFD só consulta o inteiro fd; não consome o descritor nem recebe ponteiros, e erro é recusado.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        ensure!(flags >= 0, "personal listener missing");
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: stat é buffer alinhado de libc::stat; fstat não retém o ponteiro nem toma ownership de fd.
        let status = unsafe { libc::fstat(fd, stat.as_mut_ptr()) };
        ensure!(status == 0, "invalid personal descriptor");
        // SAFETY: fstat retornou zero e inicializou libc::stat, antes de qualquer leitura do buffer.
        let s = unsafe { stat.assume_init() };
        // sockfs e pathname são objetos distintos: não comparar inodes ou modo0660.
        ensure!(
            s.st_uid == 0
                && s.st_mode & libc::S_IFMT == libc::S_IFSOCK
                && option(fd, libc::SO_DOMAIN)? == libc::AF_UNIX
                && option(fd, libc::SO_TYPE)? == libc::SOCK_STREAM
                && option(fd, libc::SO_ACCEPTCONN)? == 0,
            "personal listener origin/state mismatch"
        );
        let mut address = std::mem::MaybeUninit::<libc::sockaddr_un>::zeroed();
        let mut len = libc::socklen_t::try_from(size_of::<libc::sockaddr_un>())?;
        // SAFETY: address está zerado/alinhado e len cabe no sockaddr_un; ambos ficam vivos e a chamada não retém ponteiros.
        let status = unsafe { libc::getsockname(fd, address.as_mut_ptr().cast(), &raw mut len) };
        ensure!(status == 0, "personal socket name unavailable");
        // SAFETY: sockaddr_un zerado tem campos escalares/array válidos; a chamada bem-sucedida só preenche esse buffer.
        let address = unsafe { address.assume_init() };
        let name = address.sun_path.iter().map(|c| c.to_ne_bytes()[0]).take_while(|c| *c != 0).collect::<Vec<_>>();
        ensure!(
            i32::from(address.sun_family) == libc::AF_UNIX && name == SOCKET.as_bytes(),
            "personal socket name mismatch"
        );
        let before = path()?;
        Ok((flags, before))
    }
    pub(super) fn adopt() -> Result<Listener> {
        // O chamador já tem Prepared admitido. Perfil legado nunca toca fd3.
        let fd = 3;
        let (flags, before) = validate(fd)?;
        // SAFETY: fd3 foi validado e ainda não é adotado; F_SETFD recebe só flags válidas, sem ponteiros ou spawn concorrente aqui.
        let status = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
        ensure!(status == 0, "personal socket CLOEXEC failed");
        // SO_PEERCRED do servidor é capturado aqui, sob UID real do serviço, não root.
        // SAFETY: geteuid consulta a identidade efetiva do processo, não recebe nem retém ponteiros.
        let uid = unsafe { libc::geteuid() };
        ensure!(uid != 0, "personal service must not run as root");
        // SAFETY: validate confirmou AF_UNIX/SOCK_STREAM/bind-sem-listen; fd segue vivo e não adotado, backlog é positivo.
        let status = unsafe { libc::listen(fd, 8) };
        ensure!(status == 0 && path()? == before, "personal listener changed");
        // Só agora tomamos ownership. Nenhum spawn precede CLOEXEC no serve.
        // SAFETY: fd3 validado pertence a esta entrega única; a conversão o adota uma vez e não há outro owner Rust.
        let socket = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
        socket.set_nonblocking(true)?;
        Ok(Listener { socket: tokio::net::UnixListener::from_std(socket)?, path_identity: before })
    }
    pub(super) fn recheck(expected: (u64, u64)) -> Result<()> {
        ensure!(path()? == expected, "personal helper pathname changed");
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn root_peer(stream: &tokio::net::UnixStream) -> Result<()> {
    ensure!(stream.peer_cred()?.uid() == 0, "helper peer must be root");
    Ok(())
}
impl Listener {
    pub(super) fn adopt() -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::adopt()
        }
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!("personal helper requires Linux")
        }
    }
    pub(crate) async fn run(self, hub: Arc<Hub>) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            loop {
                let (mut stream, _) = self.socket.accept().await?;
                linux::recheck(self.path_identity)?;
                if root_peer(&stream).is_err() {
                    continue;
                }
                let hub = hub.clone();
                tokio::spawn(async move {
                    let _ = session(&hub, &mut stream).await;
                });
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = hub;
            anyhow::bail!("personal helper requires Linux")
        }
    }
}
pub(super) async fn read<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Value> {
    let first = stream.read_u8().await?;
    let mut tail = [0u8; 3];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut tail)).await??;
    let len = usize::try_from(u32::from_be_bytes([first, tail[0], tail[1], tail[2]]))?;
    ensure!((1..=protocol::LIMIT).contains(&len), "invalid helper frame size");
    let mut bytes = vec![0; len];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut bytes)).await??;
    protocol::canonical(&bytes)
}
pub(super) async fn write<S: AsyncWrite + Unpin>(stream: &mut S, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(!bytes.is_empty() && bytes.len() <= protocol::LIMIT, "helper result too large");
    stream.write_u32(u32::try_from(bytes.len())?).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}
async fn session<S: AsyncRead + AsyncWrite + Unpin>(hub: &Arc<Hub>, stream: &mut S) -> Result<()> {
    // Identidade privada retida na conexão. UID0 não seleciona tentativas globais.
    let capture = crypto::b64(&crypto::random::<32>());
    let mut requests = std::collections::BTreeMap::<String, (Value, Value)>::new();
    loop {
        let frame = read(stream).await?;
        let a = frame.as_array().context("invalid helper request")?;
        ensure!(a.len() >= 4 && a[0] == DOMAIN && a[1] == 1, "invalid helper protocol");
        let id = a[2].as_str().context("invalid helper id")?;
        ensure!(crypto::unb64_n::<16>(id).is_ok_and(|b| crypto::b64(&b) == id), "invalid helper id");
        if let Some((old, result)) = requests.get(id) {
            ensure!(*old == frame, "helper id conflict");
            write(stream, result).await?;
            continue;
        }
        ensure!(requests.len() < 128, "helper session limit");
        let result = match tokio::time::timeout(Duration::from_secs(5), ceremony::helper(hub, &capture, &frame)).await {
            Ok(Ok(value)) => json!([DOMAIN, 1, id, "ok", a[3], value]),
            _ => json!([DOMAIN, 1, id, "error", "unavailable"]),
        };
        requests.insert(id.into(), (frame.clone(), result.clone()));
        write(stream, &result).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn helper_frame_fragmentation_limits_and_no_partial_effect() -> Result<()> {
        let (mut a, mut b) = tokio::io::duplex(32_768);
        let request = json!([DOMAIN, 1, crypto::b64(&[9; 16]), "time"]);
        let expected = request.clone();
        let producer = tokio::spawn(async move {
            let bytes = serde_json::to_vec(&request)?;
            a.write_u32(u32::try_from(bytes.len())?).await?;
            for byte in bytes {
                a.write_all(&[byte]).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        assert_eq!(read(&mut b).await?, expected);
        producer.await??;
        for size in [0, 16_385] {
            let (mut a, mut b) = tokio::io::duplex(4);
            a.write_u32(size).await?;
            assert!(read(&mut b).await.is_err());
        }
        let (mut a, mut b) = tokio::io::duplex(32);
        a.write_u32(4).await?;
        a.write_all(b"[]").await?;
        drop(a);
        assert!(read(&mut b).await.is_err());
        let (mut a, mut b) = tokio::io::duplex(32);
        a.write_u32(4).await?;
        a.write_all(b" [1]").await?;
        assert!(read(&mut b).await.is_err());
        Ok(())
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn helper_actual_socket_peer_comes_from_kernel_not_frame() -> Result<()> {
        let (a, _b) = tokio::net::UnixStream::pair()?;
        let uid = a.peer_cred()?.uid();
        assert_eq!(root_peer(&a).is_ok(), uid == 0);
        // Não há campo UID no frame que possa substituir a credencial SO_PEERCRED.
        let (mut source, mut sink) = tokio::io::duplex(128);
        write(&mut source, &json!([DOMAIN,1,crypto::b64(&[4;16]),"time",{"uid":0}])).await?;
        assert_eq!(read(&mut sink).await?.as_array().map(Vec::len), Some(5));
        assert_eq!(root_peer(&a).is_ok(), uid == 0);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn legacy_does_not_adopt_or_close_an_unrecognized_descriptor() -> Result<()> {
        // Testa a regra de não tomar ownership: a validação recusada não usa from_raw_fd.
        use std::os::fd::AsRawFd;
        let (a, _b) = std::os::unix::net::UnixStream::pair()?;
        assert!(linux::validate(a.as_raw_fd()).is_err());
        // SAFETY: a mantém seu fd vivo; F_GETFD apenas consulta flags e não transfere nem fecha o descritor.
        assert!(unsafe { libc::fcntl(a.as_raw_fd(), libc::F_GETFD) } >= 0);
        Ok(())
    }
}
