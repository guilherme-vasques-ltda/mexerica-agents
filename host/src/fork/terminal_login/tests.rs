use super::*;
#[test]
fn comandos_fixos_sem_shell_e_sem_pi_ou_hermes() {
    assert_eq!(fixed_args("cursor").unwrap(), ["login"]);
    assert_eq!(fixed_args("grok").unwrap(), ["login", "--device-auth"]);
    assert_eq!(fixed_args("antigravity-acp").unwrap(), [] as [String; 0]);
    for harness in backends::HARNESSES {
        if !matches!(harness.id, "cursor" | "grok" | "antigravity") {
            assert!(fixed_args(harness.id).unwrap_err().to_string().contains("não oferecido"), "{}", harness.id);
        }
    }
    for engine in ["/bin/sh", "sh -c whoami", "unknown"] {
        assert!(fixed_args(engine).unwrap_err().to_string().contains("sem comando de login verificado"));
    }
}
pub(super) fn login() -> Login {
    Login {
        hub: Weak::new(),
        authority: Authority::Local,
        backend: "antigravity".into(),
        space: "personal".into(),
        executable: "/fixture".into(),
        args: vec![],
        home: "/perfil-fixture-inexistente".into(),
        env: vec![],
        deadline: Instant::now() + TTL,
        attached: AtomicBool::new(false),
        ready: tokio::sync::Notify::new(),
        cancelled: AtomicBool::new(false),
        stopped: tokio::sync::Notify::new(),
        state: Mutex::default(),
        admission: Mutex::new(()),
        promoted: AtomicBool::new(false),
        verified: AtomicBool::new(false),
        agent_mode: AtomicBool::new(false),
    }
}
#[test]
fn entrada_so_codigo_apos_desafio_sem_escape_e_sem_eco() {
    let login = login();
    assert!(login.input(b"fixture-code\r").is_err());
    let clean =
        login.output(b"\x1b]52;c;secret\x07https://accounts.google.com/o/oauth2/v2/auth?fixture=1\r\n").unwrap();
    assert!(!String::from_utf8_lossy(&clean).contains("secret"));
    for escape in [
        b"\x03".as_slice(),
        b"\x1a",
        b"\x04",
        b"\x1b]52;evil\x07",
        b"fixture\nwhoami",
        b"https://localhost/?code=fixture",
        b"$(whoami)",
        b"a\x7f/shell\r",
        b"a\x08/shell\r",
    ] {
        assert!(login.input(escape).is_err());
    }
    assert!(login.input(b"\x1b[A").is_err());
    assert_eq!(login.input(b"fixture-code\r").unwrap(), b"fixture-code\r");
    assert!(login.input(b"outro\r").is_err());
    assert_eq!(login.output(b"echo fixture-code\r\n").unwrap(), [] as [u8; 0]);
}
#[test]
fn url_invalida_nao_libera_codigo_e_reader_nao_tem_replay() {
    let login = login();
    login.output(b"https://accounts.google.com.evil.test/auth\r\n").unwrap();
    assert!(login.input(b"fixture\r").is_err());
    login.claim_reader().unwrap();
    assert!(login.claim_reader().is_err());
    for url in ["http://cursor.com/login", "https://user@cursor.com/login", "https://cursor.com/login#secret"] {
        assert!(!official_url("cursor", url));
    }
}
#[test]
fn agy_exige_linha_de_modelo_na_conferencia() {
    assert!(agy_models(true, "gemini-fixture\tGemini fixture"));
    assert!(!agy_models(true, "Authentication required"));
    assert!(!agy_models(false, "gemini-fixture\tFixture"));
}

