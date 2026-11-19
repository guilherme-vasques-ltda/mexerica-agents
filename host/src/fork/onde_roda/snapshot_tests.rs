//! D70: fronteiras do leitor integral; executar somente nos gates autorizados.
use super::super::{Blob, Entrada, Manifesto, Pacote, digest_manifesto};
use super::*;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::Write as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _, symlink};
use std::process::{Command, Stdio};
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

struct Repo {
    root: std::path::PathBuf,
    formato: FormatoGit,
}
impl Repo {
    fn new(formato: FormatoGit) -> Self {
        let root = std::path::PathBuf::from(format!("/tmp/donna-snapshot-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("criar diretório exclusivo da fixture");
        let format = match formato {
            FormatoGit::Sha1 => "sha1",
            FormatoGit::Sha256 => "sha256",
        };
        let value = Self { root, formato };
        value.git(&["init", "--bare", &format!("--object-format={format}"), "."], b"");
        value
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new("/usr/bin/git");
        c.args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("HOME", &self.root)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_NO_LAZY_FETCH", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    }
    fn git(&self, args: &[&str], input: &[u8]) -> String {
        let mut child = self.command(args).spawn().expect("iniciar Git da fixture");
        let mut pipe = child.stdin.take().expect("stdin piped do Git da fixture");
        pipe.write_all(input).expect("enviar bytes ao Git da fixture");
        drop(pipe);
        let out = child.wait_with_output().expect("recolher Git e saída da fixture");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).expect("saída textual UTF-8 do Git da fixture").trim().to_owned()
    }
    fn object(&self, kind: &str, bytes: &[u8]) -> String {
        self.git(&["hash-object", "-w", "--stdin", "--literally", "-t", kind], bytes)
    }
    fn tree(&self, entries: &[(&str, &str, &str)]) -> String {
        let mut bytes = Vec::new();
        for (mode, name, oid) in entries {
            bytes.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            for pair in oid.as_bytes().as_chunks::<2>().0 {
                bytes.push(
                    u8::from_str_radix(std::str::from_utf8(pair).expect("par ASCII do OID da fixture"), 16)
                        .expect("par hexadecimal do OID da fixture"),
                );
            }
        }
        self.object("tree", &bytes)
    }
    fn commit(&self, tree: &str) -> String {
        self.object("commit", format!("tree {tree}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n").as_bytes())
    }
    fn package(&self, rows: &[(&str, u32, &[u8])]) -> Pacote {
        // Fixtures planas; tipos/árvores malformados são montados explicitamente.
        let objects: Vec<_> = rows.iter().map(|(_, _, bytes)| self.object("blob", bytes)).collect();
        let entries: Vec<_> = rows
            .iter()
            .zip(&objects)
            .map(|((path, mode, _), oid)| (if *mode == 0o100_755 { "100755" } else { "100644" }, *path, oid.as_str()))
            .collect();
        let tree = self.tree(&entries);
        let commit = self.commit(&tree);
        Self::candidate(&commit, &tree, rows)
    }
    fn candidate(commit: &str, tree: &str, rows: &[(&str, u32, &[u8])]) -> Pacote {
        let mut p = Pacote {
            manifesto: Manifesto {
                version: 1,
                repo: "repo-escolhido".into(),
                commit: commit.into(),
                tree: tree.into(),
                arquivos: Vec::new(),
                digest_total: [0; 32],
            },
            blobs: Vec::new(),
        };
        for (path, mode, bytes) in rows {
            p.manifesto.arquivos.push(Entrada {
                path: (*path).into(),
                mode: *mode,
                digest: Sha256::digest(bytes).into(),
                size: u64::try_from(bytes.len()).expect("tamanho da fixture cabe em u64"),
            });
            p.blobs.push(Blob { path: (*path).into(), bytes: bytes.to_vec() });
        }
        seal(&mut p);
        p
    }
    fn check<'a>(&self, candidate: &'a Pacote, expected: &Pacote) -> Result<PacoteConferidoGit<'a>> {
        let selection = SelecaoGit {
            repo: &expected.manifesto.repo,
            formato: self.formato,
            commit: &expected.manifesto.commit,
            tree: &expected.manifesto.tree,
        };
        conferir_git(candidate, &selection, &self.root.join("objects"), &AtomicBool::new(false))
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remover diretório da fixture");
    }
}
fn seal(p: &mut Pacote) {
    p.manifesto.digest_total = digest_manifesto(&p.manifesto).expect("selar manifesto válido da fixture");
}

