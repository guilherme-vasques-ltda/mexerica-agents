//! Ponte dentro do namespace de rede: TCP em 127.0.0.1 até o socket Unix do proxy.
//! A ponte só copia bytes; a autenticação continua no proxy e o token nunca passa por ela.
#![allow(dead_code)]

use anyhow::{Result, ensure};
use std::fmt::Write as _;
use std::{io, path::Path};

/// Modo interno do próprio binário, executado dentro do namespace (sem tokio).
/// Argumentos: `<porta-ou-0> <socket-unix> <arquivo-da-porta>`.
#[cfg(not(unix))]
pub(crate) fn run_bridge_process(_args: &[String]) -> Result<()> {
    anyhow::bail!("ponte interna indisponível nesta plataforma")
}

#[cfg(unix)]
pub(crate) fn run_bridge_process(args: &[String]) -> Result<()> {
    use std::{net::TcpListener as StdListener, os::unix::net::UnixStream as StdUnix, thread};
    #[cfg(target_os = "linux")]
    {
        // Se o Hub cair, a ponte não fica órfã segurando o namespace.
        // SAFETY: prctl/getppid não recebem ponteiros.
        // SAFETY: getppid não recebe ponteiros.
        let parent = unsafe { libc::getppid() };
        // SAFETY: prctl não recebe ponteiros.
        if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: getppid não recebe ponteiros.
        if unsafe { libc::getppid() } != parent {
            anyhow::bail!("Hub terminou durante a inicialização da ponte");
        }
    }
    ensure!(args.len() == 3, "ponte interna: argumentos inválidos");
    let listener = StdListener::bind((std::net::Ipv4Addr::LOCALHOST, args[0].parse::<u16>()?))?;
    let port = listener.local_addr()?.port();
    std::fs::write(&args[2], port.to_string())?;
    let socket = Path::new(&args[1]).to_owned();
    for client in listener.incoming() {
        let Ok(mut client) = client else { continue };
        let socket = socket.clone();
        thread::spawn(move || {
            let Ok(mut upstream) = StdUnix::connect(&socket) else { return };
            let Ok(mut client_read) = client.try_clone() else { return };
            let Ok(mut upstream_read) = upstream.try_clone() else { return };
            let forward = thread::spawn(move || {
                let _ = io::copy(&mut client_read, &mut upstream);
            });
            let _ = io::copy(&mut upstream_read, &mut client);
            let _ = forward.join();
        });
    }
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Script executado dentro do namespace externo (root mapeado) que sobe `lo`,
/// inicia a ponte e entra nos namespaces internos (usuário com o uid original
/// e PID novo, com `/proc` novo) para o Chromium.
pub(crate) fn launch_script(
    host_binary: &Path,
    profile: &Path,
    chromium_args: &[String],
    uid: u32,
    gid: u32,
    hide_paths: &[std::path::PathBuf],
    display: u32,
) -> Result<String> {
    ensure!(
        !chromium_args.iter().any(|arg| arg.contains("Proxy-Authorization")),
        "script não pode carregar credencial"
    );
    // A ponte roda depois dos tmpfs: se o binário estiver sob um caminho
    // escondido, o script morreria sem diagnóstico. Recusa explícita aqui.
    ensure!(
        !hide_paths.iter().any(|hidden| host_binary.starts_with(hidden) && !host_binary.starts_with(profile)),
        "binário do Hub inacessível após os mounts de contenção: {} está sob um caminho escondido da sessão; a sessão do navegador não foi aberta",
        host_binary.display()
    );
    let socket = profile.join("proxy.sock");
    let port_file = profile.join("bridge.port");
    let profile_q = shell_quote(&profile.display().to_string());
    // Contenção por lista do que volta, não só do que some: staging privado
    // dentro do próprio namespace (tmpfs novo em /dev/shm + mktemp -d 0700,
    // nunca caminho previsível em diretório compartilhado), e devolução só
    // do perfil da sessão e do socket X<display> desta sessão. Qualquer
    // mount que falhe encerra o script (fail-closed).
    let mut mounts = String::from(
        "mount -t tmpfs -o mode=1777 tmpfs /dev/shm\nstage=$(mktemp -d /dev/shm/.mx-browser-stage.XXXXXX)\nmkdir -p $stage/profile $stage/x11\n",
    );
    let _ = writeln!(mounts, "mount --bind {profile_q} $stage/profile");
    let _ = writeln!(
        mounts,
        "[ ! -S /tmp/.X11-unix/X{display} ] || {{ touch $stage/x11/X{display}; mount --bind /tmp/.X11-unix/X{display} $stage/x11/X{display}; }}"
    );
    let mut hidden: Vec<String> = Vec::new();
    for path in hide_paths {
        let quoted = shell_quote(&path.display().to_string());
        if hidden.contains(&quoted) {
            continue;
        }
        hidden.push(quoted.clone());
        let _ = writeln!(mounts, "[ ! -d {quoted} ] || mount -t tmpfs -o mode=0700 tmpfs {quoted}");
    }
    let _ = write!(
        mounts,
        "mkdir -p {profile_q}\nmount --bind $stage/profile {profile_q}\nmkdir -p /tmp/.X11-unix\n[ ! -S $stage/x11/X{display} ] || {{ touch /tmp/.X11-unix/X{display}; mount --bind $stage/x11/X{display} /tmp/.X11-unix/X{display}; }}\numount $stage/profile 2>/dev/null || true\numount $stage/x11/X{display} 2>/dev/null || true\nrm -f $stage/x11/X{display}\nrmdir $stage/profile $stage/x11 $stage\n"
    );
    Ok(format!(
        "#!/bin/sh\nset -e\nip link set lo up\n{mounts}{} __browser-bridge 0 {} {} 3<&- 4<&- &\nbridge_pid=$!\ni=0\nwhile [ ! -s {} ]; do i=$((i+1)); [ $i -gt 200 ] && exit 1; sleep 0.05; done\nport=$(cat {})\nexec unshare -Upf --mount-proc --map-user={uid} --map-group={gid} {} --proxy-server=http://127.0.0.1:$port {args}\n",
        shell_quote(&host_binary.display().to_string()),
        shell_quote(&socket.display().to_string()),
        shell_quote(&port_file.display().to_string()),
        shell_quote(&port_file.display().to_string()),
        shell_quote(&port_file.display().to_string()),
        shell_quote(chromium_args.first().map(String::as_str).unwrap_or_default()),
        uid = uid,
        gid = gid,
        args = chromium_args.iter().skip(1).map(|arg| shell_quote(arg)).collect::<Vec<_>>().join(" "),
    ))
}

/// Falha fechada quando o servidor não permite namespaces de usuário/rede.
pub(crate) fn namespace_error(detail: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "contenção indisponível: o servidor não permite namespaces de usuário e de rede ({detail}); a sessão do navegador não foi aberta"
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn ensure_namespaces_available() -> Result<()> {
    let output = std::process::Command::new("unshare")
        .args([
            "-Urmnpf",
            "--mount-proc",
            "--map-root-user",
            "/bin/sh",
            "-c",
            "mount -t tmpfs tmpfs /tmp && mount -t tmpfs tmpfs /dev/shm",
        ])
        .output()
        .map_err(|error| namespace_error(&error.to_string()))?;
    ensure!(output.status.success(), "{}", namespace_error("unshare -Urmn ou mount tmpfs recusado"));
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn ensure_namespaces_available() -> Result<()> {
    Err(namespace_error("plataforma sem namespaces Linux"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn ponte_de_producao_encaminha_ate_socket_unix() {
        use std::{
            io::{Read, Write},
            net::TcpStream,
            os::unix::net::{UnixListener, UnixStream},
        };
        let dir = std::env::temp_dir().join(format!("mex-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("proxy.sock");
        let upstream = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut byte = [0u8; 4];
            stream.read_exact(&mut byte).unwrap();
            stream.write_all(b"pong").unwrap();
            byte
        });
        let port_file = dir.join("bridge.port");
        let bridge_socket = socket.clone();
        let bridge_port = port_file.clone();
        let bridge = std::thread::spawn(move || {
            run_bridge_process(&["0".into(), bridge_socket.display().to_string(), bridge_port.display().to_string()])
        });
        let mut port = String::new();
        for _ in 0..100 {
            if let Ok(value) = std::fs::read_to_string(&port_file) {
                port = value;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let mut client = TcpStream::connect(format!("127.0.0.1:{}", port.trim())).unwrap();
        client.write_all(b"ping").unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"pong");
        assert_eq!(&server.join().unwrap(), b"ping");
        drop(bridge);
        let _ = UnixStream::connect(&socket);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn script_nao_contem_token_e_usa_namespace_interno() {
        let script = launch_script(
            Path::new("/usr/local/bin/donnabot-host"),
            Path::new("/perfil"),
            &["/usr/bin/chromium".into(), "--user-data-dir=/perfil".into(), "about:blank".into()],
            1000,
            1000,
            &[Path::new("/home/hub").into(), Path::new("/tmp").into()], // vazamento-ok: caminho fictício em teste
            7,
        )
        .unwrap();
        assert!(script.contains("ip link set lo up"));
        assert!(script.contains("__browser-bridge"));
        assert!(script.contains("3<&- 4<&-"), "ponte deve fechar os fds do CDP no script");
        assert!(script.contains("--map-user=1000"));
        assert!(script.contains("--map-group=1000"));
        assert!(!script.contains("--setgroups"), "setgroups deny quebra o userns interno na Instabox");
        assert!(script.contains("--proxy-server=http://127.0.0.1:$port"));
        assert!(script.contains("mount -t tmpfs"), "script sem mount namespace");
        assert!(script.contains("mount --bind"), "script sem devolução do perfil");
        assert!(!script.contains("segredo"));
        // P2-1/P2-2: PID ns com /proc novo, /dev/shm novo, staging privado
        // dentro do ns e só o socket X<display> da sessão de volta.
        assert!(script.contains("unshare -Upf --mount-proc"), "Chromium sem PID ns/proc novo: {script}");
        assert!(script.contains("mount -t tmpfs -o mode=1777 tmpfs /dev/shm"), "sem tmpfs novo em /dev/shm");
        assert!(script.contains("mktemp -d /dev/shm/.mx-browser-stage.XXXXXX"), "staging previsível ou fora do ns");
        assert!(!script.contains("stage=/dev/shm/.mx-browser-stage\n"), "staging em caminho fixo compartilhado");
        assert!(script.contains("/tmp/.X11-unix/X7"), "socket X da sessão ausente");
        assert!(!script.contains("mount --bind /tmp/.X11-unix $stage"), "diretório X inteiro devolvido");
    }

    #[test]
    fn script_recusa_ponte_sob_caminho_escondido() {
        let error = launch_script(
            Path::new("/home/hub/bin/donnabot-host"), // vazamento-ok: caminho fictício em teste
            Path::new("/perfil"),
            &["/usr/bin/chromium".into(), "about:blank".into()],
            1000,
            1000,
            &[Path::new("/home/hub").into()], // vazamento-ok: caminho fictício em teste
            0,
        )
        .unwrap_err();
        assert!(error.to_string().contains("binário do Hub inacessível"), "{error}");
    }

    #[test]
    fn mensagem_de_namespace_eh_clara() {
        assert!(namespace_error("teste").to_string().contains("a sessão do navegador não foi aberta"));
    }
}
