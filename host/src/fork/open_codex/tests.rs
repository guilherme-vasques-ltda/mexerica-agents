#![allow(clippy::unwrap_used)]

use super::{
    AUTH_CONFIG, CA_BUNDLE, CHAT_OVERRIDES, LOGIN_NETWORK, POLICY, SANDBOX, SELECTION, Selection, VERSION,
    admin_policy_safe, codex_home_clean, command, hex, prepare_profile, root_owned,
};
use crate::fork::private_fs::{self, Directory};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

struct Fixture {
    scope: PathBuf,
    open: PathBuf,
    executable: PathBuf,
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn comando_mediado_recusa_troca_de_executavel_ou_pasta() -> anyhow::Result<()> {
    crate::fork::rules::terminal::executor::tests::macos_executa_o_pedido_e_recusa_troca_de_executavel_ou_pasta().await
}

impl Fixture {
    fn new() -> Self {
        let scope = std::env::temp_dir().canonicalize().unwrap().join(format!("open-codex-{}", uuid::Uuid::new_v4()));
        Directory::open(&scope, true).unwrap();
        let home = &scope;
        let open = home.join(".mexerica-open");
        Directory::open(&open, true).unwrap();
        let installed = home.join(".local/bin");
        Directory::open(&installed, true).unwrap();
        let executable = installed.join("codex");
        std::fs::write(&executable, b"synthetic executable").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self { scope, open, executable }
    }

