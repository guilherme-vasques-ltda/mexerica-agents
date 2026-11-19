//! Focais B no transporte TCP/WebSocket REAL; relógio/cofre/instalação são fixtures de lógica.
use super::*;
use crate::fork::personal_link::{channel, channel_crypto::Cifra, channel_root::RaizCanal};
use futures::{FutureExt, SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn gerida(p: &Procedimento, a: &Aparelho) -> Result<()> {
    let (ra, _) = raiz(p, a)?;
    let mut s = Sessao::nova();
    let m = pedir(p, &mut s, "management-challenge", &[json!(a.chave()), json!(canon::hash(&ra)?)])?;
    pedir(p, &mut s, "management-accept", &[a.assinar(&m)?])?;
    Ok(())
}
async fn servidor(
    p: Arc<Procedimento>,
) -> Result<(String, tokio::sync::watch::Sender<bool>, tokio::task::JoinHandle<Result<()>>)> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("ws://{}/personal-link/channel", l.local_addr()?);
    let (stop, rx) = tokio::sync::watch::channel(false);
    let handshake = Arc::new(tokio::sync::Semaphore::new(channel::HANDSHAKES)).acquire_owned().await?;
    let job = tokio::spawn(async move {
        let (s, _) = l.accept().await?;
        channel::sessao(p, s, rx, Arc::new(tokio::sync::Semaphore::new(channel::SESSOES)), handshake).await
    });
    Ok((url, stop, job))
}
async fn enviar(s: &mut Client, v: &Value) -> Result<()> {
    s.send(Message::Binary(canon::j(v)?.into())).await?;
    Ok(())
}
async fn receber(s: &mut Client) -> Result<Value> {
    let msg = tokio::time::timeout(Duration::from_secs(3), s.next()).await?.context("EOF")??;
    match msg {
        Message::Binary(b) => canon::ler(&b),
        _ => anyhow::bail!("frame não binário"),
    }
}
async fn cifrado(s: &mut Client, c: &mut Cifra, v: &Value) -> Result<()> {
    s.send(Message::Binary(c.selar(v)?.into())).await?;
    Ok(())
}
async fn decifrado(s: &mut Client, c: &mut Cifra) -> Result<Value> {
    let msg = tokio::time::timeout(Duration::from_secs(3), s.next()).await?.context("EOF")??;
    match msg {
        Message::Binary(b) => c.abrir(&b),
        _ => anyhow::bail!("frame não binário"),
    }
}
struct Pronto {
    socket: Client,
    entrada: Cifra,
    saida: Cifra,
    sid: Value,
    th: Value,
}
async fn preparar(url: &str, procedimento: &Procedimento, aparelho: &Aparelho) -> Result<(Pronto, Value)> {
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await?;
    let root = RaizCanal::ler(&procedimento.hub.store.db.locked(), &procedimento.k, &aparelho.chave())?;
    enviar(&mut socket, &root.open()).await?;
    let ch = receber(&mut socket).await?;
    let ch = canon::verificar(&ch, "donna.personal-link/channel-challenge/v1", 16, &procedimento.k.h)?;
    let campos_desafio = ch.payload();
    let secret = x25519_dalek::StaticSecret::from(crate::remote::crypto::random::<32>());
    let ec = crate::remote::crypto::b64(&crate::remote::crypto::x25519_pub(&secret));
    let cr = aparelho.assinar(&json!([
        "donna.personal-link/channel-response/v1",
        1,
        ch.digest()?,
        campos_desafio[3],
        campos_desafio[4],
        campos_desafio[5],
        campos_desafio[6],
        "manager",
        campos_desafio[10],
        ec,
        canon::aleatorio::<32>(),
        campos_desafio[11],
        campos_desafio[12],
        wire::p1(),
        0
    ]))?;
    enviar(&mut socket, &cr).await?;
    let welcome_wire = receber(&mut socket).await?;
    let welcome = canon::verificar(&welcome_wire, "donna.personal-link/channel-welcome/v1", 13, &procedimento.k.h)?;
    let sid = welcome.payload()[9].clone();
    let transcript = json!(["donna.personal-link/channel-final/v1", 1, ch.valor(), cr, welcome.valor()]);
    let th = json!(canon::hash(&transcript)?);
    let shared = crate::remote::crypto::x25519(&secret, &canon::b64n::<32>(&campos_desafio[9])?)?;
    let saida = Cifra::nova(&shared, &transcript, sid.as_str().context("sid")?, "c2h")?;
    let entrada = Cifra::nova(&shared, &transcript, sid.as_str().context("sid")?, "h2c")?;
    let finish = json!(["donna.personal-link/channel-finish/v1", 1, sid, th, welcome.digest()?]);
    Ok((Pronto { socket, entrada, saida, sid, th }, finish))
}
async fn abrir(url: &str, p: &Procedimento, a: &Aparelho) -> Result<Pronto> {
    let (mut b, finish) = preparar(url, p, a).await?;
    cifrado(&mut b.socket, &mut b.saida, &finish).await?;
    assert_eq!(
        decifrado(&mut b.socket, &mut b.entrada).await?,
        json!(["donna.personal-link/channel-ready/v1", 1, b.sid, b.th])
    );
    Ok(b)
}
type Ledger = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;
fn ledger(p: &Procedimento) -> Result<Ledger> {
    let db = p.hub.store.db.locked();
    let mut snapshot = Vec::new();
    for table in [
        "personal_link_installation",
        "personal_link_runtimes",
        "personal_link_journal",
        "personal_link_events",
        "personal_link_origins",
        "personal_link_rights",
        "personal_link_reservations",
        "personal_link_roots",
    ] {
        let mut stmt = db.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                (0..columns).map(|i| row.get::<_, rusqlite::types::Value>(i)).collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        snapshot.push((table.into(), rows));
    }
    Ok(snapshot)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_finish_ack_status_close_sem_escrita_e_generico_fechado() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, clock) = f.subir(true)?;
    sair_da_quarentena(&clock);
    let a = Aparelho::novo();
    gerida(&p, &a)?;
    let before = ledger(&p)?;
    let (url, _stop, job) = servidor(p.clone()).await?;
    let mut b = abrir(&url, &p, &a).await?;
    let id = json!(canon::aleatorio::<16>());
    cifrado(&mut b.socket, &mut b.saida, &json!(["donna.personal-link/link-status-request/v1", 1, id, b.sid, b.th]))
        .await?;
    let status = decifrado(&mut b.socket, &mut b.entrada).await?;
    assert_eq!(status[2], id);
    assert_eq!(status[3], p.k.h);
    assert_eq!(status[7], a.chave());
    assert_eq!(status[10], wire::p1());
    assert_eq!(status[14], "active");
    assert!(autoridade::autenticar(&p.hub, &a.chave()).is_err());
    cifrado(&mut b.socket, &mut b.saida, &json!(["donna.personal-link/channel-close/v1", 1, b.sid, b.th])).await?;
    assert!(tokio::time::timeout(Duration::from_secs(3), job).await??.is_ok());
    assert_eq!(ledger(&p)?, before);
    assert_eq!(p.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_operacoes_desconhecidas_e_replay_fecham_sem_efeito() -> Result<()> {
    let fixture = Fixture::nova()?;
    let (procedimento, clock) = fixture.subir(true)?;
    sair_da_quarentena(&clock);
    let aparelho = Aparelho::novo();
    gerida(&procedimento, &aparelho)?;
    let before = ledger(&procedimento)?;
    for op in [
        "credentialList",
        "credentialStatus",
        "setAgentEnv",
        "agentSetup",
        "engine.accounts",
        "devices.list",
        "subscribe",
        "payload",
    ] {
        let (url, _stop, job) = servidor(procedimento.clone()).await?;
        let mut canal = abrir(&url, &procedimento, &aparelho).await?;
        cifrado(&mut canal.socket, &mut canal.saida, &json!([op, 1, canal.sid, canal.th])).await?;
        assert!(tokio::time::timeout(Duration::from_secs(3), job).await??.is_err(), "{op}");
    }
    let (url, _stop, job) = servidor(procedimento.clone()).await?;
    let mut canal = abrir(&url, &procedimento, &aparelho).await?;
    let pedido_status =
        json!(["donna.personal-link/link-status-request/v1", 1, canon::aleatorio::<16>(), canal.sid, canal.th]);
    let bytes = canal.saida.selar(&pedido_status)?;
    canal.socket.send(Message::Binary(bytes.clone().into())).await?;
    decifrado(&mut canal.socket, &mut canal.entrada).await?;
    canal.socket.send(Message::Binary(bytes.into())).await?;
    assert!(tokio::time::timeout(Duration::from_secs(3), job).await??.is_err());
    assert_eq!(ledger(&procedimento)?, before);
    assert_eq!(procedimento.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_captura_pre_ack_e_status_eof_parada_retem_lock_ate_drain() -> Result<()> {
    for status in [false, true] {
        for parada in [false, true] {
            let fixture = Fixture::nova()?;
            let (procedimento, clock) = fixture.subir(true)?;
            sair_da_quarentena(&clock);
            let aparelho = Aparelho::novo();
            gerida(&procedimento, &aparelho)?;
            let (url, stop, job) = servidor(procedimento.clone()).await?;
            let (mut canal, pedido) = if status {
                let canal = abrir(&url, &procedimento, &aparelho).await?;
                let pedido_status = json!([
                    "donna.personal-link/link-status-request/v1",
                    1,
                    canon::aleatorio::<16>(),
                    canal.sid,
                    canal.th
                ]);
                (canal, pedido_status)
            } else {
                preparar(&url, &procedimento, &aparelho).await?
            };
            let trava = std::fs::File::open(fixture.dir.join("host.lock"))?;
            assert!(crate::lock_file(&trava, Duration::ZERO)?);
            let mut drain = tokio::spawn(async move {
                let resultado = job.await;
                drop(trava);
                resultado
            });
            let p2 = procedimento.clone();
            let (held, seen) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let lock = tokio::task::spawn_blocking(move || {
                let _db = p2.hub.store.db.locked();
                let _ = held.send(());
                let _ = wait.recv();
            });
            seen.await?;
            cifrado(&mut canal.socket, &mut canal.saida, &pedido).await?;
            // Aguarda o transporte entrar na captura; resposta não pode sair com DB retida.
            assert!(tokio::time::timeout(Duration::from_millis(100), &mut drain).await.is_err());
            if parada {
                stop.send(true)?;
            } else {
                canal.socket.close(None).await?;
            }
            assert!(tokio::time::timeout(Duration::from_millis(100), &mut drain).await.is_err());
            let outra = std::fs::File::open(fixture.dir.join("host.lock"))?;
            assert!(
                !crate::lock_file(&outra, Duration::ZERO)?,
                "lock retido durante captura bloqueada, status={status},parada={parada}"
            );
            release.send(())?;
            lock.await?;
            assert!(tokio::time::timeout(Duration::from_secs(3), drain).await???.is_err());
            assert!(crate::lock_file(&outra, Duration::ZERO)?, "lock só livre após drain");
            assert_eq!(procedimento.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), 0);
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_revisao_tombstone_e_tempo_mudados_encerram_original() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, clock) = f.subir(true)?;
    sair_da_quarentena(&clock);
    let a = Aparelho::novo();
    gerida(&p, &a)?;
    let root = RaizCanal::ler(&p.hub.store.db.locked(), &p.k, &a.chave())?;
    let (url, _stop, job) = servidor(p.clone()).await?;
    let mut b = abrir(&url, &p, &a).await?;
    p.hub.store.db.locked().execute("UPDATE personal_link_origins SET tombstone=1 WHERE key=?", [a.chave()])?;
    assert!(root.conferir(&p.hub.store.db.locked()).is_err());
    cifrado(
        &mut b.socket,
        &mut b.saida,
        &json!(["donna.personal-link/link-status-request/v1", 1, canon::aleatorio::<16>(), b.sid, b.th]),
    )
    .await?;
    assert!(tokio::time::timeout(Duration::from_secs(3), job).await??.is_err());
    Relogio::avancar(&clock, 1, 50);
    assert!(p.relogio.amostra().is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_raiz_incompleta_p1_exata_origem_e_cabeca_obrigatorias() -> Result<()> {
    let fixture = Fixture::nova()?;
    let (procedimento, clock) = fixture.subir(true)?;
    let aparelho = Aparelho::novo();
    assert!(RaizCanal::ler(&procedimento.hub.store.db.locked(), &procedimento.k, &aparelho.chave()).is_err());
    sair_da_quarentena(&clock);
    let (ra, _) = raiz(&procedimento, &aparelho)?;
    assert!(
        RaizCanal::ler(&procedimento.hub.store.db.locked(), &procedimento.k, &aparelho.chave()).is_err(),
        "connect-only nunca recebe gestão para ligar"
    );
    let mut administrativa = Sessao::nova();
    let gestao = pedir(
        &procedimento,
        &mut administrativa,
        "management-challenge",
        &[json!(aparelho.chave()), json!(canon::hash(&ra)?)],
    )?;
    pedir(&procedimento, &mut administrativa, "management-accept", &[aparelho.assinar(&gestao)?])?;
    let original = RaizCanal::ler(&procedimento.hub.store.db.locked(), &procedimento.k, &aparelho.chave())?;
    let (url, _stop, job) = servidor(procedimento.clone()).await?;
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await?;
    let mut wrong = original.open();
    wrong[5] = json!("d".repeat(64));
    enviar(&mut socket, &wrong).await?;
    assert!(tokio::time::timeout(Duration::from_secs(3), job).await??.is_err(), "C não escolhe orçamento/autoridade");
    let (url, _stop, job) = servidor(procedimento.clone()).await?;
    let mut canal = abrir(&url, &procedimento, &aparelho).await?;
    procedimento.hub.store.db.locked().execute("INSERT INTO personal_link_rights(key,revision,policy,grant_seq,consent_seq) SELECT key,3,policy,grant_seq,consent_seq FROM personal_link_rights WHERE key=? AND revision=2",[aparelho.chave()])?;
    assert!(original.conferir(&procedimento.hub.store.db.locked()).is_err());
    cifrado(
        &mut canal.socket,
        &mut canal.saida,
        &json!(["donna.personal-link/link-status-request/v1", 1, canon::aleatorio::<16>(), canal.sid, canal.th]),
    )
    .await?;
    assert!(tokio::time::timeout(Duration::from_secs(3), job).await??.is_err());
    assert_eq!(procedimento.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_limites_websocket_recusam_antes_de_payload() -> Result<()> {
    use crate::fork::personal_link::channel_socket;
    use tokio::io::AsyncWriteExt;
    for header in [
        vec![0x02, 0x80],
        vec![0x81, 0x80],
        vec![0xc2, 0x80],
        vec![0x82, 0x00],
        vec![0x82, 0xff],
        vec![0x82, 0xfe, 0xff, 0xff],
    ] {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer.write_all(&header).await?;
        assert!(channel_socket::read(&mut reader).await.is_err());
    }
    for path in ["/personal-link/channel?x=1", "/channel?v=1", "/health"] {
        let (mut writer, mut reader) = tokio::io::duplex(4096);
        writer.write_all(format!("GET {path} HTTP/1.1\r\n\r\n").as_bytes()).await?;
        assert!(channel_socket::upgrade(&mut reader).await.is_err());
    }
    let deep = format!("{}0{}", "[".repeat(50), "]".repeat(50));
    assert!(canon::ler(deep.as_bytes()).is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_listener_do_perfil_qualificado_e_unqualified_sem_tcp() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, clock) = f.subir(true)?;
    sair_da_quarentena(&clock);
    let a = Aparelho::novo();
    gerida(&p, &a)?;
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = probe.local_addr()?.port();
    drop(probe);
    let (stop, rx) = tokio::sync::watch::channel(false);
    let p2 = p.clone();
    let job = tokio::spawn(async move { channel::servir(p2, "127.0.0.1", port, rx).await });
    let url = format!("ws://127.0.0.1:{port}/personal-link/channel");
    let mut ready = false;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ready, "listener próprio do perfil alcançável na fixture qualificada");
    let mut b = abrir(&url, &p, &a).await?;
    cifrado(
        &mut b.socket,
        &mut b.saida,
        &json!(["donna.personal-link/link-status-request/v1", 1, canon::aleatorio::<16>(), b.sid, b.th]),
    )
    .await?;
    assert_eq!(decifrado(&mut b.socket, &mut b.entrada).await?[14], "active");
    stop.send(true)?;
    tokio::time::timeout(Duration::from_secs(3), job).await???;
    let f2 = Fixture::nova()?;
    let (p2, _) = f2.subir(false)?;
    let (stop, rx) = tokio::sync::watch::channel(false);
    let job = tokio::spawn(async move { channel::servir(p2, "127.0.0.1", port, rx).await });
    tokio::task::yield_now().await;
    let probe = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await?;
    assert!(
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await.is_ok(),
        "a porta pertence só à sonda do teste"
    );
    drop(probe);
    stop.send(true)?;
    tokio::time::timeout(Duration::from_secs(3), job).await???;
    Ok(())
}

/// PRB-SEC-R1-01: o listener real não pode ocupar outro endereço além do escolhido.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr_b_listener_respeita_bind_escolhido_sem_ocupar_outra_interface() -> Result<()> {
    let fixture = Fixture::nova()?;
    let (procedimento, clock) = fixture.subir(true)?;
    sair_da_quarentena(&clock);
    let aparelho = Aparelho::novo();
    gerida(&procedimento, &aparelho)?;
    let requested_bind = "127.0.0.1";
    let other_bind = "127.0.0.2";
    let probe = tokio::net::TcpListener::bind((requested_bind, 0)).await?;
    let port = probe.local_addr()?.port();
    // Pré-condição de ambiente: endereços distintos permitem reservas independentes neste porto.
    let other_probe = tokio::net::TcpListener::bind((other_bind, port)).await?;
    drop(other_probe);
    drop(probe);
    let (stop, rx) = tokio::sync::watch::channel(false);
    let listener_procedure = procedimento.clone();
    let job = tokio::spawn(async move { channel::servir(listener_procedure, requested_bind, port, rx).await });
    let observed = std::panic::AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if tokio::net::TcpStream::connect((requested_bind, port)).await.is_ok() {
                    return Ok::<(), anyhow::Error>(());
                }
                anyhow::ensure!(!job.is_finished(), "listener terminou antes da conexão");
                tokio::task::yield_now().await;
            }
        })
        .await??;
        let url = format!("ws://{requested_bind}:{port}/personal-link/channel");
        let _ready = abrir(&url, &procedimento, &aparelho).await?;
        // Listener já admitiu F/ACK no primário: agora a reserva do outro endereço é discriminante.
        let other_listener = tokio::net::TcpListener::bind((other_bind, port)).await;
        let diagnostic = other_listener.as_ref().err().map(ToString::to_string);
        Ok::<_, anyhow::Error>((other_listener.is_ok(), diagnostic))
    })
    .catch_unwind()
    .await;
    let _ = stop.send(true);
    let drained = tokio::time::timeout(Duration::from_secs(3), job).await;
    let (other_available, diagnostic) = match observed {
        Ok(result) => result?,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    drained???;
    assert!(other_available, "bind {requested_bind} ocupou {other_bind}:{port}: {diagnostic:?}");
    Ok(())
}
