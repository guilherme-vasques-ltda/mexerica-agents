//! D70: fronteiras do painel e JSON-RPC real, sem CLI/login do operador.
use super::*;
use crate::remote::identity::Identity;
use serde_json::json;

const FIXTURE_TIMEOUT: Duration = Duration::from_secs(60);

struct Fixture {
    hub: std::sync::Arc<Hub>,
    directory: std::path::PathBuf,
    executable: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let store = Store::open(Path::new(":memory:")).expect("operação da fixture");
        let root = store.engine_setup_root.clone();
        std::fs::create_dir_all(&root).expect("operação da fixture");
        let identity = Identity::load_or_create(&root).expect("operação da fixture");
        let directory = root.join(".codex");
        std::fs::create_dir_all(&directory).expect("operação da fixture");
        let executable = root.join("codex-fixture");
        crate::fork::test_support::write_executable(
            &executable,
            include_bytes!("../../../tests/fixtures/codex_current_usage.py"),
            0o700,
        );
        let hub = Hub::new(store, "fixture-host".into(), identity, "fixture-token".into(), 0);
        Self { hub, directory, executable }
    }
    fn rates(&self, value: &Value) {
        std::fs::write(self.directory.join("rates.json"), value.to_string()).expect("operação da fixture");
    }
    async fn refresh(&self) {
        refresh_at(&self.hub, Some(&self.executable), &self.hub.store.engine_setup_root, self.env(), FIXTURE_TIMEOUT)
            .await;
    }
    fn env(&self) -> Vec<(OsString, OsString)> {
        vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), self.hub.store.engine_setup_root.as_os_str().into()),
            ("CODEX_HOME".into(), self.directory.as_os_str().into()),
        ]
    }
    fn codex(&self) -> Option<Provider> {
        self.hub.usage.locked().providers.iter().find(|p| p.id == "codex").cloned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.hub.store.engine_setup_root);
    }
}
fn rates(percent: i64) -> Value {
    json!({"rateLimits":{"primary":{"usedPercent":percent,"resetsAt":4_102_444_800_i64}}})
}

#[tokio::test]
async fn refresh_admitido_publica_e_epoch_antigo_nao_substitui_consumo() {
    use crate::{
        api::devices::Caller,
        fork::{
            autoridade,
            workspace::{self, Motivo},
        },
        store::{Device, DeviceSource, Scope, now_ms},
    };
    let f = Fixture::new();
    let key = "fixture-usage-device";
    f.hub
        .store
        .put_device(&Device {
            key: key.into(),
            name: "Fixture".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let (_, caller) = autoridade::autenticar(&f.hub, key).unwrap();
    let Caller::Device { autoridade: Some(original), .. } = caller else { panic!("aparelho autenticado") };
    let authority = original.para_requisicao(f.hub.workspace.atual(), original.geracao_capturada().unwrap()).unwrap();
    for (changed, percent) in [(false, 23), (true, 71)] {
        if changed {
            workspace::bump(&f.hub, Motivo::RoleChanged).unwrap();
        }
        f.rates(&rates(percent));
        let result = refresh_at_authorized(
            &f.hub,
            Some(&f.executable),
            &f.hub.store.engine_setup_root,
            f.env(),
            FIXTURE_TIMEOUT,
            Some(&authority),
        )
        .await;
        assert_eq!(result.is_ok(), !changed);
        assert_eq!(f.codex().unwrap().windows[0].percent, 23.0);
    }
}

#[tokio::test]
async fn rpc_atual_substitui_rollout_e_remove_janelas_ausentes_sem_apagar_claude() {
    let f = Fixture::new();
    let sessions = f.directory.join("sessions/2026/11/06");
    std::fs::create_dir_all(&sessions).expect("operação da fixture");
    std::fs::write(sessions.join("rollout-old.jsonl"), "{\"rate_limits\":{\"primary\":{\"used_percent\":99}}}\n")
        .expect("operação da fixture");
    crate::usage::ingest_statusline(&f.hub, &json!({"rate_limits":{"five_hour":{"used_percentage":17}}}));
    f.rates(&json!({"rateLimitsByLimitId":{"codex":{
        "primary":{"usedPercent":23,"resetsAt":4_102_444_800_i64},
        "secondary":{"usedPercent":42,"resetsAt":4_102_444_800_i64}}},"rateLimits":{"primary":{"usedPercent":99}}}));
    f.refresh().await;
    assert_eq!(f.codex().expect("operação da fixture").windows[0].percent, 23.0);
    assert_eq!(f.codex().expect("operação da fixture").windows.len(), 2);
    assert_eq!(
        std::fs::read_to_string(f.directory.join("requests")).expect("operação da fixture"),
        "initialize\ninitialized\naccount/rateLimits/read\n"
    );
    f.rates(&rates(31));
    f.refresh().await;
    assert_eq!(f.codex().expect("operação da fixture").windows.len(), 1);
    assert_eq!(f.codex().expect("operação da fixture").windows[0].percent, 31.0);
    f.rates(&json!({"rateLimits":{}}));
    f.refresh().await;
    assert!(f.codex().is_none(), "empty não é 0% nem conserva snapshot");
    assert_eq!(f.hub.usage.locked().providers[0].id, "claude");
}

#[tokio::test]
async fn falha_desautenticacao_e_cli_ausente_limpam_snapshot() {
    let f = Fixture::new();
    for mode in ["error", "signed-out", "malformed"] {
        f.rates(&rates(19));
        std::fs::write(f.directory.join("mode"), "ready").expect("operação da fixture");
        f.refresh().await;
        assert!(f.codex().is_some());
        std::fs::write(f.directory.join("mode"), mode).expect("operação da fixture");
        f.refresh().await;
        assert!(f.codex().is_none(), "{mode}");
    }
    std::fs::write(f.directory.join("mode"), "ready").expect("operação da fixture");
    f.refresh().await;
    refresh_at(&f.hub, None, &f.hub.store.engine_setup_root, f.env(), FIXTURE_TIMEOUT).await;
    assert!(f.codex().is_none());
    f.refresh().await;
    std::fs::remove_file(&f.executable).expect("operação da fixture");
    f.refresh().await;
    assert!(f.codex().is_none());
}

#[test]
fn replace_grava_amostra_no_historico_de_uso() {
    let f = Fixture::new();
    replace(
        &f.hub,
        vec![Window {
            id: "primary".into(),
            label: "5-hour".into(),
            percent: 23.0,
            resets_at: Some(4_102_444_800_000),
            resets_text: None,
            minutes: None,
        }],
    );
    let amostras: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_usage_samples WHERE provider='codex' AND window_id='primary'", [], |r| {
            r.get(0)
        })
        .expect("operação da fixture");
    assert_eq!(amostras, 1);
}

