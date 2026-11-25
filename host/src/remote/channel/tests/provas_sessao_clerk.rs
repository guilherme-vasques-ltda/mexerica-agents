//! Prova do `/channel` num Hub com vínculo: pareamento liga o `userId` verificado, todo pedido
//! exige sessão Clerk, prova de posse amarrada ao canal, `workspaceEpoch` e `sub` igual ao
//! `userId` do aparelho; o canal fecha no `exp` (ou na validade da JWKS) com `auth_invalid`/4001 e,
//! quando um papel registrado muda (o epoch sobe), com 4003. A guarda da admissão vale no ponto do efeito e na entrega, nos
//! caminhos direto e relay. Sem vínculo, nada muda.
use super::*;
use crate::fork::sessao_clerk::{self as sessao, prova::*};
use crate::fork::workspace::{EPOCH_FIELD, METHOD as CONTEXTO};

#[derive(Clone, Copy, Debug)]
enum Via {
    Direto,
    Relay,
}

async fn pedir(p: &mut Phone, id: u64, inner: Value) -> Value {
    let mut inner = inner;
    inner["id"] = id.into();
    p.send(inner).await;
    loop {
        let v = p.recv().await.expect("channel stays open");
        if v["id"] == id && (v.get("ok").is_some() || v.get("err").is_some()) {
            return v;
        }
    }
}

/// O corpo com o `workspaceEpoch` atual (menos onde ele não é exigido).
fn com_epoch(hub: &Hub, m: &str, mut b: Value) -> Value {
    if m != CONTEXTO && m != sessao::METODO {
        b[EPOCH_FIELD] = hub.workspace.atual().into();
    }
    b
}

fn envelope(p: &Phone, hub: &Hub, m: &str, b: Value) -> Value {
    json!({"m": m, "b": com_epoch(hub, m, b), "pop": pop(&p.key, &hub.host_id, &p.th, m)})
}

async fn com_posse(p: &mut Phone, hub: &Hub, id: u64, m: &str, b: Value) -> Value {
    let inner = envelope(p, hub, m, b);
    pedir(p, id, inner).await
}

async fn autenticar(p: &mut Phone, hub: &Hub, id: u64, token: &str) -> Value {
    com_posse(p, hub, id, sessao::METODO, json!({ sessao::CAMPO_TOKEN: token })).await
}

async fn assinar_eventos(p: &mut Phone, hub: &Hub, id: u64) {
    let b = com_epoch(hub, "sub:events", json!({"since": hub.store.current_rev()}));
    let pop_sub = pop(&p.key, &hub.host_id, &p.th, "sub:events");
    p.send(json!({"id": id, "sub": "events", "b": b, "pop": pop_sub})).await;
    assert_eq!(p.recv().await.unwrap()["ev"]["type"], "hello");
}

/// O fechamento do canal e o último recado em claro.
async fn fim(p: &mut Phone) -> (u16, Value) {
    loop {
        if let Err(fim) = p.recv().await {
            return fim;
        }
    }
}

/// Pareia por código com o token de `afirmacoes` e devolve a chave.
async fn parear(hub: &Arc<Hub>, afirmacoes: &Value) -> Result<SigningKey, Value> {
    let key = SigningKey::from_bytes(&crypto::random());
    let code = hub.pairing.locked().issue().code;
    let mut p = Phone::direct(hub, key.clone());
    assert!(matches!(p.hello(hub, true).await, Out::Msg(v) if v["t"] == "welcome"));
    let b =
        json!({"code": code, "name": "Telefone fictício", "platform": "ios", sessao::CAMPO_TOKEN: token(afirmacoes)});
    p.send(json!({"id": 1, "m": "pair", "b": b, "pop": pop(&key, &hub.host_id, &p.th, "pair")})).await;
    let reply = p.recv().await.unwrap();
    if reply.get("ok").is_some() {
        assert_eq!(p.recv().await.unwrap_err().0, close::PAIRED);
        Ok(key)
    } else {
        Err(reply)
    }
}