#[test]
fn leitor_sha1_sha256_loose_pack_modificacao_local_e_bytes_crus() {
    let _serial = SERIAL.lock().unwrap();
    for formato in [FormatoGit::Sha1, FormatoGit::Sha256] {
        let r = Repo::new(formato);
        let p = r.package(&[
            (".gitattributes", 0o100_644, b"* filter=sentinela export-ignore export-subst text eol=crlf\n"),
            ("a", 0o100_755, b"$Format:%H$\n\0\xff"),
            ("vazio", 0o100_644, b""),
        ]);
        assert_eq!(p.manifesto.commit.len(), r.formato.largura());
        // Vetores independentes do framing Git para blob vazio.
        assert_eq!(
            r.object("blob", b""),
            match formato {
                FormatoGit::Sha1 => "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
                FormatoGit::Sha256 => "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813",
            }
        );
        fs::write(r.root.join("a"), b"dirty").unwrap();
        fs::write(r.root.join("untracked"), b"ignorar").unwrap();
        // Nem config hostil com include inexistente é lida pelo produtor.
        let mut config = fs::OpenOptions::new().append(true).open(r.root.join("config")).unwrap();
        config.write_all(b"[include]\npath=/nao-existe/config\n[filter \"sentinela\"]\nprocess=/nao-existe/filtro\n[core]\nhooksPath=/nao-existe/hooks\n").unwrap();
        let proof = r.check(&p, &p).expect("pacote loose da fixture deve conferir; preservar erro Git/layout");
        assert!(exigir_vinculo_git(Some(proof)).is_ok());
        // Ref exclusiva da fixture para empacotar; nenhuma ref chega ao leitor.
        r.git(&["update-ref", "refs/heads/fixture", &p.manifesto.commit], b"");
        r.git(&["-c", "repack.writeBitmaps=false", "-c", "pack.writeReverseIndex=false", "repack", "-a", "-d"], b"");
        r.git(&["prune-packed"], b"");
        r.check(&p, &p).expect("pacote da fixture deve conferir após repack; preservar erro Git/layout");
    }
}

#[test]
fn pacote_b_selado_nao_vira_a_nem_com_manifesto_relabelado_e_repo_divergente() {
    let _serial = SERIAL.lock().unwrap();
    for formato in [FormatoGit::Sha1, FormatoGit::Sha256] {
        let r = Repo::new(formato);
        let a = r.package(&[("a", 0o100_644, b"A")]);
        let mut b = r.package(&[("a", 0o100_644, b"B")]);
        assert!(r.check(&b, &a).is_err());
        b.manifesto.commit.clone_from(&a.manifesto.commit);
        b.manifesto.tree.clone_from(&a.manifesto.tree);
        seal(&mut b);
        assert!(super::super::conferir_manifesto(&b).is_ok());
        assert!(r.check(&b, &a).is_err());
        let mut other_repo = Repo::candidate(&a.manifesto.commit, &a.manifesto.tree, &[("a", 0o100_644, b"A")]);
        other_repo.manifesto.repo = "outro-repo".into();
        seal(&mut other_repo);
        assert!(super::super::conferir_manifesto(&other_repo).is_ok());
        assert_eq!(r.check(&other_repo, &a).err().unwrap().to_string(), "snapshot_git_repo");
    }
}

