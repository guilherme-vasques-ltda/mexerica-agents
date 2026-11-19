//! PR-B: transporte direto próprio → finish → ingresso privado → mesma Autoridade.
use super::{
    canon,
    channel_crypto::{self, Cifra},
    channel_root::RaizCanal,
    channel_socket as ws,
    procedure::Procedimento,
    time::Relogio,
    wire, writer,
};
use crate::{
    LockExt,
    fork::autoridade::{Autoridade, personal_channel::IntencaoCanal},
};
use anyhow::{Context, Result, ensure};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    sync::{Semaphore, mpsc, watch},
};

// PRB-DOC-P01: globais por instância, antes de upgrade/DB/RNG/assinatura; sem namespace fornecido por O.
pub(super) const CONEXOES: usize = 8;
pub(super) const HANDSHAKES: usize = 4;
pub(super) const SESSOES: usize = 4;
pub(super) const RESPOSTAS_POR_CONEXAO: usize = 1;
pub(super) const UPGRADE_MS: u64 = 5_000;
pub(super) const HANDSHAKE_MS: u64 = 30_000;
pub(super) const SESSAO_MS: u64 = 300_000;
const SAIDA_MS: u64 = 5_000;

/// Fatos RAM originais; somente o produtor abaixo constrói após F/consumo único.
pub(crate) struct AuthenticatedIngress {
    fatos: Arc<FatosSessao>,
}
impl AuthenticatedIngress {
    pub(crate) fn fatos(&self) -> Arc<FatosSessao> {
        self.fatos.clone()
    }
}
pub(crate) struct FatosSessao {
    raiz: RaizCanal,
    runtime: String,
    session: String,
    th: String,
    deadline: Value,
    born: u64,
    relogio: Arc<Relogio>,
    viva: Arc<AtomicBool>,
    instancia: Arc<AtomicBool>,
}
impl std::fmt::Debug for FatosSessao {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FatosSessao(PR-B)")
    }
}
impl FatosSessao {
    pub(crate) fn conferir(&self, db: &rusqlite::Connection) -> Result<()> {
        ensure!(
            self.viva.load(Ordering::Acquire) && self.instancia.load(Ordering::Acquire),
            "not_allowed: conexão encerrada"
        );
        ensure!(writer::runtime_atual(db)?.as_deref() == Some(self.runtime.as_str()), "not_allowed: runtime alterado");
        ensure!(!Relogio::vencido(self.born, SESSAO_MS, self.relogio.amostra()?)?, "not_allowed: sessão expirada");
        self.raiz.conferir(db)
    }
    fn status(&self, id: &Value) -> Value {
        let r = &self.raiz;
        json!([
            "donna.personal-link/link-status/v1",
            1,
            id,
            r.k.h,
            r.k.g,
            r.c,
            self.runtime,
            r.t,
            r.ra,
            r.chain,
            wire::p1(),
            0,
            self.session,
            self.th,
            "active",
            self.deadline
        ])
    }
}
struct Vida(Arc<AtomicBool>);
impl Drop for Vida {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Observação independente dos workers e da fila: fonte/continuidade/prazo também encerram espera ociosa.
async fn invalida(p: &Procedimento, prazo: Option<(u64, u64)>) {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tick.tick().await;
        if !p.viva.load(Ordering::Acquire) {
            return;
        }
        match p.relogio.amostra() {
            Err(_) => return,
            Ok(agora) => {
                if prazo.is_some_and(|(born, ttl)| Relogio::vencido(born, ttl, agora).unwrap_or(true)) {
                    return;
                }
            }
        }
    }
}

