//! Build identity. Development and production must never own the same service or data.
//!
//! Donnabot: os recursos locais são do Donnabot (`app.donnabot.*`, `~/.donnabot*`, portas 194xx)
//! para conviver no mesmo computador com o Codync e o Codync Dev instalados
//! (docs/guides/mexerica-recursos.md).

use std::path::{Component, Path, PathBuf};

/// Portas do Codync (19222), do Codync Dev (19223) e das sandboxes dele (19300–19399).
const CODYNC_PORTS: [std::ops::RangeInclusive<u16>; 3] = [19222..=19222, 19223..=19223, 19300..=19399];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Main,
    Dev,
    Open,
}

impl Environment {
    pub const fn current() -> Self {
        match option_env!("CODYNC_ENV") {
            Some(value) => match value.as_bytes() {
                b"main" => Self::Main,
                b"open" => Self::Open,
                _ => Self::Dev,
            },
            None => Self::Dev,
        }
    }

    pub const fn is_open() -> bool {
        matches!(Self::current(), Self::Open)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Dev => "dev",
            Self::Open => "open",
        }
    }

    /// O nome que o usuário vê (terminal, TUI, mensagens).
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Main => "Mexerica",
            Self::Dev => "Mexerica Dev",
            Self::Open => "Mexerica Open",
        }
    }

    pub const fn port(self) -> u16 {
        match self {
            Self::Main => 19422,
            Self::Dev => 19423,
            Self::Open => 19524,
        }
    }

    pub const fn data_folder(self) -> &'static str {
        match self {
            Self::Main => ".mexerica",
            Self::Dev => ".mexerica-dev",
            Self::Open => ".mexerica-open",
        }
    }

    #[cfg(any(unix, test))]
    pub const fn service_label(self) -> &'static str {
        match self {
            Self::Main => "app.donnabot.host",
            Self::Dev => "app.donnabot.dev.host",
            Self::Open => "app.mexerica.open.host",
        }
    }

    /// Job avulso do `launchctl submit` que aplicava atualizações (desligadas no fork).
    pub const fn update_label(self) -> &'static str {
        match self {
            Self::Main => "app.donnabot.update",
            Self::Dev => "app.donnabot.dev.update",
            Self::Open => "app.mexerica.open.update",
        }
    }

    #[cfg(any(unix, test))]
    pub const fn systemd_unit(self) -> &'static str {
        match self {
            Self::Main => "donnabot-host.service",
            Self::Dev => "donnabot-dev-host.service",
            Self::Open => "mexerica-open-host.service",
        }
    }

    /// Unidade transitória do `systemd-run` do atualizador.
    pub const fn systemd_update_unit(self) -> &'static str {
        match self {
            Self::Main => "donnabot-host-update",
            Self::Dev => "donnabot-dev-host-update",
            Self::Open => "mexerica-open-host-update",
        }
    }

    /// Pasta privada do socket do cua-driver (o helper de tela do macOS usa a mesma).
    #[cfg(any(target_os = "linux", test))]
    pub const fn cua_folder(self) -> &'static str {
        match self {
            Self::Main => "donnabot-cua",
            Self::Dev => "donnabot-dev-cua",
            Self::Open => "mexerica-open-cua",
        }
    }

    /// Serviço do Keychain/Secret Service que guarda a chave mestra do cofre.
    pub const fn vault_service(self) -> &'static str {
        match self {
            Self::Main => "app.donnabot.credentials",
            Self::Dev => "app.donnabot.dev.credentials",
            Self::Open => "app.mexerica.open.credentials",
        }
    }

    #[cfg(any(windows, test))]
    pub const fn windows_value(self) -> &'static str {
        match self {
            Self::Main => "DonnabotHost",
            Self::Dev => "DonnabotDevHost",
            Self::Open => "MexericaOpenHost",
        }
    }

    /// Nome do binário supervisor no Windows (o Windows está fora do plano; só o nome muda).
    #[cfg(any(windows, test))]
    pub const fn supervisor(self) -> &'static str {
        match self {
            Self::Main => "donnabot-hostw.exe",
            Self::Dev => "donnabot-dev-hostw.exe",
            Self::Open => "mexerica-open-hostw.exe",
        }
    }

    /// Nuvem do fork ainda não configurada; nenhum fallback para o upstream.
    #[allow(clippy::unused_self)] // Preserva a costura por ambiente, sem fallback de rede.
    pub const fn cloud_url(self) -> Option<&'static str> {
        None
    }

    pub const fn url_scheme(self) -> &'static str {
        match self {
            Self::Main => "donnabot",
            Self::Dev => "donnabot-dev",
            Self::Open => "mexerica-open",
        }
    }

    #[allow(clippy::unused_self)] // O contrato continua por ambiente; ambos recusam updates.
    pub fn require_release_updates(self) -> anyhow::Result<()> {
        anyhow::bail!("atualizações desativadas na Mexerica; recompile o host/app")
    }
}