async fn conectado_via(hub: &Arc<Hub>, key: SigningKey, via: Via) -> Phone {
    let mut p = match via {
        Via::Direto => Phone::direct(hub, key),
        Via::Relay => {
            let dk = crypto::b64(key.verifying_key().as_bytes());
            Phone::connect(hub, key, ChannelKind::Relay { link: "l-sessao".into(), dk, pair: false })
        }
    };
    assert!(matches!(p.hello(hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    p
}

async fn conectado(hub: &Arc<Hub>, key: SigningKey) -> Phone {
    conectado_via(hub, key, Via::Direto).await
}

fn codigo(v: &Value) -> (&Value, &Value) {
    (&v["err"]["status"], &v["err"]["code"])
}

/// Token de `sub` com vida de `segundos`.
fn curto(a: Value, segundos: i64) -> String {
    let mut a = a;
    let agora = now_ms() / 1000;
    a["iat"] = json!(agora);
    a["exp"] = json!(agora + segundos);
    token(&a)
}

fn pausadas(hub: &Hub) -> bool {
    hub.routines.pause_state()["paused"] == true
}

/// Apresenta num canal novo um token com outro papel: o registro sobe o epoch, e o próprio canal que
/// o apresentou fecha com os demais (1011 até a confirmação da sessão nova). Espera o segundo seguinte e emite o token
/// depois, para o `iat` ser mais novo que o registrado.
async fn trocar_papel(hub: &Arc<Hub>, key: &SigningKey, via: Via, afirmacoes: &Value) {
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let mut q = conectado_via(hub, key.clone(), via).await;
    let novo = curto(afirmacoes.clone(), 50);
    let mut pedido = envelope(&q, hub, sessao::METODO, json!({ sessao::CAMPO_TOKEN: novo }));
    pedido["id"] = 1.into();
    q.send(pedido).await;
    let (code, reject) = fim(&mut q).await;
    assert_eq!((code, reject), (close::TIMEOUT, Value::Null), "{via:?}");
}

#[tokio::test]
async fn sem_vinculo_o_aparelho_pareado_segue_igual() {
    let hub = temp_hub();
    let mut p = conectado(&hub, paired_phone(&hub).await).await;
    let hello = p.call(1, "hello", json!({})).await;
    assert_eq!(hello["ok"]["computerId"], hub.identity.computer_id(), "sem sessão nem prova de posse");
    let contexto = p.call(2, CONTEXTO, json!({})).await;
    assert_eq!(
        (contexto["ok"]["workspaceId"].as_str(), contexto["ok"]["role"].as_str()),
        (Some("personal:unbound"), Some("owner"))
    );
    assert_eq!(p.call(3, sessao::METODO, json!({})).await["err"]["status"], 404, "sem vínculo, o método não existe");
}

#[tokio::test]
async fn hub_pessoal_exige_sessao_posse_e_sub_do_dono_em_cada_pedido() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    // Pareamento: token de outro usuário não pareia; o dono pareia e fica ligado.
    let recusa = parear(&hub, &afirmacoes(OUTRO)).await.unwrap_err();
    assert_eq!(codigo(&recusa), (&json!(403), &json!("workspace_forbidden")));
    assert!(hub.pairing.locked().open(), "token recusado não gasta o código");
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let mut p = conectado(&hub, key.clone()).await;

    let sem_sessao = com_posse(&mut p, &hub, 1, "hello", json!({})).await;
    assert_eq!(codigo(&sem_sessao), (&json!(401), &json!("auth_invalid")));
    let ruim = autenticar(&mut p, &hub, 2, &assinar(&cabecalho(), &afirmacoes(DONO), &chaves()[1])).await;
    assert_eq!(codigo(&ruim), (&json!(401), &json!("auth_invalid")), "assinado pela chave do kid errado");
    let ok = autenticar(&mut p, &hub, 3, &token(&afirmacoes(DONO))).await;
    assert_eq!((ok["ok"]["userId"].as_str(), ok["ok"]["role"].as_str()), (Some(DONO), Some("owner")));

    let hello = com_posse(&mut p, &hub, 4, "hello", json!({})).await;
    assert_eq!(hello["ok"]["computerId"], hub.identity.computer_id());
    let sem_prova = p.call(5, "hello", json!({})).await;
    assert_eq!(codigo(&sem_prova), (&json!(401), &json!("device_unpaired")));
    let b = com_epoch(&hub, "hello", json!({}));
    let th = p.th;
    let prova = pop(&key, &hub.host_id, &th, "hello");
    assert!(pedir(&mut p, 6, json!({"m": "hello", "b": b, "pop": prova})).await.get("ok").is_some());
    let replay = pedir(&mut p, 7, json!({"m": "hello", "b": b, "pop": prova})).await;
    assert_eq!(codigo(&replay), (&json!(401), &json!("device_unpaired")), "nonce repetido");
    let outro_metodo =
        pedir(&mut p, 8, json!({"m": "hello", "b": b, "pop": pop(&key, &hub.host_id, &th, "send")})).await;
    assert_eq!(codigo(&outro_metodo), (&json!(401), &json!("device_unpaired")));
    let outro_hub = pedir(&mut p, 9, json!({"m": "hello", "b": b, "pop": pop(&key, "outro-hub", &th, "hello")})).await;
    assert_eq!(codigo(&outro_hub), (&json!(401), &json!("device_unpaired")), "prova feita para outro Hub");
    // A prova feita para outro canal do mesmo aparelho não vale aqui.
    let outro_canal = conectado(&hub, key.clone()).await;
    let alheia = pop(&key, &hub.host_id, &outro_canal.th, "hello");
    let replay_canal = pedir(&mut p, 10, json!({"m": "hello", "b": b, "pop": alheia})).await;
    assert_eq!(codigo(&replay_canal), (&json!(401), &json!("device_unpaired")), "prova de outro canal");

    let contexto = com_posse(&mut p, &hub, 11, CONTEXTO, json!({})).await;
    let esperado = format!("personal:{DONO}");
    assert_eq!(contexto["ok"]["workspaceId"].as_str(), Some(esperado.as_str()));
    assert_eq!((contexto["ok"]["userId"].as_str(), contexto["ok"]["role"].as_str()), (Some(DONO), Some("owner")));
    // Assinatura de stream também passa pela sessão e pela prova.
    let sub_b = com_epoch(&hub, "sub:events", json!({"since": hub.store.current_rev()}));
    let sub = pedir(&mut p, 12, json!({"sub": "events", "b": sub_b})).await;
    assert_eq!(codigo(&sub), (&json!(401), &json!("device_unpaired")));
    let pop_sub = pop(&key, &hub.host_id, &p.th, "sub:events");
    p.send(json!({"id": 13, "sub": "events", "b": sub_b, "pop": pop_sub})).await;
    let hello = p.recv().await.unwrap();
    assert_eq!(hello["ev"]["type"], "hello");
    assert_eq!(hello["ev"]["workspace"]["userId"], DONO);
}

#[tokio::test]
async fn pedido_sem_workspace_epoch_e_envelope_ambiguo_sao_recusados() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let mut p = conectado(&hub, key.clone()).await;
    assert!(autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await.get("ok").is_some());
    let sem_epoch = json!({"m": "hello", "b": {}, "pop": pop(&key, &hub.host_id, &p.th, "hello")});
    let recusa = pedir(&mut p, 2, sem_epoch).await;
    assert_eq!(codigo(&recusa), (&json!(409), &json!(crate::fork::workspace::EPOCH_MISMATCH)));
    let contexto = json!({"m": CONTEXTO, "b": {}, "pop": pop(&key, &hub.host_id, &p.th, CONTEXTO)});
    assert!(pedir(&mut p, 3, contexto).await.get("ok").is_some(), "workspaceContext não exige epoch");
    // `m` e `sub` juntos (ou nenhum dos dois) não viram método vazio: o canal fecha.
    for inner in [
        json!({"id": 4, "m": "hello", "sub": "events", "b": com_epoch(&hub, "hello", json!({}))}),
        json!({"id": 4, "b": com_epoch(&hub, "hello", json!({}))}),
        json!({"id": 4, "m": sessao::METODO, "sub": "events", "b": { sessao::CAMPO_TOKEN: token(&afirmacoes(DONO)) }}),
        // Discriminador presente com outro tipo não conta como ausente.
        json!({"id": 4, "m": 5, "sub": "events", "b": com_epoch(&hub, "sub:events", json!({}))}),
        json!({"id": 4, "m": "hello", "sub": null, "b": com_epoch(&hub, "hello", json!({}))}),
        json!({"id": 4, "m": ["hello"], "b": com_epoch(&hub, "hello", json!({}))}),
    ] {
        let mut p = conectado(&hub, key.clone()).await;
        assert!(autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await.get("ok").is_some());
        let mut inner = inner;
        inner["pop"] = pop(&key, &hub.host_id, &p.th, "hello");
        p.send(inner).await;
        assert_eq!(fim(&mut p).await.0, close::PROTOCOL);
    }
}

#[tokio::test]
async fn cancel_sem_sessao_so_encerra_assinatura_do_proprio_canal() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let mut dono = conectado(&hub, key.clone()).await;
    assert!(autenticar(&mut dono, &hub, 1, &token(&afirmacoes(DONO))).await.get("ok").is_some());
    assinar_eventos(&mut dono, &hub, 7).await;
    // Outro canal, sem sessão, cancela o mesmo id: nada responde nem muda no primeiro.
    let mut p = conectado(&hub, key.clone()).await;
    p.send(json!({"id": 7, "cancel": true})).await;
    let hello = com_posse(&mut p, &hub, 8, "hello", json!({})).await;
    assert_eq!(codigo(&hello), (&json!(401), &json!("auth_invalid")), "o canal segue aberto e sem sessão");
    dono.send(json!({"id": 7, "cancel": true})).await;
    let fim = dono.recv().await.unwrap();
    assert_eq!(fim, json!({"id": 7, "end": true}), "só o próprio canal encerra a assinatura");
}