    fn select(&self, executable: &Path, version: &str, digest: &str) {
        let reference = serde_json::json!({"executable": executable, "version": version, "sha256": digest});
        private_fs::create_atomic(&self.open.join(SELECTION), reference.to_string().as_bytes()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scope);
    }
}

fn digest() -> String {
    hex(&Sha256::digest(b"synthetic executable"))
}

#[test]
fn selection_requires_a_private_explicit_path_and_pinned_version_and_content() {
    let missing = Fixture::new();
    assert!(Selection::load(&missing.open).is_err());

    let fixture = Fixture::new();
    fixture.select(&fixture.executable, VERSION, &digest());
    assert_eq!(Selection::load(&fixture.open).unwrap().executable, fixture.executable);
    std::fs::write(&fixture.executable, b"replacement").unwrap();
    assert!(Selection::load(&fixture.open).is_err());

    let wrong_version = Fixture::new();
    wrong_version.select(&wrong_version.executable, "0.1.0", &digest());
    assert!(Selection::load(&wrong_version.open).is_err());

    let permissive = Fixture::new();
    permissive.select(&permissive.executable, VERSION, &digest());
    std::fs::set_permissions(permissive.open.join(SELECTION), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Selection::load(&permissive.open).is_err());
}

#[test]
fn selection_never_follows_a_link_or_accepts_a_personal_profile_as_an_installation() {
    let fixture = Fixture::new();
    let personal = fixture.open.parent().unwrap().join(".mexerica");
    Directory::open(&personal, true).unwrap();
    let other = personal.join("codex");
    std::fs::rename(&fixture.executable, &other).unwrap();
    symlink(&other, &fixture.executable).unwrap();
    fixture.select(&fixture.executable, VERSION, &digest());
    assert!(Selection::load(&fixture.open).is_err());

    let explicit_personal = Fixture::new();
    explicit_personal.select(&other, VERSION, &digest());
    assert!(Selection::load(&explicit_personal.open).is_err());
}

#[test]
fn login_pins_file_storage_and_refuses_to_replace_a_conflicting_config() {
    let fixture = Fixture::new();
    let directory = fixture.open.join("home/.mexerica-open/engine-setup/accounts/fixture/.codex");
    Directory::open(&directory, true).unwrap();
    assert!(prepare_profile(&fixture.open, &directory).is_ok());
    assert_eq!(private_fs::read(&directory.join("config.toml")).unwrap().unwrap(), AUTH_CONFIG);
    assert!(prepare_profile(&fixture.open, &directory).is_ok());
    private_fs::write(&directory.join("config.toml"), b"cli_auth_credentials_store = \"keyring\"\n").unwrap();
    assert!(prepare_profile(&fixture.open, &directory).is_err());
    assert!(prepare_profile(&fixture.open, fixture.open.parent().unwrap()).is_err());
}

#[test]
fn launch_passes_only_the_selected_binary_and_its_private_environment() {
    let fixture = Fixture::new();
    let profile = fixture.open.join("home/.mexerica-open/engine-setup/accounts/fixture/.codex");
    Directory::open(&profile, true).unwrap();
    let command = command(&fixture.executable, &profile, true, None).unwrap();
    let std = command.as_std();
    assert_eq!(std.get_program(), SANDBOX);
    let vars: Vec<_> = std.get_envs().collect();
    assert_eq!(vars.len(), 6);
    assert!(vars.iter().any(|(key, val)| *key == "SSL_CERT_FILE" && *val == Some(std::ffi::OsStr::new(CA_BUNDLE))));
    assert!(vars.iter().any(|(key, val)| *key == "CODEX_HOME" && *val == Some(profile.as_os_str())));
    let args: Vec<_> = std.get_args().collect();
    assert_eq!(args.last().copied(), Some(fixture.executable.as_os_str()));
    assert!(POLICY.contains("(deny default)"));
    assert!(POLICY.contains("(allow file-read* file-map-executable\n  (literal (param \"CODEX\"))"));
    assert!(POLICY.contains("(subpath \"/System/Volumes/Preboot/Cryptexes/OS\")"));
    assert!(!POLICY.contains("(subpath \"/System/Volumes/Data\")"));
    assert!(
        POLICY.contains("(subpath \"/private/etc/codex\")"),
        "o Codex lê a política do administrador em /etc/codex"
    );
    assert!(!POLICY.contains("(subpath \"/private/etc\")"));
    assert!(!LOGIN_NETWORK.contains("securityd") && !LOGIN_NETWORK.contains("SecurityServer"));
    assert!(LOGIN_NETWORK.contains(&format!("(allow file-read-data file-read-metadata (literal \"{CA_BUNDLE}\"))")));
    assert!(!LOGIN_NETWORK.contains("file-read*"), "os certificados são lidos sem atributos estendidos");
    assert!(
        POLICY
            .trim_end()
            .ends_with(r#"(deny file-read* file-write* (regex #"/\.[Cc][Oo][Dd][Ee][Xx]/\.[Ee][Nn][Vv]$"))"#),
        "o .env do perfil, que trocaria a confiança do TLS, fica fora do alcance do Codex e a negação vem por último"
    );
    assert!(
        POLICY.contains("(literal \"/etc\")")
            && LOGIN_NETWORK.contains("(allow file-read-metadata (literal \"/var\"))")
    );
    assert!(
        !POLICY.contains("(subpath \"/etc\")")
            && !POLICY.contains("(subpath \"/var\")")
            && !POLICY.contains("(subpath \"/private/var\")")
    );
}

#[test]
fn chat_uses_cached_web_search_without_enabling_local_tools() {
    for tool in ["shell_tool", "unified_exec", "view_image", "multi_agent", "apps", "plugins", "image_generation"] {
        assert!(CHAT_OVERRIDES.contains(&format!("features.{tool}=false").as_str()), "{tool}");
    }
    assert!(CHAT_OVERRIDES.contains(&"web_search=\"cached\""));
    for setting in [
        "sandbox_mode=\"read-only\"",
        "approval_policy=\"never\"",
        "features.hooks=false",
        "features.sleep_tool=false",
        "features.standalone_web_search=false",
        "tools.experimental_request_user_input.enabled=false",
        "mcp_oauth_credentials_store=\"file\"",
    ] {
        assert!(CHAT_OVERRIDES.contains(&setting), "{setting}");
    }
}

#[test]
fn admin_policy_must_belong_to_root() {
    let fixture = Fixture::new();
    assert!(admin_policy_safe(&fixture.scope.join("absent")));
    let owned_by_user = fixture.scope.join("codex");
    Directory::open(&owned_by_user, true).unwrap();
    if private_fs::uid() != 0 {
        assert!(!admin_policy_safe(&owned_by_user), "a policy folder that a non-root user controls is refused");
    }
    assert!(!admin_policy_safe(&fixture.executable), "a file in place of the folder is refused");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_login_sandbox_denies_personal_and_dev_files_even_through_a_link() {
    let fixture = Fixture::new();
    let profile = fixture.open.join("home/.mexerica-open/engine-setup/accounts/fixture/.codex");
    Directory::open(&profile, true).unwrap();
    let open_file = profile.join("sample");
    std::fs::write(&open_file, b"open").unwrap();
    let personal = fixture.scope.join(".mexerica");
    let dev = fixture.scope.join(".mexerica-dev");
    Directory::open(&personal, true).unwrap();
    Directory::open(&dev, true).unwrap();
    let files = [personal.join("sample"), dev.join("sample")];
    for path in &files {
        std::fs::write(path, b"fixture").unwrap();
    }
    let redirect = profile.join("redirect");
    symlink(&files[0], &redirect).unwrap();
    let env_file = profile.join(".env");
    std::fs::write(&env_file, b"SSL_CERT_FILE=/dev/null\n").unwrap();
    let temporary = PathBuf::from(format!("/private/tmp/mexerica-open-probe-{}", uuid::Uuid::new_v4()));
    let source = fixture.scope.join("probe.c");
    std::fs::write(
        &source,
        r#"#include <arpa/inet.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>
static int attempt(const char *op) {
  const char *path = op + 2;
  if (op[0] == 'p' || op[0] == 'n') {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -2;
    struct sockaddr_in addr = {0};
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons((unsigned short)strtoul(path, NULL, 10));
    int result = connect(fd, (struct sockaddr *)&addr, sizeof(addr));
    int saved = errno;
    close(fd);
    errno = saved;
    return result;
  }
  if (op[0] == 'd') {
    DIR *dir = opendir(path);
    if (!dir) return -1;
    int listed = readdir(dir) != NULL;
    closedir(dir);
    return listed ? 0 : -2;
  }
  int fd = op[0] == 'w' ? open(path, O_WRONLY | O_CREAT, 0600) : open(path, O_RDONLY);
  if (fd < 0) return -1;
  char byte = 0;
  int ok = op[0] == 'w' || read(fd, &byte, 1) == 1;
  close(fd);
  return ok ? 0 : -2;
}
int main(int argc, char **argv) {
  if (argc == 1) return 0;
  const char marker[] = "auxiliar nativo chegou a main\n";
  (void)write(STDOUT_FILENO, marker, sizeof(marker) - 1);
  if (argc < 3) return 1;
  int fd = open(argv[2], O_RDONLY);
  char byte = 0;
  if (fd < 0 || read(fd, &byte, 1) != 1 || byte != 'o') return 2;
  close(fd);
  int sandboxed = argv[1][0] == 's';
  for (int i = 3; i < argc; ++i) {
    int result = attempt(argv[i]);
    if (argv[i][0] == 'a' || argv[i][0] == 'p' || !sandboxed) {
      if (result != 0) return 30 + i;
      continue;
    }
    if (result == 0) return 10 + i;
    if (result == -2 || (errno != EPERM && errno != EACCES)) return 20 + i;
  }
  return 0;
}"#,
    )
    .unwrap();
    assert!(
        std::process::Command::new("/usr/bin/cc")
            .arg(source)
            .arg("-o")
            .arg(&fixture.executable)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new(&fixture.executable).status().unwrap().success(),
        "o auxiliar fictício não chegou a main no controle sem sandbox"
    );
    let mut denied: Vec<String> =
        files.iter().chain([&redirect, &env_file]).map(|path| format!("r:{}", path.display())).collect();
    denied.extend(
        ["r:/etc/hosts", "r:/private/etc/passwd", "d:/etc", "d:/private/etc", "d:/private/var"].map(String::from),
    );
    if Path::new("/private/var/run/resolv.conf").exists() {
        denied.push("r:/private/var/run/resolv.conf".into());
    }
    let other_case = profile.join(".ENV");
    if other_case.exists() {
        denied.push(format!("r:{}", other_case.display()));
    }
    denied.push(format!("w:{}", profile.join(".Env").display()));
    denied.push(format!("d:{}", fixture.scope.display()));
    for path in [&env_file, &fixture.scope.join("outside"), &temporary] {
        denied.push(format!("w:{}", path.display()));
    }
    for login in [true, false] {
        let mut sandbox = command(&fixture.executable, &profile, login, None).unwrap();
        sandbox
            .arg("s")
            .arg(&open_file)
            .args(&denied)
            .arg(format!("{}:{CA_BUNDLE}", if login { "a" } else { "r" }))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), sandbox.status()).await.unwrap().unwrap();
        assert!(status.success(), "sandbox ou prova nativa recusada (rede: {login}): {status}");
    }
    let allowed = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let denied_port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = allowed.local_addr().unwrap().port();
    let probe = [format!("p:{port}"), format!("n:{}", denied_port.local_addr().unwrap().port())];
    let control =
        std::process::Command::new(&fixture.executable).arg("c").arg(&open_file).args(&probe).status().unwrap();
    assert!(control.success(), "sem sandbox, as duas portas escutam: {control}");
    let mut sandbox = command(&fixture.executable, &profile, false, Some(port)).unwrap();
    sandbox.arg("s").arg(&open_file).args(&probe);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), sandbox.output()).await.unwrap().unwrap();
    assert!(output.status.success(), "só a porta local selecionada pode conectar: {}", output.status);
    assert_eq!(output.stdout, b"auxiliar nativo chegou a main\n");
    let control = std::process::Command::new(&fixture.executable)
        .arg("c")
        .arg(&open_file)
        .args(&denied)
        .arg(format!("a:{CA_BUNDLE}"))
        .status()
        .unwrap();
    let _ = std::fs::remove_file(&temporary);
    assert!(control.success(), "sem o isolamento, cada operação negada acima funciona: {control}");
}

