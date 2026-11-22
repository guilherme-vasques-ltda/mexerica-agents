//! Follow-up dos P3 da r3 do #827: modo agente, limpeza com links, perfil substituído e Space.
use super::*;
#[cfg(unix)]
use crate::fork::terminal_login::{self, Login};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
fn script(f: &Fixture, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = f.root.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
#[cfg(unix)]
fn promovivel(f: &Fixture, caller: &Caller, space: &str) -> Arc<Login> {
    let cli = script(f, "cursor-ok", "#!/usr/bin/python3\nprint('Logged in as fixture')\n");
    Login::fixture_in(&f.hub, caller, "cursor", cli, space).unwrap()
}
#[cfg(unix)]
fn space_b(f: &Fixture) {
    let mut state = f.hub.store.spaces().unwrap();
    let mut space = state.spaces[0].clone();
    space.id = "fixture-space-b".into();
    space.kind = "space".into();
    space.name = "Space B sintético".into();
    state.spaces.push(space);
    f.hub.store.kv_set("donna.spaces.v1", &serde_json::to_string(&state).unwrap()).unwrap();
}
#[cfg(unix)]
fn vivo(pid: i32) -> bool {
    // SAFETY: sinal zero apenas consulta o PID criado pela fixture.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// agy falso: o login dá certo, o CLI abre o agente e registra qualquer byte que chegar depois.
#[cfg(unix)]
#[tokio::test]
async fn login_ok_e_modo_agente_conecta_com_entrada_fechada_e_processo_encerrado() {
    use base64::{Engine as _, prelude::BASE64_STANDARD as B64};
    use futures::StreamExt;
    let f = Fixture::new();
    let pid_file = f.root.join("agy-pid");
    let after = f.root.join("bytes-depois-do-agente");
    let cli = script(
        &f,
        "agy-login-ok-agente",
        &format!(
            r"#!/usr/bin/python3
import os, sys
home = os.environ['HOME']
if len(sys.argv) > 1:
    if sys.argv[1] == 'models' and os.path.exists(home + '/logado'):
        print('gemini-fixture\tGemini fixture')
        sys.exit(0)
    sys.exit(1)
open({pid:?}, 'w').write(str(os.getpid()))
print('https://accounts.google.com/auth digite o código', flush=True)
sys.stdin.readline()
open(home + '/logado', 'w').write('ok')
print('Agent mode', flush=True)
for line in sys.stdin:
    open({after:?}, 'a').write(line)
",
            pid = pid_file.to_string_lossy(),
            after = after.to_string_lossy()
        ),
    );
    let id = f.hub.terms.fixture_login(&f.hub, &Caller::Local, "antigravity", cli).unwrap();
    let term = f.hub.terms.get(&id).unwrap();
    let login = term.login.clone().unwrap();
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &Caller::Local).unwrap().unwrap());
    let mut seen = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            if let Some(data) = event["data"].as_str() {
                seen.push_str(&String::from_utf8_lossy(&B64.decode(data).unwrap()));
                if seen.contains("digite o código") {
                    return;
                }
            }
        }
        panic!("desafio ausente");
    })
    .await
    .unwrap();
    crate::api::dispatch(&f.hub, &Caller::Local, "termInput", json!({"term":id,"data":B64.encode("CODIGO\r")}))
        .await
        .unwrap();
    let mut verified = false;
    let mut code = None;
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(event) = stream.next().await {
            if let Some(data) = event["data"].as_str() {
                let text = String::from_utf8_lossy(&B64.decode(data).unwrap()).to_lowercase();
                assert!(!text.contains("agent mode") && !text.contains("codigo"), "o agente não aparece ao titular");
            }
            verified |= event["type"] == "verified" && event["cliOnly"] == true;
            if event["type"] == "exit" {
                code = event["code"].as_i64();
                return;
            }
        }
    })
    .await
    .unwrap();
    assert!(verified, "login confirmado deve ser anunciado");
    assert_eq!(code, Some(0), "login que deu certo não é tratado como cancelado");
    // Entrada fechada: o filtro recusa e o PTY descarta o que já estava na fila.
    assert!(login.agent_mode());
    assert!(login.input(b"x\r").unwrap_err().to_string().contains("Aguarde"));
    assert!(login.enqueue(b"\r", |_| panic!("nada chega ao agente")).is_err());
    let slot = terminal_login::slot("antigravity", crate::fork::spaces::PERSONAL);
    assert_eq!(f.hub.store.kv_read(&slot).unwrap().as_deref(), login.home.to_str(), "perfil promovido");
    assert!(login.home.exists());
    esperar(|| f.hub.terms.get(&id).is_none()).await;
    assert!(!vivo(std::fs::read_to_string(&pid_file).unwrap().parse().unwrap()), "processo encerrado");
    assert!(!after.exists(), "nenhum byte do cliente chegou ao agente");
}