#[test]
fn grafo_inteiro_recusa_tipos_modos_entrada_extra_e_subtree_vazia() {
    let _serial = SERIAL.lock().unwrap();
    let r = Repo::new(FormatoGit::Sha1);
    let blob = r.object("blob", b"A");
    let empty = r.tree(&[]);
    for (mode, target) in
        [("120000", &blob), ("160000", &blob), ("40000", &blob), ("100644", &empty), ("40000", &empty)]
    {
        let tree = r.tree(&[(mode, "a", target)]);
        let commit = r.commit(&tree);
        let p = Repo::candidate(&commit, &tree, &[("a", 0o100_644, b"A")]);
        assert!(r.check(&p, &p).is_err(), "{mode}");
    }
    let tree = r.tree(&[("100644", "a", &blob), ("100644", "b", &blob)]);
    let commit = r.commit(&tree);
    let p = Repo::candidate(&commit, &tree, &[("a", 0o100_644, b"A")]);
    assert!(r.check(&p, &p).is_err());
    let p = Repo::candidate(&commit, &tree, &[("a", 0o100_644, b"A"), ("b", 0o100_644, b"A")]);
    assert!(r.check(&p, &p).is_ok()); // mesmo blob, bytes 1:1 por path
    let p = r.package(&[]);
    assert!(r.check(&p, &p).is_ok()); // raiz vazia é representável
}

#[test]
fn oid_falso_e_objeto_faltante_recusam_mesmo_pacote_selado() {
    let _serial = SERIAL.lock().unwrap();
    let r = Repo::new(FormatoGit::Sha1);
    let p = r.package(&[("a", 0o100_644, b"A")]);
    let original = r.object("blob", b"A");
    let fake = r.object("blob", b"B");
    let location = r.root.join("objects").join(&original[..2]).join(&original[2..]);
    let alternate = r.root.join("objects").join(&fake[..2]).join(&fake[2..]);
    fs::remove_file(&location).unwrap();
    fs::copy(&alternate, &location).unwrap();
    assert!(r.check(&p, &p).is_err());
    fs::remove_file(location).unwrap();
    assert!(r.check(&p, &p).is_err());
}