#[tokio::test]
async fn aparelho_sem_userid_ligado_ou_de_outro_usuario_nao_entra() {
    let hub = temp_hub();
    // Pareado antes do vínculo: não tem `userId` ligado.
    let antigo = paired_phone(&hub).await;
    vincular(&hub, &vinculo_time());
    let mut p = conectado(&hub, antigo).await;
    let recusa = autenticar(&mut p, &hub, 1, &token(&com_org(afirmacoes(DONO), ORG, "admin"))).await;
    assert_eq!(codigo(&recusa), (&json!(401), &json!("device_unpaired")));
    // Pareado por um membro; outro membro da mesma org usa o mesmo aparelho.
    let do_outro = parear(&hub, &com_org(afirmacoes(OUTRO), ORG, "member")).await.unwrap();
    let mut p = conectado(&hub, do_outro.clone()).await;
    let recusa = autenticar(&mut p, &hub, 1, &token(&com_org(afirmacoes(DONO), ORG, "admin"))).await;
    assert_eq!(codigo(&recusa), (&json!(401), &json!("device_unpaired")), "sub ≠ userId do aparelho");
    // Um token do mesmo segundo com outro desfecho não prova ordem: espera o segundo seguinte.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    // Org divergente: o `sub` verificado fica sem acesso, o epoch sobe e o canal fecha.
    let fora = curto(com_org(afirmacoes(OUTRO), "org_outra", "member"), 50);
    let mut pedido = envelope(&p, &hub, sessao::METODO, json!({ sessao::CAMPO_TOKEN: fora }));
    pedido["id"] = 2.into();
    p.send(pedido).await;
    let (code, reject) = fim(&mut p).await;
    assert_eq!((code, reject), (close::TIMEOUT, Value::Null), "org divergente");
    let mut q = conectado(&hub, do_outro).await;
    let recusa = autenticar(&mut q, &hub, 1, &curto(com_org(afirmacoes(OUTRO), "org_outra", "member"), 50)).await;
    assert_eq!(codigo(&recusa), (&json!(403), &json!("workspace_forbidden")), "org divergente");
}