/// Recusa a pasta de dados do Codync (`~/.codync`, `~/.codync-dev`, `~/.codync-dev-<sandbox>`
/// ou algo dentro delas): o Donnabot nunca lê nem escreve nos dados do Codync instalado. Vale em
/// qualquer grafia: symlinks e `..` são resolvidos e a comparação ignora maiúsculas (o APFS
/// padrão do macOS também ignora).
pub fn ensure_own_data_dir(home: &Path, data_dir: &Path) -> anyhow::Result<()> {
    let fold = |path: &Path| PathBuf::from(path.to_string_lossy().to_lowercase());
    let lexical_home = fold(home);
    let lexical_dir = fold(data_dir);
    let old = |base: &Path, target: &Path| {
        target.strip_prefix(base).ok().and_then(|rest| rest.components().next()).is_some_and(|part| {
            matches!(part, Component::Normal(name) if [".codync", ".donnabot"].iter().any(|p| name.to_string_lossy().starts_with(p)))
        })
    };
    let (resolved_home, resolved_dir) = (fold(&resolve(home)), fold(&resolve(data_dir)));
    anyhow::ensure!(
        !resolved_home.starts_with(&resolved_dir)
            && !old(&lexical_home, &lexical_dir)
            && !old(&resolved_home, &resolved_dir),
        "pasta legada/upstream recusada; configure MEXERICA_HOME em pasta própria da Mexerica"
    );
    // Aliases de famílias antigas não autorizam abrir dados sob outro nome.
    if let Ok(entries) = std::fs::read_dir(home) {
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if [".codync", ".donnabot"].iter().any(|p| name.starts_with(p)) {
                let legacy = fold(&resolve(&entry.path()));
                anyhow::ensure!(!resolved_dir.starts_with(legacy), "alias de pasta legada/upstream recusado");
            }
        }
    }
    Ok(())
}

/// O caminho real: absoluto, com symlinks e `..` resolvidos até onde ele existe; o resto, que
/// ainda não existe, é normalizado só pelo texto.
fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => continue,
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
        if let Ok(real) = std::fs::canonicalize(&out) {
            out = real;
        }
    }
    out
}

