#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::fork::engine_accounts::{
    Account, AuthorizedScope, Backend, Billing, LaunchReference, Model, SLOT as ACCOUNTS_SLOT, managed,
};
use crate::remote::identity::Identity;
use crate::store::Store;

const CONTA: &str = "ac_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OUTRA: &str = "ac_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn conta(id: &str, state: State) -> Account {
    Account {
        managed: Some(managed::Metadata {
            label: "Codex".into(),
            revision: 1,
            generation: 0,
            enabled: true,
            paused: false,
            request_id: "teste".into(),
            principal: None,
            profile: None,
            setup_request: None,
            catalog: None,
        }),
        id: id.into(),
        backend: Backend::Codex,
        billing: Billing::Subscription,
        state,
        launch: LaunchReference::CliProfile { executable: "/fixture/codex".into(), directory: "/fixture".into() },
        scope: AuthorizedScope::Owner,
        quota_identity: None,
        verified_at: Some(crate::store::now_ms()),
        subscription_proof: None,
        models: vec![
            Model { id: "gpt-padrao".into(), options: Vec::new() },
            Model { id: "gpt-outro".into(), options: Vec::new() },
        ],
        hidden_model_ids: Vec::new(),
    }
}

struct Env {
    hub: Arc<Hub>,
    _dir: tempfile::TempDir,
}

fn env(accounts: Vec<Account>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("test.db")).unwrap();
    let identity = Identity::load_or_create(dir.path()).unwrap();
    let hub = Hub::new(store, "test-host".into(), identity, "test-token".into(), 0);
    hub.store.initialize_default_donna().unwrap();
    let mut registry = Registry::load(&hub.store).unwrap();
    registry.accounts = accounts;
    hub.store.kv_set(ACCOUNTS_SLOT, &serde_json::to_string(&registry).unwrap()).unwrap();
    Env { hub, _dir: dir }
}

fn berga(hub: &Hub) -> BotConfig {
    let id = hub.store.team_settings().unwrap().primary_bot_id.unwrap();
    hub.store.bot(&id).unwrap().unwrap().config
}

async fn novo_bot(hub: &Arc<Hub>, name: &str, extra: Value) -> String {
    let mut body = json!({"name": name, "backend": "codex"});
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    let created = crate::api::dispatch(hub, &Caller::Local, "createBot", body).await.unwrap();
    created["bot"]["id"].as_str().unwrap().to_owned()
}

fn set(account: &str, model: &str, bot: Option<&str>) -> Set {
    Set {
        account_id: account.into(),
        model: model.into(),
        bot_id: bot.map(str::to_owned),
        space_id: None,
        hub_id: None,
    }
}

fn run(hub: &Arc<Hub>, request: &Set) -> Result<Value> {
    let owner = crate::fork::autoridade::Autoridade::capturar(hub, &Caller::Local).unwrap();
    let space = check_scope(hub, request)?;
    apply(hub, request, &space, &engine_setup::access::Authority::Local, &owner)
}

#[tokio::test]
async fn primeira_conta_vai_para_berga_e_bots_sem_escolha_sem_sobrescrever_escolha() {
    let env = env(vec![conta(CONTA, State::Connected), conta(OUTRA, State::Connected)]);
    let livre = novo_bot(&env.hub, "Livre", json!({})).await;
    let com_conta = novo_bot(&env.hub, "ComConta", json!({"forkEditor": {"engineAccountId": OUTRA}})).await;
    let answer = run(&env.hub, &set(CONTA, "gpt-padrao", None)).unwrap();
    let principal = berga(&env.hub);
    assert_eq!(principal.fork_editor.engine_account_id.as_deref(), Some(CONTA));
    assert_eq!(principal.model.as_deref(), Some("gpt-padrao"));
    let livre_cfg = env.hub.store.bot(&livre).unwrap().unwrap().config;
    assert_eq!(livre_cfg.fork_editor.engine_account_id.as_deref(), Some(CONTA));
    assert_eq!(livre_cfg.model.as_deref(), Some("gpt-padrao"));
    let escolhido = env.hub.store.bot(&com_conta).unwrap().unwrap().config;
    assert_eq!(escolhido.fork_editor.engine_account_id.as_deref(), Some(OUTRA), "escolha existente fica");
    assert!(escolhido.model.is_none());
    assert!(!answer["applied"].as_array().unwrap().iter().any(|id| id == com_conta.as_str()));

    // Uma segunda conta conectada depois não muda quem já recebeu a primeira.
    let again = run(&env.hub, &set(OUTRA, "gpt-outro", None)).unwrap();
    assert_eq!(again["applied"], json!([]));
    assert_eq!(berga(&env.hub).fork_editor.engine_account_id.as_deref(), Some(CONTA));
}