#[tokio::test]
async fn ligacao_do_usuario_cai_com_a_concessao_a_revogacao_e_o_repareamento_sem_vinculo() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let tentar = async |hub: &Arc<Hub>, key: &SigningKey| {
        let mut p = conectado(hub, key.clone()).await;
        autenticar(&mut p, hub, 1, &token(&afirmacoes(DONO))).await
    };
    let unpaired = (json!(401), json!("device_unpaired"));
    // Mudança na concessão do aparelho (outra geração): a ligação antiga não vale.
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let dk = crypto::b64(key.verifying_key().as_bytes());
    assert!(tentar(&hub, &key).await.get("ok").is_some());
    let mut aparelho = hub.store.device(&dk).unwrap();
    aparelho.scopes = vec![Scope::Control];
    hub.store.put_device(&aparelho).unwrap();
    let r = tentar(&hub, &key).await;
    assert_eq!((r["err"]["status"].clone(), r["err"]["code"].clone()), unpaired, "geração nova");
    // Com vínculo, o caminho do pareamento sem vínculo é recusado na própria Tx.
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let dk = crypto::b64(key.verifying_key().as_bytes());
    let aparelho = hub.store.device(&dk).unwrap();
    assert!(crate::fork::workspace::readmitir_pareado(&hub.store, &aparelho).is_err());
    // Repareamento por código sem vínculo (o vínculo sai e volta) apaga a ligação.
    let chave = crate::fork::sessao_clerk::vinculo::CHAVE;
    hub.store.db.locked().execute("DELETE FROM kv WHERE k=?1", [chave]).unwrap();
    crate::fork::workspace::readmitir_pareado(&hub.store, &aparelho).unwrap();
    vincular(&hub, &vinculo_pessoal());
    let r = tentar(&hub, &key).await;
    assert_eq!((r["err"]["status"].clone(), r["err"]["code"].clone()), unpaired, "repareado sem vínculo");
    // Revogação apaga a ligação.
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let dk = crypto::b64(key.verifying_key().as_bytes());
    assert!(hub.revoke_device(&dk).unwrap());
    let ligadas: i64 = hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM fork_sessao_aparelho_usuario WHERE key=?1", [&dk], |r| r.get(0))
        .unwrap();
    assert_eq!(ligadas, 0);
}