/// O observador não fica dentro de `spawn_blocking`: EOF/stop/dado prematuro/timeout invalidam imediatamente,
/// depois aguardam TODO job. Nunca abortar `spawn_blocking` nem liberar `host.lock` antes do retorno.
async fn vigiar<T: Send + 'static, R: AsyncRead + Unpin>(
    mut job: tokio::task::JoinHandle<Result<T>>,
    r: &mut R,
    stop: &mut watch::Receiver<bool>,
    viva: &AtomicBool,
    limite: tokio::time::Instant,
    p: &Procedimento,
    prazo: Option<(u64, u64)>,
) -> Result<T> {
    let mut byte = [0; 1];
    tokio::select! {
        result=&mut job=>return result?,
        ()=invalida(p,prazo)=>{},
        _=r.read(&mut byte)=>{},
        _=stop.changed()=>{},
        ()=tokio::time::sleep_until(limite)=>{},
    }
    viva.store(false, Ordering::Release);
    let _ = job.await;
    anyhow::bail!("not_allowed: vida encerrada")
}
async fn receber<R: AsyncRead + Unpin>(
    r: &mut R,
    stop: &mut watch::Receiver<bool>,
    limite: tokio::time::Instant,
    p: &Procedimento,
    prazo: Option<(u64, u64)>,
) -> Result<Vec<u8>> {
    ensure!(!*stop.borrow(), "not_allowed: parada");
    tokio::select! {()=invalida(p,prazo)=>anyhow::bail!("not_allowed: fonte/prazo"),result=ws::read(r)=>result,_=stop.changed()=>anyhow::bail!("not_allowed: parada"),()=tokio::time::sleep_until(limite)=>anyhow::bail!("not_allowed: timeout")}
}
async fn enviar<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8], stop: &mut watch::Receiver<bool>) -> Result<()> {
    tokio::select! {result=tokio::time::timeout(Duration::from_millis(SAIDA_MS),ws::write(w,bytes))=>result?,_=stop.changed()=>anyhow::bail!("not_allowed: parada")}
}

