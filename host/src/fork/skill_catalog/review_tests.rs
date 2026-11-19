//! d2a: dispatch real, só comparação; não executa motor, conta ou leitura nativa.
use super::*;

fn opening(skill: &Value) -> Value {
    json!({"id":skill["id"],"review":{"protocol":1,"expectedInstallationId":skill["installationId"]}})
}
async fn manual(f: &ScopeFixture) -> Value {
    f.call("installSkill", json!({"name":"d2a-manual","description":"Comparação","instructions":"BASE INTEGRAL"}))
        .await
        .unwrap()["skill"]
        .clone()
}
fn changes(f: &ScopeFixture) -> u64 {
    f.hub.store.db.locked().total_changes()
}

#[tokio::test]
async fn d2a_base_proposta_integral_alcance_e_zero_escrita() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let skill = manual(&f).await;
    let id = skill["id"].as_str().unwrap();
    for (bot, skills) in [("z-bot", vec![id]), ("a-bot", vec![id, id]), ("off", vec![])] {
        let cfg: crate::store::BotConfig = serde_json::from_value(json!({
            "id":bot,"name":bot,"backend":"fixture","cwd":"/tmp","skills":skills
        }))
        .unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
    }
    let before = f.snapshot();
    let count = changes(&f);
    let opened = f.call("skillDetails", opening(&skill)).await.unwrap();
    let base = &opened["review"]["base"];
    assert_eq!(base["revision"], 1);
    let md = fs::read_to_string(f.hub.store.skills_root.join(id).join("SKILL.md")).unwrap();
    assert_eq!(base["markdown"], md);
    assert_eq!(base["bodyDigest"], hash(md.as_bytes()));
    assert!(opened["review"]["proposal"].is_null());
    let bots = opened["review"]["affectedBots"].as_array().unwrap();
    assert_eq!(bots.iter().map(|b| b["id"].as_str().unwrap()).collect::<Vec<_>>(), ["a-bot", "z-bot"]);
    for bot in bots {
        assert_eq!(
            bot["selectionVersion"],
            f.hub.store.bot(bot["id"].as_str().unwrap()).unwrap().unwrap().skills_version.unwrap()
        );
    }
    let proposal = format!("---\nname: {id}\ndescription: Nova descrição\n---\n\nPROPOSTA INTEGRAL\n");
    let mut request = opening(&skill);
    request["review"]["proposal"] = json!({"expectedBodyDigest":base["bodyDigest"],"markdown":proposal});
    let compared = f.call("skillDetails", request.clone()).await.unwrap();
    assert_eq!(compared["review"]["base"], *base);
    assert_eq!(
        compared["review"]["proposal"],
        json!({"revision":2,"bodyDigest":hash(proposal.as_bytes()),"markdown":proposal})
    );
    assert_eq!(compared["review"]["effect"], "none");
    let mut envelope = compared["review"].clone();
    let digest = envelope.as_object_mut().unwrap().remove("comparisonDigest").unwrap();
    assert_eq!(digest, hash(registry::canonical(&envelope).unwrap().as_bytes()));
    assert_eq!(f.call("skillDetails", request).await.unwrap(), compared, "comparação determinística, sem recibo novo");
    assert_eq!(f.snapshot(), before);
    assert_eq!(changes(&f), count, "nem escrita idêntica é permitida");
}

#[tokio::test]
async fn d2a_malformados_caller_space_recusados_antes_do_catalogo() {
    use crate::api::{devices::Caller, dispatch};
    use crate::store::Scope;
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let skill = manual(&f).await;
    let base = opening(&skill);
    let count = changes(&f);
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let mut bad = Vec::new();
    for (key, value) in [("extra", json!(1)), ("source", json!("find-skills"))] {
        let mut b = base.clone();
        b[key] = value;
        bad.push(b);
    }
    for (key, value) in [
        ("protocol", json!(2)),
        ("extra", json!(1)),
        ("expectedInstallationId", json!("fake")),
        ("proposal", Value::Null),
        ("proposal", json!({"expectedBodyDigest":"0".repeat(64),"markdown":"malformado"})),
    ] {
        let mut b = base.clone();
        b["review"][key] = value;
        bad.push(b);
    }
    let too_long = format!("---\nname: d2a-manual\ndescription: manual\n---\n{}", "x".repeat(registry::MAX_SKILL));
    let mut oversized = base.clone();
    oversized["review"]["proposal"] = json!({"expectedBodyDigest":"0".repeat(64),"markdown":too_long});
    bad.push(oversized);
    for b in bad {
        assert!(f.call("skillDetails", b).await.is_err());
        assert_eq!(trace.take(), 0);
    }
    for caller in [
        Caller::Pairing { key: "fixture".into() },
        Caller::Device { key: "fixture".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "fixture".into(), scopes: vec![], autoridade: None },
    ] {
        assert!(dispatch(&f.hub, &caller, "skillDetails", base.clone()).await.is_err());
        assert_eq!(trace.take(), 0);
    }
    let mut other = base;
    other["spaceId"] = f.additional.clone();
    assert!(f.call("skillDetails", other).await.unwrap_err().to_string().contains("skills_personal_only"));
    assert_eq!(trace.take(), 0);
    assert_eq!(changes(&f), count);
}

