use crate::{
    LockExt,
    api::devices::Caller,
    fork::autoridade::Autoridade,
    hub::Hub,
    store::{BotConfig, Entry, Lane},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha512};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

pub(crate) const ID: &str = "antigravity";
pub(crate) const ENTRY_MARKER: &str = "interactiveAntigravity";
pub(crate) const INTERACTIVE: &str = "O login pessoal do Antigravity só funciona no chat, com uma mensagem digitada pelo dono em um aparelho autenticado. Para automações, use uma chave de API do Gemini/Vertex em um motor compatível ou escolha outro motor.";
const INSTALL: &str =
    "Instale a versão oficial 1.5.15 do Antigravity CLI e entre diretamente no CLI com agy acp --login.";
const LIFETIME: Duration = Duration::from_secs(90);

pub(crate) fn mode_diverged(update: &serde_json::Value) -> bool {
    match update["sessionUpdate"].as_str() {
        Some("current_mode_update") => update["currentModeId"] != "default",
        Some("config_option_update") => update["configOptions"]
            .as_array()
            .is_none_or(|options| options.iter().any(|o| o["category"] == "mode" && o["currentValue"] != "default")),
        _ => false,
    }
}

fn digest_for_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(
            "d28b98e61c79129a3d5615af48ceb9f61737ac8c3feb6d077558605b40bf7159012b4a32084e1f728d8e14417182d4344a0bf7e10ba5934b3c8c0e6cb0a55c60",
        ),
        ("linux", "aarch64") => Some(
            "4f9993957b391c046728435fff2e657ecdc16a82ddfcb774c83dd3becba6a8980e1ec2fc1e610cab9594edf690cc535d8ad5795a67faa2ae4197301d241bd492",
        ),
        ("macos", "x86_64") => Some(
            "91091e6c2efb1312dcf542fcbb8a055fa95b57cd07b9aab55fea98693b6e8ea238543a8a571ec299ef44494b909aef9be3988fbbeb1a318246e4581125b90c72",
        ),
        ("macos", "aarch64") => Some(
            "e96d893cafbaef8b504d46ec5b231dfefa242290f6148827261f6dafa08e9fdff785ae8343de372063b6dac1437ef5a94af61a3beded1eaca9f19c312bf33cad",
        ),
        ("windows", "x86_64") => Some(
            "f9b9d4ffe050c9b909df2afffc5dcca834cd97ed18c1a4cc1b686244c24cf53fd44b45c34c99c25b9892cb8057cbf35a27a376db0a62a5222e1cbf9e8e0bb47e",
        ),
        ("windows", "aarch64") => Some(
            "1be97ba06c66176bae16c9034ac0f813df495232084c40df0c76460f947b58ed688019593dfb430f2256acf544f233055a3390b3e9043fadffc9ff7b46a9660a",
        ),
        _ => None,
    }
}

pub(crate) fn platform_supported() -> bool {
    digest_for_platform().is_some() && !crate::fork::motor_exec::configured()
}

pub(crate) struct Official(PathBuf);
impl Official {
    pub(crate) fn resolve() -> Result<Self> {
        crate::open_hub::require_executor()?;
        ensure!(!crate::fork::motor_exec::configured(), "{INSTALL}");
        let expected = digest_for_platform().context(INSTALL)?;
        let path = crate::agent::backends::which("agy").context(INSTALL)?;
        ensure!(path.is_absolute(), "{INSTALL}");
        let path = path.canonicalize().context(INSTALL)?;
        #[cfg(unix)]
        trusted_path(&path).context(INSTALL)?;
        ensure!(verify(&path, expected).unwrap_or(false), "{INSTALL}");
        Ok(Self(path))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(unix)]
fn trusted_path(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let uid = crate::fork::private_fs::uid();
    for part in path.ancestors() {
        let meta = std::fs::symlink_metadata(part)?;
        ensure!(
            (meta.uid() == 0 || meta.uid() == uid)
                && meta.mode() & 0o022 == 0
                && if part == path { meta.is_file() } else { meta.is_dir() },
            "{INSTALL}"
        );
    }
    Ok(())
}

fn verify(path: &Path, expected: &str) -> Result<bool> {
    const LIMIT: u64 = 512 * 1024 * 1024;
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Ok(false);
        }
    }
    let mut file = file.take(LIMIT + 1);
    let mut hash = Sha512::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(file.limit() > 0 && crate::chat::files::digest_hex(&hash.finalize()) == expected);
        }
        hash.update(&buffer[..n]);
    }
}