/// Sem confirmação do CLI, o modo agente continua falhando fechado e apaga o perfil.
#[cfg(unix)]
#[tokio::test]
async fn modo_agente_sem_login_confirmado_nao_promove_e_apaga_o_perfil() {
    use base64::{Engine as _, prelude::BASE64_STANDARD as B64};
    use futures::StreamExt;
    let f = Fixture::new();
    let cli = script(
        &f,
        "agy-agente-sem-login",
        r"#!/usr/bin/python3
import sys, time
if len(sys.argv) > 1:
    sys.exit(1)
print('https://accounts.google.com/auth digite o código', flush=True)
sys.stdin.readline()
print('how can I help', flush=True)
time.sleep(300)
",
    );
    let id = f.hub.terms.fixture_login(&f.hub, &Caller::Local, "antigravity", cli).unwrap();
    let login = f.hub.terms.get(&id).unwrap().login.clone().unwrap();
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &Caller::Local).unwrap().unwrap());
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            if event["data"]
                .as_str()
                .is_some_and(|d| String::from_utf8_lossy(&B64.decode(d).unwrap()).contains("digite o código"))
            {
                return;
            }
        }
    })
    .await
    .unwrap();
    crate::api::dispatch(&f.hub, &Caller::Local, "termInput", json!({"term":id,"data":B64.encode("CODIGO\r")}))
        .await
        .unwrap();
    esperar(|| f.hub.terms.get(&id).is_none() && !login.home.exists()).await;
    let slot = terminal_login::slot("antigravity", crate::fork::spaces::PERSONAL);
    assert_eq!(f.hub.store.kv_read(&slot).unwrap(), None);
}

