//! Coordenador H restrito. Não assina fora de transições verificadas/duráveis.
use super::{clock::Clock, enrollment, helper, prepare_wire, protocol, shapes::Kind, transport, wire};
use crate::{
    hub::Hub,
    store::{personal_prepare, personal_session::Session},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use zeroize::Zeroizing;
const NAMES: &[(&str, &str)] = &[
    ("intent", "intent"),
    ("ticket", "local-ticket"),
    ("appConsent", "app-consent"),
    ("localConsent", "local-consent"),
    ("boxChallenge", "box-challenge"),
    ("boxProof", "box-proof"),
    ("approval", "approve"),
    ("localConfirmation", "local-confirm"),
    ("consume", "consume"),
    ("grant", "grant"),
    ("challenge", "challenge"),
    ("proof", "proof"),
    ("receipt", "receipt"),
    ("publish", "publish"),
    ("revocation", "revocation"),
    ("fence", "fence"),
    ("fenceAck", "fence-ack"),
];
pub(super) fn packet(originals: &Value) -> Value {
    let mut map = serde_json::Map::new();
    for (_, name) in NAMES {
        if let Some(v) = originals.get(*name) {
            map.insert((*name).into(), v.clone());
        }
    }
    Value::Object(map)
}
pub(super) fn checked(
    cfg: &super::provisioning::AdmittedConfig,
    originals: &Value,
    now: u64,
) -> Result<wire::Ceremony> {
    check_at(cfg, originals, wire::Time::Current(now))
}
pub(super) fn history(
    cfg: &super::provisioning::AdmittedConfig,
    originals: &Value,
    now: u64,
) -> Result<wire::Ceremony> {
    check_at(cfg, originals, wire::Time::Historical(now))
}
fn check_at(cfg: &super::provisioning::AdmittedConfig, originals: &Value, at: wire::Time) -> Result<wire::Ceremony> {
    let app = originals["intent"][0][6].as_str().context("personal intent missing")?;
    wire::ceremony(&cfg.verifier(app)?, &serde_json::to_vec(&packet(originals))?, at)
}
pub(super) fn step<T>(
    hub: &Hub,
    id: &str,
    action: impl FnOnce(&super::provisioning::AdmittedConfig, &mut Clock, &mut Session<'_>) -> Result<T>,
) -> Result<T> {
    hub.personal.with(|f| {
        f.config.matches(&hub.identity)?;
        f.clock.validity()?;
        hub.store.personal_session(&f.config, id, |s| {
            let result = action(&f.config, &mut f.clock, s)?;
            f.clock.validity()?;
            Ok(result)
        })
    })
}
/// Espera causal curta fora de Cloud/runtime/DB. Não fixa original antes de alcançar o predecessor.
pub(super) async fn wait_causal(hub: &Hub, predecessor: u64, until: u64) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let reached = hub.personal.with(|f| {
            let w = f.clock.window()?;
            ensure!(w.upper < until, "personal causal deadline expired");
            Ok(w.lower >= predecessor)
        })?;
        if reached {
            return Ok(());
        }
        ensure!(tokio::time::Instant::now() < deadline, "personal causal time unavailable");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
fn ticket(hub: &Hub, id: &str, mode: &str, intent: &Value) -> Result<Value> {
    step(hub, id, |cfg, clock, s| {
        if let Some(q) = s.originals().get("local-ticket") {
            ensure!(
                q[0][3] == mode && (mode == "device" || s.originals().get("intent") == Some(intent)),
                "personal ticket conflict"
            );
            return Ok(q.clone());
        }
        s.preparing()?;
        ensure!(mode == "command" || mode == "device", "invalid personal mode");
        let valid_now = clock.validity()?;
        let now = clock.issued()?;
        let (intent_id, until) = if mode == "command" {
            let app = intent[0][6].as_str().context("invalid intent")?;
            let i = cfg.verifier(app)?.verify(
                Kind::Intent,
                &serde_json::to_vec(intent)?,
                wire::Time::Current(valid_now),
                wire::Reader::Hub,
            )?;
            ensure!(i.payload()[3] == "command", "invalid command intent");
            s.freeze("intent", intent.clone())?;
            (i.payload()[2].clone(), i.payload()[9].as_u64().context("intent expiry")?.min(now + 600_000))
        } else {
            ensure!(intent.is_null(), "device ticket must not select app");
            (json!(format!("pi_{}", transport::nonce())), now + 600_000)
        };
        let predecessor = if mode == "command" { intent[0][8].as_u64().context("I time")? } else { 0 };
        let now = clock.issued_after(predecessor, until)?;
        let i = cfg.identity();
        let q = transport::signed(
            hub,
            json!([
                wire::domain("local-ticket"),
                cfg.context(),
                intent_id,
                mode,
                format!("pc_{}", transport::nonce()),
                i.generation,
                i.computer_id,
                i.sign_key,
                i.box_key,
                transport::nonce(),
                "first-owner",
                now,
                until
            ]),
        )?;
        s.freeze("local-ticket", q.clone())?;
        personal_prepare::register(
            s.db,
            s.id(),
            &prepare_wire::capture(cfg.context(), &q)?,
            &wire::hash(&q[0])?,
            &protocol::hash(&serde_json::to_vec(&q)?),
        )?;
        Ok(q)
    })
}
async fn effect(hub: &Hub, id: &str, route: &str, body: Value, marker: &str) -> Result<Value> {
    step(hub, id, |_, _, s| {
        s.preparing()?;
        ensure!(s.originals().get(format!("_{marker}")).is_none(), "personal send outcome unknown");
        s.private(marker, body.clone())
    })?;
    transport::post(hub, route, body).await
}
async fn begin(hub: &Hub, id: &str, q: &Value) -> Result<()> {
    let (cfg, b, known) = step(hub, id, |cfg, clock, s| {
        let b = if let Some(old) = s.originals().get("_begin") {
            old.clone()
        } else {
            let p = &q[0];
            let b = transport::signed(
                hub,
                json!([
                    wire::domain("begin"),
                    cfg.context(),
                    p[2],
                    p[4],
                    wire::hash(p)?,
                    p[9],
                    clock.issued_after(p[11].as_u64().context("Q time")?, p[12].as_u64().context("Q expiry")?)?,
                    p[12]
                ]),
            )?;
            s.private("begin", b.clone())?;
            b
        };
        let capture = prepare_wire::capture(cfg.context(), q)?;
        Ok((cfg.clone(), b, personal_prepare::enrollment(s.db, &capture)?.is_some()))
    })?;
    if known {
        return Ok(());
    }
    let body = json!({"begin":b,"ticket":q});
    step(hub, id, |_, _, s| {
        s.preparing()?;
        ensure!(s.originals().get("_sentBegin").is_none(), "begin outcome unknown");
        s.private("sentBegin", body.clone())
    })?;
    let response = transport::post_bytes(hub, "device", &serde_json::to_vec(&body)?).await?;
    let raw = response.original;
    let binding = enrollment::Binding::new(&cfg, q, &b)?;
    let (facts, sealed) = binding.seal(&hub.identity, &raw)?;
    // KDF/cifra fora de Store/Cloud; somente persistência e rechecagens na Connection.
    step(hub, id, |current, clock, s| {
        s.preparing()?;
        ensure!(
            current.original_bytes() == cfg.original_bytes()
                && s.originals()["local-ticket"] == *q
                && s.originals()["_begin"] == b
                && s.originals().get("_sentBegin") == Some(&body),
            "begin binding changed"
        );
        ensure!(
            facts[5].as_u64().context("begin time")? <= clock.validity()?.saturating_add(30_000),
            "future begin response"
        );
        personal_prepare::retain_enrollment(s.db, &binding.capture, &facts, &sealed)
    })
}
fn read_credential(hub: &Hub, id: &str) -> Result<Option<Zeroizing<String>>> {
    let value = step(hub, id, |cfg, _, s| {
        let q = &s.originals()["local-ticket"];
        if q[0][3] != "device" {
            return Ok(None);
        }
        let binding = enrollment::Binding::new(cfg, q, &s.originals()["_begin"])?;
        let (facts, sealed) =
            personal_prepare::enrollment(s.db, &binding.capture)?.context("device read secret unavailable")?;
        Ok(Some((binding, facts, sealed)))
    })?;
    value
        .map(|(binding, facts, sealed)| {
            let plain = binding.open(&hub.identity, &sealed, &facts)?;
            let body = protocol::canonical(&plain)?;
            Ok(Zeroizing::new(body["deviceCode"].as_str().context("missing device code")?.to_owned()))
        })
        .transpose()
}
/// S do próprio read: escopo, nonce e janela do pedido feito por H (não reclassifica como A).
fn checked_read(
    cfg: &super::provisioning::AdmittedConfig,
    s: &Session<'_>,
    now: u64,
    request: &Value,
    reply: &Value,
) -> Result<wire::Signed> {
    let app = s.originals()["intent"][0][6].as_str().context("intent missing")?;
    let state = cfg.verifier(app)?.verify(
        Kind::State,
        &serde_json::to_vec(&reply["state"])?,
        wire::Time::Current(now),
        wire::Reader::Hub,
    )?;
    let rp = &request["read"][0];
    let sp = state.payload();
    let sent = rp[6].as_u64().context("read time")?;
    let until = rp[7].as_u64().context("read expiry")?;
    ensure!(
        sp[2] == rp[2]
            && sp[3] == rp[3]
            && sp[4] == rp[4]
            && sp[14] == rp[5]
            && now < until
            && sp[15]
                .as_u64()
                .is_some_and(|at| at >= sent.saturating_sub(30_000) && at <= until && at <= now.saturating_add(30_000)),
        "state request scope/nonce/window mismatch"
    );
    Ok(state)
}
/// HOST-R1-01: consumo autenticado pela raiz (consumedAt da linha D1) para o predecessor do PRIMEIRO K. Transitório:
/// não persiste, não vira bool; o orçamento de 5 s vale do envio do read até a guarda que congela K.
pub(super) struct Witness {
    pub(super) consumed_at: u64,
    start: (String, u64),
}
impl Witness {
    pub(super) fn current(&self) -> Result<()> {
        let (boot, now) = Clock::monotonic()?;
        ensure!(
            boot == self.start.0 && now.checked_sub(self.start.1).is_some_and(|n| n < 5000),
            "consumption witness budget expired"
        );
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn fixture(consumed_at: u64) -> Result<Self> {
        Ok(Self { consumed_at, start: Clock::monotonic()? })
    }
    #[cfg(test)]
    pub(super) fn fixture_aged(consumed_at: u64, age_ms: u64) -> Result<Self> {
        let (boot, now) = Clock::monotonic()?;
        Ok(Self { consumed_at, start: (boot, now.checked_sub(age_ms).context("fixture age")?) })
    }
}
/// Um único read (nonce novo, sem `_lastPoll`, sem retry) depois de G verificado e antes do primeiro K.
pub(super) async fn consumption_witness(hub: &Hub, id: &str) -> Result<Witness> {
    let secret = read_credential(hub, id)?;
    let request = step(hub, id, |cfg, clock, s| {
        ensure!(
            !s.closed() && !s.fenced() && s.originals().get("challenge").is_none(),
            "personal witness not applicable"
        );
        let now = clock.issued()?;
        let c = history(cfg, s.originals(), now)?;
        let t = c.tuple();
        let read = transport::signed(
            hub,
            json!([wire::domain("read"), cfg.context(), t[1], t[9], c.digest(), transport::nonce(), now, now + 30_000]),
        )?;
        let mut body = json!({"read":read,"witness":true});
        if let Some(code) = &secret {
            body["deviceCode"] = json!(code.as_str());
        }
        Ok(body)
    })?;
    let start = Clock::monotonic()?;
    let reply = transport::post(hub, "read", request.clone()).await?;
    let consumed_at = step(hub, id, |cfg, clock, s| {
        let now = clock.validity()?;
        let state = checked_read(cfg, s, now, &request, &reply)?;
        let sp = state.payload();
        let c = checked(cfg, s.originals(), now)?;
        let g = c.original(Kind::Grant)?;
        ensure!(
            sp[5] == "personalConfirmed"
                && sp[7].is_null()
                && sp[8] == json!(g.digest())
                && (9..=13).all(|i| sp[i].is_null())
                && reply["originals"]["grant"] == s.originals()["grant"],
            "personal witness state mismatch"
        );
        let nonce = request["read"][0][5].as_str().context("read nonce")?;
        let w = cfg.verify_witness(&serde_json::to_vec(&reply["consumptionWitness"])?, nonce)?;
        let p = w.payload();
        ensure!(p[2] == json!(c.digest()) && p[3] == json!(g.digest()), "consumption witness of another G/T");
        p[6].as_u64().context("consumedAt")
    })?;
    let w = Witness { consumed_at, start };
    w.current()?;
    Ok(w)
}
/// Consulta só originais do próprio candidato. None significa poll ainda não devido.
pub(super) async fn read_state(hub: &Hub, id: &str) -> Result<Option<Value>> {
    if step(hub, id, |_, _, s| Ok(s.closed() || s.originals().get("intent").is_none()))? {
        return Ok(None);
    }
    let secret = read_credential(hub, id)?;
    let request = step(hub, id, |cfg, clock, s| {
        if s.originals().get("intent").is_none() {
            return Ok(None);
        }
        let Some(marker) = Clock::poll_marker(&s.originals()["_lastPoll"])? else { return Ok(None) };
        s.private("lastPoll", marker)?;
        let now = clock.issued()?;
        let c = history(cfg, s.originals(), now)?;
        let t = c.tuple();
        let read = transport::signed(
            hub,
            json!([wire::domain("read"), cfg.context(), t[1], t[9], c.digest(), transport::nonce(), now, now + 30_000]),
        )?;
        let mut body = json!({"read":read});
        if let Some(code) = &secret {
            body["deviceCode"] = json!(code.as_str());
        }
        Ok(Some(body))
    })?;
    let Some(request) = request else { return Ok(None) };
    let reply = transport::post(hub, "read", request.clone()).await?;
    step(hub, id, |cfg, clock, s| {
        let now = clock.validity()?;
        let state = checked_read(cfg, s, now, &request, &reply)?;
        let app = s.originals()["intent"][0][6].as_str().context("intent missing")?;
        let mut merged = packet(s.originals());
        if let Some((record, _)) = super::live::current(s.db)? {
            ensure!(record.binding.capture == id, "personal snapshot capture changed");
            for name in ["host-state-request", "host-state", "activation"] {
                if let Some(v) = record.originals.get(name) {
                    merged[name] = v.clone();
                }
            }
        }
        super::fences::project(s.db, id, &mut merged, &reply)?;
        for (remote, local) in NAMES {
            if matches!(*local, "fence" | "fence-ack" | "revocation") {
                continue;
            }
            if let Some(value) = reply["originals"].get(*remote).filter(|v| !v.is_null()) {
                if let Some(old) = merged.get(*local) {
                    ensure!(old == value, "Worker changed original");
                } else {
                    ensure!(
                        matches!(*local, "box-challenge" | "approve" | "grant" | "fence" | "publish"),
                        "missing local H original"
                    );
                }
                merged[*local] = value.clone();
            }
        }
        merged["state"] = reply["state"].clone();
        // ceremony verifica Read como A. Este Read foi produzido por H aqui:
        // scope/nonce/janela são conferidos acima; não o reclassificar como A.
        let c = wire::ceremony(&cfg.verifier(app)?, &serde_json::to_vec(&merged)?, wire::Time::Historical(now))?;
        // Histórico serve ao fechamento; nunca habilita novas assinaturas/posse.
        if merged.get("fence").is_some() {
            let _ = c; // Cerca aplica draft/CAS próprio após sair desta transação.
        } else {
            for (_, name) in NAMES {
                if let Some(value) = merged.get(*name) {
                    s.freeze(name, value.clone())?;
                }
            }
        }
        if matches!(state.payload()[5].as_str(), Some("canceled" | "expired"))
            && merged.get("grant").is_none()
            && s.originals().get("_sentConsume").is_none()
        {
            s.private("closure", reply["state"].clone())?;
            let capture = prepare_wire::capture(cfg.context(), &s.originals()["local-ticket"])?;
            personal_prepare::purge_unconsumed(
                s.db,
                &capture,
                &serde_json::to_vec(
                    &json!({"read":request["read"],"state":reply["state"],"originals":packet(s.originals())}),
                )?,
            )?;
        }
        Ok(())
    })?;
    if reply["originals"]["fence"].is_array() {
        super::fences::apply(hub, id, &reply).await?;
    }
    Ok(Some(reply))
}
pub(super) async fn helper(hub: &Hub, id: &str, frame: &Value) -> Result<Value> {
    let a = frame.as_array().context("invalid helper request")?;
    ensure!(a.len() >= 4 && a[0] == helper::DOMAIN && a[1] == 1, "invalid helper protocol");
    match a[3].as_str().context("invalid helper operation")? {
        "time" => {
            ensure!(a.len() == 4, "invalid time frame");
            transport::time(hub).await
        }
        "ticket" => {
            ensure!(a.len() == 6, "invalid ticket frame");
            let mode = a[4].as_str().context("invalid mode")?;
            let q = ticket(hub, id, mode, &a[5])?;
            if mode == "device" {
                begin(hub, id, &q).await?;
            }
            Ok(q)
        }
        "consent" => {
            ensure!(a.len() == 8, "consent frame");
            let (at, until) = hub.personal.with(|f| {
                let app = a[4][0][6].as_str().context("I app")?;
                let ca = f.config.verifier(app)?.verify(
                    Kind::AppConsent,
                    &serde_json::to_vec(&a[6])?,
                    wire::Time::Current(f.clock.validity()?),
                    wire::Reader::Hub,
                )?;
                Ok((ca.payload()[5].as_u64().context("CA time")?, ca.payload()[6].as_u64().context("CA expiry")?))
            })?;
            wait_causal(hub, at, until).await?;
            ensure!(a.len() == 8, "invalid consent frame");
            hub.personal.refresh()?;
            hub.personal.with(|f| {
                let now = f.clock.validity()?;
                let root = f.journal.as_ref().context("root consent missing")?;
                ensure!(a[7] == root.revision(), "root revision mismatch");
                hub.store.personal_session(&f.config, id, |s| {
                    ensure!(
                        !s.closed() && !s.fenced() && s.originals()["local-ticket"] == a[5],
                        "capture closed/changed"
                    );
                    s.freeze("intent", a[4].clone())?;
                    s.freeze("app-consent", a[6].clone())?;
                    let c = checked(&f.config, s.originals(), now)?;
                    root.authorize(&f.config, &packet(s.originals()), now)?;
                    if let Some(ch) = s.originals().get("local-consent") {
                        return Ok(ch.clone());
                    }
                    s.preparing()?;
                    let ca = c.original(Kind::AppConsent)?;
                    ensure!(
                        now >= ca.payload()[5].as_u64().context("CA time")?,
                        "CA is not yet effective on qualified H time"
                    );
                    let until =
                        ca.payload()[6].as_u64().context("CA expiry")?.min(c.tuple()[18].as_u64().context("T expiry")?);
                    let now = f.clock.issued_after(ca.payload()[5].as_u64().context("CA time")?, until)?;
                    let ch = transport::signed(
                        hub,
                        json!([
                            wire::domain("local-consent"),
                            f.config.context(),
                            c.digest(),
                            ca.digest(),
                            "approve",
                            transport::nonce(),
                            now,
                            until
                        ]),
                    )?;
                    s.freeze("local-consent", ch.clone())?;
                    checked(&f.config, s.originals(), f.clock.validity()?)?;
                    Ok(ch)
                })
            })
        }
        "retained" => {
            ensure!(a.len() == 7 && (a[4] == "localConsent" || a[4] == "localConfirmation"), "invalid retain frame");
            hub.personal.refresh()?;
            hub.personal.with(|f| {
                let root = f.journal.as_ref().context("root retention missing")?;
                ensure!(a[6] == root.revision(), "root revision mismatch");
                let kind = if a[4] == "localConsent" { Kind::LocalConsent } else { Kind::LocalConfirm };
                root.retained(&f.config, &mut f.clock, kind, &serde_json::to_vec(&a[5])?)?;
                hub.store.personal_session(&f.config, id, |s| {
                    let name = if kind == Kind::LocalConsent { "local-consent" } else { "local-confirm" };
                    ensure!(s.originals().get(name) == Some(&a[5]), "retention capture mismatch");
                    Ok(json!([a[4], a[5][0][2], wire::hash(&a[5][0])?, root.revision()]))
                })
            })
        }
        "prefix" | "status" => {
            ensure!(a.len() == 5, "invalid status frame");
            let prefix = a[3] == "prefix";
            if !prefix {
                progress(hub, id).await?;
            }
            step(hub, id, |cfg, clock, s| {
                let c = if s.originals().get("intent").is_some() {
                    Some(history(cfg, s.originals(), clock.validity()?)?)
                } else {
                    None
                };
                let h = c.as_ref().map_or(Value::Null, |c| json!(c.digest()));
                ensure!(a[4] == h, "session tuple mismatch");
                if prefix {
                    let mut p = packet(s.originals());
                    p.as_object_mut().context("prefix")?.retain(|k, _| {
                        matches!(
                            k.as_str(),
                            "intent"
                                | "local-ticket"
                                | "app-consent"
                                | "local-consent"
                                | "box-challenge"
                                | "box-proof"
                                | "approve"
                                | "local-confirm"
                        )
                    });
                    return Ok(json!([s.revision(), p]));
                }
                let phase = if s.closed() {
                    "closed"
                } else if s.fenced() {
                    "fenced"
                } else if s.originals().get("local-confirm").is_some() {
                    "confirmed"
                } else if s.originals().get("local-consent").is_some() {
                    "consented"
                } else {
                    "prepared"
                };
                Ok(json!([
                    phase,
                    h,
                    s.originals().get("local-ticket"),
                    s.originals().get("local-consent"),
                    s.originals().get("local-confirm"),
                    s.originals().get("fence").map(|w| wire::hash(&w[0])).transpose()?,
                    s.revision().max(1)
                ]))
            })
        }
        _ => anyhow::bail!("unsupported helper operation"),
    }
}
async fn progress(hub: &Hub, id: &str) -> Result<()> {
    let Some(_) = read_state(hub, id).await? else { return Ok(()) };
    for (name, route, field, marker) in [
        ("local-consent", "local-consent", "localConsent", "sentLocalConsent"),
        ("box-proof", "box-proof", "boxProof", "sentBoxProof"),
        ("local-confirm", "local-confirm", "localConfirmation", "sentLocalConfirmation"),
    ] {
        hub.personal.refresh()?;
        let next = hub.personal.with(|f| {
            hub.store.personal_session(&f.config, id, |s| {
                if s.closed()
                    || s.fenced()
                    || s.originals().get("app-consent").is_none()
                    || s.originals().get("grant").is_some()
                {
                    return Ok(None);
                }
                s.preparing()?;
                let now = f.clock.validity()?;
                let c = checked(&f.config, s.originals(), now)?;
                let root = f.journal.as_ref().context("root journal missing")?;
                if name == "box-proof"
                    && s.originals().get(name).is_none()
                    && s.originals().get("box-challenge").is_some()
                {
                    if root
                        .retained(
                            &f.config,
                            &mut f.clock,
                            Kind::LocalConsent,
                            c.original(Kind::LocalConsent)?.original_bytes(),
                        )
                        .is_err()
                    {
                        return Ok(None);
                    }
                    let b = c.original(Kind::BoxChallenge)?;
                    if now < b.payload()[9].as_u64().context("B time")? {
                        return Ok(None);
                    }
                    let now = f.clock.issued_after(
                        b.payload()[9].as_u64().context("B time")?,
                        b.payload()[10].as_u64().context("B expiry")?,
                    )?;
                    let secret = wire::box_open(
                        &hub.identity.box_secret,
                        b,
                        c.digest(),
                        c.original(Kind::AppConsent)?.digest(),
                        c.original(Kind::LocalConsent)?.digest(),
                    )?;
                    s.freeze(
                        name,
                        transport::signed(
                            hub,
                            json!([
                                wire::domain("box-proof"),
                                f.config.context(),
                                c.digest(),
                                b.digest(),
                                secret.as_str(),
                                transport::nonce(),
                                now,
                                b.payload()[10]
                            ]),
                        )?,
                    )?;
                }
                if name == "local-confirm"
                    && s.originals().get(name).is_none()
                    && s.originals().get("approve").is_some()
                {
                    if root
                        .retained(
                            &f.config,
                            &mut f.clock,
                            Kind::LocalConsent,
                            c.original(Kind::LocalConsent)?.original_bytes(),
                        )
                        .is_err()
                    {
                        return Ok(None);
                    }
                    let ap = c.original(Kind::Approve)?;
                    if now < ap.payload()[7].as_u64().context("AP time")? {
                        return Ok(None);
                    }
                    let now = f.clock.issued_after(
                        ap.payload()[7].as_u64().context("AP time")?,
                        ap.payload()[8].as_u64().context("AP expiry")?,
                    )?;
                    s.freeze(
                        name,
                        transport::signed(
                            hub,
                            json!([
                                wire::domain("local-confirm"),
                                f.config.context(),
                                c.digest(),
                                ap.digest(),
                                c.original(Kind::BoxProof)?.digest(),
                                "approve",
                                transport::nonce(),
                                now,
                                ap.payload()[8]
                            ]),
                        )?,
                    )?;
                }
                checked(&f.config, s.originals(), f.clock.validity()?)?;
                let Some(value) = s.originals().get(name).cloned() else { return Ok(None) };
                if s.originals().get(format!("_{marker}")).is_some() {
                    return Ok(None);
                }
                let kind = if name == "local-confirm" { Kind::LocalConfirm } else { Kind::LocalConsent };
                if root
                    .retained(
                        &f.config,
                        &mut f.clock,
                        kind,
                        &serde_json::to_vec(if name == "local-consent" || name == "local-confirm" {
                            &value
                        } else {
                            &s.originals()["local-consent"]
                        })?,
                    )
                    .is_err()
                {
                    return Ok(None);
                }
                f.clock.validity()?;
                Ok(Some(value))
            })
        })?;
        if let Some(value) = next {
            effect(hub, id, route, json!({(field):value}), marker).await?;
        }
    }
    hub.personal.refresh()?;
    let body = hub.personal.with(|f| {
        hub.store.personal_session(&f.config, id, |s| {
            if s.closed()
                || s.fenced()
                || s.originals().get("local-confirm").is_none()
                || s.originals().get("grant").is_some()
                || s.originals().get("_sentConsume").is_some()
            {
                return Ok(None);
            }
            s.preparing()?;
            let now = f.clock.validity()?;
            let c = checked(&f.config, s.originals(), now)?;
            let l = c.original(Kind::LocalConfirm)?;
            if f.journal
                .as_ref()
                .context("root journal missing")?
                .retained(&f.config, &mut f.clock, Kind::LocalConfirm, l.original_bytes())
                .is_err()
            {
                return Ok(None);
            }
            if s.originals().get("consume").is_none() {
                let now = f.clock.issued_after(
                    l.payload()[7].as_u64().context("L time")?,
                    l.payload()[8].as_u64().context("L expiry")?,
                )?;
                s.freeze(
                    "consume",
                    transport::signed(
                        hub,
                        json!([
                            wire::domain("consume"),
                            f.config.context(),
                            c.digest(),
                            c.original(Kind::Approve)?.digest(),
                            l.digest(),
                            c.tuple()[16],
                            format!("op_{}", transport::nonce()),
                            now,
                            l.payload()[8]
                        ]),
                    )?,
                )?;
            }
            checked(&f.config, s.originals(), f.clock.validity()?)?;
            let mut b = serde_json::Map::new();
            for (remote, local) in NAMES.iter().take(9) {
                b.insert((*remote).into(), s.originals()[*local].clone());
            }
            f.clock.validity()?;
            Ok(Some(Value::Object(b)))
        })
    })?;
    if let Some(body) = body {
        let result = effect(hub, id, "consume", body, "sentConsume").await?;
        step(hub, id, |cfg, clock, s| {
            s.freeze("grant", result["grant"].clone())?;
            checked(cfg, s.originals(), clock.validity()?)?;
            Ok(())
        })?;
    }
    Ok(())
}
