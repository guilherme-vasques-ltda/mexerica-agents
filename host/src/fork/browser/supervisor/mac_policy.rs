use super::Watch;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

const MANAGED: &str = "browser_managed_policy";
const UNVERIFIED: &str = "browser_policy_unverified";

fn enrollment(output: &[u8]) -> Result<(), &'static str> {
    let output = std::str::from_utf8(output).map_err(|_| UNVERIFIED)?;
    let mut lines = output.lines();
    match (lines.next(), lines.next(), lines.next()) {
        (Some("Enrolled via DEP: No"), Some("MDM enrollment: No"), None) => Ok(()),
        (Some("Enrolled via DEP: Yes"), _, _) | (_, Some("MDM enrollment: Yes"), _) => Err(MANAGED),
        (_, Some(line), _) if line.starts_with("MDM enrollment: Yes (") => Err(MANAGED),
        _ => Err(UNVERIFIED),
    }
}

fn preferences(output: &[u8]) -> Result<PathBuf, &'static str> {
    let output = std::str::from_utf8(output).map_err(|_| UNVERIFIED)?;
    if output == "managed\n" {
        return Err(MANAGED);
    }
    let path = output.strip_prefix("clear\n").and_then(|s| s.strip_suffix('\n')).ok_or(UNVERIFIED)?;
    let path = Path::new(path);
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().iter().any(u8::is_ascii_control) {
        return Err(UNVERIFIED);
    }
    Ok(path.to_owned())
}

fn verify_results(
    device: Result<Vec<u8>, &'static str>,
    browser: Result<Vec<u8>, &'static str>,
) -> Result<PathBuf, &'static str> {
    enrollment(&device?)?;
    preferences(&browser?)
}

fn persisted_management(enrollment: &Path, stores: &[&Path]) -> Result<(), &'static str> {
    match fs::symlink_metadata(enrollment) {
        Ok(_) => return Err(MANAGED),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(UNVERIFIED),
    }
    for store in stores {
        match fs::symlink_metadata(store) {
            Ok(meta) if meta.is_dir() => {
                if fs::read_dir(store).map_err(|_| UNVERIFIED)?.next().transpose().map_err(|_| UNVERIFIED)?.is_some() {
                    return Err(MANAGED);
                }
            }
            Ok(_) => return Err(MANAGED),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(UNVERIFIED),
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const QUERY: &str = r#"
ObjC.import('Foundation'); ObjC.import('CoreFoundation');
const protectedKeys = [
  'ProxySettings', 'ProxyMode', 'ProxyServer', 'ProxyPacUrl', 'ProxyBypassList',
  'ExtensionInstallForcelist', 'ExtensionSettings', 'ExtensionInstallSources',
  'DnsOverHttpsMode', 'DnsOverHttpsTemplates', 'BuiltInDnsClientEnabled',
  'QuicAllowed', 'WebRtcIPHandling', 'CloudManagementEnrollmentToken', 'CloudManagementEnrollmentMandatory',
  'RestoreOnStartup', 'RestoreOnStartupURLs', 'HomepageLocation',
  'URLAllowlist', 'URLBlocklist'
];
function hasPolicy(name) {
  const domain = ObjC.wrap(name);
  if (!$.CFPreferencesAppSynchronize(domain)) throw Error('unavailable');
  const defaults = $.NSUserDefaults.alloc.initWithSuiteName(name);
  if (!defaults) throw Error('unavailable');
  const values = defaults.dictionaryRepresentation;
  if (!values) throw Error('unavailable');
  const keys = values.allKeys;
  const count = Number(keys.count);
  if (!Number.isSafeInteger(count) || count < 0) throw Error('unavailable');
  for (let i = 0; i < count; i++) {
    if (defaults.objectIsForcedForKeyInDomain(keys.objectAtIndex(i), name)) return true;
  }
  return protectedKeys.some(key => {
    const k = ObjC.wrap(key);
    if ($.CFPreferencesAppValueIsForced(k, domain)) return true;
    return !defaults.objectForKey(key).isNil();
  });
}
function inspect() {
  if (['com.google.Chrome', 'org.chromium.Chromium'].some(hasPolicy)) return 'managed';
  const applicationSupport = 14;
  const userDomain = 1;
  const dirs = $.NSSearchPathForDirectoriesInDomains(applicationSupport, userDomain, true);
  if (Number(dirs.count) !== 1) throw Error('unavailable');
  const userData = ObjC.unwrap(dirs.objectAtIndex(0));
  if (typeof userData !== 'string' || !userData.startsWith('/')) throw Error('unavailable');
  return 'clear\n' + userData;
}
inspect();
"#;

#[cfg(target_os = "macos")]
async fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, &'static str> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).env_clear().env("LC_ALL", "C").kill_on_drop(true);
    let output = command.output().await.map_err(|_| UNVERIFIED)?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(UNVERIFIED);
    }
    Ok(output.stdout)
}

