//! Procedimento administrativo inicial D430 (PR-A), servido SOMENTE no socket Unix privado da
//! instância (peer conferido em `installation`). Frames J `donna.personal-link/procedure/v1`
//! apenas transportam os objetos exatos do v2; não há assinador genérico nem RPC de bots.
//! Dois atos separados: raiz connect-only (RA→PC→RC) e gestão inicial (management→MR).
use super::{
    canon::{self, Assinado},
    time::{DESAFIO_MS, Relogio},
    wire::{self, Contexto},
    writer::{self, Janela},
};
use crate::{
    LockExt,
    fork::autoridade::{Autoridade, personal_link::IntencaoInicial},
    hub::Hub,
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::TransactionBehavior;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{
        mpsc::{self, OwnedPermit},
        watch,
    },
};

pub(crate) const PROTOCOLO: &str = "donna.personal-link/procedure/v1";

/// Fatos RAM do canal administrativo atual. Campos privados, sem Deserialize/construtor público.
pub(crate) struct IngressoInicial {
    instalacao: String,
    hub: String,
    runtime: String,
    sessao: Arc<AtomicBool>,
    instancia: Arc<AtomicBool>,
}
impl IngressoInicial {
    pub(crate) fn instalacao(&self) -> &str {
        &self.instalacao
    }
    pub(crate) fn hub(&self) -> &str {
        &self.hub
    }
    pub(crate) fn runtime(&self) -> &str {
        &self.runtime
    }
    pub(crate) fn sessao(&self) -> Arc<AtomicBool> {
        self.sessao.clone()
    }
    pub(crate) fn instancia(&self) -> Arc<AtomicBool> {
        self.instancia.clone()
    }
}

pub(crate) struct Procedimento {
    #[cfg(all(test, target_os = "linux"))]
    pub(super) sincronia: super::test_sync::Sincronia,
    pub(super) hub: Arc<Hub>,
    pub(super) k: Contexto,
    pub(super) relogio: Arc<Relogio>,
    pub(super) runtime: String,
    /// Vida da instância: a parada a desliga antes do drain (SEG-A-R1-02).
    pub(super) viva: Arc<AtomicBool>,
}

enum Pendente {
    Raiz { desafio: Assinado, nascido: u64 },
    Persistencia { t: String, admission_id: String, n_h2: String, d30: Value, nascido: u64 },
    Gestao { payload: Value, nascido: u64 },
}

/// Sessão de UMA conexão do peer conferido. Desafios one-shot e ligados a ela.
pub(crate) struct Sessao {
    viva: Arc<AtomicBool>,
    pendente: Option<Pendente>,
}
impl Sessao {
    pub(crate) fn nova() -> Self {
        Self { viva: Arc::new(AtomicBool::new(true)), pendente: None }
    }
    /// Mesma bandeira observada pela tarefa da conexão, FORA do worker (EOF/parada a desligam).
    fn viva(&self) -> Arc<AtomicBool> {
        self.viva.clone()
    }
}
impl Drop for Sessao {
    fn drop(&mut self) {
        self.viva.store(false, Ordering::Release);
    }
}

/// Erros públicos uniformes; detalhes não saem do Hub.
fn codigo(p: &Procedimento, erro: &anyhow::Error) -> &'static str {
    let texto = erro.to_string();
    if !p.relogio.qualificada() {
        "clockUnqualified"
    } else if texto.contains("(D456)") {
        "reopened"
    } else if texto.contains("quarentena") {
        "quarantine"
    } else {
        "refused"
    }
}

fn texto(args: &[Value], i: usize) -> Result<&str> {
    args.get(i).and_then(Value::as_str).context("not_allowed: argumento ausente")
}

impl Procedimento {
    fn ingresso(&self, sessao: &Arc<AtomicBool>) -> IngressoInicial {
        IngressoInicial {
            instalacao: self.k.instalacao.clone(),
            hub: self.k.h.clone(),
            runtime: self.runtime.clone(),
            sessao: sessao.clone(),
            instancia: self.viva.clone(),
        }
    }

    #[cfg(test)]
    pub(super) fn ingresso_para_teste(&self, sessao: &Sessao) -> IngressoInicial {
        self.ingresso(&sessao.viva)
    }

    fn vivo(&self, nascido: u64) -> Result<()> {
        let agora = self.relogio.amostra()?;
        ensure!(!Relogio::vencido(nascido, DESAFIO_MS, agora)?, "not_allowed: desafio expirado");
        Ok(())
    }