/// Um desafio proprietário por conexão, consumido por movimento, sem tabela persistida/restauração.
pub(super) async fn sessao<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    procedimento: Arc<Procedimento>,
    mut stream: S,
    mut stop: watch::Receiver<bool>,
    sessions: Arc<Semaphore>,
    handshake: tokio::sync::OwnedSemaphorePermit,
) -> Result<()> {
    let viva = Arc::new(AtomicBool::new(true));
    let _vida = Vida(viva.clone());
    let fim_handshake = tokio::time::Instant::now() + Duration::from_millis(HANDSHAKE_MS);
    tokio::select! {r=tokio::time::timeout(Duration::from_millis(UPGRADE_MS),ws::upgrade(&mut stream))=>r??,_=stop.changed()=>return Ok(())};
    let (mut read, mut write) = tokio::io::split(stream);
    let open = canon::ler(&receber(&mut read, &mut stop, fim_handshake, &procedimento, None).await?)?;
    let campos = open.as_array().context("not_allowed: O")?;
    ensure!(
        campos.len() == 9
            && campos[0] == "donna.personal-link/channel-open/v1"
            && campos[1] == 1
            && campos[2] == "manager",
        "not_allowed: O"
    );
    canon::b64n::<32>(&campos[3])?;
    canon::b64n::<32>(&campos[4])?;
    canon::hex64(&campos[5])?;
    canon::b64n::<32>(&campos[6])?;
    canon::hex64(&campos[7])?;
    canon::hex64(&campos[8])?;
    let chave = campos[6].as_str().context("not_allowed: T")?.to_owned();
    let p2 = procedimento.clone();
    let open2 = open.clone();
    let viva2 = viva.clone();
    let job = tokio::task::spawn_blocking(move || {
        ensure!(viva2.load(Ordering::Acquire) && p2.viva.load(Ordering::Acquire), "not_allowed: vida");
        p2.relogio.amostra()?;
        p2.hub.cloud.with_transport_owner(|_, _| {
            let db = p2.hub.store.db.locked();
            let root = RaizCanal::ler(&db, &p2.k, &chave)?;
            ensure!(root.open() == open2, "not_allowed: O divergente");
            Ok(root)
        })
    });
    let raiz = vigiar(job, &mut read, &mut stop, &viva, fim_handshake, &procedimento, None).await?;
    let secret = x25519_dalek::StaticSecret::from(channel_crypto::random::<32>()?);
    let eh = crate::remote::crypto::b64(&crate::remote::crypto::x25519_pub(&secret));
    let (d30, born_ch) = procedimento.relogio.prazo(&procedimento.runtime, HANDSHAKE_MS)?;
    let ch = canon::assinar(
        &procedimento.hub.identity,
        json!([
            "donna.personal-link/channel-challenge/v1",
            1,
            "manager",
            raiz.k.h,
            raiz.k.g,
            raiz.c,
            procedimento.runtime,
            channel_crypto::id::<32>()?,
            channel_crypto::id::<32>()?,
            eh,
            raiz.t,
            raiz.ra,
            raiz.chain,
            wire::p1(),
            0,
            d30
        ]),
    )?;
    enviar(&mut write, &ch.bytes()?, &mut stop).await?;
    let cr_raw = canon::ler(
        &receber(&mut read, &mut stop, fim_handshake, &procedimento, Some((born_ch.boottime_ms, HANDSHAKE_MS))).await?,
    )?;
    let cr = canon::verificar(&cr_raw, "donna.personal-link/channel-response/v1", 15, &raiz.t)?;
    let campos_resposta = cr.payload();
    ensure!(
        campos_resposta[2] == ch.digest()?
            && campos_resposta[3] == raiz.k.h
            && campos_resposta[4] == raiz.k.g
            && campos_resposta[5] == raiz.c
            && campos_resposta[6] == procedimento.runtime
            && campos_resposta[7] == "manager"
            && campos_resposta[8] == raiz.t
            && campos_resposta[11] == raiz.ra
            && campos_resposta[12] == raiz.chain
            && campos_resposta[13] == wire::p1()
            && campos_resposta[14] == 0,
        "not_allowed: CR divergente"
    );
    let ec = canon::b64n::<32>(&campos_resposta[9])?;
    canon::b64n::<32>(&campos_resposta[10])?;
    ensure!(
        !Relogio::vencido(born_ch.boottime_ms, HANDSHAKE_MS, procedimento.relogio.amostra()?)?,
        "not_allowed: CH expirado"
    );
    let shared = zeroize::Zeroizing::new(crate::remote::crypto::x25519(&secret, &ec)?);
    drop(secret);
    let sid = channel_crypto::id::<16>()?;
    let th0 = canon::hash(&json!(["donna.personal-link/channel-transcript/v1", 1, ch.valor(), cr.valor()]))?;
    let (deadline, born) = procedimento.relogio.prazo(&procedimento.runtime, SESSAO_MS)?;
    let fim_sessao = tokio::time::Instant::now() + Duration::from_millis(SESSAO_MS);
    let welcome = canon::assinar(
        &procedimento.hub.identity,
        json!([
            "donna.personal-link/channel-welcome/v1",
            1,
            raiz.k.h,
            raiz.k.g,
            raiz.c,
            procedimento.runtime,
            "manager",
            raiz.t,
            th0,
            sid,
            deadline,
            wire::p1(),
            0
        ]),
    )?;
    let transcript = json!(["donna.personal-link/channel-final/v1", 1, ch.valor(), cr.valor(), welcome.valor()]);
    let th = canon::hash(&transcript)?;
    let mut entrada = Cifra::nova(&shared, &transcript, &sid, "c2h")?;
    let mut saida = Cifra::nova(&shared, &transcript, &sid, "h2c")?;
    drop(shared);
    enviar(&mut write, &welcome.bytes()?, &mut stop).await?;
    let finish = entrada.abrir(
        &receber(&mut read, &mut stop, fim_handshake, &procedimento, Some((born_ch.boottime_ms, HANDSHAKE_MS))).await?,
    )?;
    ensure!(
        finish == json!(["donna.personal-link/channel-finish/v1", 1, sid, th, welcome.digest()?]),
        "not_allowed: F"
    );
    ensure!(
        !Relogio::vencido(born_ch.boottime_ms, HANDSHAKE_MS, procedimento.relogio.amostra()?)?,
        "not_allowed: CH expirado"
    );
    let _session_permit = sessions.try_acquire_owned()?;
    let (p2, r2, v2) = (procedimento.clone(), raiz.clone(), viva.clone());
    let job = tokio::task::spawn_blocking(move || {
        p2.hub.cloud.with_transport_owner(|_, _| {
            ensure!(v2.load(Ordering::Acquire) && p2.viva.load(Ordering::Acquire), "not_allowed: vida");
            ensure!(
                !Relogio::vencido(born_ch.boottime_ms, HANDSHAKE_MS, p2.relogio.amostra()?)?,
                "not_allowed: CH expirado"
            );
            let mut db = p2.hub.store.db.locked();
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            r2.conferir(&tx)?;
            ensure!(writer::runtime_atual(&tx)?.as_deref() == Some(p2.runtime.as_str()), "not_allowed: runtime");
            r2.conferir(&tx)?;
            tx.commit()?;
            Ok(())
        })
    });
    vigiar(job, &mut read, &mut stop, &viva, fim_handshake, &procedimento, Some((born_ch.boottime_ms, HANDSHAKE_MS)))
        .await?;
    // CH pertence somente a este stack; nenhum caminho volta a recebê-lo depois do finish.
    drop(handshake);
    let ingresso = Arc::new(AuthenticatedIngress {
        fatos: Arc::new(FatosSessao {
            raiz,
            runtime: procedimento.runtime.clone(),
            session: sid.clone(),
            th: th.clone(),
            deadline,
            born: born.boottime_ms,
            relogio: procedimento.relogio.clone(),
            viva: viva.clone(),
            instancia: procedimento.viva.clone(),
        }),
    });
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(RESPOSTAS_POR_CONEXAO);
    let mut proxima = IntencaoCanal::SessionOpen;
    let mut request = Value::Bool(false);
    loop {
        let permit = tx.clone().try_reserve_owned()?;
        let p2 = procedimento.clone();
        let i2 = ingresso.clone();
        let intent = proxima;
        let job = tokio::task::spawn_blocking(move || Autoridade::capturar_canal(&p2.hub, &i2, intent));
        let original =
            vigiar(job, &mut read, &mut stop, &viva, fim_sessao, &procedimento, Some((born.boottime_ms, SESSAO_MS)))
                .await?;
        let p2 = procedimento.clone();
        let i2 = ingresso.clone();
        let request2 = request.clone();
        let job = tokio::task::spawn_blocking(move || {
            original.exigir_canal(intent)?;
            let plain = original.with_db(&p2.hub, None, |_| {
                Ok(match intent {
                    IntencaoCanal::SessionOpen => {
                        json!(["donna.personal-link/channel-ready/v1", 1, i2.fatos.session, i2.fatos.th])
                    }
                    IntencaoCanal::LinkStatus => i2.fatos.status(&request2),
                    IntencaoCanal::SessionClose => Value::Bool(false),
                })
            })?;
            if intent == IntencaoCanal::SessionClose {
                return Ok(saida);
            }
            let bytes = saida.selar(&plain)?;
            original.com_saida(&p2.hub, None, || {
                drop(permit.send(bytes));
            })?;
            Ok(saida)
        });
        saida =
            vigiar(job, &mut read, &mut stop, &viva, fim_sessao, &procedimento, Some((born.boottime_ms, SESSAO_MS)))
                .await?;
        if intent == IntencaoCanal::SessionClose {
            return Ok(());
        }
        let bytes = rx.try_recv()?;
        enviar(&mut write, &bytes, &mut stop).await?;
        let pedido = entrada.abrir(
            &receber(&mut read, &mut stop, fim_sessao, &procedimento, Some((born.boottime_ms, SESSAO_MS))).await?,
        )?;
        let campos = pedido.as_array().context("not_allowed: operação")?;
        // Enum fechado antes de captura/fila/qualquer cofre. Sem alias/payload/subscription.
        if campos.len() == 5
            && campos[0] == "donna.personal-link/link-status-request/v1"
            && campos[1] == 1
            && campos[3] == sid
            && campos[4] == th
        {
            canon::b64n::<16>(&campos[2])?;
            request = campos[2].clone();
            proxima = IntencaoCanal::LinkStatus;
        } else if pedido == json!(["donna.personal-link/channel-close/v1", 1, sid, th]) {
            proxima = IntencaoCanal::SessionClose;
        } else {
            anyhow::bail!("not_allowed: operação desconhecida");
        }
    }
}

