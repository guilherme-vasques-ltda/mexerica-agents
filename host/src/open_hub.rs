use crate::{Cli, Sub, environment::Environment};
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::Value;

pub(crate) const SOCKET_PEER: std::net::SocketAddr =
    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0));

#[derive(Parser)]
#[command(name = "mexerica-open-hub", version)]
struct OpenCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        port: u16,
        #[arg(long)]
        bind: String,
        #[arg(long)]
        only_this_mac: bool,
    },
}

pub(crate) fn cli() -> Cli {
    let Command::Serve { port, bind, only_this_mac } = OpenCli::parse().command;
    Cli { cmd: Some(Sub::Serve { port, bind: Some(bind), only_this_mac }) }
}

pub(crate) fn require_executor() -> std::io::Result<()> {
    if Environment::is_open() {
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Mexerica Open has no isolated executor"));
    }
    Ok(())
}

pub(crate) fn check_request(method: &str, body: &Value) -> Result<()> {
    if !Environment::is_open() {
        return Ok(());
    }
    match method {
        "hello"
        | "refreshBackends"
        | "teamSettings"
        | "usage"
        | "history"
        | "markRead"
        | "donnaSpaces"
        | "donnaSpaceCreate"
        | "donnaSpaceUpdate"
        | "donnaSpaceReorder"
        | "donnaOwnerPresentation"
        | "donnaEngineAccounts"
        | "donnaEngineAccess"
        | "donnaBotEditor"
        | "donnaBotEditorSave"
        | "donnaConversations"
        | "donnaConversationHistory"
        | "donnaConversationRead"
        | "stop"
        | "deleteBot"
        | "reorderBots" => Ok(()),
        "createBot" | "updateBot" if body.get("cwd").is_none_or(|cwd| cwd.as_str() == Some("")) => Ok(()),
        // Só texto, na conversa principal do bot: sem anexos e sem sessão extra.
        "send" if ["attachments", "sessionId"].iter().all(|key| body.get(key).is_none_or(Value::is_null)) => Ok(()),
        _ if crate::fork::engine_setup::open_login_method(method) => Ok(()),
        _ => Err(crate::api::devices::Forbidden.into()),
    }
}

#[cfg(unix)]
pub(crate) use unix::serve;

#[cfg(unix)]
mod unix {
    use super::{Environment, Result, SOCKET_PEER};
    use crate::fork::private_fs::{self, Directory};
    use crate::hub::Hub;
    use crate::service::mac_local::LOOPBACK;
    use anyhow::{Context, ensure};
    use axum::{Extension, extract::ConnectInfo};
    use std::ffi::{CString, OsStr, OsString};
    use std::fs;
    use std::future::IntoFuture;
    use std::io;
    use std::net::Ipv4Addr;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tokio::net::{TcpListener, UnixListener};
    use tokio::sync::oneshot;

    const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
    const MARKER: &str = ".open-profile-v1";
    const PROFILE_FORMAT: &[u8] = b"mexerica-open-v1\n";

    struct Profile {
        data: PathBuf,
        socket: PathBuf,
    }

    impl Profile {
        fn from_environment() -> Result<Self> {
            for name in ["MEXERICA_HOME", "DONNABOT_HOME", "CODYNC_HOME"] {
                ensure!(std::env::var_os(name).is_none(), "Mexerica Open does not accept data overrides");
            }
            ensure!(std::env::var_os("PATH").is_some_and(|path| path == SYSTEM_PATH), "invalid Mexerica Open PATH");
            let path = |key| std::env::var_os(key).map(PathBuf::from).with_context(|| format!("{key} is required"));
            let home = path("HOME")?;
            let tmp = path("TMPDIR")?;
            let socket = path("OPEN_HUB_SOCKET")?;
            let cwd = std::env::current_dir()?;
            Self::validate(&home, &tmp, &socket, &cwd)
        }