    fn janela(&self) -> Result<Janela<'_>> {
        let agora = self.relogio.amostra()?;
        let quarentena_terminada = self.relogio.quarentena_terminada(agora)?;
        Ok(Janela { runtime: &self.runtime, boot_id: self.relogio.boot_id(), agora, quarentena_terminada })
    }

    fn ler_raiz(&self, t: &str) -> Result<Option<writer::Raiz>> {
        let db = self.hub.store.db.locked();
        writer::raiz(&db, &self.k, t)
    }

    /// Captura ORIGINAL da intenção fechada, feita pela tarefa da conexão ANTES da fila do worker.
    fn capturar(&self, sessao: &Arc<AtomicBool>, tipo: &str) -> Result<Option<Autoridade>> {
        let intencao = match tipo {
            "root-accept" => IntencaoInicial::AceiteRaiz,
            "persist-confirm" => IntencaoInicial::ConfirmaPersistencia,
            "management-accept" => IntencaoInicial::ConcedeGestao,
            _ => return Ok(None),
        };
        Autoridade::capturar_inicial(&self.hub, &self.ingresso(sessao), intencao).map(Some)
    }

    /// Lógica do procedimento com captura já feita (testes de lógica chamam por aqui sem socket).
    #[cfg(test)]
    pub(crate) fn tratar(&self, sessao: &mut Sessao, tipo: &str, args: &[Value]) -> Result<Value> {
        let autoridade = self.capturar(&sessao.viva, tipo)?;
        self.executar(sessao, tipo, args, autoridade.as_ref())
    }

    fn executar(&self, sessao: &mut Sessao, tipo: &str, args: &[Value], a: Option<&Autoridade>) -> Result<Value> {
        match tipo {
            "root-challenge" => self.desafio_raiz(sessao, args),
            "root-accept" => self.aceite_raiz(sessao, args, a.context("not_allowed: captura ausente")?),
            "persist-challenge" => self.desafio_persistencia(sessao, args),
            "persist-confirm" => self.confirma_persistencia(sessao, args, a.context("not_allowed: captura ausente")?),
            "management-challenge" => self.desafio_gestao(sessao, args),
            "management-accept" => self.aceite_gestao(sessao, args, a.context("not_allowed: captura ausente")?),
            "reconcile" => self.reconciliar(args),
            _ => bail!("not_allowed: operação D430 desconhecida"),
        }
    }

    fn desafio_raiz(&self, sessao: &mut Sessao, args: &[Value]) -> Result<Value> {
        ensure!(args.len() == 3, "not_allowed: aridade");
        let (t, c, n_t) = (texto(args, 0)?, texto(args, 1)?, texto(args, 2)?);
        canon::b64n::<32>(&args[0])?;
        canon::hex64(&args[1])?;
        canon::b64n::<32>(&args[2])?;
        let janela = self.janela()?;
        ensure!(janela.quarentena_terminada, "not_allowed: quarentena de novas admissões");
        let bloqueada = writer::nova_raiz_bloqueada(&self.hub.store.db.locked(), &self.k, &self.runtime, t)?;
        ensure!(!bloqueada, "not_allowed: raiz D430 já existe");
        let (d30, nascido) = self.relogio.prazo(&self.runtime, DESAFIO_MS)?;
        let (op, n_h, root_id) = (canon::aleatorio::<16>(), canon::aleatorio::<32>(), canon::aleatorio::<16>());
        let desafio = wire::desafio(
            &self.hub.identity,
            &self.k,
            &wire::Desafio { c, r: &self.runtime, op: &op, t, n_t, n_h: &n_h, d30: &d30, root_id: &root_id },
        )?;
        let valor = desafio.valor().clone();
        sessao.pendente = Some(Pendente::Raiz { desafio, nascido: nascido.boottime_ms });
        Ok(valor)
    }

    fn aceite_raiz(&self, sessao: &mut Sessao, args: &[Value], autoridade: &Autoridade) -> Result<Value> {
        let Some(Pendente::Raiz { desafio, nascido }) = sessao.pendente.take() else {
            bail!("not_allowed: sem desafio de raiz vivo")
        };
        ensure!(args.len() == 1, "not_allowed: aridade");
        let aceite = wire::aceite(&args[0], &desafio)?;
        self.vivo(nascido)?;
        autoridade.exigir_inicial(IntencaoInicial::AceiteRaiz)?;
        let janela = self.janela()?;
        let ra = autoridade.with_db(&self.hub, None, |tx| {
            self.vivo(nascido)?;
            writer::admitir(tx, &self.hub.identity, &self.k, &desafio, &aceite, &janela)
        })?;
        // Housekeeping depois do commit OBSERVADO: âncora conservadora; não admite nem libera vaga.
        if let Ok(depois) = self.relogio.amostra() {
            let mut db = self.hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            writer::ancorar(&tx, &self.runtime, self.relogio.boot_id(), depois)?;
            tx.commit()?;
        }
        Ok(ra.valor().clone())
    }

    fn desafio_persistencia(&self, sessao: &mut Sessao, args: &[Value]) -> Result<Value> {
        ensure!(args.len() == 2, "not_allowed: aridade");
        let (t, admission_id) = (texto(args, 0)?, texto(args, 1)?);
        let raiz = self.ler_raiz(t)?.context("not_allowed: admissão ausente")?;
        ensure!(raiz.admission_id == admission_id && raiz.rc.is_none(), "not_allowed: admissão não pendente");
        // D456: só o MESMO runtime conclui (resposta perdida); depois da reabertura fica incerta/fechada.
        ensure!(!raiz.pendente_de_outro_runtime(&self.runtime), "not_allowed: admissão reaberta fica fechada (D456)");
        let (d30, nascido) = self.relogio.prazo(&self.runtime, DESAFIO_MS)?;
        let n_h2 = canon::aleatorio::<32>();
        let resposta = json!([n_h2, d30, self.runtime]);
        sessao.pendente = Some(Pendente::Persistencia {
            t: t.to_owned(),
            admission_id: admission_id.to_owned(),
            n_h2,
            d30,
            nascido: nascido.boottime_ms,
        });
        Ok(resposta)
    }

    fn confirma_persistencia(&self, sessao: &mut Sessao, args: &[Value], autoridade: &Autoridade) -> Result<Value> {
        let Some(Pendente::Persistencia { t, admission_id, n_h2, d30, nascido }) = sessao.pendente.take() else {
            bail!("not_allowed: sem desafio de persistência vivo")
        };
        ensure!(args.len() == 1, "not_allowed: aridade");
        let raiz = self.ler_raiz(&t)?.context("not_allowed: admissão ausente")?;
        ensure!(raiz.admission_id == admission_id, "not_allowed: admissão divergente");
        let pc = wire::persistido(&args[0], &raiz.ra, &self.runtime, &n_h2, &d30)?;
        self.vivo(nascido)?;
        autoridade.exigir_inicial(IntencaoInicial::ConfirmaPersistencia)?;
        let rc = autoridade.with_db(&self.hub, None, |tx| {
            self.vivo(nascido)?;
            writer::concluir(tx, &self.hub.identity, &self.k, &pc, &self.runtime)
        })?;
        Ok(rc.valor().clone())
    }

    fn desafio_gestao(&self, sessao: &mut Sessao, args: &[Value]) -> Result<Value> {
        ensure!(args.len() == 2, "not_allowed: aridade");
        let (t, digest) = (texto(args, 0)?, canon::hex64(&args[1])?);
        let raiz = self.ler_raiz(t)?.context("not_allowed: raiz ausente")?;
        let c = raiz.ra.payload()[4].as_str().context("not_allowed: contexto")?.to_owned();
        ensure!(
            raiz.rc.is_some() && raiz.gestao.is_none() && raiz.ra.digest()? == digest,
            "not_allowed: gestão inicial indisponível"
        );
        // Gestão não é admissão: pode ocorrer em quarentena, com desafio e guardas atuais.
        let (d30, nascido) = self.relogio.prazo(&self.runtime, DESAFIO_MS)?;
        let (op, n_h) = (canon::aleatorio::<16>(), canon::aleatorio::<32>());
        let payload = wire::gestao_payload(
            &self.k,
            &wire::Gestao {
                r: &self.runtime,
                op: &op,
                t,
                c: &c,
                digest_raiz: digest,
                revisao_anterior: 1,
                n_h: &n_h,
                d30: &d30,
            },
        );
        sessao.pendente = Some(Pendente::Gestao { payload: payload.clone(), nascido: nascido.boottime_ms });
        Ok(payload)
    }

    fn aceite_gestao(&self, sessao: &mut Sessao, args: &[Value], autoridade: &Autoridade) -> Result<Value> {
        let Some(Pendente::Gestao { payload, nascido }) = sessao.pendente.take() else {
            bail!("not_allowed: sem desafio de gestão vivo")
        };
        ensure!(args.len() == 1, "not_allowed: aridade");
        let m = wire::gestao(&args[0], &payload)?;
        self.vivo(nascido)?;
        autoridade.exigir_inicial(IntencaoInicial::ConcedeGestao)?;
        let mr = autoridade.with_db(&self.hub, None, |tx| {
            self.vivo(nascido)?;
            writer::conceder(tx, &self.hub.identity, &self.k, &m, &self.runtime)
        })?;
        Ok(mr.valor().clone())
    }

    /// Devolve os envelopes ORIGINAIS (públicos, assinados). Recibo não é autorização de sessão.
    fn reconciliar(&self, args: &[Value]) -> Result<Value> {
        ensure!(args.len() == 1, "not_allowed: aridade");
        canon::b64n::<32>(&args[0])?;
        let Some(r) = self.ler_raiz(texto(args, 0)?)? else { return Ok(json!(["none"])) };
        Ok(match (&r.rc, &r.gestao) {
            (None, _) if r.pendente_de_outro_runtime(&self.runtime) => json!(["closed", r.admission_id, r.ra.valor()]),
            (None, _) => json!(["pending", r.admission_id, r.ra.valor()]),
            (Some(rc), None) => json!(["complete", r.admission_id, r.ra.valor(), rc.valor()]),
            (Some(rc), Some((m, mr))) => json!([
                "managed",
                r.admission_id,
                r.ra.valor(),
                rc.valor(),
                m.valor(),
                mr.valor(),
                wire::cadeia_t(&r.ra, rc, m, mr)?
            ]),
        })
    }
}