/// Composição própria: nenhum router comum/hub.start. Unqualified não abre TCP.
pub(super) async fn servir(p: Arc<Procedimento>, bind: &str, port: u16, mut stop: watch::Receiver<bool>) -> Result<()> {
    if !p.relogio.qualificada() {
        let _ = stop.changed().await;
        return Ok(());
    }
    // Não expõe listener enquanto a raiz ainda não estiver completa/gerida. O procedimento A pode
    // concluir na mesma instância; polling só lê e cada job é drenado inclusive na parada.
    loop {
        let p2 = p.clone();
        let mut job = tokio::task::spawn_blocking(move || {
            p2.hub.cloud.with_transport_owner(|_, _| {
                let db = p2.hub.store.db.locked();
                let t: Option<String> = db
                    .query_row("SELECT key FROM personal_link_roots WHERE state='complete' LIMIT 1", [], |r| r.get(0))
                    .optional()?;
                Ok(t.is_some_and(|t| RaizCanal::ler(&db, &p2.k, &t).is_ok()))
            })
        });
        let eligible = tokio::select! {
            r=&mut job=>r??,
            _=stop.changed()=>{let _=job.await;return Ok(());}
            ()=invalida(&p,None)=>{let _=job.await;let _=stop.changed().await;return Ok(());}
        };
        if eligible {
            break;
        }
        tokio::select! {_=stop.changed()=>return Ok(()),()=tokio::time::sleep(Duration::from_secs(1))=>{}}
        if p.relogio.amostra().is_err() {
            let _ = stop.changed().await;
            return Ok(());
        }
    }
    let (aviso, parada) = watch::channel(false);
    let listener = tokio::net::TcpListener::bind((bind, port)).await?;
    let connections = Arc::new(Semaphore::new(CONEXOES));
    let handshakes = Arc::new(Semaphore::new(HANDSHAKES));
    let sessions = Arc::new(Semaphore::new(SESSOES));
    let mut jobs = tokio::task::JoinSet::new();
    let mut fonte_fechada = false;
    let result = loop {
        tokio::select! {
            _=stop.changed()=>break Ok(()),
            ()=invalida(&p,None)=>{fonte_fechada=true;break Ok(());}
            Some(_)=jobs.join_next(),if !jobs.is_empty()=>{},
            accepted=listener.accept()=>{
                let (stream,_)=match accepted{Ok(v)=>v,Err(e)=>break Err(e.into())};
                let Ok(connection)=connections.clone().try_acquire_owned() else{continue};
                let Ok(handshake)=handshakes.clone().try_acquire_owned() else{continue};
                let (p,stop,sessions)=(p.clone(),parada.clone(),sessions.clone());
                jobs.spawn(async move{let _permit=connection;let _=sessao(p,stream,stop,sessions,handshake).await;});
            }
        }
    };
    let _ = aviso.send(true);
    drop(listener);
    if !fonte_fechada {
        p.viva.store(false, Ordering::Release);
    }
    // O chamador distribui stop antes de aguardar este drain.
    while jobs.join_next().await.is_some() {}
    if fonte_fechada {
        let _ = stop.changed().await;
    }
    result
}