#[test]
fn certificates_count_only_when_the_file_and_every_folder_above_belong_to_root() {
    let fixture = Fixture::new();
    let file = fixture.scope.join("cert.pem");
    std::fs::write(&file, b"fixture").unwrap();
    if private_fs::uid() != 0 {
        assert!(!root_owned(&file), "a file a regular user controls is refused");
    }
    assert!(!root_owned(&fixture.scope.join("absent.pem")));
    assert!(!root_owned(Path::new("/etc")), "a folder in place of the file is refused");
    let passwd = if cfg!(target_os = "macos") { "/private/etc/passwd" } else { "/etc/passwd" };
    assert!(root_owned(Path::new(passwd)), "a root file under real root folders counts (/etc is a link on macOS)");
}

#[test]
fn a_codex_folder_with_any_env_link_or_extra_name_is_refused_before_each_launch() {
    let fixture = Fixture::new();
    let profile = fixture.open.join("home/.mexerica-open/engine-setup/accounts/fixture/.codex");
    Directory::open(&profile, true).unwrap();
    std::fs::write(profile.join("config.toml"), AUTH_CONFIG).unwrap();
    Directory::open(&profile.join("tmp"), true).unwrap();
    assert!(codex_home_clean(&profile));
    for name in [".env", ".ENV", ".Env"] {
        std::fs::write(profile.join(name), b"SSL_CERT_FILE=/dev/null\n").unwrap();
        assert!(!codex_home_clean(&profile), "{name}");
        std::fs::remove_file(profile.join(name)).unwrap();
    }
    Directory::open(&profile.join(".env"), true).unwrap();
    assert!(!codex_home_clean(&profile), "a folder named .env");
    std::fs::remove_dir(profile.join(".env")).unwrap();
    symlink(profile.join("config.toml"), profile.join("alias")).unwrap();
    assert!(!codex_home_clean(&profile), "a link");
    std::fs::remove_file(profile.join("alias")).unwrap();
    std::fs::hard_link(profile.join("config.toml"), profile.join("second-name")).unwrap();
    assert!(!codex_home_clean(&profile), "a file with two names");
    std::fs::remove_file(profile.join("second-name")).unwrap();
    assert!(codex_home_clean(&profile));
    assert!(!codex_home_clean(&profile.join("absent")));
}
