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

fn preferences(output: &[u8]) -> Result<(), &'static str> {
    match output {
        b"clear\n" => Ok(()),
        b"managed\n" => Err(MANAGED),
        _ => Err(UNVERIFIED),
    }
}

fn verify_results(
    device: Result<Vec<u8>, &'static str>,
    browser: Result<Vec<u8>, &'static str>,
) -> Result<(), &'static str> {
    enrollment(&device?)?;
    preferences(&browser?)
}

#[cfg(target_os = "macos")]
const QUERY: &str = r#"
ObjC.import('Foundation'); ObjC.import('CoreFoundation');
const protectedKeys = [
  'ProxySettings', 'ProxyMode', 'ProxyServer', 'ProxyPacUrl', 'ProxyBypassList',
  'ExtensionInstallForcelist', 'ExtensionSettings', 'ExtensionInstallSources',
  'DnsOverHttpsMode', 'DnsOverHttpsTemplates', 'BuiltInDnsClientEnabled',
  'QuicAllowed', 'WebRtcIPHandling', 'CloudManagementEnrollmentToken',
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
['com.google.Chrome', 'org.chromium.Chromium'].some(hasPolicy) ? 'managed' : 'clear';
"#;

#[cfg(target_os = "macos")]
async fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, &'static str> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).env_clear().env("LC_ALL", "C").kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(5), command.output())
        .await
        .map_err(|_| UNVERIFIED)?
        .map_err(|_| UNVERIFIED)?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(UNVERIFIED);
    }
    Ok(output.stdout)
}

#[cfg(target_os = "macos")]
pub(in crate::fork::browser) async fn verify() -> Result<(), &'static str> {
    match std::fs::symlink_metadata("/Library/Google/Chrome/CloudManagementEnrollmentToken") {
        Ok(_) => return Err(MANAGED),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(UNVERIFIED),
    }
    let (device, browser) = tokio::join!(
        run("/usr/bin/profiles", &["status", "-type", "enrollment"]),
        run("/usr/bin/osascript", &["-l", "JavaScript", "-e", QUERY]),
    );
    verify_results(device, browser)
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
            verify_results(Ok(b"Enrolled via DEP: No\nMDM enrollment: No\n".to_vec()), Ok(b"clear\n".to_vec())),
            Ok(())
        );
    }
}