#[cfg(target_os = "macos")]
pub(in crate::fork::browser) async fn verify() -> Result<(), &'static str> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let (device, browser) = tokio::join!(
            run("/usr/bin/profiles", &["status", "-type", "enrollment"]),
            run("/usr/bin/osascript", &["-l", "JavaScript", "-e", QUERY]),
        );
        let user_data = verify_results(device, browser)?;
        tokio::task::spawn_blocking(move || {
            persisted_management(
                Path::new("/Library/Google/Chrome/CloudManagementEnrollmentToken"),
                &[
                    &user_data.join("Google/Chrome Cloud Enrollment"),
                    Path::new("/Library/Application Support/Google/CloudManagement"),
                ],
            )
        })
        .await
        .map_err(|_| UNVERIFIED)?
    })
    .await
    .map_err(|_| UNVERIFIED)?
}

pub(in crate::fork::browser) async fn enforce(
    watch: &Watch,
    probe: impl Future<Output = Result<(), &'static str>>,
) -> Result<(), &'static str> {
    if let Err(reason) = probe.await {
        watch.revoke(|_, _| true);
        return Err(reason);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recusa_gestao_do_mac_ou_do_navegador() {
        assert_eq!(enrollment(b"Enrolled via DEP: No\nMDM enrollment: Yes (User Approved)\n"), Err(MANAGED));
        assert_eq!(preferences(b"managed\n"), Err(MANAGED));
    }

    #[test]
    fn recusa_consulta_ilegivel_ou_incompleta() {
        assert_eq!(enrollment(b"Enrolled via DEP: No\n"), Err(UNVERIFIED));
        assert_eq!(preferences(b""), Err(UNVERIFIED));
        assert_eq!(
            verify_results(Ok(b"Enrolled via DEP: No\nMDM enrollment: No\n".to_vec()), Err(UNVERIFIED)),
            Err(UNVERIFIED)
        );
    }

    #[test]
    fn aceita_apenas_respostas_explicitas_sem_politica() {
        assert_eq!(
            verify_results(
                Ok(b"Enrolled via DEP: No\nMDM enrollment: No\n".to_vec()),
                Ok(b"clear\n/application-support\n".to_vec())
            ),
            Ok(PathBuf::from("/application-support"))
        );
        assert_eq!(preferences(b"clear\n"), Err(UNVERIFIED));
        assert_eq!(preferences(b"clear\nrelative\n"), Err(UNVERIFIED));
    }

    #[test]
    fn recusa_credencial_persistida_mesmo_sem_token_de_inscricao() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let enrollment = root.path().join("enrollment");
        let user_store = root.path().join("user");
        let system_store = root.path().join("system");
        let stores = [user_store.as_path(), system_store.as_path()];
        fs::create_dir(&user_store)?;
        fs::create_dir(&system_store)?;
        assert_eq!(persisted_management(&enrollment, &stores), Ok(()));
        let device = user_store.join("device");
        fs::File::create(&device)?;
        assert_eq!(persisted_management(&enrollment, &stores), Err(MANAGED));
        fs::remove_file(device)?;
        fs::File::create(system_store.join("device"))?;
        assert_eq!(persisted_management(&enrollment, &stores), Err(MANAGED));
        Ok(())
    }

    #[tokio::test]
    async fn corta_a_sessao_se_a_politica_mudar_ou_ficar_inconclusiva() {
        use super::super::{Cut, Scope};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let watch = Watch::default();
        let scope = Scope { hub: "hub".into(), space: "space".into(), bot: "bot".into() };
        for reason in [MANAGED, UNVERIFIED] {
            let proxy = tokio::spawn(std::future::pending::<()>());
            let stopped = Arc::new(AtomicBool::new(false));
            let mark = Arc::clone(&stopped);
            let cut = Arc::new(Cut::new(proxy.abort_handle(), Arc::new(move || mark.store(true, Ordering::Release))));
            watch.insert(scope.clone(), "turn".into(), 1, cut);
            assert_eq!(enforce(&watch, async { Ok(()) }).await, Ok(()));
            assert!(watch.visible(&scope).is_some());
            assert!(!stopped.load(Ordering::Acquire));
            assert_eq!(enforce(&watch, async { Err(reason) }).await, Err(reason));
            assert!(watch.visible(&scope).is_none());
            assert!(stopped.load(Ordering::Acquire));
            assert!(proxy.await.is_err_and(|error| error.is_cancelled()));
        }
    }
}
