//! Papel isolado: não abre Store, cofre, relay, serviço ou runtime de agentes.
use super::{
    protocolo as p,
    tipos::{AccountState, Axis, Driver, Manifest, Observation, RUNNER_PORT, Unavailable, resource_id},
    transporte,
};
use anyhow::{Result, anyhow, ensure};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

#[derive(clap::Subcommand)]
pub enum Action {
    /// Inicializa identidade própria a partir da configuração pública autenticada.
    Init {
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long)]
        config: PathBuf,
    },
    /// Serve somente consulta autenticada em loopback, sem abrir o Hub.
    Serve {
        #[arg(long)]
        state_dir: PathBuf,
    },
    /// Atualiza só configuração pública, com serviço parado e CAS da chave anterior.
    Reconfigure {
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        expected_control_public: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Troca identidade própria explicitamente; conserva perfil e histórico.
    RotateIdentity {
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long)]
        expected_pin: String,
        #[arg(long)]
        confirm: bool,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Config {
    hub_id: String,
    place_id: String,
    resource_id: String,
    control_public: String,
    manifest: Manifest,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct State {
    config: Config,
    seed: String,
}
impl Drop for State {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.seed.zeroize();
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    operation: String,
    intent: String,
    observation: Option<Observation>,
    state: JournalState,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum JournalState {
    Accepted,
    Complete,
    CancelledUncertain,
}

pub async fn run(action: Action) -> Result<()> {
    ensure!(cfg!(target_os = "linux"), "runner_platform_not_admitted");
    match action {
        Action::Init { state_dir, config } => {
            validate_dir(&state_dir)?;
            let bytes = bounded_file(&config, 64 * 1024)?;
            let config: Config = serde_json::from_slice(&bytes)?;
            config.manifest.validate()?;
            ensure!(resource_id(&config.resource_id) && !config.hub_id.is_empty(), "runner_binding_invalid");
            uuid::Uuid::parse_str(&config.place_id)?;
            let control = p::fixed::<32>(&config.control_public)?;
            ed25519_dalek::VerifyingKey::from_bytes(&control)?;
            if !state_dir.try_exists()? {
                transporte::private_create(&state_dir)?;
            }
            transporte::private_root(&state_dir)?;
            let seed = zeroize::Zeroizing::new(p::random::<32>()?);
            let key = SigningKey::from_bytes(&seed);
            let state = State { config, seed: p::b64(seed.as_ref()) };
            // create_new impede reinicializar/clonar uma identidade por acidente.
            transporte::write_private(&state_dir.join("identity.json"), &serde_json::to_vec(&state)?)?;
            transporte::private_create(&state_dir.join("operations"))?;
            transporte::private_create(&state_dir.join("codex"))?;
            transporte::private_create(&state_dir.join("probes"))?;
            println!(
                "{}",
                serde_json::json!({"runnerPin":p::b64(key.verifying_key().as_bytes()),
                "hubId":state.config.hub_id,"placeId":state.config.place_id,"resourceId":state.config.resource_id})
            );
            Ok(())
        }
        Action::Serve { state_dir } => serve(&state_dir).await,
        Action::Reconfigure { state_dir, config, expected_control_public, confirm } => {
            ensure!(confirm, "confirmation_required");
            validate_dir(&state_dir)?;
            transporte::private_root(&state_dir)?;
            let _lock = maintenance_lock(&state_dir)?;
            let raw = zeroize::Zeroizing::new(bounded_file(&state_dir.join("identity.json"), 64 * 1024)?);
            let mut state: State = serde_json::from_slice(&raw)?;
            ensure!(state.config.control_public == expected_control_public, "control_revision_conflict");
            let config: Config = serde_json::from_slice(&bounded_file(&config, 64 * 1024)?)?;
            ensure!(
                config.hub_id == state.config.hub_id
                    && config.place_id == state.config.place_id
                    && config.resource_id == state.config.resource_id,
                "runner_binding_invalid"
            );
            config.manifest.validate()?;
            ed25519_dalek::VerifyingKey::from_bytes(&p::fixed::<32>(&config.control_public)?)?;
            state.config = config;
            replace_state(&state_dir, &state)?;
            println!("Configuração pública atualizada. Perfil e identidade preservados.");
            Ok(())
        }
        Action::RotateIdentity { state_dir, expected_pin, confirm } => {
            ensure!(confirm, "confirmation_required");
            validate_dir(&state_dir)?;
            transporte::private_root(&state_dir)?;
            let _lock = maintenance_lock(&state_dir)?;
            let raw = zeroize::Zeroizing::new(bounded_file(&state_dir.join("identity.json"), 64 * 1024)?);
            let mut state: State = serde_json::from_slice(&raw)?;
            let seed = zeroize::Zeroizing::new(p::fixed::<32>(&state.seed)?);
            ensure!(
                p::b64(SigningKey::from_bytes(&seed).verifying_key().as_bytes()) == expected_pin,
                "runner_pin_conflict"
            );
            let seed = zeroize::Zeroizing::new(p::random::<32>()?);
            let key = SigningKey::from_bytes(&seed);
            {
                use zeroize::Zeroize as _;
                state.seed.zeroize();
            }
            state.seed = p::b64(seed.as_ref());
            replace_state(&state_dir, &state)?;
            println!(
                "{}",
                serde_json::json!({"runnerPin":p::b64(key.verifying_key().as_bytes()),"hubId":state.config.hub_id,
                "placeId":state.config.place_id,"resourceId":state.config.resource_id})
            );
            Ok(())
        }
    }
}
fn maintenance_lock(dir: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(dir.join("runner.lock"))?;
    ensure!(crate::lock_file(&file, crate::FLOCK_WAIT)?, "runner_must_be_stopped");
    Ok(file)
}
fn replace_state(dir: &Path, state: &State) -> Result<()> {
    let path = dir.join(format!("identity-{}.pending", uuid::Uuid::new_v4()));
    let raw = zeroize::Zeroizing::new(serde_json::to_vec(state)?);
    transporte::write_private(&path, &raw)?;
    std::fs::rename(path, dir.join("identity.json"))?;
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}
fn validate_dir(dir: &Path) -> Result<()> {
    ensure!(
        dir.is_absolute()
            && dir.components().all(|c| matches!(c, std::path::Component::RootDir | std::path::Component::Normal(_))),
        "runner_directory_invalid"
    );
    // Não usa service::data_dir nem variáveis do Hub para escolher estado.
    ensure!(dir == Path::new(transporte::RUNNER_STATE), "runner_directory_invalid");
    Ok(())
}
fn bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= u64::try_from(limit)?,
        "runner_file_invalid"
    );
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut b = Vec::new();
    options.open(path)?.take(u64::try_from(limit + 1)?).read_to_end(&mut b)?;
    ensure!(b.len() <= limit, "runner_file_limit");
    Ok(b)
}
async fn serve(dir: &Path) -> Result<()> {
    validate_dir(dir)?;
    transporte::private_root(dir)?;
    transporte::private_root(&dir.join("operations"))?;
    let _lock = maintenance_lock(dir)?;
    let raw = zeroize::Zeroizing::new(bounded_file(&dir.join("identity.json"), 64 * 1024)?);
    let state: State = serde_json::from_slice(&raw)?;
    state.config.manifest.validate()?;
    ensure!(state.config.manifest.arch == std::env::consts::ARCH, "artifact_arch_mismatch");
    let exe = std::env::current_exe()?;
    ensure!(
        std::fs::metadata(&exe)?.len() == state.config.manifest.size
            && file_digest(&exe)? == state.config.manifest.sha256,
        "artifact_digest_mismatch"
    );
    let seed = zeroize::Zeroizing::new(p::fixed::<32>(&state.seed)?);
    let key = SigningKey::from_bytes(&seed);
    let boot = uuid::Uuid::parse_str(
        std::str::from_utf8(&bounded_file(Path::new("/proc/sys/kernel/random/boot_id"), 100)?)?.trim(),
    )?;
    let generation = p::b64(&p::random::<32>()?);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, RUNNER_PORT)).await?;
    loop {
        let (mut socket, _) = listener.accept().await?;
        // Sem fila de tarefas: uma conexão limitada, uma inspeção, nenhum retry.
        let _ = tokio::time::timeout(
            Duration::from_secs(60),
            session(&mut socket, &state.config, &key, boot, &generation, dir),
        )
        .await;
    }
}
async fn session(
    socket: &mut tokio::net::TcpStream,
    config: &Config,
    key: &SigningKey,
    boot: uuid::Uuid,
    generation: &str,
    dir: &Path,
) -> Result<()> {
    let hello: p::Hello = tokio::time::timeout(Duration::from_secs(5), p::read(socket)).await??;
    let runner_key = p::b64(key.verifying_key().as_bytes());
    ensure!(
        hello.hub_id == config.hub_id
            && hello.place_id == config.place_id
            && hello.resource_id == config.resource_id
            && hello.control_key == config.control_public
            && hello.runner_id == p::b64(&p::hash(key.verifying_key().as_bytes())),
        "runner_binding_invalid"
    );
    let mut welcome = p::Welcome {
        runner_key,
        nonce: p::b64(&p::random::<32>()?),
        boot_id: boot,
        generation: generation.into(),
        manifest: config.manifest.clone(),
        role: p::RunnerRole::Runner,
        hub_id: config.hub_id.clone(),
        resource_id: config.resource_id.clone(),
        signature: String::new(),
    };
    let channel = p::hash(&p::transcript(&hello, &welcome)?);
    welcome.signature = p::signature(key, &p::hello_input(p::Direction::Runner, &channel)?);
    p::write(socket, &welcome).await?;
    let confirm: p::Confirm = tokio::time::timeout(Duration::from_secs(5), p::read(socket)).await??;
    let control = p::fixed::<32>(&config.control_public)?;
    p::verify(&control, &p::hello_input(p::Direction::Control, &channel)?, &confirm.signature)?;
    let mut codec = p::Channel::new(channel, p::Direction::Runner, key.clone(), control);
    let e: p::Envelope = p::read(socket).await?;
    let op = e.operation_id;
    let journal_path = dir.join("operations").join(format!("{op}.json"));
    let payload = codec.open(&e, op)?;
    if matches!(payload, p::Payload::Status {}) {
        let response = status_from_journal(&journal_path, op)?;
        p::write(socket, &codec.seal(op, &response)?).await?;
        return Ok(());
    }
    if matches!(payload, p::Payload::Cancel {}) {
        let mut journal: Journal = serde_json::from_slice(&bounded_file(&journal_path, 64 * 1024)?)?;
        ensure!(journal.operation == op.to_string(), "operation_invalid");
        let response = if matches!(journal.state, JournalState::Complete) {
            completed_result(&journal)?
        } else {
            journal.state = JournalState::CancelledUncertain;
            replace_journal(&journal_path, &journal)?;
            p::Payload::Cancelled {}
        };
        p::write(socket, &codec.seal(op, &response)?).await?;
        return Ok(());
    }
    let p::Payload::ChallengeRequest { intent, admission_revision, connection_revision } = payload else {
        anyhow::bail!("challenge_request_required")
    };
    ensure!(!journal_path.try_exists()?, "operation_already_seen");
    let mut challenge =
        p::Challenge::issue(op, intent, boot, generation.into(), admission_revision, connection_revision)?;
    let probe_dir = SocketProbe::prepare(dir, op)?;
    let response = p::Payload::LeaseChallenge {
        intent,
        nonce: challenge.nonce.clone(),
        boot_id: boot,
        generation: generation.into(),
        socket_probe_ready: probe_dir.ready,
    };
    p::write(socket, &codec.seal(op, &response)?).await?;
    let e: p::Envelope = p::read(socket).await?;
    let grant = codec.open(&e, op)?;
    let lease = challenge.grant(op, &grant)?;
    let e: p::Envelope = p::read(socket).await?;
    ensure!(matches!(codec.open(&e, op)?, p::Payload::Inspect {}), "inspect_required");
    lease.remaining()?;
    // Durável ANTES de consultar: reconexão/reinício nunca repete o efeito.
    let mut journal = Journal {
        operation: op.to_string(),
        intent: intent.to_string(),
        observation: None,
        state: JournalState::Accepted,
    };
    transporte::write_private(&journal_path, &serde_json::to_vec(&journal)?)?;
    let profile = dir.join("codex");
    let query = query_account(&config.manifest, &profile);
    tokio::pin!(query);
    let observation = loop {
        let remaining = lease.remaining()?;
        tokio::select! {
            result = &mut query => break result,
            () = tokio::time::sleep(remaining) => { anyhow::bail!("inspect_expired"); },
            frame = p::read::<_,p::Envelope>(socket) => {
                let payload = codec.open(&frame?,op)?;
                match payload {
                    p::Payload::Cancel {} => {
                        journal.state = JournalState::CancelledUncertain;
                        replace_journal(&journal_path,&journal)?;
                        p::write(socket,&codec.seal(op,&p::Payload::Cancelled {})?).await?;
                        return Ok(());
                    },
                    p::Payload::Status {} => {
                        let response=completed_result(&journal)?;
                        p::write(socket,&codec.seal(op,&response)?).await?;
                    },
                    _ => anyhow::bail!("runner_command_not_admitted"),
                }
            }
        }
    };
    lease.remaining()?;
    journal.observation = Some(observation.clone());
    journal.state = JournalState::Complete;
    replace_journal(&journal_path, &journal)?;
    p::write(socket, &codec.seal(op, &p::Payload::Result { observation })?).await?;
    Ok(())
}
struct SocketProbe {
    path: PathBuf,
    ready: bool,
}
fn completed_result(journal: &Journal) -> Result<p::Payload> {
    ensure!(matches!(journal.state, JournalState::Complete), "inspection_result_uncertain");
    let observation = journal.observation.clone().ok_or_else(|| anyhow!("inspection_result_uncertain"))?;
    Ok(p::Payload::Result { observation })
}
pub(super) fn status_from_journal(path: &Path, operation: uuid::Uuid) -> Result<p::Payload> {
    let journal: Journal = serde_json::from_slice(&bounded_file(path, 64 * 1024)?)?;
    ensure!(journal.operation == operation.to_string(), "operation_invalid");
    completed_result(&journal)
}
impl SocketProbe {
    fn prepare(dir: &Path, op: uuid::Uuid) -> Result<Self> {
        require_ssh_user()?;
        transporte::private_root(&dir.join("probes"))?;
        let path = dir.join("probes").join(op.to_string());
        transporte::private_create(&path)?;
        let mut probe = Self { path, ready: false };
        #[cfg(unix)]
        {
            let socket = probe.path.join("listen.sock");
            let listener = std::os::unix::net::UnixListener::bind(&socket)?;
            drop(listener);
            std::fs::remove_file(&socket)?;
            probe.ready = true;
        }
        Ok(probe)
    }
}
#[cfg(unix)]
pub(super) fn probe_uid_matches(effective_uid: libc::uid_t, ssh_uid: Option<libc::uid_t>) -> bool {
    effective_uid != 0 && ssh_uid == Some(effective_uid)
}
fn require_ssh_user() -> Result<()> {
    #[cfg(unix)]
    {
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut found = std::ptr::null_mut();
        let mut buffer = vec![0_u8; 64 * 1024];
        // SAFETY: buffers válidos durante a chamada; getpwnam_r preenche entry e
        // found sem reter ponteiros. Nome fixo da mesma conta usada pelo SSH.
        let status = unsafe {
            libc::getpwnam_r(
                c"donna-runner".as_ptr(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &raw mut found,
            )
        };
        ensure!(status == 0 && !found.is_null(), "ssh_user_unavailable");
        // SAFETY: getpwnam_r teve sucesso e retornou entry inicializado.
        let ssh_uid = unsafe { entry.assume_init() }.pw_uid;
        // SAFETY: geteuid não recebe ponteiros nem modifica estado do processo.
        let effective_uid = unsafe { libc::geteuid() };
        ensure!(probe_uid_matches(effective_uid, Some(ssh_uid)), "ssh_user_mismatch");
        Ok(())
    }
    #[cfg(not(unix))]
    anyhow::bail!("platform_not_admitted")
}
impl Drop for SocketProbe {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn replace_journal(path: &Path, journal: &Journal) -> Result<()> {
    let pending = path.with_extension(format!("{}.pending", uuid::Uuid::new_v4()));
    transporte::write_private(&pending, &serde_json::to_vec(journal)?)?;
    std::fs::rename(pending, path)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub fn account_state(response: &serde_json::Value) -> AccountState {
    let Some(account) = response.get("account") else {
        return AccountState::Indisponivel;
    };
    if account.is_null() {
        return AccountState::Desconectado;
    }
    match account["type"].as_str() {
        Some("apiKey") => AccountState::ApiNaoAdmitida,
        Some("chatgpt")
            if matches!(
                account["planType"].as_str(),
                Some("plus" | "pro" | "team" | "business" | "enterprise" | "edu")
            ) =>
        {
            AccountState::AssinaturaOficial
        }
        _ => AccountState::NaoVerificado,
    }
}
async fn query_account(manifest: &Manifest, profile: &Path) -> Observation {
    let mut observation = Observation {
        connection: Axis::Conferido,
        identity: Axis::Conferido,
        preparation: Axis::Conferido,
        account: AccountState::Indisponivel,
        helpers: Unavailable::Indisponivel,
    };
    let supervisor = std::fs::read_to_string("/proc/self/cgroup")
        .is_ok_and(|s| s.lines().any(|l| l.ends_with("/donna-runner.service")));
    if !supervisor {
        observation.preparation = Axis::Indisponivel;
        return observation;
    }
    if let Driver::CodexAccountV1 { sha256, .. } = &manifest.driver {
        // Caminho fixo, nenhuma descoberta de CLI/perfil do Hub e nenhum prefixo/env livre.
        if let Ok(account) = official_query(Path::new("/opt/donna-runner/bin/codex"), profile, sha256).await {
            observation.account = account;
        }
    }
    observation
}
pub(super) async fn official_query(executable: &Path, profile: &Path, digest: &str) -> Result<AccountState> {
    let meta = std::fs::symlink_metadata(executable)?;
    ensure!(meta.is_file() && !meta.file_type().is_symlink(), "driver_invalid");
    transporte::private_root(profile)?;
    ensure!(file_digest(executable)? == digest, "driver_digest_mismatch");
    let mut child = crate::fork::motor_exec::spawn(
        crate::fork::motor_exec::command(executable, profile)
            .arg("app-server")
            .env_clear()
            .env("HOME", profile)
            .env("CODEX_HOME", profile)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(crate::fork::motor_exec::hub_cwd(profile))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true),
    )?;
    let result = tokio::time::timeout(Duration::from_secs(10),async {
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("driver_pipe"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("driver_pipe"))?;
        // Limite TOTAL de saída, inclusive notificações; nada cru é projetado.
        let mut out = BufReader::new(stdout.take(64*1024+1));
        stdin.write_all(b"{\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"donna-runner-inspect\",\"version\":\"1\"}}}\n").await?;
        rpc_result(&mut out,1).await?;
        stdin.write_all(b"{\"method\":\"initialized\"}\n{\"id\":2,\"method\":\"account/read\",\"params\":{\"refreshToken\":false}}\n").await?;
        Ok::<_,anyhow::Error>(account_state(&rpc_result(&mut out,2).await?))
    }).await;
    child.kill().await?;
    result.map_err(|_| anyhow!("driver_timeout"))?
}
pub(super) fn file_digest(path: &Path) -> Result<String> {
    use sha2::Digest as _;
    use std::io::Read as _;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(digest, "{byte:02x}")?;
    }
    Ok(digest)
}
async fn rpc_result<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R, id: u32) -> Result<serde_json::Value> {
    for _ in 0..32 {
        let mut line = String::new();
        ensure!(r.read_line(&mut line).await? > 0 && line.len() <= 64 * 1024, "driver_output_invalid");
        let v: serde_json::Value = serde_json::from_str(&line)?;
        if v["id"] == id {
            ensure!(v.get("error").is_none(), "driver_query_failed");
            return v.get("result").cloned().ok_or_else(|| anyhow!("driver_result_missing"));
        }
    }
    anyhow::bail!("driver_output_limit")
}

#[cfg(test)]
#[test]
fn maintenance_lock_waits_inherited_copy_and_refuses_running_runner() {
    let dir = std::env::temp_dir().join(format!("onde-roda-lock-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    crate::assert_flock_waits(|| maintenance_lock(&dir), "runner_must_be_stopped");
    std::fs::remove_dir_all(dir).unwrap();
}
