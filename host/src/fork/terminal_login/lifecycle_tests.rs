//! Fronteiras de promoção e do vínculo de uma tentativa de login.
use super::*;
use crate::fork::terminal_login::{self, Login};

#[cfg(unix)]
fn attempt(f: &Fixture, caller: &Caller) -> Arc<Login> {
    use std::os::unix::fs::PermissionsExt;
    let cli = f.root.join("cursor-check");
    std::fs::write(&cli, "#!/usr/bin/python3\nprint('Logged in as fixture')\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    Login::fixture(&f.hub, caller, "cursor", cli).unwrap()
}

#[cfg(unix)]
#[tokio::test]
async fn so_promove_depois_da_conferencia_e_conserva_anterior_ao_cancelar_ou_expirar() {
    let f = Fixture::new();
    let first = attempt(&f, &Caller::Local);
    assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), "");
    assert_eq!(terminal_login::runtime_env(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), []);
    assert!(first.complete().unwrap_err().to_string().contains("não confirmado"));
    assert_eq!(first.check().await, Some(true));
    first.complete().unwrap();
    std::fs::write(first.home.join("anterior"), "preservada").unwrap();
    let before = terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap();
    let env = terminal_login::runtime_env(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap();
    for expired in [false, true] {
        let mut next = attempt(&f, &Caller::Local);
        assert_ne!(first.home, next.home);
        std::fs::write(next.home.join("credencial"), "provisoria").unwrap();
        assert_eq!(next.check().await, Some(true));
        assert_eq!(terminal_login::runtime_env(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), env);
        assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), before);
        if expired {
            Arc::get_mut(&mut next).unwrap().deadline = tokio::time::Instant::now() - Duration::from_secs(1);
        } else {
            next.stop();
        }
        assert!(next.complete().unwrap_err().to_string().contains("encerrado"));
        next.discard().unwrap();
        assert!(!next.home.exists());
        assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), before);
        assert_eq!(std::fs::read_to_string(first.home.join("anterior")).unwrap(), "preservada");
    }
    let next = attempt(&f, &Caller::Local);
    assert_eq!(next.check().await, Some(true));
    next.complete().unwrap();
    assert_ne!(terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), before);
    // Troca confirmada: a credencial da conta anterior sai do disco.
    assert!(!first.home.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn revogar_e_conceder_de_novo_ao_mesmo_aparelho_nao_renova_login_em_andamento() {
    let f = Fixture::new();
    let owner = f.owner();
    let login = attempt(&f, &owner);
    let original = f.hub.store.device(owner.device_key()).unwrap();
    assert_eq!(login.check().await, Some(true));
    f.hub.revoke_device(owner.device_key()).unwrap();
    // A mesma concessão regravada (releitura atrasada) fica bloqueada; concessão nova ao mesmo
    // aparelho é outro `grant_id`, e a ligação do motor era da concessão revogada.
    f.hub.store.put_device(&original).unwrap();
    assert_eq!(
        crate::fork::autoridade::autenticar(&f.hub, owner.device_key()).err(),
        Some(crate::api::devices::RejectCode::Revoked)
    );
    let nova = crate::store::Device { grant_id: Some("grt_concessao_nova".into()), ..original };
    f.hub.store.put_device(&nova).unwrap();
    f.hub.store.bootstrap_activation_envelope(&f.hub.identity.sign, crate::store::now_ms()).unwrap();
    let renewed = crate::fork::autoridade::autenticar(&f.hub, owner.device_key()).unwrap().1;
    assert!(access::resolve(&f.hub, &renewed).is_err(), "a ligação do motor não passa para a concessão nova");
    assert!(login.require(&renewed).is_err(), "tentativa continua vinculada à origem anterior");
    assert!(login.deliver(|| panic!("não pode entregar saída")).is_err());
    assert!(login.complete().is_err());
    login.discard().unwrap();
    assert!(!login.home.exists());
    assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn troca_de_posse_nao_promove_nem_renova_tentativa_ao_voltar_ao_dono() {
    let f = Fixture::new();
    let owner = f.owner();
    let login = attempt(&f, &owner);
    assert_eq!(login.check().await, Some(true));
    crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "outro-dono".into());
    assert!(login.require(&owner).is_err());
    assert!(login.complete().is_err());
    crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "owner".into());
    assert!(login.require(&owner).is_err(), "a geração de posse original foi encerrada");
    assert!(login.complete().is_err());
    login.discard().unwrap();
    assert!(!login.home.exists());
    assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", crate::fork::spaces::PERSONAL).unwrap(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn outro_aparelho_com_control_inclusive_do_dono_nao_le_nem_escreve_login() {
    let f = Fixture::new();
    let owner = f.owner();
    let mut device = f.hub.store.device(owner.device_key()).unwrap();
    device.key = "outro-aparelho-do-dono".into();
    f.hub.store.put_device(&device).unwrap();
    f.hub.store.bootstrap_activation_envelope(&f.hub.identity.sign, crate::store::now_ms()).unwrap();
    let other_owner = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap().1;
    let ordinary =
        Caller::Device { key: "control-sem-acs4".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    let login = attempt(&f, &owner);
    let id = f.hub.terms.fixture_login(&f.hub, &owner, "cursor", login.executable.clone()).unwrap();
    for caller in [ordinary, other_owner, Caller::Local] {
        assert!(crate::api::term_events(&f.hub, &id, &caller).is_err());
        for method in ["termInput", "termResize", "termClose"] {
            let error =
                crate::api::dispatch(&f.hub, &caller, method, json!({"term":id,"data":"QUJDRA0=","cols":80,"rows":24}))
                    .await
                    .unwrap_err();
            assert!(
                error.to_string().contains("vínculo")
                    || error.to_string().contains("autorização")
                    || error.to_string().contains("autoridade"),
                "{method}: {error:#}"
            );
        }
    }
    f.hub.terms.close(&id);
}

#[test]
fn atualizar_sem_perfil_nao_acrescenta_sufixo_nem_muda_carimbo_de_motor_algum() {
    let f = Fixture::new();
    for h in crate::agent::backends::HARNESSES {
        assert!(
            terminal_login::stamp(&f.hub.store, h.id, crate::fork::spaces::PERSONAL).unwrap().is_empty(),
            "{}",
            h.id
        );
    }
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Rodar na máquina com os CLIs reais instalados: MEXERICA_REAL_LOGIN_ENGINES=cursor,grok,antigravity; não usa contas anteriores nem envia código"]
async fn cli_real_por_motor_recusa_escape_controles_editor_e_pager() {
    use base64::Engine as _;
    use futures::StreamExt;
    let engines = std::env::var("MEXERICA_REAL_LOGIN_ENGINES").expect("informe os motores reais a conferir");
    assert_ne!(engines, "");
    for backend in engines.split(',') {
        terminal_login::fixed_args(backend).unwrap();
        let f = Fixture::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let id = f
            .hub
            .terms
            .start_login(&f.hub, &Caller::Local, backend, crate::fork::spaces::PERSONAL, 80, 24)
            .await
            .unwrap();
        let login = f.hub.terms.get(&id).unwrap().login.clone().unwrap();
        let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &Caller::Local).unwrap().unwrap());
        let event = tokio::time::timeout(Duration::from_secs(30), stream.next()).await.unwrap().unwrap();
        assert_eq!(event["type"], "output", "{backend}: o CLI real precisa abrir o login");
        for input in ["!ls\n", "/shell\n", "\u{3}", "\u{4}", "\u{1a}", "\u{1b}", "vi /tmp/test\n", "less /tmp/test\n"] {
            let data = base64::prelude::BASE64_STANDARD.encode(input);
            let error = crate::api::dispatch(&f.hub, &Caller::Local, "termInput", json!({"term":id,"data":data}))
                .await
                .unwrap_err();
            // agy antes da escolha só aceita Enter; depois todos os motores usam o filtro de código.
            assert!(
                error.to_string().contains("login")
                    || error.to_string().contains("comandos")
                    || error.to_string().contains("código")
                    || error.to_string().contains("OAuth"),
                "{backend}: {error:#}"
            );
        }
        f.hub.terms.close(&id);
        esperar(|| !login.home.exists()).await;
        assert_eq!(terminal_login::runtime_env(&f.hub.store, backend, crate::fork::spaces::PERSONAL).unwrap(), []);
    }
}