#[cfg(unix)]
#[tokio::test]
async fn link_simbolico_no_perfil_provisorio_nao_trava_a_limpeza_nem_segue_o_destino() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let outside = f.root.join("fora-do-perfil");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("credencial"), "preservada").unwrap();
    // Cancelado pelo próprio login.
    let login = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    std::fs::create_dir_all(login.home.join(".local/bin")).unwrap();
    symlink(&outside, login.home.join(".local/bin/cursor-agent")).unwrap();
    symlink("/nao/existe", login.home.join("quebrado")).unwrap();
    symlink(outside.join("credencial"), login.home.join("arquivo")).unwrap();
    std::fs::write(login.home.join("credencial"), "provisoria").unwrap();
    login.stop();
    login.discard().unwrap();
    assert!(!login.home.exists());
    // Vencido, varrido pela tentativa seguinte.
    let stale = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    symlink(&outside, stale.home.join("link")).unwrap();
    let expired = (crate::store::now_ms() - 1).to_string();
    let operation = stale.home.file_name().unwrap().to_str().unwrap();
    std::fs::write(stale.home.join(".donnabot-login-operation"), format!("{operation} {expired}\n")).unwrap();
    f.hub.store.kv_set(&terminal_login::attempt_key(&stale.home).unwrap(), &expired).unwrap();
    let next = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    assert!(!stale.home.exists(), "o link não impede a varredura");
    assert_eq!(std::fs::read_to_string(outside.join("credencial")).unwrap(), "preservada");
    next.stop();
    next.discard().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn aviso_de_perfis_sem_marcador_nunca_derruba_o_login() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let first = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    let account = first.home.parent().unwrap().to_path_buf();
    // Pasta sem marcador, link para pasta e pasta ilegível: só contam ou são ignorados.
    std::fs::create_dir(account.join("sem-marcador")).unwrap();
    symlink(&f.root, account.join("link-para-pasta")).unwrap();
    assert_eq!(crate::fork::motor_exec::login_cleanup::count_unmarked(&account).unwrap(), 1);
    // Ancestral que é link (por exemplo, /home → /var/home): a contagem falha, o login segue.
    let alias = f.root.join("alias-de-contas");
    symlink(account.parent().unwrap(), &alias).unwrap();
    assert!(crate::fork::motor_exec::login_cleanup::count_unmarked(&alias.join(account.file_name().unwrap())).is_err());
    let second = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    assert!(second.home.exists());
    for login in [first, second] {
        login.stop();
        login.discard().unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn perfil_substituido_some_do_disco_sem_seguir_links_e_o_novo_fica() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let outside = f.root.join("fora-do-perfil");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("credencial"), "preservada").unwrap();
    let personal = crate::fork::spaces::PERSONAL;
    let first = promovivel(&f, &Caller::Local, personal);
    assert_eq!(first.check().await, Some(true));
    first.complete().unwrap();
    std::fs::write(first.home.join("credencial-antiga"), "conta anterior").unwrap();
    symlink(&outside, first.home.join("link")).unwrap();
    let second = promovivel(&f, &Caller::Local, personal);
    assert_eq!(second.check().await, Some(true));
    assert!(first.home.exists(), "nada some antes da troca ser confirmada");
    second.complete().unwrap();
    assert!(!first.home.exists(), "credencial da conta anterior apagada");
    assert!(second.home.exists());
    assert_eq!(std::fs::read_to_string(outside.join("credencial")).unwrap(), "preservada");
    let slot = terminal_login::slot("cursor", personal);
    assert_eq!(f.hub.store.kv_read(&slot).unwrap().as_deref(), second.home.to_str());
    assert_eq!(f.hub.store.kv_prefix("terminal-login-replaced:"), Vec::<String>::new());
    assert!(
        terminal_login::stamp(&f.hub.store, "cursor", personal).unwrap().starts_with(second.home.to_str().unwrap())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn remocao_do_perfil_substituido_que_falha_fica_registrada_e_a_proxima_tentativa_repete() {
    let f = Fixture::new();
    let personal = crate::fork::spaces::PERSONAL;
    let first = promovivel(&f, &Caller::Local, personal);
    assert_eq!(first.check().await, Some(true));
    first.complete().unwrap();
    // Marcador que não confere impede a remoção segura.
    let marker = first.home.join(".donnabot-login-operation");
    let original = std::fs::read_to_string(&marker).unwrap();
    std::fs::write(&marker, "outra-operacao 1\n").unwrap();
    let second = promovivel(&f, &Caller::Local, personal);
    assert_eq!(second.check().await, Some(true));
    second.complete().unwrap();
    assert!(first.home.exists(), "sem remoção segura, nada é apagado");
    assert!(second.home.exists(), "a troca vale mesmo assim");
    assert_eq!(f.hub.store.kv_prefix("terminal-login-replaced:").len(), 1);
    std::fs::write(&marker, original).unwrap();
    let third = promovivel(&f, &Caller::Local, personal);
    assert!(!first.home.exists(), "a tentativa seguinte repete a limpeza");
    assert_eq!(f.hub.store.kv_prefix("terminal-login-replaced:"), Vec::<String>::new());
    assert!(second.home.exists());
    third.stop();
    third.discard().unwrap();
}

/// Corrida controlada, 50 vezes: a leitura marca o modo agente enquanto a entrada tenta
/// entregar. A fronteira garante que a escrita nunca executa depois da marca.
#[cfg(unix)]
#[test]
fn corrida_entre_marca_do_agente_e_entrega_nunca_entrega_depois_da_marca() {
    for _ in 0..50 {
        let f = Fixture::new();
        let login = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let entregue_depois = Arc::new(AtomicBool::new(false));
        let leitor = {
            let (login, barrier) = (login.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                login.output(b"Agent mode").unwrap();
            })
        };
        let escritor = {
            let (login, barrier, entregue) = (login.clone(), barrier.clone(), entregue_depois.clone());
            std::thread::spawn(move || {
                barrier.wait();
                login.deliver_input(|| {
                    if login.agent_mode() {
                        entregue.store(true, Ordering::Release);
                    }
                    Ok::<(), anyhow::Error>(())
                })
            })
        };
        leitor.join().unwrap();
        let resultado = escritor.join().unwrap();
        assert!(resultado.is_ok(), "a fronteira não pode falhar na corrida: {resultado:?}");
        assert!(!entregue_depois.load(Ordering::Acquire), "byte entregue depois da marca do agente");
        assert!(login.agent_mode());
        let recusado = login.deliver_input(|| -> Result<()> { panic!("nada chega ao agente") }).unwrap();
        assert!(recusado.is_none(), "depois da marca, a entrega é sempre recusada");
        login.stop();
        login.discard().unwrap();
    }
}

