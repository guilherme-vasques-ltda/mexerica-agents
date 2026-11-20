#![allow(clippy::unwrap_used)]

use super::{
    AUTH_CONFIG, LOGIN_NETWORK, POLICY, SANDBOX, SELECTION, Selection, VERSION, command, hex, prepare_profile,
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

    fn select(&self, executable: &Path, version: &str, digest: String) {
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
    fixture.select(&fixture.executable, VERSION, digest());
    assert_eq!(Selection::load(&fixture.open).unwrap().executable, fixture.executable);
    std::fs::write(&fixture.executable, b"replacement").unwrap();
    assert!(Selection::load(&fixture.open).is_err());

    let wrong_version = Fixture::new();
    wrong_version.select(&wrong_version.executable, "0.1.0", digest());
    assert!(Selection::load(&wrong_version.open).is_err());

    let permissive = Fixture::new();
    permissive.select(&permissive.executable, VERSION, digest());
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
    fixture.select(&fixture.executable, VERSION, digest());
    assert!(Selection::load(&fixture.open).is_err());

    let explicit_personal = Fixture::new();
    explicit_personal.select(&other, VERSION, digest());
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
    let command = command(&fixture.executable, &profile, true).unwrap();
    let std = command.as_std();
    assert_eq!(std.get_program(), SANDBOX);
    let vars: Vec<_> = std.get_envs().collect();
    assert_eq!(vars.len(), 5);
    assert!(vars.iter().any(|(key, val)| *key == "CODEX_HOME" && *val == Some(profile.as_os_str())));
    let args: Vec<_> = std.get_args().collect();
    assert_eq!(args.last().copied(), Some(fixture.executable.as_os_str()));
    assert!(POLICY.contains("(deny default)"));
    assert!(!LOGIN_NETWORK.contains("securityd"));
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
    let source = fixture.scope.join("probe.c");
    std::fs::write(
        &source,
        r#"#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
int main(int argc, char **argv) {
  if (argc != 5) return 1;
  int fd = open(argv[1], O_RDONLY);
  char byte = 0;
  if (fd < 0 || read(fd, &byte, 1) != 1 || byte != 'o') return 2;
  close(fd);
  for (int i = 2; i < argc; ++i) {
    fd = open(argv[i], O_RDONLY);
    if (fd >= 0 || (errno != EPERM && errno != EACCES)) return 3;
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
    let mut sandbox = command(&fixture.executable, &profile, true).unwrap();
    sandbox.arg(open_file).args(files).arg(redirect);
    let status = tokio::time::timeout(std::time::Duration::from_secs(10), sandbox.status()).await.unwrap().unwrap();
    assert!(status.success(), "sandbox ou prova nativa recusada: {status}");
}
