//! Canal pessoal estritamente probe. Não cria Caller e não alcança `api::dispatch`.
use super::{live, protocol, sync};
use crate::{
    hub::Hub,
    remote::{
        channel::{Out, Transport},
        crypto,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::time::{Instant, timeout_at};
use x25519_dalek::StaticSecret;
#[derive(Clone)]
pub(crate) struct PersonalRelayAdmission {
    record: live::Record,
    epoch: u64,
    link: String,
    dk: String,
    deadline: Instant,
}
impl PersonalRelayAdmission {
    // Chamado apenas pelo supervisor do socket host pessoal autenticado.
    pub(super) fn from_open(hub: &Hub, epoch: u64, link: &str, dk: &str) -> Result<Self> {
        ensure!(!link.is_empty() && link.len() <= 128 && crypto::unb64_n::<32>(dk).is_ok(), "personal open invalid");
        let record = sync::guard(hub, Some(epoch), true, |r| {
            ensure!(r.binding.app == dk, "personal open key");
            Ok(r.clone())
        })?;
        Ok(Self { record, epoch, link: link.into(), dk: dk.into(), deadline: Instant::now() + Duration::from_secs(5) })
    }
    pub(super) fn recheck(&self, hub: &Hub) -> Result<()> {
        ensure!(Instant::now() < self.deadline, "personal probe expired");
        sync::guard(hub, Some(self.epoch), true, |r| {
            ensure!(
                r.sequence == self.record.sequence
                    && r.activation == self.record.activation
                    && r.binding == self.record.binding
                    && self.dk == r.binding.app
                    && !self.link.is_empty(),
                "personal admission replaced"
            );
            Ok(())
        })
    }
}
struct ProbePermit {
    admission: PersonalRelayAdmission,
    deadline: Instant,
}
impl ProbePermit {
    fn recheck(&self, hub: &Hub) -> Result<()> {
        ensure!(Instant::now() < self.deadline, "personal probe expired");
        self.admission.recheck(hub)
    }
    async fn send(
        &self,
        hub: &Hub,
        out: &tokio::sync::mpsc::Sender<Out>,
        value: Value,
        sealer: Option<&mut crypto::Sealer>,
    ) -> Result<()> {
        self.recheck(hub)?;
        // Reserva após a fila; guarda final imediatamente antes de selar e enviar.
        let permit = timeout_at(self.deadline, out.reserve()).await??;
        self.recheck(hub)?;
        if let Some(sealer) = sealer {
            let frames = sealer.seal(&serde_json::to_vec(&value)?).context("personal rekey")?;
            ensure!(frames.len() == 1, "personal fragmented response");
            self.recheck(hub)?;
            permit.send(Out::Msg(frames.into_iter().next().context("empty personal response")?));
        } else {
            permit.send(Out::Msg(value));
        }
        Ok(())
    }
}
async fn serve(hub: &Hub, t: &mut Transport, admission: PersonalRelayAdmission) -> Result<()> {
    let deadline = admission.deadline;
    let permit = ProbePermit { admission, deadline };
    permit.recheck(hub)?;
    let hello = timeout_at(permit.deadline, t.incoming.recv()).await?.context("personal hello missing")?;
    ensure!(
        hello
            .as_object()
            .is_some_and(|m| m.len() == 6 && ["t", "v", "dk", "ek", "n", "sig"].iter().all(|k| m.contains_key(*k)))
            && hello["t"] == "hello"
            && hello["v"] == 1
            && hello["dk"] == permit.admission.dk,
        "personal hello invalid"
    );
    let key = crypto::unb64_n::<32>(&permit.admission.dk)?;
    let ek = crypto::unb64_n::<32>(hello["ek"].as_str().context("personal ek")?)?;
    let nonce = crypto::unb64_n::<32>(hello["n"].as_str().context("personal n")?)?;
    let sig = crypto::unb64_n::<64>(hello["sig"].as_str().context("personal sig")?)?;
    crypto::verify(&key, &crypto::hs1_input(&hub.identity.cid_raw(), &key, &ek, &nonce), &sig)?;
    permit.recheck(hub)?;
    let ephemeral = StaticSecret::from(crypto::random::<32>());
    let public = crypto::x25519_pub(&ephemeral);
    let th = crypto::transcript_hash(&hub.identity.cid_raw(), &key, &ek, &nonce, &public);
    let keys = crypto::channel_keys(&crypto::x25519(&ephemeral, &ek)?, &th);
    permit
        .send(
            hub,
            &t.outgoing,
            json!({"t":"welcome","v":1,"ek":crypto::b64(&public),"sig":crypto::b64(&hub.identity.sign(&th))}),
            None,
        )
        .await?;
    let mut opener = crypto::Opener::new(keys.d2h, 1024);
    let mut sealer = crypto::Sealer::new(keys.h2d);
    for method in ["hello", "donnaPersonalSessionProof"] {
        let frame = timeout_at(permit.deadline, t.incoming.recv()).await?.context("personal frame missing")?;
        permit.recheck(hub)?;
        let raw = opener
            .open(&frame)
            .map_err(|_| anyhow::anyhow!("personal ciphertext invalid"))?
            .context("personal fragmented request")?;
        ensure!(raw.len() <= 1024, "personal RPC too large");
        let request: Value = serde_json::from_slice(&raw)?;
        ensure!(
            request.as_object().is_some_and(|m| m.len() == 3 && ["id", "m", "b"].iter().all(|k| m.contains_key(*k)))
                && request["id"].as_u64().is_some_and(|id| u32::try_from(id).is_ok())
                && request["m"] == method,
            "personal RPC refused"
        );
        let b = &permit.admission.record.binding;
        let result = if method == "hello" {
            ensure!(request["b"].as_object().is_some_and(serde_json::Map::is_empty), "personal hello body");
            json!({"hostId":hub.host_id,"computerId":b.computer,"signKey":b.sign,"boxKey":b.box_key,"name":crate::service::host_name(),"version":env!("CARGO_PKG_VERSION"),"minApp":crate::compat::MIN_APP})
        } else {
            ensure!(
                request["b"].as_object().is_some_and(|b| b.len() == 1 && b.contains_key("nonce")),
                "personal proof body"
            );
            let nonce = request["b"]["nonce"].as_str().context("personal proof nonce")?;
            let bytes = crypto::unb64_n::<32>(nonce)?;
            ensure!(crypto::b64(&bytes) == nonce, "personal nonce canonical");
            let r = &permit.admission.record;
            json!({"nonce":nonce,"profile":"personal-linux-v1","C":b.context,"hT":b.h_t,"computerId":b.computer,"hubGeneration":b.generation,"grantId":b.grant,"hostStateRequest":protocol::canonical(&r.request)?,"hostState":protocol::canonical(&r.snapshot)?,"publication":r.originals["publish"],"activation":protocol::canonical(&r.activation)?})
        };
        ensure!(serde_json::to_vec(&result)?.len() <= 80 * 1024, "personal proof too large");
        permit.send(hub, &t.outgoing, json!({"id":request["id"],"ok":result}), Some(&mut sealer)).await?;
    }
    // As duas RPCs não renovam a admissão. O Main conserva este MESMO probe
    // enquanto publica o PIN, até peerclose ou o deadline original de5s.
    // Nenhum frame adicional entra em dispatch ou produz outro efeito.
    permit.recheck(hub)?;
    let extra = timeout_at(permit.deadline, t.incoming.recv()).await?;
    permit.recheck(hub)?;
    ensure!(extra.is_none(), "personal probe extra frame refused");
    Ok(())
}
pub(crate) async fn run(hub: Arc<Hub>, mut transport: Transport, admission: PersonalRelayAdmission) {
    let _ = serve(&hub, &mut transport, admission).await;
    let _ = transport.outgoing.try_send(Out::Close(1000, "personal probe complete"));
}