#[test]
fn copia_independente_recusa_links_fifo_alternates_promisor_e_layout_desconhecido() {
    let _serial = SERIAL.lock().unwrap();
    let r = Repo::new(FormatoGit::Sha256);
    let p = r.package(&[("a", 0o100_644, b"A")]);
    let oid = r.object("blob", b"A");
    let location = r.root.join("objects").join(&oid[..2]).join(&oid[2..]);
    let saved = fs::read(&location).unwrap();
    fs::remove_file(&location).unwrap();
    symlink(r.root.join("config"), &location).unwrap();
    assert!(r.check(&p, &p).is_err());
    fs::remove_file(&location).unwrap();
    let path = std::ffi::CString::new(location.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: caminho exclusivo da fixture; não abre ou lê o FIFO.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    assert!(r.check(&p, &p).is_err());
    fs::remove_file(&location).unwrap();
    fs::write(&location, saved).unwrap();
    for name in [
        "info/alternates",
        "info/http-alternates",
        "pack/multi-pack-index",
        "pack/pack-0000000000000000000000000000000000000000000000000000000000000000.promisor",
    ] {
        let path = r.root.join("objects").join(name);
        fs::write(&path, b"nao consumir").unwrap();
        assert!(r.check(&p, &p).is_err(), "{name}");
        fs::remove_file(path).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let budget = snapshot::Budget::new(&cancel);
    let mut copy = odb::Private::create().unwrap();
    let selection = SelecaoGit {
        repo: &p.manifesto.repo,
        formato: r.formato,
        commit: &p.manifesto.commit,
        tree: &p.manifesto.tree,
    };
    copy.copy(&r.root.join("objects"), &selection, &budget).unwrap();
    fs::remove_dir_all(r.root.join("objects")).unwrap();
    fs::create_dir_all(r.root.join("objects")).unwrap();
    let proc = git::Process::start(&copy, &["cat-file", "-t", &p.manifesto.commit], &budget).unwrap();
    assert_eq!(proc.finish(b"", 256).unwrap(), b"commit\n");
    copy.cleanup().unwrap();
    assert!(!copy.root.exists());
}

#[test]
fn cancelamento_limites_de_copia_e_sobra_encerram_sem_prova() {
    let _serial = SERIAL.lock().unwrap();
    let r = Repo::new(FormatoGit::Sha1);
    let p = r.package(&[("a", 0o100_644, b"A")]);
    let selection = SelecaoGit {
        repo: &p.manifesto.repo,
        formato: r.formato,
        commit: &p.manifesto.commit,
        tree: &p.manifesto.tree,
    };
    let cancelled = AtomicBool::new(true);
    assert!(conferir_git(&p, &selection, &r.root.join("objects"), &cancelled).is_err());
    let huge = (0..=255).map(|n| r.root.join("objects").join(format!("{n:02x}"))).find(|path| !path.exists()).unwrap();
    fs::create_dir_all(&huge).unwrap();
    let file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(huge.join("0".repeat(38))).unwrap();
    file.set_len(128 * 1024 * 1024 + 1).unwrap();
    assert!(r.check(&p, &p).is_err());
    drop(file);
    fs::remove_dir_all(huge).unwrap();
    let cancel = AtomicBool::new(false);
    let budget = snapshot::Budget::new(&cancel);
    let mut copy = odb::Private::create().unwrap();
    copy.copy(&r.root.join("objects"), &selection, &budget).unwrap();
    let proc = git::Process::start(&copy, &["cat-file", "-t", &p.manifesto.commit], &budget).unwrap();
    let pid_path = copy.root.clone();
    assert!(proc.finish(b"", 0).is_err());
    copy.cleanup().unwrap();
    assert!(!pid_path.exists());
}

#[test]
fn fd_herdado_nao_chega_ao_exec_da_guarda() {
    let _serial = SERIAL.lock().unwrap();
    let r = Repo::new(FormatoGit::Sha1);
    fs::write(r.root.join("dummy"), b"[fixture]\nvalue=1\n").unwrap();
    let file = fs::File::open(r.root.join("dummy")).unwrap();
    // SAFETY: novo FD alto controlado pela fixture; sem CLOEXEC para provar a guarda.
    let inherited = unsafe {
        let fd = libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 200);
        assert!(fd >= 200);
        OwnedFd::from_raw_fd(fd)
    };
    let fd_path = format!("/proc/self/fd/{}", inherited.as_raw_fd());
    assert!(r.command(&["config", "--file", &fd_path, "--list"]).output().unwrap().status.success());
    let mut command = r.command(&["config", "--file", &fd_path, "--list"]);
    linux::command(&mut command).expect("preparar guarda composta para provar fechamento de FD");
    assert!(!command.output().unwrap().status.success());
}

#[test]
fn pipes_excesso_timeout_cancelamento_e_stdin_travado_fazem_reap() {
    let _serial = SERIAL.lock().unwrap();
    for (script, output_limit, duration, cancel_now, input, code) in [
        ("while :; do printf x; done", 0, 2000, false, b"".as_slice(), "snapshot_git_extra_output"),
        ("while :; do printf x >&2; done", 0, 2000, false, b"".as_slice(), "snapshot_git_stderr_limit"),
        ("while :; do :; done", 0, 50, false, b"x".as_slice(), "snapshot_git_timeout"),
        ("while :; do :; done", 0, 2000, true, b"".as_slice(), "snapshot_git_cancelled"),
    ] {
        let cancel = AtomicBool::new(false);
        let mut budget = snapshot::Budget::new(&cancel);
        budget.deadline_for_test(std::time::Duration::from_millis(duration));
        let mut private = odb::Private::create().unwrap();
        let child = git::Process::probe(&private, script, &budget).unwrap();
        let pid = i32::try_from(child.pid()).unwrap();
        if cancel_now {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        // Encher stdin demonstra que o writer não bloqueia a supervisão/prazo.
        let blocked = vec![b'x'; 1024 * 1024];
        let input = if input.is_empty() { input } else { &blocked };
        let error = child.finish(input, output_limit).unwrap_err();
        assert_eq!(error.to_string(), code);
        // SAFETY: consultar apenas nosso PID já encerrado; ECHILD comprova reap anterior.
        unsafe {
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &raw mut status, libc::WNOHANG), -1);
            assert_eq!(*libc::__errno_location(), libc::ECHILD);
        }
        private.cleanup().unwrap();
        assert!(!private.root.exists());
    }
}