#[tokio::test]
async fn membro_recebe_role_insufficient_e_o_corpo_nao_muda_identidade() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_time());
    let key = parear(&hub, &com_org(afirmacoes(OUTRO), ORG, "member")).await.unwrap();
    let mut p = conectado(&hub, key).await;
    // Identidade no corpo de `sessionAuthenticate` não conta: o papel sai do token.
    let b = json!({ sessao::CAMPO_TOKEN: token(&com_org(afirmacoes(OUTRO), ORG, "member")), "userId": DONO, "orgId": ORG, "role": "admin" });
    let ok = com_posse(&mut p, &hub, 1, sessao::METODO, b).await;
    assert_eq!((ok["ok"]["userId"].as_str(), ok["ok"]["role"].as_str()), (Some(OUTRO), Some("member")));
    for (id, m) in [(2, "saveRoutine"), (3, "respondPermission"), (4, "readFile"), (5, "runRoutine")] {
        let negado = com_posse(&mut p, &hub, id, m, json!({"role": "admin", "userId": DONO})).await;
        assert_eq!(codigo(&negado), (&json!(403), &json!("role_insufficient")), "{m}");
    }
    assert!(com_posse(&mut p, &hub, 6, "hello", json!({})).await.get("ok").is_some(), "sessão segue aberta");
    let contexto = com_posse(&mut p, &hub, 7, CONTEXTO, json!({})).await["ok"].clone();
    let org = format!("org:{ORG}");
    assert_eq!((contexto["kind"].as_str(), contexto["workspaceId"].as_str()), (Some("team"), Some(org.as_str())));
    assert_eq!((contexto["userId"].as_str(), contexto["role"].as_str()), (Some(OUTRO), Some("member")));
    assert_eq!(contexto["capabilities"], json!(["chat"]));
}

#[tokio::test]
async fn tela_remota_e_negada_com_vinculo_ate_para_o_dono() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    for via in [Via::Direto, Via::Relay] {
        let mut p = conectado_via(&hub, key.clone(), via).await;
        assert!(autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await.get("ok").is_some());
        for (id, m) in [(2, "screenPrepare"), (3, "screenOffer"), (4, "screenTakeover")] {
            let negado = com_posse(&mut p, &hub, id, m, json!({"session": "s", "sdp": "v=0"})).await;
            assert_eq!(codigo(&negado), (&json!(403), &json!("role_insufficient")), "{via:?} {m}");
        }
    }
    // No ponto do efeito, mesmo um aparelho que chegasse ao despacho é recusado.
    let dk = crypto::b64(key.verifying_key().as_bytes());
    let (_, caller) = autoridade::autenticar(&hub, &dk).unwrap();
    for m in ["screenPrepare", "screenOffer", "screenTakeover"] {
        let erro = api::dispatch(&hub, &caller, m, json!({"session": "s", "sdp": "v=0"})).await.unwrap_err();
        assert!(erro.is::<devices::Forbidden>(), "{m}");
    }
}

/// Avança o relógio do runtime sem esperar o tempo real.
async fn avancar(quanto: Duration) {
    tokio::time::pause();
    tokio::time::advance(quanto).await;
    tokio::time::resume();
}