#[tokio::test]
async fn troca_de_um_bot_aplica_so_nele_e_so_com_modelo_comprovado() {
    let env = env(vec![conta(CONTA, State::Connected)]);
    let outro = novo_bot(&env.hub, "Outro", json!({})).await;
    let principal = berga(&env.hub).id;
    run(&env.hub, &set(CONTA, "gpt-outro", Some(&principal))).unwrap();
    assert_eq!(berga(&env.hub).model.as_deref(), Some("gpt-outro"));
    assert!(env.hub.store.bot(&outro).unwrap().unwrap().config.model.is_none(), "outro bot não muda");

    let error = run(&env.hub, &set(CONTA, "gpt-inventado", Some(&principal))).unwrap_err();
    assert!(error.to_string().starts_with("model_unavailable"), "{error}");
    assert_eq!(berga(&env.hub).model.as_deref(), Some("gpt-outro"));
}

#[tokio::test]
async fn conta_desconectada_ou_fora_da_open_e_recusada_sem_gravar() {
    let mut legada = conta(OUTRA, State::Connected);
    legada.managed = None;
    let env = env(vec![conta(CONTA, State::Disconnected), legada]);
    for account in [CONTA, OUTRA] {
        let error = run(&env.hub, &set(account, "gpt-padrao", None)).unwrap_err();
        assert!(error.to_string().starts_with("account_unavailable"), "{error}");
    }
    assert!(berga(&env.hub).fork_editor.engine_account_id.is_none());
    assert!(berga(&env.hub).model.is_none());
}

#[tokio::test]
async fn fora_da_open_o_metodo_nao_existe_e_remoto_nunca_chega() {
    let env = env(vec![conta(CONTA, State::Connected)]);
    let body = json!({"accountId": CONTA, "model": "gpt-padrao"});
    if !crate::environment::Environment::is_open() {
        let error = call(&env.hub, &Caller::Local, body.clone()).await.unwrap_err();
        assert!(error.is::<Forbidden>());
    }
    let remote = Caller::Device { key: "dev-1".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    assert!(crate::api::dispatch(&env.hub, &remote, SET_METHOD, body).await.is_err());
    assert!(berga(&env.hub).model.is_none());
}

#[test]
fn padrao_do_codex_vem_primeiro_no_catalogo() {
    let models = crate::fork::engine_accounts::probe::codex_models(&json!({"data": [
        {"id": "gpt-a"}, {"id": "gpt-b", "isDefault": true}, {"id": "gpt-c"}
    ]}));
    assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["gpt-b", "gpt-a", "gpt-c"]);
}

