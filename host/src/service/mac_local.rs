//! Hub local do Mac (modo "só neste Mac", plano `docs/plans/mac-local-hub.md`, fatia P).
//!
//! Tudo aqui é puro ou toca só caminhos recebidos, para o CI Linux provar: o plist do launchd
//! sempre fecha o Hub em loopback, o Hub local não liga o relay de saída e a desinstalação dos
//! níveis 1 e 2 nunca toca o binário, a pasta de dados nem o Keychain.
#![cfg_attr(windows, allow(dead_code, reason = "o serviço do Mac só existe em Unix"))]

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// Espera entre reinícios do `KeepAlive` (um plist herdado que recusa subir não gira a cada 10 s).
/// Espelhado em `THROTTLE_SECONDS` de `apps/desktop/src/shared/fork-mac-local-hub.ts`. [A medir]
pub const THROTTLE_SECONDS: u32 = 30;

/// Único endereço que o serviço do Mac aceita.
pub const LOOPBACK: &str = "127.0.0.1";

/// Quem pode falar com este Hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Hub remoto ou instalado à mão: relay de saída ligado; escuta na rede só com `--bind` explícito.
    Network,
    /// "Só neste Mac": nada escuta fora de `127.0.0.1` e nada conecta ao relay.
    ThisMacOnly,
}

impl Scope {
    pub fn from_flag(only_this_mac: bool) -> Self {
        if only_this_mac { Self::ThisMacOnly } else { Self::Network }
    }

    /// Falha fechada: o modo "só neste Mac" recusa qualquer endereço que não seja loopback.
    pub fn check_bind(self, bind: &str) -> Result<()> {
        if self == Self::ThisMacOnly && bind != LOOPBACK {
            bail!("o modo \"só neste Mac\" só aceita --bind {LOOPBACK}, recebeu {bind}");
        }
        Ok(())
    }

    /// O Hub local não inicia o relay de saída; login, motores e Hubs remotos seguem iguais.
    pub fn runs_outbound_relay(self) -> bool {
        self == Self::Network
    }
}

/// Resolve `serve`: endereço e escopo finais. Falha fechada: rodando como serviço do Mac
/// (launchd com o label do Mexerica) sem `--only-this-mac`, ou seja, com um plist herdado,
/// o Hub recusa subir em vez de cair na rede + relay. Sem `--bind`, o Hub escuta só em
/// loopback em qualquer modo; escutar fora deste computador exige `--bind` explícito.
pub fn resolve_serve(bind: Option<&str>, only_this_mac: bool, mac_service: bool) -> Result<(String, Scope)> {
    if mac_service && !only_this_mac {
        bail!("o serviço do Mac só sobe com --only-this-mac (plist herdado?); reinstale o Hub pelo app");
    }
    let scope = Scope::from_flag(only_this_mac);
    let bind = bind.unwrap_or(LOOPBACK);
    scope.check_bind(bind)?;
    Ok((bind.to_owned(), scope))
}

/// Resolve `serve` com a interface Web local (`--web-dir`): falha fechada em qualquer
/// endereço que não seja o loopback, nunca sobe o relay de saída e ignora o plist herdado
/// só quando o modo é explícito — sem `has_web`, vale `resolve_serve`.
pub fn resolve_serve_with_web(
    bind: Option<&str>,
    only_this_mac: bool,
    mac_service: bool,
    has_web: bool,
) -> Result<(String, Scope)> {
    if !has_web {
        return resolve_serve(bind, only_this_mac, mac_service);
    }
    if mac_service && !only_this_mac {
        bail!("o serviço do Mac só sobe com --only-this-mac (plist herdado?); reinstale o Hub pelo app");
    }
    if let Some(bind) = bind {
        anyhow::ensure!(bind == LOOPBACK, "a interface Web local só aceita --bind {LOOPBACK}, recebeu {bind}");
    }
    Ok((LOOPBACK.to_owned(), Scope::ThisMacOnly))
}

/// Os níveis 1 e 2 da desinstalação nunca tocam em `~/.claude`; só o `uninstall` sem `--nivel`
/// (caminho herdado) restaura a status line do Claude Code.
pub fn touches_claude_settings(nivel: Option<u8>) -> bool {
    nivel.is_none()
}

/// O que `launchctl print gui/<uid>/<label>` diz sobre o serviço.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    Absent,
    Loaded,
    Unknown(String),
}

/// `print` bem-sucedido = carregado; falha por ausência (código 113 ou "Could not find service")
/// = ausente; qualquer outra falha não prova nada e não conta como remoção.
pub fn classify_print(code: Option<i32>, output: &str) -> Presence {
    let text = output.to_lowercase();
    match code {
        Some(0) => Presence::Loaded,
        Some(113) => Presence::Absent,
        _ if text.contains("could not find service") || text.contains("could not find specified service") => {
            Presence::Absent
        }
        _ => Presence::Unknown(output.trim().to_owned()),
    }
}