#[tokio::test]
async fn canal_fecha_no_exp_com_auth_invalid_a_menos_que_reautentique() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    // Relógio controlado: o monotônico da sessão só anda quando a prova manda. O `exp` civil fica
    // de pé (o relógio real mal anda), então quem decide o fim é o prazo monotônico.
    for via in [Via::Direto, Via::Relay] {
        // Reautenticação antes do exp mantém o canal: a primeira sessão vale no máximo 5 s.
        let mut p = conectado_via(&hub, key.clone(), via).await;
        assert!(autenticar(&mut p, &hub, 1, &curto(afirmacoes(DONO), 5)).await.get("ok").is_some());
        avancar(Duration::from_secs(1)).await;
        assert!(autenticar(&mut p, &hub, 2, &curto(afirmacoes(DONO), 60)).await.get("ok").is_some());
        avancar(Duration::from_secs(5)).await;
        assert!(com_posse(&mut p, &hub, 3, "hello", json!({})).await.get("ok").is_some(), "o exp novo vale");

        // Sem reautenticar: o stream aberto para e o canal fecha com o terminal da sessão.
        let mut p = conectado_via(&hub, key.clone(), via).await;
        assert!(autenticar(&mut p, &hub, 1, &curto(afirmacoes(DONO), 5)).await.get("ok").is_some());
        assinar_eventos(&mut p, &hub, 2).await;
        avancar(Duration::from_secs(6)).await;
        let (code, reject) = fim(&mut p).await;
        assert_eq!(code, 4001, "{via:?}");
        assert_eq!(
            (reject["code"].as_str(), reject["stableCode"].as_str()),
            (Some("unauthorized"), Some("auth_invalid"))
        );
    }
}

#[tokio::test]
async fn papel_trocado_ou_retirado_fecha_o_canal_e_a_renovacao_nao_troca_o_papel() {
    let admin = || com_org(afirmacoes(OUTRO), ORG, "admin");
    for via in [Via::Direto, Via::Relay] {
        let hub = temp_hub();
        vincular(&hub, &vinculo_time());
        let key = parear(&hub, &admin()).await.unwrap();
        for (org, rol) in [(ORG, "member"), ("org_outra", "admin")] {
            let mut p = conectado_via(&hub, key.clone(), via).await;
            assert_eq!(autenticar(&mut p, &hub, 1, &token(&admin())).await["ok"]["role"], "admin");
            assinar_eventos(&mut p, &hub, 2).await;
            // A renovação com outro papel não troca a sessão aberta: o papel registrado muda, o epoch
            // sobe e o canal fecha.
            tokio::time::sleep(Duration::from_millis(1100)).await;
            let novo = com_org(afirmacoes(OUTRO), org, rol);
            let mut renovar = envelope(&p, &hub, sessao::METODO, json!({ sessao::CAMPO_TOKEN: token(&novo) }));
            renovar["id"] = 3.into();
            p.send(renovar).await;
            let (code, reject) = fim(&mut p).await;
            assert_eq!((code, reject), (close::TIMEOUT, Value::Null), "{via:?}");
            // O admin de antes não volta com um token mais velho que o papel novo.
            let mut velho = admin();
            velho["iat"] = json!(now_ms() / 1000 - 5);
            let mut q = conectado_via(&hub, key.clone(), via).await;
            let recusa = autenticar(&mut q, &hub, 1, &token(&velho)).await;
            assert_eq!(codigo(&recusa), (&json!(401), &json!("auth_invalid")));
            // O token novo vale: volta a admin (o epoch sobe de novo) e um canal novo entra.
            trocar_papel(&hub, &key, via, &admin()).await;
            let mut r = conectado_via(&hub, key.clone(), via).await;
            assert_eq!(autenticar(&mut r, &hub, 1, &token(&admin())).await["ok"]["role"], "admin", "token novo vale");
        }
    }
}