        fn validate(home: &Path, tmp: &Path, socket: &Path, cwd: &Path) -> Result<Self> {
            let root = home.parent().context("missing Mexerica Open profile")?;
            ensure!(
                root.file_name().is_some_and(|name| name == Environment::Open.data_folder())
                    && home == root.join("home")
                    && cwd == home
                    && tmp == root.join("tmp"),
                "invalid Mexerica Open profile layout"
            );
            let session = session_directory(socket, tmp)?;
            for dir in [root, home, tmp, session] {
                private_directory(dir)?;
            }
            match fs::symlink_metadata(socket) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
                Ok(_) => anyhow::bail!("Mexerica Open socket already exists"),
            }
            Ok(Self { data: home.join(Environment::Open.data_folder()), socket: socket.to_path_buf() })
        }

        fn reserve_data(&self) -> Result<fs::File> {
            match fs::symlink_metadata(&self.data) {
                Ok(_) => private_directory(&self.data)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Directory::open(&self.data, true)?;
                }
                Err(error) => return Err(error.into()),
            }
            validate_data(&self.data)?;
            let lock = crate::service::lock_host_at(&self.data.join("host.lock"))?;
            let marker = self.data.join(MARKER);
            if let Some(contents) = private_fs::read(&marker)? {
                ensure!(contents == PROFILE_FORMAT, "invalid Mexerica Open data marker");
            } else {
                for entry in fs::read_dir(&self.data)? {
                    ensure!(entry?.file_name() == "host.lock", "refusing data from another Mexerica installation");
                }
                private_fs::create_atomic(&marker, PROFILE_FORMAT)?;
            }
            drop_idle_codex_helpers(&self.data);
            Ok(lock)
        }

        fn open_store(&self) -> Result<crate::store::Store> {
            use crate::store::{Store, personal_database};
            let path = self.data.join("codync.db");
            if let Some(database) = personal_database::capture(&path)? {
                ensure!(!database.evidence().personal, "personal database unavailable in Mexerica Open");
                return Store::open_personal(database);
            }
            Store::open_created(personal_database::create_exclusive(&path)?)
        }
    }

    fn session_directory<'a>(socket: &'a Path, tmp: &Path) -> Result<&'a Path> {
        let session = socket.parent().context("missing Mexerica Open session directory")?;
        ensure!(
            socket.as_os_str() == session.join("s").as_os_str()
                && socket.as_os_str().as_bytes().len() <= 103
                && session.parent() == Some(tmp)
                && session.file_name().is_some_and(|name| name.as_bytes().starts_with(b"h-")),
            "invalid Mexerica Open socket layout"
        );
        Ok(session)
    }

    fn private_directory(path: &Path) -> Result<()> {
        ensure!(path.is_absolute(), "relative Mexerica Open directory");
        let metadata = Directory::open(path, false)?.0.metadata()?;
        ensure!(path.canonicalize()?.as_os_str() == path, "noncanonical Mexerica Open directory");
        ensure!(metadata.mode() & 0o7777 == 0o700, "Mexerica Open directories require mode 0700");
        Ok(())
    }

    /// Pasta de ajudantes que o Codex recria a cada início em `.codex/tmp`; fica com links quando ele é morto à força.
    fn codex_helpers(path: &Path) -> bool {
        fn name(path: Option<&Path>) -> &str {
            path.and_then(Path::file_name).and_then(|name| name.to_str()).unwrap_or_default()
        }
        let folder = name(Some(path));
        (folder == "arg0" || folder.strip_prefix("arg0-").is_some_and(|os| os.bytes().all(|b| b.is_ascii_lowercase())))
            && name(path.parent()) == "tmp"
            && name(path.parent().and_then(Path::parent)) == ".codex"
    }

    fn validate_data(root: &Path) -> Result<()> {
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                let metadata = entry.metadata()?;
                ensure!(metadata.uid() == private_fs::uid(), "wrong owner in Mexerica Open data");
                if kind.is_dir() && codex_helpers(&entry.path()) {
                    // Tolerada aqui; quem a limpa é `drop_idle_codex_helpers`, só depois da trava do perfil.
                } else if kind.is_dir() {
                    ensure!(metadata.mode() & 0o022 == 0, "writable directory in Mexerica Open data");
                    pending.push(entry.path());
                } else {
                    ensure!(
                        kind.is_file() && metadata.nlink() == 1 && metadata.mode() & 0o022 == 0,
                        "unsafe file in Mexerica Open data"
                    );
                }
            }
        }
        Ok(())
    }

    /// Pastas de ajudantes que um Codex morto à força deixou em `.codex/tmp`. A limpeza roda uma única vez no
    /// arranque, com o perfil já travado e o marcador confirmado e antes de este Hub lançar qualquer Codex: o
    /// `host.lock` e o marcador bastam como prova, sem `flock` nem confiança no `.lock`. Uma resposta da conversa
    /// da Open em curso adia tudo para o próximo início.
    fn drop_idle_codex_helpers(root: &Path) {
        if !crate::fork::open_chat::idle() {
            tracing::warn!("resposta da conversa da Open em curso; pastas de ajudantes do Codex ficaram sem limpeza");
            return;
        }
        limpar_codex_helpers(root, |_, _| ());
    }

    /// Limites da limpeza: uma estrutura hostil não faz a remoção girar sem fim.
    const ARG0_PROFUNDIDADE: u32 = 8;
    const ARG0_ENTRADAS: usize = 10_000;

    /// Desce pelo perfil até o `.codex/tmp` de cada conta — `engine-setup/accounts/<id>/<home>/.codex/tmp`, o
    /// layout que a criação de perfil monta — sempre por descritor, sem seguir links, e esvazia as pastas `arg0*`
    /// de dono do Hub. `troca` roda depois de abrir a pasta `arg0*` e antes de remover.
    fn limpar_codex_helpers(root: &Path, mut troca: impl FnMut(&fs::File, &OsStr)) {
        let mut pasta = match Directory::open(root, false) {
            Ok(base) => base.0,
            Err(_) => return,
        };
        for parte in ["engine-setup", "accounts"] {
            let Some(proxima) = abrir_pasta(&pasta, OsStr::new(parte)) else { return };
            pasta = proxima;
        }
        for conta in nomes(&pasta) {
            let Some(conta) = abrir_pasta(&pasta, &conta) else { continue };
            for casa in nomes(&conta) {
                let Some(casa) = abrir_pasta(&conta, &casa) else { continue };
                let Some(codex) = abrir_pasta(&casa, OsStr::new(".codex")) else { continue };
                let Some(tmp) = abrir_pasta(&codex, OsStr::new("tmp")) else { continue };
                limpar_tmp(&tmp, &mut troca);
            }
        }
    }

    /// Esvazia as pastas `arg0*` do `.codex/tmp` aberto por descritor; o resto da pasta não é tocado.
    fn limpar_tmp<F: FnMut(&fs::File, &OsStr)>(tmp: &fs::File, troca: &mut F) {
        let uid = private_fs::uid();
        let mut entradas = ARG0_ENTRADAS;
        for nome in nomes(tmp) {
            if !nome_arg0(&nome) {
                continue;
            }
            let Some(pasta) = abrir_pasta(tmp, &nome) else { continue };
            if !pasta.metadata().is_ok_and(|meta| meta.uid() == uid) {
                tracing::warn!("pasta de ajudantes do Codex de outro dono ficou sem limpeza");
                continue;
            }
            troca(tmp, &nome);
            if !esvaziar(&pasta, 0, &mut entradas) {
                tracing::warn!("pasta de ajudantes do Codex fora do esperado ficou sem limpeza");
            } else if !remover_pelo_nome(tmp, &nome, true) {
                tracing::warn!("pasta de ajudantes do Codex esvaziada não pôde ser removida pelo nome");
            }
        }
    }

    /// Esvazia a pasta aberta por descritor: arquivo comum e link saem sem ser seguidos, pasta desce e sai por
    /// último. Falso quando algo fora do esperado — entrada de tipo especial, dono estranho, limite estourado —
    /// aborta o resto dessa pasta.
    fn esvaziar(pasta: &fs::File, nivel: u32, entradas: &mut usize) -> bool {
        if nivel > ARG0_PROFUNDIDADE {
            return false;
        }
        let uid = private_fs::uid();
        for nome in nomes(pasta) {
            if *entradas == 0 {
                return false;
            }
            *entradas -= 1;
            let Some(meta) = fstatat_nofollow(pasta, &nome) else { return false };
            if meta.st_uid != uid {
                return false;
            }
            match meta.st_mode & libc::S_IFMT {
                libc::S_IFDIR => {
                    let Some(filha) = abrir_pasta(pasta, &nome) else { return false };
                    if !esvaziar(&filha, nivel + 1, entradas) {
                        return false;
                    }
                    if !remover_pelo_nome(pasta, &nome, true) {
                        return false;
                    }
                }
                libc::S_IFREG | libc::S_IFLNK => {
                    if !remover_pelo_nome(pasta, &nome, false) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    /// Nome de pasta de ajudantes do Codex: `arg0` ou `arg0-<letras minúsculas>`.
    fn nome_arg0(nome: &OsStr) -> bool {
        let bytes = nome.as_bytes();
        bytes == b"arg0" || bytes.strip_prefix(b"arg0-").is_some_and(|sufixo| sufixo.iter().all(u8::is_ascii_lowercase))
    }

    /// Abre a pasta `nome` dentro do diretório já aberto, sem seguir links; `None` quando não existe ou não é pasta.
    fn abrir_pasta(pai: &fs::File, nome: &OsStr) -> Option<fs::File> {
        let valor = CString::new(nome.as_bytes()).ok()?;
        // SAFETY: descritor do pai vivo e nome sem barra; a abertura é relativa e não segue links.
        let fd = unsafe {
            libc::openat(
                pai.as_raw_fd(),
                valor.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }
        // SAFETY: fd recém-aberto transferido uma única vez.
        Some(unsafe { fs::File::from_raw_fd(fd) })
    }

    /// Nomes das entradas do diretório aberto, lidos pelo descritor, sem ordem garantida.
    fn nomes(dir: &fs::File) -> Vec<OsString> {
        let mut saida = Vec::new();
        // SAFETY: o duplicado passa ao fdopendir e morre no closedir; o descritor original segue vivo.
        unsafe {
            let duplicado = libc::dup(dir.as_raw_fd());
            if duplicado < 0 {
                return saida;
            }
            let fluxo = libc::fdopendir(duplicado);
            if fluxo.is_null() {
                libc::close(duplicado);
                return saida;
            }
            loop {
                let entrada = libc::readdir(fluxo);
                if entrada.is_null() {
                    break;
                }
                // SAFETY: readdir devolve nome com terminador nulo dentro da própria estrutura.
                let bytes = std::ffi::CStr::from_ptr((*entrada).d_name.as_ptr()).to_bytes();
                if bytes != b"." && bytes != b".." {
                    saida.push(OsString::from_vec(bytes.to_vec()));
                }
            }
            // SAFETY: o fluxo consome só o duplicado; o descritor original não é dele.
            libc::closedir(fluxo);
        }
        saida
    }

    /// Metadados da entrada sem seguir link, relativos ao diretório aberto.
    fn fstatat_nofollow(pai: &fs::File, nome: &OsStr) -> Option<libc::stat> {
        let valor = CString::new(nome.as_bytes()).ok()?;
        let mut meta = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: descritor e nome válidos; escreve só na estrutura local.
        if unsafe { libc::fstatat(pai.as_raw_fd(), valor.as_ptr(), meta.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0
        {
            return None;
        }
        // SAFETY: fstatat retornou sucesso e preencheu a estrutura.
        Some(unsafe { meta.assume_init() })
    }

    /// Remove a entrada pelo nome relativo ao diretório aberto: pasta vazia (`AT_REMOVEDIR`, que recusa link) ou
    /// arquivo/link comum (removido como link, nunca seguido).
    fn remover_pelo_nome(pai: &fs::File, nome: &OsStr, pasta: bool) -> bool {
        let Ok(valor) = CString::new(nome.as_bytes()) else { return false };
        let bandeira = if pasta { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: descritor e nome válidos; nenhuma das duas formas segue link.
        unsafe { libc::unlinkat(pai.as_raw_fd(), valor.as_ptr(), bandeira) == 0 }
    }

    #[cfg(test)]
    fn perfil_de_teste() -> (PathBuf, PathBuf) {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("open-data-{}", uuid::Uuid::new_v4()));
        let tmp = root.join("engine-setup/accounts/a1/h1/.codex/tmp");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&tmp).unwrap();
        (root, tmp)
    }

    #[cfg(test)]
    fn pasta_de_ajudante(tmp: &Path, nome: &str) -> PathBuf {
        use std::os::unix::fs::DirBuilderExt;
        let pasta = tmp.join(nome).join("codex-sessao");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&pasta).unwrap();
        pasta
    }

    #[cfg(test)]
    fn escrever_privado(path: &Path, bytes: &[u8]) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[cfg(test)]
    #[test]
    fn codex_helpers_sao_limpos_no_arranque_sem_confiar_no_lock() {
        use std::os::unix::fs::symlink;
        let (root, tmp) = perfil_de_teste();
        let com_lock = pasta_de_ajudante(&tmp, "arg0-macos");
        escrever_privado(&com_lock.join(".lock"), b"");
        symlink("/usr/bin/true", com_lock.join("apply_patch")).unwrap();
        let sem_lock = pasta_de_ajudante(&tmp, "arg0-linux");
        validate_data(&root).unwrap();
        assert!(com_lock.exists() && sem_lock.exists(), "a validação inicial não apaga nada");
        limpar_codex_helpers(&root, |_, _| ());
        assert!(
            !com_lock.exists() && !sem_lock.exists(),
            "sem flock não há lock a conferir: o arranque limpa as duas pastas de uma vez"
        );
        symlink("/usr/bin/true", tmp.join("apply_patch")).unwrap();
        assert!(validate_data(&root).is_err(), "link fora da pasta de ajudantes continua recusado");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(test)]
    #[test]
    fn limpeza_do_arg0_remove_link_sem_seguir_o_alvo() {
        use std::os::unix::fs::symlink;
        let (root, tmp) = perfil_de_teste();
        let sentinela = root.join("sentinela");
        fs::write(&sentinela, "intacto").unwrap();
        let pasta = pasta_de_ajudante(&tmp, "arg0-macos");
        escrever_privado(&pasta.join(".lock"), b"");
        symlink(&sentinela, pasta.join("apply_patch")).unwrap();
        limpar_codex_helpers(&root, |_, _| ());
        assert!(!tmp.join("arg0-macos").exists(), "a pasta de ajudantes sai");
        assert_eq!(fs::read(&sentinela).unwrap(), b"intacto", "o alvo do link fora continua intacto");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(test)]
    #[test]
    fn limpeza_do_arg0_fica_presa_ao_descritor_aberto() {
        use std::os::unix::fs::{DirBuilderExt, symlink};
        let (root, tmp) = perfil_de_teste();
        let fora = root.join("fora");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&fora).unwrap();
        fs::write(fora.join("sentinela"), "intacto").unwrap();
        let pasta = pasta_de_ajudante(&tmp, "arg0-linux");
        escrever_privado(&pasta.join(".lock"), b"");
        symlink("/usr/bin/true", pasta.join("apply_patch")).unwrap();
        limpar_codex_helpers(&root, |_, nome| {
            // Troca hostil depois da abertura: a pasta aberta sai do nome e um link entra no lugar.
            let original = tmp.join(nome);
            fs::rename(&original, tmp.join("arg0-linux-trocada")).unwrap();
            symlink(&fora, &original).unwrap();
        });
        let trocada = tmp.join("arg0-linux-trocada");
        assert!(trocada.read_dir().unwrap().count() == 0, "a limpeza esvaziou a pasta que foi aberta");
        assert!(
            fs::symlink_metadata(tmp.join("arg0-linux")).unwrap().file_type().is_symlink(),
            "o link da troca não é seguido nem removido por engano"
        );
        assert_eq!(fs::read(fora.join("sentinela")).unwrap(), b"intacto", "o alvo do link trocado fica intacto");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(test)]
    #[test]
    fn lock_falso_nao_e_confiado_nem_seguido() {
        use std::os::unix::fs::symlink;
        let (root, tmp) = perfil_de_teste();
        let com_link = pasta_de_ajudante(&tmp, "arg0-macos");
        symlink("/usr/bin/true", com_link.join(".lock")).unwrap();
        let com_fifo = pasta_de_ajudante(&tmp, "arg0-linux");
        let fifo = CString::new(com_fifo.join(".lock").as_os_str().as_bytes()).unwrap();
        // SAFETY: caminho nul-terminado do próprio teste; cria só o fifo da fixture.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        limpar_codex_helpers(&root, |_, _| ());
        assert!(!tmp.join("arg0-macos").exists(), "lock que é link é removido como link, sem seguir");
        assert!(Path::new("/usr/bin/true").exists());
        assert!(com_fifo.join(".lock").exists(), "fifo aborta a limpeza dessa pasta e ela fica");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(test)]
    #[test]
    fn resposta_viva_da_conversa_adi_a_a_limpeza_dos_ajudantes() {
        use std::os::unix::fs::DirBuilderExt;
        let (root, tmp) = perfil_de_teste();
        let pasta = pasta_de_ajudante(&tmp, "arg0-macos");
        escrever_privado(&pasta.join(".lock"), b"");
        let dados = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&dados).unwrap();
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let cfg: crate::store::BotConfig = serde_json::from_value(
            serde_json::json!({"id": "bot-da-limpeza", "name": "b", "backend": "claude", "cwd": "/tmp"}),
        )
        .unwrap();
        store.save_bot(&cfg).unwrap();
        let identidade = crate::remote::identity::Identity::load_or_create(&dados).unwrap();
        let hub = Hub::new(store, "fixture".into(), identidade, "fixture".into(), 0);
        let reserva = crate::fork::open_chat::admit_for_test(&hub, "bot-da-limpeza", "conta-da-limpeza").unwrap();
        drop_idle_codex_helpers(&root);
        assert!(tmp.join("arg0-macos").exists(), "resposta em curso: nada é limpo");
        drop(reserva);
        limpar_codex_helpers(&root, |_, _| ());
        assert!(!tmp.join("arg0-macos").exists(), "sem resposta em curso, a limpeza segue");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&dados);
    }

    #[cfg(test)]
    #[test]
    fn limpeza_nao_encosta_em_fora_do_tmp_nem_em_nome_fora_do_padrao() {
        use std::os::unix::fs::{DirBuilderExt, symlink};
        let (root, tmp) = perfil_de_teste();
        let maiuscula = pasta_de_ajudante(&tmp, "arg0-MACOS");
        fs::write(tmp.join("arg0-arquivo"), b"").unwrap();
        symlink("/usr/bin/true", tmp.join("arg0-link")).unwrap();
        let no_codex = root.join("engine-setup/accounts/a1/h1/.codex/arg0-linux");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(no_codex.join("codex-sessao")).unwrap();
        escrever_privado(&no_codex.join("codex-sessao/.lock"), b"");
        let na_raiz = root.join("arg0-linux");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(na_raiz.join("codex-sessao")).unwrap();
        let boa = pasta_de_ajudante(&tmp, "arg0-macos");
        escrever_privado(&boa.join(".lock"), b"");
        limpar_codex_helpers(&root, |_, _| ());
        assert!(!tmp.join("arg0-macos").exists(), "a pasta no padrão sai");
        assert!(maiuscula.exists(), "nome fora do padrão não é tocado");
        assert!(tmp.join("arg0-arquivo").exists() && tmp.join("arg0-link").exists(), "fora do padrão no tmp fica");
        assert!(no_codex.join("codex-sessao/.lock").exists(), "fora do tmp do Codex não é tocado");
        assert!(na_raiz.join("codex-sessao").exists(), "o resto do perfil não é varrido");
        let _ = fs::remove_dir_all(&root);
    }

    struct SocketFile {
        path: PathBuf,
        dev: u64,
        ino: u64,
    }

    impl SocketFile {
        fn bind(path: PathBuf) -> Result<(UnixListener, Self)> {
            let listener = UnixListener::bind(&path).context("binding the private Mexerica Open socket")?;
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.file_type().is_socket() && metadata.uid() == private_fs::uid(),
                "unexpected socket identity"
            );
            let file = Self { path, dev: metadata.dev(), ino: metadata.ino() };
            fs::set_permissions(&file.path, fs::Permissions::from_mode(0o600))?;
            Ok((listener, file))
        }
    }

    impl Drop for SocketFile {
        fn drop(&mut self) {
            if fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino)
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    fn watch_stdin() -> Result<oneshot::Receiver<()>> {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat só escreve na estrutura válida; o descritor é stdin da invocação.
        let result = unsafe { libc::fstat(libc::STDIN_FILENO, stat.as_mut_ptr()) };
        ensure!(result == 0, "Mexerica Open requires piped stdin");
        // SAFETY: fstat retornou sucesso.
        let kind = unsafe { stat.assume_init() }.st_mode & libc::S_IFMT;
        ensure!(matches!(kind, libc::S_IFIFO | libc::S_IFSOCK), "Mexerica Open requires piped stdin");
        let (tx, rx) = oneshot::channel();
        std::thread::Builder::new().name("open-stdin".into()).spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            let _ = tx.send(());
        })?;
        Ok(rx)
    }

    pub(crate) async fn serve(port: u16, bind: Option<&str>, only_this_mac: bool) -> Result<()> {
        ensure!(
            only_this_mac && bind == Some(LOOPBACK) && port == Environment::Open.port(),
            "Mexerica Open requires its fixed loopback configuration"
        );
        let mut eof = watch_stdin()?;
        let profile = Profile::from_environment()?;
        // SAFETY: processo Open próprio, antes de criar qualquer arquivo ou filho.
        unsafe { libc::umask(0o077) };
        let mut stop = std::pin::pin!(async {
            tokio::select! {
                _ = &mut eof => (),
                () = crate::shutdown_signal() => (),
            }
        });
        let tcp = tokio::select! {
            biased;
            () = &mut stop => return Ok(()),
            result = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) => result.context("binding the dedicated Mexerica Open port")?,
        };
        let (unix, socket_file) = SocketFile::bind(profile.socket.clone())?;
        let _lock = profile.reserve_data()?;
        crate::configure_logging();
        let hub = tokio::select! {
            biased;
            () = &mut stop => return Ok(()),
            result = initialize(&profile, port) => result?,
        };
        let socket_router = crate::api::open_socket_router(hub.clone()).layer(Extension(ConnectInfo(SOCKET_PEER)));
        let result = tokio::select! {
            biased;
            () = &mut stop => Ok(()),
            result = axum::serve(unix, socket_router).into_future() => result,
            result = axum::serve(tcp, axum::Router::new()).into_future() => result,
        };
        crate::fork::engine_setup::shutdown_open(&hub).await;
        crate::fork::open_chat::shutdown(&hub).await;
        hub.shutdown().await;
        crate::agent::acp::stopped().await;
        drop(socket_file);
        result?;
        std::process::exit(0)
    }

    async fn initialize(profile: &Profile, port: u16) -> Result<Arc<Hub>> {
        tokio::task::spawn_blocking(crate::service::capture_binary_identity).await??;
        crate::agent::harness::install_docs().await;
        crate::chat::context::load_harness_context().await;
        let store = profile.open_store()?;
        let host_id = crate::store_host_id(&store)?;
        crate::chat::manage::recover(&store);
        let identity = crate::remote::identity::Identity::load_or_create(&profile.data)?;
        let token = crate::new_token();
        let hub = Hub::new(store, host_id, identity, token, port);
        hub.start()?;
        crate::write_token_file(&hub.token)?;
        Ok(hub)
    }
}