#[tokio::test]
async fn bot_so_no_proprio_space_e_lote_global_so_no_pessoal() {
    let env = env(vec![conta(CONTA, State::Connected)]);
    let pessoal = env.hub.store.spaces().unwrap().personal_space_id;
    let principal = berga(&env.hub).id;
    let com = |bot: Option<&str>, space: Option<&str>, hub: Option<&str>| Set {
        space_id: space.map(str::to_owned),
        hub_id: hub.map(str::to_owned),
        ..set(CONTA, "gpt-outro", bot)
    };
    let outro = run(&env.hub, &com(Some(&principal), Some("sp_outro"), None)).unwrap_err();
    assert!(outro.to_string().starts_with("bot_not_found"), "{outro}");
    let lote = run(&env.hub, &com(None, Some("sp_outro"), None)).unwrap_err();
    assert!(lote.to_string().starts_with("space_not_personal"), "{lote}");
    let hub = run(&env.hub, &com(Some(&principal), Some(&pessoal), Some("outro-hub"))).unwrap_err();
    assert!(hub.to_string().starts_with("cross_hub_reference"), "{hub}");
    assert!(berga(&env.hub).model.is_none(), "nada gravado nas recusas");
    run(&env.hub, &com(Some(&principal), Some(&pessoal), Some("test-host"))).unwrap();
    assert_eq!(berga(&env.hub).model.as_deref(), Some("gpt-outro"));
    let body = json!({"accountId": CONTA, "model": "gpt-padrao", "botId": principal, "spaceId": pessoal, "hubId": "test-host"});
    assert!(serde_json::from_value::<Set>(body).is_ok(), "o cliente vinculado manda spaceId e hubId");
}

#[tokio::test]
async fn sem_escolha_a_conta_que_comprovou_o_modelo_legado_vence_a_mais_recente() {
    let mut antiga = conta(CONTA, State::Connected);
    antiga.verified_at = Some(1);
    antiga.models.push(Model { id: "gpt-legado".into(), options: Vec::new() });
    let env = env(vec![antiga, conta(OUTRA, State::Connected)]);
    let mut cfg = berga(&env.hub);
    let (account, _, model) = crate::fork::open_chat::account_profile(&env.hub, &cfg).unwrap();
    assert_eq!((account.as_str(), model), (OUTRA, None), "sem modelo: a mais recente");
    cfg.model = Some("gpt-legado".into());
    let (account, _, model) = crate::fork::open_chat::account_profile(&env.hub, &cfg).unwrap();
    assert_eq!((account.as_str(), model.as_deref()), (CONTA, Some("gpt-legado")));
}

#[tokio::test]
async fn aba_usa_a_conta_gravada_na_criacao_e_nao_a_atual_do_bot() {
    use crate::fork::sessoes::{admissao, origin::SessionOrigin, store as sessoes};
    let env = env(vec![conta(CONTA, State::Connected), conta(OUTRA, State::Connected)]);
    let principal = berga(&env.hub).id;
    run(&env.hub, &set(CONTA, "gpt-padrao", Some(&principal))).unwrap();
    let aba = sessoes::create(
        &env.hub.store.db.locked(),
        &sessoes::NewSession {
            chat: principal.clone(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: "Aba".into(),
            runs_on: None,
        },
    )
    .unwrap()
    .id;
    run(&env.hub, &set(OUTRA, "gpt-outro", Some(&principal))).unwrap();
    let lane = crate::store::Lane { chat: principal.clone(), session: Some(aba) };

    let mut efetiva = berga(&env.hub);
    admissao::apply_configured(&env.hub.store.db.locked(), &lane, &mut efetiva).unwrap();
    assert_eq!(crate::fork::open_chat::account_profile(&env.hub, &berga(&env.hub)).unwrap().0, OUTRA);
    assert_eq!(crate::fork::open_chat::account_profile(&env.hub, &efetiva).unwrap().0, CONTA);

    let mut registry = Registry::load(&env.hub.store).unwrap();
    registry.accounts.retain(|a| a.id != CONTA);
    env.hub.store.kv_set(ACCOUNTS_SLOT, &serde_json::to_string(&registry).unwrap()).unwrap();
    let error = crate::fork::open_chat::account_profile(&env.hub, &efetiva).unwrap_err();
    assert_eq!(
        error.to_string(),
        "open_chat_account_required",
        "conta da aba indisponível recusa em vez de cair na do bot"
    );
}