#[test]
fn alteracao_da_origem_durante_copia_aborta_por_truncamento_ou_crescimento() {
    fn truncate(fd: i32) {
        fs::set_permissions(format!("/proc/self/fd/{fd}"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::OpenOptions::new().write(true).open(format!("/proc/self/fd/{fd}")).unwrap().set_len(1).unwrap();
    }
    fn grow(fd: i32) {
        fs::set_permissions(format!("/proc/self/fd/{fd}"), fs::Permissions::from_mode(0o600)).unwrap();
        let file = fs::OpenOptions::new().write(true).open(format!("/proc/self/fd/{fd}")).unwrap();
        file.set_len(file.metadata().unwrap().len() + 1).unwrap();
    }
    let _serial = SERIAL.lock().unwrap();
    for observer in [truncate as fn(i32), grow as fn(i32)] {
        let r = Repo::new(FormatoGit::Sha1);
        let p = r.package(&[("a", 0o100_644, b"A")]);
        let selection = SelecaoGit {
            repo: &p.manifesto.repo,
            formato: r.formato,
            commit: &p.manifesto.commit,
            tree: &p.manifesto.tree,
        };
        let cancel = AtomicBool::new(false);
        let budget = snapshot::Budget::new(&cancel);
        budget.copy_observer.set(Some(observer));
        let mut private = odb::Private::create().unwrap();
        assert!(private.copy(&r.root.join("objects"), &selection, &budget).is_err());
        assert!(private.require_sealed().is_err());
        private.cleanup().unwrap();
    }
}

#[test]
fn cabecalho_eof_status_e_contadores_sao_terminais() {
    let _serial = SERIAL.lock().unwrap();
    let cancel = AtomicBool::new(false);
    let budget = snapshot::Budget::new(&cancel);
    budget.stdout(80 * 1024 * 1024).unwrap();
    assert!(budget.stdout(1).is_err());
    budget.stderr(64 * 1024).unwrap();
    assert!(budget.stderr(1).is_err());
    for _ in 0..4 {
        budget.launch().unwrap();
    }
    assert!(budget.launch().is_err());
    let mut private = odb::Private::create().unwrap();
    let budget = snapshot::Budget::new(&cancel);
    let mut process = git::Process::probe(&private, "printf '%0257d' 0", &budget).unwrap();
    assert_eq!(process.line().unwrap_err().to_string(), "snapshot_git_header_limit");
    drop(process);
    let mut process = git::Process::probe(&private, "printf x", &budget).unwrap();
    assert!(process.exact(&mut [0; 2]).is_err());
    drop(process);
    let process = git::Process::probe(&private, "exit 7", &budget).unwrap();
    assert_eq!(process.finish(b"", 0).unwrap_err().to_string(), "snapshot_git_exit");
    private.cleanup().unwrap();
}

#[test]
fn formato_confiavel_recusa_oids_e_odb_validos_do_outro_formato_sem_prova() {
    let _serial = SERIAL.lock().unwrap();
    for (origem, esperado) in [(FormatoGit::Sha1, FormatoGit::Sha256), (FormatoGit::Sha256, FormatoGit::Sha1)] {
        let r = Repo::new(origem);
        let p = r.package(&[("a", 0o100_644, b"A")]);
        let selection = SelecaoGit {
            repo: &p.manifesto.repo,
            formato: esperado,
            commit: &p.manifesto.commit,
            tree: &p.manifesto.tree,
        };
        let result = conferir_git(&p, &selection, &r.root.join("objects"), &AtomicBool::new(false));
        assert_eq!(result.err().expect("formato incompatível não emite prova").to_string(), "snapshot_git_format");
    }
}
