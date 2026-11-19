use super::{
    api,
    autoridade::Authority,
    protocolo as p, store,
    tipos::{Axis, Observation, PURPOSE, Phase},
    transporte::Transport,
};
use crate::{hub::Hub, market::vault};
use anyhow::{Result, anyhow, ensure};
use ed25519_dalek::SigningKey;
use rusqlite::OptionalExtension as _;
use std::{sync::Arc, time::Duration};
use tokio::io::AsyncReadExt as _;
use zeroize::Zeroizing;

pub fn spawn(hub: Arc<Hub>, authority: Authority, id: String) {
    tokio::spawn(async move {
        let work = run(&hub, &authority, &id);
        tokio::pin!(work);
        let result = tokio::select! {
            result = &mut work => result,
            () = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if store::live(&hub.store,&authority,&id).is_err() { break; }
                }
            } => Err(anyhow!("operation_invalidated")),
            () = tokio::time::sleep(Duration::from_secs(60)) => Err(anyhow!("inspection_timeout")),
        };
        // Nenhum erro bruto, endereço, conteúdo de conta ou segredo no DB/log/renderer.
        let _ = store::transaction(&hub.store, |db| {
            let mut op = store::operation(db, &id)?;
            let mut place = store::get(db, &op.place_id)?;
            if op.phase != Phase::Verificando
                || authority.target(&place).is_err()
                || op.admission_revision != place.admission_revision
                || op.connection_revision != place.connection_revision
            {
                return Ok(());
            }
            if op.deadline <= crate::store::now_ms() {
                op.phase = Phase::InterrompidaIncerta;
                op.reason = Some("inspection_timeout".into());
                return store::save_operation(db, &op);
            }
            if let Ok(observation) = result {
                op.phase = Phase::Concluida;
                op.observation = observation;
                op.observed_at = Some(crate::store::now_ms());
                place.capacity_revision += 1;
                store::save(db, &place)?;
            } else {
                op.phase = Phase::Falhou;
                op.reason = Some("inspection_not_proven".into());
            }
            store::save_operation(db, &op)
        });
    });
}
async fn run(hub: &Arc<Hub>, a: &Authority, id: &str) -> Result<Observation> {
    store::live(&hub.store, a, id)?;
    let runtime_root = store::runtime_root(&hub.store)?;
    vault::unlock(hub.clone()).await.map_err(|_| anyhow!("vault_unavailable"))?;
    let (place, op, ssh_raw, control_raw) = store::transaction(&hub.store, |db| {
        let op = store::operation(db, id)?;
        let place = store::get(db, &op.place_id)?;
        a.operation(&place, &op)?;
        let read = |kind| -> Result<String> {
            db.query_row("SELECT v FROM kv WHERE k=?", [api::slot(&place.id, kind)?], |r| r.get(0))
                .optional()?
                .ok_or_else(|| anyhow!("prepare_required"))
        };
        let ssh = read("ssh")?;
        let control = read("control")?;
        Ok((place, op, ssh, control))
    })?;
    let ssh = Zeroizing::new(vault::open_ciphertext(&hub.store, &api::slot(&place.id, "ssh")?, &ssh_raw)?);
    let seed = Zeroizing::new(vault::open_ciphertext(&hub.store, &api::slot(&place.id, "control")?, &control_raw)?);
    let seed = Zeroizing::new(p::fixed::<32>(&seed)?);
    let key = SigningKey::from_bytes(&seed);
    ensure!(
        place.control_public.as_deref() == Some(p::b64(key.verifying_key().as_bytes()).as_str()),
        "control_key_mismatch"
    );
    let check = || store::live(&hub.store, a, id).map(|_| ());
    check()?;
    let transport = Transport::new(
        &runtime_root,
        place.connection.as_ref().ok_or_else(|| anyhow!("connection_required"))?,
        &place.resource_id,
        &ssh,
    )?;
    transport.probes(&check).await?;
    check()?;
    let mut child = transport.connect()?;
    let mut input = child.stdin.take().ok_or_else(|| anyhow!("ssh_pipe"))?;
    let mut output = child.stdout.take().ok_or_else(|| anyhow!("ssh_pipe"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("ssh_pipe"))?;
    let exchange = async {
        let pin = place.runner_pin.as_ref().ok_or_else(|| anyhow!("runner_pin_required"))?;
        let peer = p::fixed::<32>(pin)?;
        let hello = p::Hello {
            protocol: 1,
            hub_id: hub.host_id.clone(),
            place_id: place.id.clone(),
            runner_id: p::b64(&p::hash(&peer)),
            resource_id: place.resource_id.clone(),
            control_key: p::b64(key.verifying_key().as_bytes()),
            nonce: p::b64(&p::random::<32>()?),
        };
        check()?;
        p::write(&mut input, &hello).await?;
        let welcome: p::Welcome = p::read(&mut output).await?;
        check()?;
        ensure!(
            welcome.runner_key == *pin && place.manifest.as_ref() == Some(&welcome.manifest),
            "runner_incompatible"
        );
        let channel = p::hash(&p::transcript(&hello, &welcome)?);
        p::verify(&peer, &p::hello_input(p::Direction::Runner, &channel)?, &welcome.signature)?;
        let confirm = p::Confirm { signature: p::signature(&key, &p::hello_input(p::Direction::Control, &channel)?) };
        check()?;
        p::write(&mut input, &confirm).await?;
        let mut codec = p::Channel::new(channel, p::Direction::Control, key, peer);
        let operation = uuid::Uuid::parse_str(id)?;
        let intent = uuid::Uuid::parse_str(&op.intent)?;
        let request = p::Payload::ChallengeRequest {
            intent,
            admission_revision: op.admission_revision,
            connection_revision: op.connection_revision,
        };
        check()?;
        p::write(&mut input, &codec.seal(operation, &request)?).await?;
        let e: p::Envelope = p::read(&mut output).await?;
        check()?;
        let p::Payload::LeaseChallenge { intent: observed, nonce, boot_id, generation, socket_probe_ready } =
            codec.open(&e, operation)?
        else {
            anyhow::bail!("challenge_required")
        };
        ensure!(
            observed == intent && boot_id == welcome.boot_id && generation == welcome.generation,
            "challenge_invalid"
        );
        p::fixed::<32>(&nonce)?;
        transport.remote_socket_probe(operation, socket_probe_ready, &check).await?;
        let grant = p::Payload::InspectGrant {
            purpose: PURPOSE.into(),
            intent,
            nonce,
            boot_id,
            generation,
            duration_ms: 60_000,
            admission_revision: op.admission_revision,
            connection_revision: op.connection_revision,
        };
        check()?;
        p::write(&mut input, &codec.seal(operation, &grant)?).await?;
        check()?;
        p::write(&mut input, &codec.seal(operation, &p::Payload::Inspect {})?).await?;
        let e: p::Envelope = p::read(&mut output).await?;
        check()?;
        let p::Payload::Result { mut observation } = codec.open(&e, operation)? else {
            anyhow::bail!("inspect_result_required")
        };
        // Os dois eixos são provas do controlador; não aceitar anúncio do peer como prova SSH.
        observation.connection = Axis::Conferido;
        observation.identity = Axis::Conferido;
        Ok::<_, anyhow::Error>(observation)
    };
    let logs = async {
        let mut buffer = Vec::new();
        stderr.take(64 * 1024 + 1).read_to_end(&mut buffer).await?;
        ensure!(buffer.len() <= 64 * 1024, "ssh_output_limit");
        // EOF antes do resultado não é prova de conexão íntegra.
        anyhow::bail!("ssh_disconnected")
    };
    let result: Result<Observation> = tokio::select! { r=exchange => r, r=logs => r };
    let _ = child.kill().await;
    let _ = child.wait().await;
    check()?;
    result
}