/// O pedido admitido espera o lock do efeito; o papel muda antes do efeito.
#[tokio::test]
async fn efeito_admitido_nao_roda_depois_que_o_papel_muda() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_time());
    let admin = || com_org(afirmacoes(OUTRO), ORG, "admin");
    let key = parear(&hub, &admin()).await.unwrap();
    for via in [Via::Direto, Via::Relay] {
        let mut p = conectado_via(&hub, key.clone(), via).await;
        assert!(autenticar(&mut p, &hub, 1, &token(&admin())).await.get("ok").is_some());
        let bloqueio = hub.store.team_lock.lock().await;
        let mut pedido = envelope(&p, &hub, "setRoutinesPaused", json!({"paused": true}));
        pedido["id"] = 2.into();
        p.send(pedido).await;
        // Barreira: o pedido passou da admissão e está na fila do `team_lock`.
        timeout(Duration::from_secs(10), hub.store.espera_equipe.notified()).await.expect("na espera");
        // Outro canal do mesmo usuário apresenta um token mais novo, agora como membro.
        trocar_papel(&hub, &key, via, &com_org(afirmacoes(OUTRO), ORG, "member")).await;
        let (code, reject) = fim(&mut p).await;
        assert_eq!((code, reject), (close::TIMEOUT, Value::Null), "{via:?}");
        drop(bloqueio);
        // O `team_lock` é justo: tomá-lo de novo espera o pedido da fila terminar o efeito.
        drop(hub.store.team_lock.lock().await);
        assert!(!pausadas(&hub), "{via:?}: o efeito confere a guarda na própria transação");
        // Volta a admin para a próxima via.
        trocar_papel(&hub, &key, via, &admin()).await;
    }
}

/// Renovar a sessão não estende o prazo de um pedido já admitido.
#[tokio::test]
async fn renovacao_nao_estende_pedido_ja_admitido() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    for via in [Via::Direto, Via::Relay] {
        let mut p = conectado_via(&hub, key.clone(), via).await;
        assert!(autenticar(&mut p, &hub, 1, &curto(afirmacoes(DONO), 3)).await.get("ok").is_some());
        let bloqueio = hub.store.team_lock.lock().await;
        let mut pedido = envelope(&p, &hub, "setRoutinesPaused", json!({"paused": true}));
        pedido["id"] = 2.into();
        p.send(pedido).await;
        assert!(autenticar(&mut p, &hub, 3, &curto(afirmacoes(DONO), 60)).await.get("ok").is_some());
        tokio::time::sleep(Duration::from_millis(3500)).await;
        drop(bloqueio);
        // A resposta do pedido vencido não sai: o canal fecha com 4011, sem o efeito.
        let (code, _) = fim(&mut p).await;
        assert_eq!(code, close::REKEY, "{via:?}");
        assert!(!pausadas(&hub), "{via:?}");
    }
}

#[tokio::test]
async fn jwks_vencida_fecha_o_remoto_e_o_local_segue() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    // O conjunto vale só mais 2 s e a renovação falha.
    hub.sessao_clerk.jwks.vence_em(Duration::from_secs(2));
    hub.sessao_clerk.jwks.servir(|| anyhow::bail!("fora do ar"));
    let mut p = conectado(&hub, key.clone()).await;
    let ok = autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await;
    assert!(ok["ok"]["expiresAt"].as_i64().unwrap() <= now_ms() + 2000, "o prazo é a validade da JWKS");
    let (code, reject) = fim(&mut p).await;
    assert_eq!((code, reject["stableCode"].as_str()), (4001, Some("auth_invalid")));
    let mut p = conectado(&hub, key).await;
    let vencida = autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await;
    assert_eq!(codigo(&vencida), (&json!(401), &json!("auth_invalid")));
    let local = api::dispatch(&hub, &Caller::Local, "hello", json!({})).await.unwrap();
    assert_eq!(local["computerId"], hub.identity.computer_id());
    let contexto = api::dispatch(&hub, &Caller::Local, CONTEXTO, json!({})).await.unwrap();
    assert_eq!(contexto["role"], "owner");
}

#[tokio::test]
async fn vinculo_ilegivel_nega_o_remoto_e_executor_nao_pareia_com_vinculo() {
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    sessao::vinculo::gravar_para_prova(&hub, "{\"schema\":7}");
    let mut p = conectado(&hub, key).await;
    let recusa = p.call(1, "hello", json!({})).await;
    assert_eq!(codigo(&recusa), (&json!(401), &json!("auth_invalid")));
    assert!(api::dispatch(&hub, &Caller::Local, "hello", json!({})).await.is_ok(), "o Local segue");

    vincular(&hub, &vinculo_pessoal());
    let code = crate::computers::pairing::issue(&hub)["code"].clone();
    let chave = SigningKey::from_bytes(&crypto::random());
    let mut e = Phone::direct(&hub, chave.clone());
    assert!(matches!(e.hello(&hub, true).await, Out::Msg(v) if v["t"] == "welcome"));
    let b = json!({"role": "executor", "code": code, "name": "Executor fictício", "platform": "macos",
        sessao::CAMPO_TOKEN: token(&afirmacoes(DONO))});
    e.send(json!({"id": 1, "m": "pair", "b": b, "pop": pop(&chave, &hub.host_id, &e.th, "pair")})).await;
    assert_eq!(e.recv().await.unwrap()["err"]["status"], 403);
    assert!(hub.store.device(&e.dk()).is_none());
}