pub(crate) fn config(cfg: &BotConfig) -> Result<()> {
    ensure!(
        cfg.backend == ID
            && !cfg.is_group()
            && cfg.space_id == crate::fork::spaces::PERSONAL
            && cfg.context.is_none()
            && cfg.command.is_none()
            && cfg.fork_editor.engine_account_id.is_none(),
        "{INTERACTIVE}"
    );
    Ok(())
}

pub(crate) fn owner(hub: &Hub, caller: &Caller, origin: &serde_json::Value) -> Result<Autoridade> {
    ensure!(typed(origin), "{INTERACTIVE}");
    authenticated_owner(hub, caller)
}

pub(crate) fn authenticated_owner(hub: &Hub, caller: &Caller) -> Result<Autoridade> {
    ensure!(
        matches!(caller, Caller::Device { scopes, .. } if scopes.contains(&crate::store::Scope::OwnerDevices)),
        "{INTERACTIVE}"
    );
    let owner = Autoridade::capturar(hub, caller).context(INTERACTIVE)?;
    ensure!(owner.outro_aparelho(), "{INTERACTIVE}");
    Ok(owner)
}

fn typed(origin: &serde_json::Value) -> bool {
    origin["status"] == "appReported"
        && origin["report"]["channel"] == "desktopComposer"
        && origin["segments"]
            .as_array()
            .is_some_and(|segments| !segments.is_empty() && segments.iter().all(|s| s["kind"] == "typed"))
}

#[derive(Default)]
pub(crate) struct Grants(Mutex<HashMap<String, Grant>>);

pub(crate) struct Grant {
    owner: Autoridade,
    lane: Lane,
    entry: String,
    revision: i64,
    expires: Instant,
}

impl Grants {
    pub(crate) fn insert(&self, owner: Autoridade, lane: Lane, entry: &Entry) {
        let now = Instant::now();
        let mut grants = self.0.locked();
        grants.retain(|_, g| now < g.expires);
        if grants.len() < 256 {
            grants.insert(
                entry.id.clone(),
                Grant { owner, lane, entry: entry.id.clone(), revision: entry.rev, expires: now + LIFETIME },
            );
        }
    }

    pub(crate) fn take(&self, id: &str) -> Option<Grant> {
        self.0.locked().remove(id)
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.0.locked().contains_key(id)
    }
}

impl Grant {
    pub(crate) fn current(&self, hub: &Hub, running: &BotConfig) -> bool {
        self.owner.with_db(hub, None, |_| Ok(())).is_ok()
            && hub.store.bot(&running.id).ok().flatten().filter(|bot| !bot.deleted).is_some_and(|bot| {
                let mut cfg = bot.config;
                if crate::fork::sessoes::admissao::apply_configured(&hub.store.db.locked(), &self.lane, &mut cfg)
                    .is_err()
                    || cfg.backend != ID
                {
                    return false;
                }
                crate::fork::sessoes::admissao::apply_choice(&hub.store.db.locked(), &self.lane, &mut cfg).is_ok()
                    && config(&cfg).is_ok()
                    && cfg.model == running.model
                    && cfg.model_options == running.model_options
                    && cfg.cwd == running.cwd
            })
    }

    pub(crate) fn write(
        &self,
        hub: &Hub,
        effect: &mut dyn FnMut() -> std::task::Poll<Result<()>>,
    ) -> std::task::Poll<Result<()>> {
        if self.expires <= Instant::now() {
            return std::task::Poll::Ready(Err(anyhow::anyhow!(INTERACTIVE)));
        }
        self.owner
            .com_saida(hub, None, effect)
            .unwrap_or_else(|_| std::task::Poll::Ready(Err(anyhow::anyhow!(INTERACTIVE))))
    }