#[test]
fn google_oauth_e_a_unica_opcao_do_agy() {
    let login = login();
    assert!(login.input(b"\r").is_err());
    login.output(b"Select login method: 1. Google OAuth\r\n").unwrap();
    assert!(login.input(b"\x1b[B").is_err());
    assert_eq!(login.input(b"\r").unwrap(), b"1\r");
    assert!(login.input(b"\r").is_err());
}
#[test]
fn cancelar_serializa_a_ultima_entrega() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let root = store.engine_setup_root.clone();
    std::fs::create_dir_all(&root).unwrap();
    let identity = crate::remote::identity::Identity::load_or_create(&root).unwrap();
    let hub = Hub::new(store, "fixture".into(), identity, "fixture".into(), 0);
    let mut login = login();
    login.hub = Arc::downgrade(&hub);
    login.stop();
    let sent = std::cell::Cell::new(false);
    assert!(login.deliver(|| sent.set(true)).is_err());
    assert!(!sent.get());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn codigo_recusado_tem_retorno_seguro_e_exige_novo_login() {
    let login = login();
    login.output(b"https://accounts.google.com/auth?fixture=1\r\n").unwrap();
    login.input(b"SEGREDO-ERRADO\r").unwrap();
    assert!(login.progress());
    assert_eq!(login.output(b"eco SEGREDO-").unwrap(), [] as [u8; 0]);
    let feedback = login.output(b"ERRADO\r\ninvalid code\r\n").unwrap();
    assert_eq!(feedback, [] as [u8; 0]);
    assert!(login.take_rejection_notice());
    assert!(!login.take_rejection_notice());
    assert!(login.input(b"NOVO-CODIGO\r").is_err());
    let challenge = login.output(b"https://accounts.google.com/auth?novo=1 ").unwrap();
    assert_eq!(challenge, b"https://accounts.google.com/auth?novo=1\r\n");
    assert_eq!(login.output(b"NOVO-CODIGO\r\n").unwrap(), [] as [u8; 0]);
}

#[test]
fn ttl_e_ambiente_minimo_sem_editores_ou_pagers() {
    assert_eq!(TTL, Duration::from_secs(9 * 60));
    let env = environment(Path::new("/fixture"));
    for key in ["EDITOR", "VISUAL", "PAGER"] {
        assert!(env.contains(&(key.into(), "/usr/bin/false".into())));
    }
    assert!(env.iter().all(|(key, _)| !["LANG", "LC_ALL", "SYSTEMROOT", "WINDIR"].contains(&key.as_str())));
}

#[test]
fn cada_motor_permitido_recusa_controles_editor_pager_e_comandos_pelo_motivo_certo() {
    for backend in ["cursor", "grok", "antigravity"] {
        let mut login = login();
        login.backend = backend.into();
        let url = match backend {
            "cursor" => "https://cursor.com/login ",
            "grok" => "https://auth.x.ai/device ",
            _ => "https://accounts.google.com/auth ",
        };
        login.output(url.as_bytes()).unwrap();
        if backend != "antigravity" {
            assert!(login.input(b"qualquer entrada\r").is_err());
            continue;
        }
        for (input, reason) in [
            (b"\x03".as_slice(), "sem comandos ou controles"),
            (b"\x04", "sem comandos ou controles"),
            (b"\x1a", "sem comandos ou controles"),
            (b"\x1b", "Tecla indisponível"),
            (b"!ls\n", "sem comandos ou controles"),
            (b"/shell\n", "Comandos não são aceitos"),
            (b"vi /tmp/test\n", "sem comandos ou controles"),
            (b"less /tmp/test\n", "sem comandos ou controles"),
            (b"$(whoami)\n", "sem comandos ou controles"),
        ] {
            assert!(login.input(input).unwrap_err().to_string().contains(reason), "{backend}: {input:?}");
        }
    }
}

#[test]
fn envio_e_filtro_sao_serializados_ao_fechar_entrada() {
    let login = Arc::new(login());
    login.output(b"https://accounts.google.com/auth ").unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let first = login.clone();
    let worker = std::thread::spawn(move || {
        first.enqueue(b"CODIGO\r", |bytes| {
            entered_tx.send(bytes).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
    });
    assert_eq!(entered_rx.recv().unwrap(), b"CODIGO\r");
    let second = login.clone();
    let later = std::thread::spawn(move || second.enqueue(b"COMANDO\r", |_| panic!("entrada chegou à fila")));
    release_tx.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert!(later.join().unwrap().is_err());
}

#[test]
fn codigo_do_desafio_por_link_continua_visivel_com_entrada_fechada() {
    let mut login = login();
    login.backend = "grok".into();
    assert_ne!(login.output(b"https://auth.x.ai/device ").unwrap(), [] as [u8; 0]);
    assert!(login.input(b"comando\r").is_err());
    assert_eq!(login.output(b"CODIGO-DE-APARELHO\r\n").unwrap(), b"CODIGO-DE-APARELHO\r\n");
}
