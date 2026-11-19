//! Supervisor pessoal: receipt/read/E/V e relay use sem registro, pull, hooks ou mailbox.
use super::{ceremony, probe::PersonalRelayAdmission, protocol, sync, transport, wire};
use crate::{
    LockExt,
    hub::Hub,
    remote::{
        channel::{self, ChannelKind, Out, Transport},
        crypto,
    },
};
use anyhow::{Context, Result, ensure};
use futures::{Sink, SinkExt, StreamExt};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

fn capture(hub: &Hub) -> Result<Option<String>> {
    let db = hub.store.db.locked();
    Ok(db.query_row("SELECT capture FROM personal_possession_slot WHERE singleton=1", [], |r| r.get(0)).optional()?)
}
pub(crate) async fn run(hub: Arc<Hub>) {
    let _ = sync::restore(&hub);
    loop {
        if transport::time(&hub).await.is_err() {
            sync::cut(&hub);
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        if let Ok(Some(id)) = capture(&hub) {
            match sync::cycle(&hub, &id).await {
                Ok(true) => {
                    let _ = session(&hub, &id).await;
                }
                Ok(false) => {}
                Err(_) => sync::cut(&hub),
            }
        }
        sync::cut(&hub);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
pub(super) fn open_request(
    hub: &Hub,
    record: &super::live::Record,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request> {
    let path = "/v1/personal-relay/host";
    let (cfg, at, upper) = hub.personal.with(|f| Ok((f.config.clone(), f.clock.issued()?, f.clock.validity()?)))?;
    let b = &record.binding;
    let until = at
        .checked_add(30_000)
        .context("open overflow")?
        .min(protocol::canonical(&record.activation)?[0][14].as_u64().context("V expiry")?);
    ensure!(upper < until, "open expired");
    let nonce = crypto::b64(&crypto::random::<16>());
    let open = transport::signed(
        hub,
        json!([
            wire::domain("channel-open"),
            cfg.context(),
            "host",
            "use",
            b.computer,
            b.h_t,
            b.grant,
            b.app,
            nonce,
            path,
            transport::nonce(),
            at,
            until
        ]),
    )?;
    let bytes = serde_json::to_vec(&open)?;
    cfg.verifier(&b.app)?.verify_open(
        &bytes,
        wire::Time::Current(upper),
        &wire::OpenRequest { role: wire::Reader::Hub, kid: &b.sign, nonce: &nonce, path, until },
    )?;
    let origin = cfg.context()[3].as_str().context("origin")?;
    let url = reqwest::Url::parse(&format!("{origin}{path}"))?;
    ensure!(url.scheme() == "https" && url.port().is_none_or(|p| p == 443) && url.query().is_none(), "relay origin");
    let authority = url.host_str().context("relay host")?;
    let input = crypto::request_sig_input("GET", authority, path, i64::try_from(at)?, &nonce, b"");
    let mut request = format!("wss://{authority}{path}").into_client_request()?;
    request.headers_mut().insert(
        "Codync-Sig",
        crypto::request_sig_header(&hub.identity.sign, i64::try_from(at)?, &nonce, &input).parse()?,
    );
    request.headers_mut().insert("X-Donna-Personal-Open", crypto::b64(&bytes).parse()?);
    Ok(request)
}
async fn session(hub: &Arc<Hub>, id: &str) -> Result<()> {
    let record = sync::guard(hub, None, true, |r| Ok(r.clone()))?;
    let request = open_request(hub, &record)?;
    let h_v = wire::hash(&protocol::canonical(&record.activation)?[0])?;
    sync::guard(hub, None, true, |r| {
        ensure!(r.activation == record.activation, "V changed before connect");
        Ok(())
    })?;
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(request)).await??;
    sync::guard(hub, None, true, |r| {
        ensure!(r.activation == record.activation, "V changed during connect");
        Ok(())
    })?;
    ws.send(Message::Text(json!({"t":"ready"}).to_string().into())).await?;
    let epoch = sync::socket(hub)?;
    let mut links: HashMap<String, (mpsc::Sender<Value>, PersonalRelayAdmission)> = HashMap::new();
    let (out, mut outgoing) = mpsc::channel::<(String, Out)>(32);
    let mut tasks = tokio::task::JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    let mut quiet = tokio::time::Instant::now();
    let result=async {
        loop {
            sync::guard(hub,Some(epoch),true,|r|{ensure!(wire::hash(&protocol::canonical(&r.activation)?[0])?==h_v,"V replaced");Ok(())})?;
            tokio::select!{
                _=tick.tick()=>{
                    ensure!(quiet.elapsed()<Duration::from_secs(85),"personal relay silent");
                    // Rechecagem de Worker e cercas, poll H não pode ser menor que 5s.
                    let _=ceremony::read_state(hub,id).await?;
                    sync::guard(hub,Some(epoch),true,|_|Ok(()))?;
                    let renewal=hub.personal.with(|f|{let a=f.applied.as_ref().context("snapshot absent")?;Ok(a.lease.until()?.saturating_sub(f.clock.validity()?)<=60_000)})?;
                    if renewal {return Ok(());}
                    ws.send(Message::Text(json!({"t":"ping"}).to_string().into())).await?;
                }
                message=ws.next()=>{
                    let message=message.context("personal relay ended")??;
                    let Message::Text(raw)=message else {anyhow::bail!("personal relay frame");};
                    ensure!(raw.len()<=1024*1024,"personal relay message too large");let v:Value=serde_json::from_str(&raw)?;quiet=tokio::time::Instant::now();
                    match v["t"].as_str(){
                        Some("pong")=>{ensure!(v.as_object().is_some_and(|v|v.len()==1),"pong fields");}
                        Some("open")=>{
                            ensure!(v.as_object().is_some_and(|v|v.len()==4) && v["pair"]==false && links.len()<8,"personal open fields");let link=v["link"].as_str().context("link")?.to_owned();let dk=v["dk"].as_str().context("device")?;
                            ensure!(!links.contains_key(&link),"personal link duplicate");let admission=PersonalRelayAdmission::from_open(hub,epoch,&link,dk)?;
                            let(tx,rx)=mpsc::channel(8);let(outgoing,mut rx_out)=mpsc::channel(2);links.insert(link.clone(),(tx,admission.clone()));
                            let h=hub.clone();tasks.spawn(async move{channel::run(h,Transport{incoming:rx,outgoing},ChannelKind::PersonalRelay{admission:Box::new(admission)}).await;});
                            let out=out.clone();tasks.spawn(async move{while let Some(item)=rx_out.recv().await{if out.send((link.clone(),item)).await.is_err(){break;}}});
                        }
                        Some("data")=>{ensure!(v.as_object().is_some_and(|v|v.len()==3),"data fields");let key=v["link"].as_str().context("link")?;if let Some((tx,admission))=links.get(key){admission.recheck(hub)?;tx.try_send(v["m"].clone()).context("personal link busy")?;}}
                        Some("close")=>{let link=v["link"].as_str().context("link")?;links.remove(link);}
                        _=>anyhow::bail!("personal relay operation refused"),
                    }
                }
                Some((link,item))=outgoing.recv()=>{
                    sync::guard(hub,Some(epoch),true,|_|Ok(()))?;
                    let value=match item{Out::Msg(m)=>{let Some((_,admission))=links.get(&link) else{continue;};admission.recheck(hub)?;json!({"t":"data","link":link,"m":m})},Out::Close(code,reason)=>{links.remove(&link);json!({"t":"close","link":link,"code":code,"reason":reason})}};
                    // Sink fila é limitada; o V/lease é rechecado depois de ready() e antes de start_send.
                    futures::future::poll_fn(|cx| std::pin::Pin::new(&mut ws).poll_ready(cx)).await?;
                    sync::guard(hub,Some(epoch),true,|_|Ok(()))?;
                    if value["t"]=="data" {let Some((_,admission))=links.get(&link) else{continue;};admission.recheck(hub)?;}
                    Sink::start_send(std::pin::Pin::new(&mut ws),Message::Text(value.to_string().into()))?;
                    ws.flush().await?;
                }
            }
        }
    }.await;
    sync::cut(hub);
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    let _ = ws.close(None).await;
    result
}