#[tokio::test]
async fn d2a_base_adulterada_legada_curada_e_r7_sem_recaptura() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let skill = manual(&f).await;
    let id = skill["id"].as_str().unwrap();
    let path = f.hub.store.skills_root.join(id).join("SKILL.md");
    let md = fs::read_to_string(&path).unwrap();
    let opened = f.call("skillDetails", opening(&skill)).await.unwrap();
    let mut wrong = opening(&skill);
    wrong["review"]["proposal"] = json!({"expectedBodyDigest":"0".repeat(64),"markdown":md});
    assert_eq!(f.call("skillDetails", wrong).await.unwrap_err().to_string(), "skill_review_base_changed");
    fs::write(&path, "ADULTERADO").unwrap();
    let before = f.snapshot();
    let count = changes(&f);
    assert_eq!(f.call("skillDetails", opening(&skill)).await.unwrap_err().to_string(), installation::CHANGED);
    assert_eq!(f.snapshot(), before);
    assert_eq!(changes(&f), count);
    fs::write(&path, &md).unwrap();
    let mut meta: Value = serde_json::from_str(&f.hub.store.kv_get(storage::META).unwrap()).unwrap();
    meta["installations"][id].as_object_mut().unwrap().remove("manualRevision");
    f.hub.store.kv_set(storage::META, &meta.to_string()).unwrap();
    assert!(f.call("skillDetails", json!({"id":id})).await.is_ok(), "legado íntegro continua legível");
    let before = f.snapshot();
    let count = changes(&f);
    assert_eq!(f.call("skillDetails", opening(&skill)).await.unwrap_err().to_string(), "skill_review_base_unknown");
    assert_eq!(f.snapshot(), before);
    assert_eq!(changes(&f), count);
    // Exame não migra/reemite nem limpa dirty R7.
    f.hub.store.kv_set("skills", &f.hub.store.kv_get("skills").unwrap()).unwrap();
    let before = f.snapshot();
    let count = changes(&f);
    assert_eq!(f.call("skillDetails", opening(&skill)).await.unwrap_err().to_string(), installation::UNAVAILABLE);
    assert_eq!(f.snapshot(), before);
    assert_eq!(changes(&f), count);
    // Leitura normal reconcilia pelo dono existente; intenção antiga deixa de valer.
    let current = f.call("skills", json!({})).await.unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
        .clone();
    assert_ne!(current["installationId"], skill["installationId"]);
    assert_eq!(f.call("skillDetails", opening(&skill)).await.unwrap_err().to_string(), installation::STALE);
    let p = registry::find("find-skills").unwrap();
    let curated = f
        .call("installSkill", json!({"catalogId":"find-skills","expectedDigest":p.metadata.digest}))
        .await
        .unwrap()["skill"]
        .clone();
    assert_eq!(f.call("skillDetails", opening(&curated)).await.unwrap_err().to_string(), "skill_review_manual_only");
    assert_eq!(opened["review"]["base"]["revision"], 1);
}

#[tokio::test]
async fn d2a_alcance_reflete_cfg_e_aba_sem_reservar_exame() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let skill = manual(&f).await;
    let cfg: crate::store::BotConfig = serde_json::from_value(json!({
        "id":"d2a-bot","name":"Bot","backend":"fixture","cwd":"/tmp","skills":[skill["id"]]
    }))
    .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    let first = f.call("skillDetails", opening(&skill)).await.unwrap();
    {
        let db = f.hub.store.db.locked();
        db.execute("UPDATE bots SET config=json_set(config,'$.skills',json('[]')) WHERE id='d2a-bot'", []).unwrap();
    }
    let off = f.call("skillDetails", opening(&skill)).await.unwrap();
    assert_eq!(off["review"]["affectedBots"], json!([]));
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE bots SET config=?1 WHERE id='d2a-bot'", [serde_json::to_string(&cfg).unwrap()])
        .unwrap();
    let again = f.call("skillDetails", opening(&skill)).await.unwrap();
    assert_ne!(first["review"]["comparisonDigest"], again["review"]["comparisonDigest"]);
    assert_ne!(
        first["review"]["affectedBots"][0]["selectionVersion"],
        again["review"]["affectedBots"][0]["selectionVersion"]
    );
    assert_eq!(first["review"]["base"], again["review"]["base"]);
    f.hub.store.db.locked().execute("UPDATE bots SET donna_skills_version=NULL WHERE id='d2a-bot'", []).unwrap();
    assert_eq!(
        f.call("skillDetails", opening(&skill)).await.unwrap_err().to_string(),
        "skill_review_scope_unavailable"
    );
}

#[tokio::test]
async fn d2a_autoridade_original_revogada_apos_captura_impede_leitura() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let skill = manual(&f).await;
    let mut device = Device {
        key: "d2a-original".into(),
        name: "Sintético".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let before = f.snapshot();
    let count = changes(&f);
    let authorized = crate::api::dispatch(&f.hub, &caller, "skillDetails", opening(&skill)).await.unwrap();
    assert_eq!(authorized["review"]["effect"], "none");
    assert_eq!(f.snapshot(), before, "Control original sem engine.accounts pode comparar sem efeito");
    assert_eq!(changes(&f), count);
    let authority = crate::fork::autoridade::Autoridade::capturar(&f.hub, &caller).unwrap();
    let intent = review::request(&opening(&skill)).unwrap();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let result = review::examine(&f.hub, &authority, &intent, || {
        // Intercalação determinística: depois da captura e dos locks, antes de dados.
        device.scopes.clear();
        f.hub.store.put_device(&device)?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(trace.take(), 0);
    assert!(matches!(caller, Caller::Device { .. }));
}