#[cfg(unix)]
async fn conferir_pty_remoto(backend: &str, binary: &str) {
    use base64::{Engine as _, prelude::BASE64_STANDARD as B64};
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let owner = f.owner();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/motor_cli.py");
    let wrapper = f.root.join(binary);
    // Exec do fixture, sem shell: conserva os argumentos e o pai real do CLI.
    std::fs::write(&wrapper, format!("#!/usr/bin/python3\nimport os,sys\nos.execv('/usr/bin/python3', ['/usr/bin/python3', {:?}, {:?}, {:?}, *sys.argv[1:]])\n", fixture.to_string_lossy(), f.root.to_string_lossy(), binary)).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(f.root.join(format!("scenario-{binary}")), "logged_out").unwrap();
    let id = f.hub.terms.fixture_login(&f.hub, &owner, backend, wrapper).unwrap();
    assert!(!f.hub.terms.busy(), "login não se confunde com instalação");
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &owner).unwrap().unwrap());
    let challenge = tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = String::new();
        while let Some(event) = stream.next().await {
            if event["type"] == "output" {
                output.push_str(&String::from_utf8(B64.decode(event["data"].as_str().unwrap()).unwrap()).unwrap());
                if output.contains("digite o código") {
                    return;
                }
            }
        }
        panic!("{backend}: CLI não apresentou desafio");
    })
    .await;
    challenge.unwrap();
    for (text, reason) in [
        ("!ls\n", "sem comandos ou controles"),
        ("/shell\n", "Comandos não são aceitos"),
        ("\u{3}", "sem comandos ou controles"),
        ("\u{4}", "sem comandos ou controles"),
        ("\u{1a}", "sem comandos ou controles"),
        ("\u{1b}", "Tecla indisponível"),
        ("vi /tmp/test\n", "sem comandos ou controles"),
        ("less /tmp/test\n", "sem comandos ou controles"),
        ("$(whoami)\n", "sem comandos ou controles"),
        ("`whoami`\n", "sem comandos ou controles"),
        (";whoami\n", "sem comandos ou controles"),
    ] {
        let error = crate::api::dispatch(&f.hub, &owner, "termInput", json!({"term":id,"data":B64.encode(text)}))
            .await
            .unwrap_err();
        let expected = if backend == "antigravity" { reason } else { "Aguarde" };
        assert!(error.to_string().contains(expected), "{backend}: {text:?}: {error:#}");
    }
    assert!(!f.root.join(format!("inputs-{binary}.log")).exists(), "nenhum escape chegou ao CLI");
    if backend == "antigravity" {
        f.remote(&owner, "termInput", json!({"term":id,"data":B64.encode("CODIGO-OK\r")})).await;
    } else {
        std::fs::write(f.root.join(format!("scenario-{binary}")), "logged_in").unwrap();
    }
    assert!(
        crate::api::dispatch(&f.hub, &owner, "termInput", json!({"term":id,"data":B64.encode("comando\r")}))
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(event) = stream.next().await {
            if let Some(data) = event["data"].as_str() {
                assert!(!String::from_utf8_lossy(&B64.decode(data).unwrap()).contains("CODIGO-OK"));
            }
            if event["type"] == "verified" {
                return;
            }
        }
        panic!("{backend}: login não confirmado");
    })
    .await
    .unwrap();
    let parent = std::fs::read_to_string(f.root.join(format!("parent-{binary}"))).unwrap();
    assert!(!["sh", "bash", "zsh", "dash", "fish"].contains(&parent.trim()), "{backend}: sem shell no pai");
    esperar(|| f.hub.terms.get(&id).is_none()).await;
    assert!(!f.root.join(format!("command-after-login-{binary}")).exists());
}