/// Recusa as portas do Codync e do Codync Dev, para os dois hosts rodarem lado a lado.
pub fn ensure_own_port(port: u16) -> anyhow::Result<()> {
    if CODYNC_PORTS.iter().any(|ports| ports.contains(&port)) {
        anyhow::bail!("a porta {port} é do Codync; a Mexerica usa {} por padrão", Environment::current().port());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Environment::{Dev, Main};
    use super::{ensure_own_data_dir, ensure_own_port};
    use std::path::Path;

    #[test]
    fn development_cannot_replace_production_resources() {
        assert_eq!(Main.port(), 19422);
        assert_eq!(Main.data_folder(), ".mexerica");
        assert_eq!(Dev.data_folder(), ".mexerica-dev");
        assert_eq!(Main.service_label(), "app.donnabot.host");
        assert_eq!(Main.systemd_unit(), "donnabot-host.service");
        assert_eq!(Main.windows_value(), "DonnabotHost");
        assert_eq!(Main.vault_service(), "app.donnabot.credentials");
        assert_eq!(Main.url_scheme(), "donnabot");
        assert_ne!(Main.port(), Dev.port());
        for (main, dev) in [
            (Main.display_name(), Dev.display_name()),
            (Main.data_folder(), Dev.data_folder()),
            (Main.service_label(), Dev.service_label()),
            (Main.update_label(), Dev.update_label()),
            (Main.systemd_unit(), Dev.systemd_unit()),
            (Main.systemd_update_unit(), Dev.systemd_update_unit()),
            (Main.cua_folder(), Dev.cua_folder()),
            (Main.vault_service(), Dev.vault_service()),
            (Main.windows_value(), Dev.windows_value()),
            (Main.supervisor(), Dev.supervisor()),
            (Main.url_scheme(), Dev.url_scheme()),
        ] {
            assert_ne!(main, dev);
        }
        assert!(Dev.require_release_updates().is_err());
        assert!(Main.require_release_updates().is_err());
    }

    /// Nenhum recurso do Donnabot coincide com os do Codync ou do Codync Dev instalados.
    #[test]
    fn donnabot_never_takes_codync_resources() {
        let codync = [
            ".codync",
            ".codync-dev",
            "com.pokai.codync.host",
            "com.pokai.codync.dev.host",
            "com.pokai.codync.update",
            "codync-host.service",
            "codync-dev-host.service",
            "codync-host-update",
            "codync-cua",
            "codync-dev-cua",
            "dev.codync.credentials",
            "dev.codync.development.credentials",
            "CodyncHost",
            "CodyncDevHost",
            "codync-hostw.exe",
            "codync-dev-hostw.exe",
            "codync",
            "codync-dev",
        ];
        for env in [Main, Dev] {
            for ours in [
                env.data_folder(),
                env.service_label(),
                env.update_label(),
                env.systemd_unit(),
                env.systemd_update_unit(),
                env.cua_folder(),
                env.vault_service(),
                env.windows_value(),
                env.supervisor(),
                env.url_scheme(),
            ] {
                assert!(!codync.contains(&ours), "{ours} é do Codync");
            }
            assert!(ensure_own_port(env.port()).is_ok());
        }
    }

    #[test]
    fn codync_data_and_ports_are_refused() {
        let home = Path::new("/Users/me");
        for theirs in [".codync", ".codync-dev", ".codync-dev-feature", ".codync/bots", ".donnabot", ".DONNABOT-dev"] {
            assert!(ensure_own_data_dir(home, &home.join(theirs)).is_err(), "{theirs}");
        }
        for ours in [".mexerica", ".mexerica-dev", ".mexerica-dev-feature", "elsewhere/.codync"] {
            assert!(ensure_own_data_dir(home, &home.join(ours)).is_ok(), "{ours}");
        }
        assert!(ensure_own_data_dir(home, Path::new("/tmp/donnabot-test")).is_ok());
        for port in [19222, 19223, 19300, 19399] {
            assert!(ensure_own_port(port).is_err(), "{port}");
        }
        for port in [19422, 19423, 19430, 19499] {
            assert!(ensure_own_port(port).is_ok(), "{port}");
        }
    }

    /// A pasta do Codync é recusada também por symlink, `..` e outra grafia.
    #[test]
    fn codync_data_is_refused_through_links_dots_and_case() {
        let root = std::env::temp_dir().join(format!("donnabot-env-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".codync")).unwrap();
        std::fs::create_dir_all(home.join("work")).unwrap();
        for theirs in [".codync/bots", "work/../.codync", "work/./../.codync-dev", ".CODYNC", ".Codync-Dev-x"] {
            assert!(ensure_own_data_dir(&home, &home.join(theirs)).is_err(), "{theirs}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.join(".codync"), home.join("work/data")).unwrap();
            std::os::unix::fs::symlink(&home, root.join("home-link")).unwrap();
            for theirs in [home.join("work/data"), home.join("work/data/bots"), root.join("home-link/.codync")] {
                assert!(ensure_own_data_dir(&home, &theirs).is_err(), "{}", theirs.display());
            }
        }
        for ours in [".mexerica", "work/.mexerica-dev", "work/../.mexerica"] {
            assert!(ensure_own_data_dir(&home, &home.join(ours)).is_ok(), "{ours}");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