/// Só confirma a remoção quando o serviço sumiu; o resultado do `bootout` entra só no motivo.
pub fn ensure_removed(bootout_detail: &str, presence: &Presence) -> Result<()> {
    match presence {
        Presence::Absent => Ok(()),
        Presence::Loaded => bail!("o serviço do Hub continua carregado após o bootout: {}", bootout_detail.trim()),
        Presence::Unknown(why) => {
            bail!("não foi possível confirmar que o serviço sumiu: {why} ({})", bootout_detail.trim())
        }
    }
}

/// Pastas padrão do macOS e das ferramentas que o Hub usa (node, claude, codex), sem herdar
/// o `PATH` curto de um app aberto pelo Finder.
pub fn service_path(home: &Path) -> String {
    let home = home.to_string_lossy();
    [
        "/opt/homebrew/bin".to_owned(),
        "/opt/homebrew/sbin".to_owned(),
        "/usr/local/bin".to_owned(),
        "/usr/bin".to_owned(),
        "/bin".to_owned(),
        "/usr/sbin".to_owned(),
        "/sbin".to_owned(),
        format!("{home}/.local/bin"),
        format!("{home}/.npm-global/bin"),
        format!("{home}/.bun/bin"),
    ]
    .join(":")
}

pub struct PlistSpec<'a> {
    pub label: &'a str,
    pub exe: &'a str,
    pub port: u16,
    pub path: &'a str,
    pub data_dir: &'a str,
    pub log: &'a str,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Plist do `LaunchAgent`. Sempre com `--bind 127.0.0.1` e `--only-this-mac`.
pub fn plist(spec: &PlistSpec<'_>) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>serve</string><string>--port</string><string>{port}</string><string>--bind</string><string>{LOOPBACK}</string><string>--only-this-mac</string></array>
  <key>EnvironmentVariables</key><dict><key>PATH</key><string>{path}</string><key>MEXERICA_HOME</key><string>{data}</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>{THROTTLE_SECONDS}</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = xml_escape(spec.label),
        exe = xml_escape(spec.exe),
        port = spec.port,
        path = xml_escape(spec.path),
        data = xml_escape(spec.data_dir),
        log = xml_escape(spec.log),
    )
}

/// Logs e caches que o Hub recria sozinho. Nada mais da pasta de dados entra aqui: banco,
/// bots, conversas, identidade, token, perfis de contas e configuração ficam.
const REGENERABLE: [&str; 4] = ["host.log", "update.log", "cua-driver.log", "registry.json"];

pub fn regenerable_files(data_dir: &Path) -> Vec<PathBuf> {
    REGENERABLE.iter().map(|name| data_dir.join(name)).collect()
}