#[test]
fn bucket_codex_presente_invalido_nao_recupera_compat_e_expirado_nao_e_atual() {
    for codex in [json!({}), Value::Null, json!({"primary":{"usedPercent":101}})] {
        assert_eq!(
            parse(&json!({"rateLimitsByLimitId":{"codex":codex},"rateLimits":{"primary":{"usedPercent":99}}}), 1000)
                .len(),
            0
        );
    }
    assert_eq!(
        parse(&json!({"rateLimitsByLimitId":{"other":{}},"rateLimits":{"primary":{"usedPercent":99}}}), 1000).len(),
        0
    );
    assert_eq!(parse(&json!({"rateLimits":{"primary":{"usedPercent":23,"resetsAt":1}}}), 1000).len(), 0);
    assert_eq!(parse(&rates(0), 1000)[0].percent, 0.0, "zero explícito é válido");
}

#[tokio::test]
async fn perfil_global_default_e_explicito_invalido_sem_fallback() {
    let f = Fixture::new();
    f.rates(&rates(27));
    let mut env = f.env();
    env.retain(|(key, _)| key != "CODEX_HOME");
    refresh_at(&f.hub, Some(&f.executable), &f.hub.store.engine_setup_root, env, FIXTURE_TIMEOUT).await;
    assert_eq!(f.codex().expect("operação da fixture").windows[0].percent, 27.0);
    let mut env = f.env();
    env.iter_mut().find(|(key, _)| key == "CODEX_HOME").expect("operação da fixture").1 =
        f.directory.join("missing").into_os_string();
    refresh_at(&f.hub, Some(&f.executable), &f.hub.store.engine_setup_root, env, FIXTURE_TIMEOUT).await;
    assert!(f.codex().is_none(), "perfil explícito inválido não usa default válido");
}

async fn ready(listener: &tokio::net::UnixListener) -> (tokio::net::UnixStream, Vec<u32>) {
    use tokio::io::AsyncBufReadExt;
    let (stream, _) = listener.accept().await.expect("operação da fixture");
    let mut reader = tokio::io::BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("operação da fixture");
    (reader.into_inner(), serde_json::from_str(&line).expect("operação da fixture"))
}

#[tokio::test]
async fn timeout_limpa_snapshot_e_encerra_grupo_da_fixture() {
    use tokio::io::AsyncReadExt;
    let f = Fixture::new();
    f.rates(&rates(19));
    f.refresh().await;
    std::fs::write(f.directory.join("mode"), "controlled-timeout").expect("operação da fixture");
    let listener = tokio::net::UnixListener::bind(f.directory.join("ready.sock")).expect("operação da fixture");
    let query = f.refresh();
    tokio::pin!(query);
    let (mut stream, pids) = tokio::select! {
        () = &mut query => panic!("terminou antes da barreira"),
        result = ready(&listener) => result,
    };
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    query.await;
    tokio::time::resume();
    assert!(f.codex().is_none());
    assert_eq!(stream.read(&mut [0]).await.expect("operação da fixture"), 0, "filho encerrou o socket");
    #[cfg(target_os = "linux")]
    for pid in pids {
        tokio::time::timeout(FIXTURE_TIMEOUT, async {
            loop {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
                if stat.is_err()
                    || stat
                        .expect("operação da fixture")
                        .split_once(") ")
                        .expect("operação da fixture")
                        .1
                        .starts_with('Z')
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("operação da fixture");
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pids;
}

#[tokio::test]
async fn duas_leituras_serializam_publicacao_sem_ressuscitar_resposta_atrasada() {
    use tokio::io::AsyncWriteExt;
    let first = Fixture::new();
    let next = Fixture::new();
    first.rates(&rates(23));
    next.rates(&rates(31));
    std::fs::write(first.directory.join("mode"), "controlled").expect("operação da fixture");
    let listener = tokio::net::UnixListener::bind(first.directory.join("ready.sock")).expect("operação da fixture");
    let first_query = first.refresh();
    tokio::pin!(first_query);
    let (mut stream, _) = tokio::select! {
        () = &mut first_query => panic!("terminou antes da barreira"),
        result = ready(&listener) => result,
    };
    let second_query =
        refresh_at(&first.hub, Some(&next.executable), &next.hub.store.engine_setup_root, next.env(), FIXTURE_TIMEOUT);
    tokio::pin!(second_query);
    tokio::select! {
        biased;
        () = &mut second_query => panic!("leitura seguinte ultrapassou a primeira"),
        () = std::future::ready(()) => {},
    }
    stream.write_all(b"x").await.expect("operação da fixture");
    tokio::join!(first_query, second_query);
    assert_eq!(first.codex().expect("operação da fixture").windows[0].percent, 31.0);
}