pub(crate) async fn ler_frame<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Value> {
    let tamanho = tokio::time::timeout(Duration::from_secs(120), stream.read_u32()).await??;
    let tamanho = usize::try_from(tamanho)?;
    ensure!((1..=canon::LIMITE).contains(&tamanho), "not_allowed: frame D430 inválido");
    let mut bytes = vec![0; tamanho];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut bytes)).await??;
    canon::ler(&bytes)
}

pub(crate) async fn escrever_frame<S: AsyncWrite + Unpin>(stream: &mut S, valor: &Value) -> Result<()> {
    let bytes = canon::j(valor)?;
    stream.write_u32(u32::try_from(bytes.len())?).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

impl Procedimento {
    /// Saída: a vaga já foi reservada; o envio síncrono ocorre DENTRO da guarda de saída (atos que gravam) ou
    /// depois de conferir sessão e instância vivas (desafios/reconcile). Falha → nada é enviado.
    fn sair(
        &self,
        autoridade: Option<&Autoridade>,
        viva: &AtomicBool,
        vaga: OwnedPermit<Value>,
        resposta: Value,
    ) -> bool {
        match autoridade {
            Some(a) => a
                .com_saida(&self.hub, None, || {
                    drop(vaga.send(resposta));
                })
                .is_ok(),
            None if viva.load(Ordering::Acquire) && self.viva.load(Ordering::Acquire) => {
                drop(vaga.send(resposta));
                true
            }
            None => false,
        }
    }
}

/// Uma conexão = uma sessão; uma operação em voo. A tarefa da conexão (fora do worker) observa EOF, dado fora de
/// hora e a parada enquanto o worker trabalha: desliga a sessão e AGUARDA o worker, sem abortar nem responder.
pub(crate) async fn sessao<S>(p: &Arc<Procedimento>, stream: S, mut parada: watch::Receiver<bool>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut leitura, mut escrita) = tokio::io::split(stream);
    let (envio, mut fila) = mpsc::channel::<Value>(1);
    let escritor = tokio::spawn(async move {
        while let Some(resposta) = fila.recv().await {
            if escrever_frame(&mut escrita, &resposta).await.is_err() {
                break;
            }
        }
    });
    let mut sessao = Sessao::nova();
    let viva = sessao.viva();
    let resultado = loop {
        if *parada.borrow() || !p.viva.load(Ordering::Acquire) {
            break Ok(());
        }
        #[cfg(all(test, target_os = "linux"))]
        p.sincronia.emitir(super::test_sync::Evento::ProntaParaPedido);
        let frame = tokio::select! {
            frame = ler_frame(&mut leitura) => frame,
            _ = parada.changed() => break Ok(()),
        };
        let Ok(frame) = frame else { break Ok(()) };
        let Some((id, tipo, args)) = pedido(&frame) else { break Err(anyhow::anyhow!("not_allowed: protocolo D430")) };
        let Ok(vaga) = envio.clone().reserve_owned().await else { break Ok(()) };
        // Captura original ANTES da fila de negócio, também vigiada: se DB/Cloud estiverem ocupados, EOF/parada
        // desligam a sessão enquanto ela espera. O worker não recaptura nem renova a concessão.
        let captura = {
            let (p2, viva2, tipo2) = (p.clone(), viva.clone(), tipo.clone());
            tokio::task::spawn_blocking(move || {
                #[cfg(all(test, target_os = "linux"))]
                let _conclusao = p2.sincronia.iniciar(&tipo2, super::test_sync::Etapa::Captura);
                p2.capturar(&viva2, &tipo2)
            })
        };
        let autoridade = vigiar(
            captura,
            &mut leitura,
            &mut parada,
            &viva,
            #[cfg(all(test, target_os = "linux"))]
            &p.sincronia,
        )
        .await?;
        if !viva.load(Ordering::Acquire) {
            break Ok(());
        }
        let (p2, viva2) = (p.clone(), viva.clone());
        let trabalho = tokio::task::spawn_blocking(move || {
            #[cfg(all(test, target_os = "linux"))]
            let _conclusao = p2.sincronia.iniciar(&tipo, super::test_sync::Etapa::Trabalho);
            let (autoridade, resultado) = match autoridade {
                Ok(a) => {
                    let r = p2.executar(&mut sessao, &tipo, &args, a.as_ref());
                    (a, r)
                }
                Err(e) => (None, Err(e)),
            };
            let resposta = match resultado {
                Ok(valor) => json!([PROTOCOLO, 1, id, "ok", tipo, valor]),
                Err(e) => json!([PROTOCOLO, 1, id, "error", codigo(&p2, &e)]),
            };
            let enviado = p2.sair(autoridade.as_ref(), &viva2, vaga, resposta);
            (sessao, enviado)
        });
        let (devolvida, enviado) = vigiar(
            trabalho,
            &mut leitura,
            &mut parada,
            &viva,
            #[cfg(all(test, target_os = "linux"))]
            &p.sincronia,
        )
        .await?;
        sessao = devolvida;
        if !enviado || !viva.load(Ordering::Acquire) {
            break Ok(());
        }
    };
    viva.store(false, Ordering::Release);
    drop(envio);
    let _ = escritor.await;
    resultado
}