/// Teto de entrada pendente: o que o dono manda mais depressa do que o PTY consome é recusado, o login segue,
/// e a recusa não adianta o estado do código (o filtro só roda depois do teto).
#[cfg(unix)]
#[test]
fn entrada_pendente_tem_teto_e_liberar_reabre_espaco() {
    use crate::fork::terminal_login::PENDING_MAX;
    let f = Fixture::new();
    let login = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    let mut aceitos = 0;
    let recusa = loop {
        match login.enqueue(b"\x1b[A", |_| Ok(())) {
            Ok(()) => aceitos += 3,
            Err(error) => break error,
        }
        assert!(aceitos <= PENDING_MAX, "o teto não valeu: {aceitos} bytes pendentes");
    };
    assert!(recusa.to_string().contains("excedida"), "{recusa}");
    assert_eq!(login.pending(), aceitos);
    assert!(aceitos + 2049 > PENDING_MAX, "recusou cedo demais: {aceitos}");
    // Escrita feita no PTY libera o teto; envio que falha não conta como pendente.
    login.release_pending(aceitos);
    assert_eq!(login.pending(), 0);
    assert!(login.enqueue(b"\x1b[A", |_| anyhow::bail!("canal fechado")).is_err());
    assert_eq!(login.pending(), 0);
    login.enqueue(b"\x1b[A", |_| Ok(())).unwrap();
    assert_eq!(login.pending(), 3);
    login.stop();
    login.discard().unwrap();
}