#[tokio::test]
async fn pareamento_exige_prova_do_proprio_canal() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = SigningKey::from_bytes(&crypto::random());
    let code = hub.pairing.locked().issue().code;
    let mut p = Phone::direct(&hub, key.clone());
    assert!(matches!(p.hello(&hub, true).await, Out::Msg(v) if v["t"] == "welcome"));
    let b = json!({"code": code, "name": "Telefone fictício", "platform": "ios",
        sessao::CAMPO_TOKEN: token(&afirmacoes(DONO))});
    p.send(json!({"id": 1, "m": "pair", "b": b, "pop": pop(&key, &hub.host_id, &crypto::random(), "pair")})).await;
    assert_eq!(codigo(&p.recv().await.unwrap()), (&json!(401), &json!("device_unpaired")));
    assert!(hub.store.device(&p.dk()).is_none());
}

#[tokio::test]
async fn voz_e_negada_com_vinculo_ate_para_o_dono() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    let mut p = conectado(&hub, key.clone()).await;
    assert!(autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await.get("ok").is_some());
    let b = json!({"provider": "openai", "model": "m", "voice": "v"});
    let negado = com_posse(&mut p, &hub, 2, "voiceSession", b.clone()).await;
    assert_eq!(codigo(&negado), (&json!(403), &json!("role_insufficient")));
    // No ponto do efeito (`api/host.rs`), a credencial do fornecedor não sai para um aparelho com
    // vínculo; o Local segue.
    let dk = crypto::b64(key.verifying_key().as_bytes());
    let (_, caller) = autoridade::autenticar(&hub, &dk).unwrap();
    assert!(sessao::negar_sem_corte(&hub, &caller, "voiceSession").unwrap_err().is::<devices::Forbidden>());
    assert!(sessao::negar_sem_corte(&hub, &Caller::Local, "voiceSession").is_ok());
    assert!(api::dispatch(&hub, &caller, "voiceSession", b).await.is_err());
}

/// Efeito que ainda escapa da guarda da sessão: negado com vínculo, para todos os papéis, no canal
/// (direto e relay) e no despacho.
#[tokio::test]
async fn metodos_sem_cerca_sao_negados_com_vinculo_ate_para_o_dono() {
    let hub = temp_hub();
    vincular(&hub, &vinculo_pessoal());
    let key = parear(&hub, &afirmacoes(DONO)).await.unwrap();
    for via in [Via::Direto, Via::Relay] {
        let mut p = conectado_via(&hub, key.clone(), via).await;
        assert!(autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await.get("ok").is_some());
        for (i, m) in crate::fork::sessao_clerk::tests::SEM_CERCA.iter().enumerate() {
            let id = u64::try_from(i).unwrap() + 2;
            let negado = if let Some(kind) = m.strip_prefix("sub:") {
                let b = com_epoch(&hub, m, json!({"term": "t"}));
                let inner = json!({"sub": kind, "b": b, "pop": pop(&key, &hub.host_id, &p.th, m)});
                pedir(&mut p, id, inner).await
            } else {
                com_posse(&mut p, &hub, id, m, json!({"botId": "bot", "term": "t", "ticket": "x"})).await
            };
            assert_eq!(codigo(&negado), (&json!(403), &json!("role_insufficient")), "{via:?} {m}");
        }
    }
    let dk = crypto::b64(key.verifying_key().as_bytes());
    let (_, caller) = autoridade::autenticar(&hub, &dk).unwrap();
    for m in crate::fork::sessao_clerk::tests::SEM_CERCA.iter().filter(|m| !m.starts_with("sub:")) {
        let erro = api::dispatch(&hub, &caller, m, json!({"botId": "bot"})).await.unwrap_err();
        assert!(erro.is::<devices::Forbidden>(), "{m}: {erro:#}");
    }
}