#[cfg(unix)]
#[tokio::test]
async fn cursor_pty_remoto_codigo_controles_editor_pager_sem_escape() {
    conferir_pty_remoto("cursor", "cursor-agent").await;
}
#[cfg(unix)]
#[tokio::test]
async fn grok_pty_remoto_codigo_controles_editor_pager_sem_escape() {
    conferir_pty_remoto("grok", "grok").await;
}
#[cfg(unix)]
#[tokio::test]
async fn antigravity_pty_remoto_codigo_controles_editor_pager_sem_escape() {
    conferir_pty_remoto("antigravity", "agy").await;
}

#[cfg(unix)]
#[tokio::test]
async fn limpa_so_operacao_vencida_e_preserva_perfis_sem_marcador_e_promovidos() {
    let f = Fixture::new();
    let promoted = attempt(&f, &Caller::Local);
    assert_eq!(promoted.check().await, Some(true));
    promoted.complete().unwrap();
    let stale = attempt(&f, &Caller::Local);
    let expired = (crate::store::now_ms() - 1).to_string();
    let operation = stale.home.file_name().unwrap().to_str().unwrap();
    std::fs::write(stale.home.join(".donnabot-login-operation"), format!("{operation} {expired}\n")).unwrap();
    f.hub.store.kv_set(&terminal_login::attempt_key(&stale.home).unwrap(), &expired).unwrap();
    let cancelled = attempt(&f, &Caller::Local);
    let key = terminal_login::attempt_key(&cancelled.home).unwrap();
    let deadline = f.hub.store.kv_read(&key).unwrap().unwrap();
    crate::fork::motor_exec::login_cleanup::cancel(&cancelled.home, &deadline).unwrap();
    f.hub.store.kv_set(&key, &format!("c:{deadline}")).unwrap();
    let legacy = stale.home.parent().unwrap().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("credencial"), "preservada").unwrap();
    assert_eq!(crate::fork::motor_exec::login_cleanup::count_unmarked(legacy.parent().unwrap()).unwrap(), 1);
    let next = attempt(&f, &Caller::Local);
    assert!(!stale.home.exists());
    assert!(!cancelled.home.exists());
    assert!(promoted.home.exists());
    assert_eq!(std::fs::read_to_string(legacy.join("credencial")).unwrap(), "preservada");
    next.stop();
    next.discard().unwrap();
    assert!(!next.home.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn pty_morre_quando_cli_falso_entra_no_modo_agente() {
    use base64::{Engine as _, prelude::BASE64_STANDARD as B64};
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let cli = f.root.join("agy-agent-mode");
    let pid_file = f.root.join("agent-mode-pid");
    std::fs::write(
        &cli,
        format!(
            r"#!/usr/bin/python3
import os, sys, time
if len(sys.argv) > 1:
    sys.exit(1)
open({:?}, 'w').write(str(os.getpid()))
print('https://accounts.google.com/auth digite o código', flush=True)
sys.stdin.readline()
print('agent mode', flush=True)
time.sleep(300)
",
            pid_file.to_string_lossy()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = f.hub.terms.fixture_login(&f.hub, &Caller::Local, "antigravity", cli).unwrap();
    let login = f.hub.terms.get(&id).unwrap().login.clone().unwrap();
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &Caller::Local).unwrap().unwrap());
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            if event["data"]
                .as_str()
                .is_some_and(|data| String::from_utf8_lossy(&B64.decode(data).unwrap()).contains("digite o código"))
            {
                return;
            }
        }
        panic!("desafio ausente");
    })
    .await
    .unwrap();
    crate::api::dispatch(&f.hub, &Caller::Local, "termInput", json!({"term":id,"data":B64.encode("CODIGO\r")}))
        .await
        .unwrap();
    esperar(|| f.hub.terms.get(&id).is_none() && !login.home.exists()).await;
    let pid: i32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
    // SAFETY: sinal zero somente consulta a existência do processo falso que o teste criou.
    let result = unsafe { libc::kill(pid, 0) };
    assert_eq!(result, -1, "processo do agente continuou vivo");
    assert!(!login.home.exists());
    assert_eq!(terminal_login::runtime_env(&f.hub.store, "antigravity", crate::fork::spaces::PERSONAL).unwrap(), []);
}