/// Aguarda trabalho bloqueante SEM abortá-lo, observando ao mesmo tempo o canal e a parada: fim do canal, dado
/// fora de hora ou parada desligam a sessão (fora do worker) e então espera o worker terminar.
async fn vigiar<T: Send + 'static, R: AsyncRead + Unpin>(
    mut trabalho: tokio::task::JoinHandle<T>,
    leitura: &mut R,
    parada: &mut watch::Receiver<bool>,
    viva: &AtomicBool,
    #[cfg(all(test, target_os = "linux"))] sincronia: &super::test_sync::Sincronia,
) -> Result<T> {
    let mut sonda = [0u8; 1];
    tokio::select! {
        r = &mut trabalho => return Ok(r?),
        _ = leitura.read(&mut sonda) => {}
        _ = parada.changed() => {}
    }
    viva.store(false, Ordering::Release);
    #[cfg(all(test, target_os = "linux"))]
    sincronia.invalidada(viva);
    #[cfg(all(test, target_os = "linux"))]
    let trabalho = sincronia.aguardar(trabalho);
    Ok(trabalho.await?)
}

fn pedido(frame: &Value) -> Option<(String, String, Vec<Value>)> {
    let a = frame.as_array()?;
    if a.len() < 4 || a[0] != PROTOCOLO || a[1] != 1 {
        return None;
    }
    let id = canon::b64n::<16>(&a[2]).ok().map(|b| crate::remote::crypto::b64(&b))?;
    Some((id, a[3].as_str()?.to_owned(), a[4..].to_vec()))
}