    pub(crate) fn check(&self, hub: &Hub, lane: &Lane, entries: &[String]) -> Result<()> {
        ensure!(
            self.expires > Instant::now() && &self.lane == lane && entries == [self.entry.as_str()],
            "{INTERACTIVE}"
        );
        self.owner
            .with_db(hub, None, |db| {
                let e = crate::store::Store::entry_on(db, &self.entry)?.context(INTERACTIVE)?;
                ensure!(
                    e.rev == self.revision
                        && e.bot_id == lane.chat
                        && e.session_id == lane.session_id()
                        && e.data["status"] == "queued"
                        && e.is_user_authored()
                        && typed(&e.data["contentOrigin"]),
                    "{INTERACTIVE}"
                );
                Ok(())
            })
            .context(INTERACTIVE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn antigravity_detecta_modo_divergente_no_turno() {
        assert!(mode_diverged(&json!({"sessionUpdate":"current_mode_update","currentModeId":"auto"})));
        assert!(mode_diverged(&json!({"sessionUpdate":"current_mode_update"})));
        assert!(mode_diverged(
            &json!({"sessionUpdate":"config_option_update","configOptions":[{"category":"mode","currentValue":"auto"}]})
        ));
        assert!(!mode_diverged(&json!({"sessionUpdate":"current_mode_update","currentModeId":"default"})));
        assert!(!mode_diverged(
            &json!({"sessionUpdate":"config_option_update","configOptions":[{"category":"mode","currentValue":"default"}]})
        ));
    }

    #[test]
    fn antigravity_arquivo_com_digest_errado_nao_e_oficial() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("agy");
        std::fs::write(&file, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let digest = crate::chat::files::digest_hex(&Sha512::digest(b"fixture"));
        assert!(verify(&file, &digest).unwrap());
        std::fs::write(&file, b"alterado").unwrap();
        assert!(!verify(&file, &digest).unwrap());
    }

    #[test]
    fn antigravity_nao_aceita_voz_colagem_ou_origem_desconhecida() {
        let body = json!({"contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,"textLength":3,
            "deviceId":uuid::Uuid::new_v4().to_string(),"channel":"desktopComposer","interaction":"send",
            "segments":[{"start":0,"end":3,"kind":"typed"}]}});
        let capture =
            |b: &serde_json::Value| crate::guard_hub::origem::capture(&Caller::Local, b, &json!({"text":"oi!"}));
        assert!(typed(&capture(&body)));
        for kind in ["pasted", "unknown", "generated", "voice"] {
            let mut altered = body.clone();
            altered["contentOrigin"]["segments"][0]["kind"] = json!(kind);
            assert!(!typed(&capture(&altered)));
        }
        assert!(!typed(&capture(&json!({"human":true}))));
    }

    #[test]
    fn antigravity_recibo_autenticado_e_unico_e_revogavel() {
        use crate::store::{Device, DeviceSource, Scope, Store};
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("test.db")).unwrap();
        let id = crate::remote::identity::Identity::load_or_create(dir.path()).unwrap();
        let hub = Hub::new(store, "fixture".into(), id, "fixture".into(), 0);
        crate::remote::cloud::update_status(&hub, |status| {
            status.owner = Some(crate::remote::cloud::Owner { user_id: "owner".into(), email: None });
        });
        hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
        let now = crate::store::now_ms();
        let mut device = Device {
            key: "device".into(),
            name: "Fixture".into(),
            platform: "desktop".into(),
            source: DeviceSource::Account,
            grant_id: Some("grant".into()),
            scopes: vec![Scope::Control],
            lease_until: Some(now + 60_000),
            created_at: now,
            last_seen_at: None,
        };
        let report =
            json!({"status":"appReported","report":{"channel":"desktopComposer"},"segments":[{"kind":"typed"}]});
        hub.store.put_device(&device).unwrap();
        let (_, caller) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
        assert!(owner(&hub, &caller, &report).is_err());
        device.scopes.push(Scope::OwnerDevices);
        hub.store.put_device(&device).unwrap();
        let (_, caller) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
        let authority = owner(&hub, &caller, &report).unwrap();
        let lane = Lane::main("bot");
        let entry = Entry {
            id: "entry".into(),
            seq: 1,
            bot_id: "bot".into(),
            rev: 1,
            kind: "user".into(),
            turn: 1,
            data: json!({}),
            created_at: now,
            updated_at: now,
            session_id: "bot".into(),
        };
        let grants = Grants::default();
        grants.insert(authority, lane, &entry);
        let grant = grants.take(&entry.id).unwrap();
        assert!(grants.take(&entry.id).is_none());
        assert!(matches!(grant.write(&hub, &mut || std::task::Poll::Ready(Ok(()))), std::task::Poll::Ready(Ok(()))));
        hub.revoke_device(&device.key).unwrap();
        assert!(matches!(grant.write(&hub, &mut || panic!("saída após revogação")), std::task::Poll::Ready(Err(_))));
    }
}