/// Escritor que só aceita `room` bytes e depois responde `WouldBlock`, como um PTY cheio.
#[cfg(unix)]
struct PtyCheio {
    room: usize,
    gravado: Vec<u8>,
}
#[cfg(unix)]
impl std::io::Write for PtyCheio {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.room == 0 {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let n = buf.len().min(self.room);
        self.room -= n;
        self.gravado.extend_from_slice(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// PTY cheio: nada se perde, nada espera. O resto fica pendente e sai quando houver espaço; no modo
/// agente o pendente é descartado sem escrever.
#[cfg(unix)]
#[test]
fn pty_cheio_guarda_o_resto_sem_perder_e_modo_agente_descarta_o_pendente() {
    use crate::fork::terminal_login::entrega::flush_backlog;
    let f = Fixture::new();
    let login = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    login.enqueue(b"\x1b[A", |_| Ok(())).unwrap();
    login.enqueue(b"\x1b[B", |_| Ok(())).unwrap();
    let mut backlog = b"\x1b[A\x1b[B".to_vec();
    let mut pty = PtyCheio { room: 4, gravado: vec![] };
    flush_backlog(&login, &mut pty, &mut backlog).unwrap();
    assert_eq!((pty.gravado.as_slice(), backlog.as_slice()), (&b"\x1b[A\x1b"[..], &b"[B"[..]));
    assert_eq!(login.pending(), 2);
    // Continua cheio: nada muda, e não é erro.
    flush_backlog(&login, &mut pty, &mut backlog).unwrap();
    assert_eq!((backlog.as_slice(), login.pending()), (&b"[B"[..], 2));
    pty.room = 100;
    flush_backlog(&login, &mut pty, &mut backlog).unwrap();
    assert_eq!(pty.gravado, b"\x1b[A\x1b[B");
    assert_eq!((backlog.len(), login.pending()), (0, 0));
    // Modo agente: o que estava pendente não chega ao PTY e libera o teto.
    login.enqueue(b"\x1b[C", |_| Ok(())).unwrap();
    let mut backlog = b"\x1b[C".to_vec();
    login.output(b"Agent mode").unwrap();
    let antes = pty.gravado.len();
    flush_backlog(&login, &mut pty, &mut backlog).unwrap();
    assert_eq!((pty.gravado.len(), backlog.len(), login.pending()), (antes, 0, 0));
    login.stop();
    login.discard().unwrap();
}

/// Muitas teclas num PTY de verdade que ninguém lê: a escrita sob o cadeado nunca espera, então o leitor
/// (`output`) segue andando e a fronteira continua fechando no modo agente.
#[cfg(unix)]
#[test]
fn muitas_teclas_com_pty_cheio_nao_travam_o_leitor_nem_a_fronteira() {
    use crate::fork::terminal_login::entrega::{flush_backlog, par::Par};
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let login = promovivel(&f, &Caller::Local, crate::fork::spaces::PERSONAL);
    let par = Par::new();
    let mut writer = par.master.take_writer().unwrap();
    let parar = Arc::new(AtomicBool::new(false));
    let leitor = {
        let (login, parar) = (login.clone(), parar.clone());
        std::thread::spawn(move || {
            let mut pior = Duration::ZERO;
            while !parar.load(Ordering::Acquire) {
                let t = Instant::now();
                login.output(b".").unwrap();
                pior = pior.max(t.elapsed());
                std::thread::sleep(Duration::from_millis(2));
            }
            pior
        })
    };
    let inicio = Instant::now();
    let mut backlog = Vec::new();
    let mut encheu = false;
    for _ in 0..20_000 {
        backlog.extend(b"\x1b[A".repeat(600));
        flush_backlog(&login, writer.as_mut(), &mut backlog).unwrap();
        if !backlog.is_empty() {
            encheu = true;
            break;
        }
    }
    assert!(encheu, "o PTY nunca encheu; o teste não provou nada");
    // Cheio: cada tentativa volta na hora, com o cadeado solto logo depois.
    for _ in 0..200 {
        let t = Instant::now();
        flush_backlog(&login, writer.as_mut(), &mut backlog).unwrap();
        assert!(t.elapsed() < Duration::from_millis(500), "a escrita esperou o PTY");
    }
    assert!(inicio.elapsed() < Duration::from_secs(20));
    parar.store(true, Ordering::Release);
    let pior = leitor.join().unwrap();
    assert!(pior < Duration::from_millis(500), "o leitor ficou preso {pior:?}");
    // Com o PTY cheio e entrada pendente, a marca do agente ainda fecha tudo na hora.
    login.output(b"Agent mode").unwrap();
    let escrito = par.legiveis();
    flush_backlog(&login, writer.as_mut(), &mut backlog).unwrap();
    assert_eq!(backlog.len(), 0);
    assert_eq!(par.legiveis(), escrito, "entrega depois da marca");
    login.stop();
    login.discard().unwrap();
}

/// De ponta a ponta: CLI em modo cru que nunca lê a entrada. O dono despeja teclas; o PTY enche, o excesso
/// é recusado (o login NÃO cai), o prazo continua valendo e o CLI morre no fim.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn despejo_de_teclas_num_cli_parado_recusa_o_excesso_e_o_prazo_continua_valendo() {
    use crate::fork::terminal_login::PENDING_MAX;
    use base64::{Engine as _, prelude::BASE64_STANDARD as B64};
    use futures::StreamExt;
    let f = Fixture::new();
    let pid_file = f.root.join("cli-parado-pid");
    let cli = script(
        &f,
        "cursor-parado",
        &format!(
            "#!/usr/bin/python3\nimport os, sys, time, tty\nopen({:?}, 'w').write(str(os.getpid()))\ntty.setraw(0)\nprint('pronto', flush=True)\ntime.sleep(300)\n",
            pid_file.to_string_lossy()
        ),
    );
    let ttl = Duration::from_secs(25);
    let id = f.hub.terms.fixture_login_backend_timeout(&f.hub, &Caller::Local, "cursor", cli, ttl).unwrap();
    let inicio = std::time::Instant::now();
    let login = f.hub.terms.get(&id).unwrap().login.clone().unwrap();
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &Caller::Local).unwrap().unwrap());
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = stream.next().await {
            if event["data"]
                .as_str()
                .is_some_and(|d| String::from_utf8_lossy(&B64.decode(d).unwrap()).contains("pronto"))
            {
                return;
            }
        }
        panic!("CLI parado não ficou pronto");
    })
    .await
    .unwrap();
    let tecla = B64.encode("\x1b[A");
    let mut recusada = false;
    for i in 0..3_000_000u32 {
        match f.hub.terms.write(&id, &tecla) {
            Ok(()) => {}
            Err(error) => {
                assert!(error.to_string().contains("excedida"), "o login caiu em vez de recusar: {error}");
                recusada = true;
                break;
            }
        }
        assert!(login.pending() <= PENDING_MAX);
        if i % 64 == 0 {
            tokio::task::yield_now().await;
        }
    }
    assert!(recusada, "nenhum excesso foi recusado");
    assert!(login.current() && f.hub.terms.get(&id).is_some(), "o excesso derrubou o login");
    // O leitor segue respondendo com o PTY cheio.
    let t = std::time::Instant::now();
    login.output(b".").unwrap();
    assert!(t.elapsed() < Duration::from_millis(500));
    // O prazo vale: o login e o CLI acabam no TTL, mesmo com entrada pendente.
    esperar(|| f.hub.terms.get(&id).is_none()).await;
    assert!(inicio.elapsed() < ttl + Duration::from_secs(20), "o prazo não valeu: {:?}", inicio.elapsed());
    let pid: i32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
    esperar(|| !vivo(pid)).await;
}