#[cfg(unix)]
#[tokio::test]
async fn pty_apos_codigo_morre_no_cancelamento_e_no_prazo() {
    use base64::{Engine as _, prelude::BASE64_STANDARD as B64};
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    for cancel in [true, false] {
        let f = Fixture::new();
        let cli = f.root.join("agy-espera");
        let pid_file = f.root.join("aguarda-pid");
        let received = f.root.join("recebeu-codigo");
        std::fs::write(
            &cli,
            format!(
                r"#!/usr/bin/python3
import os, sys, time
if len(sys.argv) > 1:
    sys.exit(1)
open({:?}, 'w').write(str(os.getpid()))
print('https://accounts.google.com/auth digite o código', flush=True)
sys.stdin.readline()
open({:?}, 'w').write('recebido')
time.sleep(300)
",
                pid_file.to_string_lossy(),
                received.to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        let id = f.hub.terms.fixture_login_timeout(&f.hub, &Caller::Local, cli, Duration::from_secs(3)).unwrap();
        let login = f.hub.terms.get(&id).unwrap().login.clone().unwrap();
        let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &Caller::Local).unwrap().unwrap());
        tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(event) = stream.next().await {
                if event["data"]
                    .as_str()
                    .is_some_and(|data| String::from_utf8_lossy(&B64.decode(data).unwrap()).contains("digite o código"))
                {
                    return;
                }
            }
            panic!("desafio ausente");
        })
        .await
        .unwrap();
        crate::api::dispatch(&f.hub, &Caller::Local, "termInput", json!({"term":id,"data":B64.encode("CODIGO\r")}))
            .await
            .unwrap();
        esperar(|| received.exists()).await;
        assert!(
            crate::api::dispatch(
                &f.hub,
                &Caller::Local,
                "termInput",
                json!({"term":id,"data":B64.encode("comando\r")})
            )
            .await
            .is_err()
        );
        let pid: i32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        if cancel {
            crate::api::dispatch(&f.hub, &Caller::Local, "termClose", json!({"term":id})).await.unwrap();
        }
        esperar(|| f.hub.terms.get(&id).is_none() && !login.home.exists()).await;
        // SAFETY: sinal zero consulta somente o PID criado por esta fixture.
        let result = unsafe { libc::kill(pid, 0) };
        assert_eq!(result, -1, "CLI permaneceu ativo: cancelamento={cancel}");
    }
}