/// Nível 2: remove só os arquivos regeneráveis. Ausente não é erro; outro erro sobe com contexto.
pub fn remove_regenerable(data_dir: &Path) -> Result<Vec<PathBuf>> {
    use anyhow::Context as _;
    let mut removed = Vec::new();
    for file in regenerable_files(data_dir) {
        match std::fs::remove_file(&file) {
            Ok(()) => removed.push(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("removing {}", file.display())),
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(port: u16, exe: &str) -> PlistSpec<'_> {
        PlistSpec { label: "x.y", exe, port, path: "/usr/bin", data_dir: "/d", log: "/d/host.log" }
    }

    #[test]
    fn plist_always_binds_loopback_only() {
        for port in [1, 8080, 65535] {
            let text = plist(&spec(port, "/Applications/Mexerica.app/Contents/Resources/donnabot-host"));
            assert!(text.contains("<string>--bind</string><string>127.0.0.1</string>"));
            assert!(text.contains("<string>--only-this-mac</string>"));
            assert!(!text.contains("0.0.0.0"));
            assert_eq!(text.matches("--bind").count(), 1);
            assert!(text.contains("<key>ThrottleInterval</key><integer>30</integer>"));
        }
    }

    #[test]
    fn plist_bind_follows_port_and_escapes_values() {
        let text = plist(&spec(4000, "/a&b/<x>"));
        assert!(text.contains("<string>/a&amp;b/&lt;x&gt;</string><string>serve</string><string>--port</string><string>4000</string><string>--bind</string>"));
    }

    #[test]
    fn service_path_is_explicit_and_covers_tools() {
        let path = service_path(Path::new("/Users/user"));
        for part in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/Users/user/.local/bin"] {
            assert!(path.split(':').any(|p| p == part), "{part}");
        }
    }

    #[test]
    fn this_mac_scope_rejects_other_binds_and_skips_relay() {
        assert!(Scope::ThisMacOnly.check_bind("127.0.0.1").is_ok());
        for bind in ["0.0.0.0", "::", "203.0.113.2", "localhost", ""] {
            assert!(Scope::ThisMacOnly.check_bind(bind).is_err(), "{bind}");
        }
        assert!(Scope::Network.check_bind("0.0.0.0").is_ok());
        assert!(!Scope::ThisMacOnly.runs_outbound_relay());
        assert!(Scope::Network.runs_outbound_relay());
        assert_eq!(Scope::from_flag(true), Scope::ThisMacOnly);
        assert_eq!(Scope::from_flag(false), Scope::Network);
    }

    #[test]
    fn inherited_mac_service_without_flag_refuses_and_flag_defaults_to_loopback() {
        assert!(resolve_serve(None, false, true).is_err());
        assert!(resolve_serve(Some("0.0.0.0"), false, true).is_err());
        let (bind, scope) = resolve_serve(None, true, true).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::ThisMacOnly));
        assert!(resolve_serve(Some("0.0.0.0"), true, true).is_err());
        let (bind, scope) = resolve_serve(None, false, false).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::Network));
    }

    #[test]
    fn default_bind_is_loopback_and_explicit_bind_is_kept() {
        for (only_this_mac, mac_service) in [(false, false), (true, false), (true, true)] {
            let (bind, _) = resolve_serve(None, only_this_mac, mac_service).unwrap();
            assert_eq!(bind, LOOPBACK, "{only_this_mac} {mac_service}");
        }
        for explicit in ["0.0.0.0", "::", "192.0.2.10", "127.0.0.1"] {
            let (bind, scope) = resolve_serve(Some(explicit), false, false).unwrap();
            assert_eq!((bind.as_str(), scope), (explicit, Scope::Network));
        }
    }

    #[test]
    fn web_mode_defaults_to_loopback_refuses_other_binds_and_skips_relay() {
        let (bind, scope) = resolve_serve_with_web(None, false, false, true).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::ThisMacOnly));
        assert!(!scope.runs_outbound_relay(), "o modo Web local nunca sobe o relay de saída");
        for bind in ["0.0.0.0", "::", "203.0.113.2", "198.51.100.5", "localhost", ""] {
            assert!(resolve_serve_with_web(Some(bind), false, false, true).is_err(), "{bind}");
        }
        // Com --bind 127.0.0.1 explícito segue válido.
        let (bind, scope) = resolve_serve_with_web(Some("127.0.0.1"), true, true, true).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::ThisMacOnly));
        // Sem a interface Web, vale `resolve_serve`: loopback por padrão, relay segue ligado.
        let (bind, scope) = resolve_serve_with_web(None, false, false, false).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::Network));
        let (bind, scope) = resolve_serve_with_web(None, true, false, false).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::ThisMacOnly));
        assert!(resolve_serve_with_web(None, false, true, false).is_err());
    }

    #[test]
    fn only_levels_without_nivel_touch_claude_settings() {
        assert!(touches_claude_settings(None));
        assert!(!touches_claude_settings(Some(1)));
        assert!(!touches_claude_settings(Some(2)));
    }

    #[test]
    fn removal_is_confirmed_only_when_service_is_gone() {
        assert_eq!(classify_print(Some(0), ""), Presence::Loaded);
        assert_eq!(classify_print(Some(113), ""), Presence::Absent);
        assert_eq!(classify_print(Some(1), "Could not find service \"x\" in domain"), Presence::Absent);
        assert!(matches!(classify_print(Some(5), "Operation not permitted"), Presence::Unknown(_)));
        assert!(matches!(classify_print(None, ""), Presence::Unknown(_)));
        // bootout falhou mas o serviço já estava ausente: ok; bootout falhou e segue carregado: erro.
        assert!(ensure_removed("Boot-out failed: 5", &Presence::Absent).is_ok());
        let loaded = ensure_removed("Boot-out failed: 5", &Presence::Loaded).unwrap_err().to_string();
        assert!(loaded.contains("continua carregado") && loaded.contains("Boot-out failed"));
        assert!(ensure_removed("", &Presence::Unknown("denied".into())).is_err());
    }

    #[test]
    fn level_two_removes_only_regenerable_files() {
        let dir = std::env::temp_dir().join(format!("mac-local-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("engine-setup/accounts/a")).unwrap();
        for keep in ["token", "store.db", "identity.json", "engine-setup/accounts/a/login"] {
            std::fs::write(dir.join(keep), b"x").unwrap();
        }
        std::fs::write(dir.join("host.log"), b"x").unwrap();
        std::fs::write(dir.join("registry.json"), b"x").unwrap();
        let removed = remove_regenerable(&dir).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(!dir.join("host.log").exists() && !dir.join("registry.json").exists());
        for keep in ["token", "store.db", "identity.json", "engine-setup/accounts/a/login"] {
            assert!(dir.join(keep).exists(), "{keep}");
        }
        assert_eq!(remove_regenerable(&dir).unwrap().len(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