#[cfg(unix)]
#[tokio::test]
async fn space_no_login_remoto_ausente_e_pessoal_nulo_e_invalido_e_contas_nao_vazam_entre_spaces() {
    let f = Fixture::new();
    space_b(&f);
    let owner = f.owner();
    for (body, expected) in [
        (json!({"backend":"cursor","step":"login","spaceId":null,"cols":80,"rows":24}), "invalid_space"),
        (json!({"backend":"cursor","step":"login","spaceId":7,"cols":80,"rows":24}), "invalid_space"),
        (json!({"backend":"cursor","step":"login","spaceId":"nao-existe","cols":80,"rows":24}), "space_not_found"),
    ] {
        let error = crate::api::dispatch(&f.hub, &owner, "agentSetup", body).await.unwrap_err();
        assert!(error.to_string().contains(expected), "{expected}: {error:#}");
    }
    for method in ["agentAuth", "agentModels"] {
        let error =
            crate::api::dispatch(&f.hub, &owner, method, json!({"backend":"cursor","spaceId":null})).await.unwrap_err();
        assert!(error.to_string().contains("invalid_space"), "{method}: {error:#}");
    }
    assert!(f.hub.terms.get("nenhum").is_none() && !f.hub.terms.busy());
    // Ausente = Pessoal: o perfil promovido no Pessoal é o que `agentAuth` sem spaceId consulta.
    let b = "fixture-space-b";
    let personal = crate::fork::spaces::PERSONAL;
    // `true` quando o `agentAuth` responde pelo perfil promovido do Space. Sem perfil, ele cai no
    // `auth::check` do CLI do computador: com o Cursor instalado responde sem `cliSignedIn`; sem ele,
    // recusa com "isn't set up". As duas formas provam que nenhum perfil foi usado, então o teste não
    // depende de o Cursor estar instalado na máquina.
    let tem_perfil = |space: Option<&str>| {
        let mut body = json!({"backend":"cursor"});
        if let Some(space) = space {
            body["spaceId"] = space.into();
        }
        let (hub, owner) = (f.hub.clone(), owner.clone());
        async move {
            match crate::api::dispatch(&hub, &owner, "agentAuth", body).await {
                Ok(status) => status.get("cliSignedIn").is_some(),
                Err(error) => {
                    assert!(format!("{error:#}").contains("isn't set up"), "{error:#}");
                    false
                }
            }
        }
    };
    let (stamp_p, env_p) = (
        terminal_login::stamp(&f.hub.store, "cursor", personal).unwrap(),
        terminal_login::runtime_env(&f.hub.store, "cursor", personal).unwrap(),
    );
    assert!(!tem_perfil(None).await, "sem perfil, cai no auth::check");
    let in_b = promovivel(&f, &owner, b);
    assert_eq!(in_b.space, b);
    assert_eq!(in_b.check().await, Some(true));
    in_b.complete().unwrap();
    assert!(tem_perfil(Some(b)).await, "o Space B enxerga a sua conta");
    assert!(!tem_perfil(None).await, "ausente é Pessoal, que não tem conta");
    assert!(!tem_perfil(Some(personal)).await);
    assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", personal).unwrap(), stamp_p);
    assert_eq!(terminal_login::runtime_env(&f.hub.store, "cursor", personal).unwrap(), env_p);
    assert_ne!(terminal_login::stamp(&f.hub.store, "cursor", b).unwrap(), "");
    // E o inverso: promover no Pessoal não muda o Space B.
    let (stamp_b, env_b) = (
        terminal_login::stamp(&f.hub.store, "cursor", b).unwrap(),
        terminal_login::runtime_env(&f.hub.store, "cursor", b).unwrap(),
    );
    let in_personal = promovivel(&f, &owner, personal);
    assert_eq!(in_personal.check().await, Some(true));
    in_personal.complete().unwrap();
    assert!(tem_perfil(None).await, "ausente consulta o perfil do Pessoal");
    assert_eq!(terminal_login::stamp(&f.hub.store, "cursor", b).unwrap(), stamp_b);
    assert_eq!(terminal_login::runtime_env(&f.hub.store, "cursor", b).unwrap(), env_b);
    assert!(in_b.home.exists(), "promover no Pessoal não apaga a conta do Space B");
}
