//! Um dono de Session no Hub; a ponte só transporta frames e handles opacos.
use super::{
    Source, available,
    contexto::ContextoExecucao,
    operacao::{self, ContaSaida, Estado, Privado, SnapshotAcesso, Tentativa},
};
use crate::{
    LockExt,
    guard_hub::mcp::{
        self,
        http::{Mediation, Session, Target},
    },
    hub::Hub,
    store::{Lane, conectores as ledger},
};
use anyhow::{Result, anyhow, bail, ensure};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    time::Instant,
};
use zeroize::Zeroizing;

#[derive(Clone)]
struct Active {
    id: String,
    tool: String,
    clock: Instant,
}

pub(super) struct Channel {
    pub(super) id: String,
    pub(super) bot: String,
    pub(super) source: SnapshotAcesso,
    /// Conta legível capturada com a concessão; comparada à fonte viva em cada cerca.
    pub(super) conta: ContaSaida,
    request: Arc<Mutex<Option<Arc<ContextoExecucao>>>>,
    lane: Lane,
    binding: OnceLock<String>,
    session: AsyncMutex<Session<HubMediation>>,
    active: Arc<Mutex<Option<Active>>>,
    closed: Arc<AtomicBool>,
    discovered: Arc<AtomicBool>,
    pub(super) changed: watch::Sender<u64>,
}
impl Channel {
    pub(super) fn new(hub: &Arc<Hub>, bot: &str, source: &Source, id: &str, lane: &Lane) -> Result<Self> {
        let request = Arc::new(Mutex::new(None));
        let active = Arc::new(Mutex::new(None));
        let closed = Arc::new(AtomicBool::new(false));
        let discovered = Arc::new(AtomicBool::new(false));
        let conta = source.conta.clone().ok_or_else(|| anyhow!("conta_saida_nao_identificada"))?;
        let mediation = HubMediation {
            hub: Arc::downgrade(hub),
            source: source.access.clone(),
            conta: conta.clone(),
            request: request.clone(),
            active: active.clone(),
            closed: closed.clone(),
            discovered: discovered.clone(),
            bridge: id.into(),
        };
        Ok(Self {
            id: id.into(),
            bot: bot.into(),
            source: source.access.clone(),
            conta,
            request,
            lane: lane.clone(),
            binding: OnceLock::new(),
            session: AsyncMutex::new(Session::new(mediation)?),
            active,
            closed,
            discovered,
            changed: watch::Sender::new(0),
        })
    }
    pub(super) fn bind(&self, hub: &Hub, sid: &str) -> Result<()> {
        ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
        let c = hub.store.db.locked();
        let bound: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_session_acp WHERE bot=?1 AND acp_session=?2 AND session=?3)",
            rusqlite::params![self.bot, sid, self.lane.session_id()],
            |r| r.get(0),
        )?;
        ensure!(bound, "Sessão ACP ainda não vinculada");
        self.binding.set(sid.into()).map_err(|_| anyhow!("Canal já vinculado"))
    }
    pub(super) fn live(&self, sid: &str) -> bool {
        !self.closed.load(Ordering::Acquire) && self.binding.get().is_some_and(|bound| bound == sid)
    }
    fn check_binding(&self, context: &ContextoExecucao) -> Result<()> {
        ensure!(
            context.channel == self.id
                && context.bot == self.bot
                && context.lane == self.lane
                && self.binding.get() == Some(&context.acp_session),
            "Vínculo ACP/canal incorreto"
        );
        Ok(())
    }
    fn capture(&self, hub: &Hub) -> Result<Arc<ContextoExecucao>> {
        ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
        ensure!(self.binding.get().is_some(), "Sessão ACP ainda não vinculada");
        hub.conector_fence(&self.bot, |runtime| {
            super::source(hub, &self.source.instance, |source| {
                available(source)?;
                ensure!(source.same(&self.source, &self.conta), "Fonte alterada");
                let c = hub.store.db.locked();
                source.verify_engine(&c)?;
                let context = ContextoExecucao::capture(hub, &self.id, &self.bot, runtime, &c)?;
                self.check_binding(&context)?;
                Ok(Arc::new(context))
            })
        })
    }
    pub(super) fn transaction<T>(
        &self,
        hub: &Hub,
        context: &ContextoExecucao,
        action: impl FnOnce(&Connection) -> Result<(T, Vec<crate::store::Entry>)>,
    ) -> Result<T> {
        ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
        self.check_binding(context)?;
        // emit precede runtime: bot_json também lê runtime sob emit no Hub atual.
        hub.conector_publish(|| {
            hub.conector_fence(&self.bot, |runtime| {
                super::source(hub, &self.source.instance, |source| {
                    available(source)?;
                    ensure!(
                        !self.closed.load(Ordering::Acquire) && source.same(&self.source, &self.conta),
                        "Concessão original revogada"
                    );
                    let c = hub.store.db.locked();
                    source.verify_engine(&c)?;
                    context.check(hub, runtime, &c)?;
                    action(&c)
                })
            })
        })
    }
    pub(super) fn private(&self, hub: &Hub, id: &str) -> Result<(Tentativa, Option<Privado>)> {
        let original = hub.store.conector_attempt(id)?.ok_or_else(|| anyhow!("Tentativa ausente"))?;
        let (op, cipher) = self.transaction(hub, &original.context, |c| {
            let op = ledger::read(c, id)?.ok_or_else(|| anyhow!("Tentativa ausente"))?;
            ensure!(op.context == original.context && op.source.same_grant(&self.source), "Canal incorreto");
            let cipher = crate::store::conectores::cipher(c, &op.slot())?;
            Ok(((op, cipher), vec![]))
        })?;
        let private = cipher
            .as_ref()
            .map(|cipher| -> Result<Privado> {
                let plain = Zeroizing::new(crate::market::vault::open(&hub.store, &op.slot(), cipher)?);
                let private = serde_json::from_str::<Privado>(&plain)?;
                // A conta mostrada é a capturada com o pedido, nunca a de outra fonte.
                ensure!(
                    private.presentation["conta"] == self.conta.presentation(),
                    "Conta de saída divergente do pedido"
                );
                Ok(private)
            })
            .transpose()?;
        self.transaction(hub, &op.context, |c| {
            let current = ledger::read(c, id)?.ok_or_else(|| anyhow!("Tentativa ausente"))?;
            ensure!(
                current.rev == op.rev && ledger::cipher(c, &op.slot())? == cipher,
                "Privado/revisão alterados durante abertura"
            );
            Ok(((), vec![]))
        })?;
        Ok((op, private))
    }
    pub(super) fn within_card(&self, id: &str) -> Result<()> {
        let active = self.active.locked();
        let active = active.as_ref().filter(|a| a.id == id).ok_or_else(|| anyhow!("Espera encerrada"))?;
        ensure!(Instant::now() < active.clock + Duration::from_secs(90), "Cartão expirado");
        Ok(())
    }
    pub(super) fn signal(&self) {
        self.changed.send_modify(|v| *v = v.wrapping_add(1));
    }
    pub(super) fn retire(&self, hub: &Hub) {
        // Serializa com before_send: sem runtime → emit e sem lock do mapa.
        if hub
            .conector_fence(&self.bot, |_| {
                self.closed.store(true, Ordering::Release);
                self.signal();
                Ok(())
            })
            .is_err()
        {
            // Sem runtime não existe admissão possível; manter fechado após exclusão.
            self.closed.store(true, Ordering::Release);
            self.signal();
        }
    }
    pub(super) fn close(&self, hub: &Hub) -> Result<()> {
        self.retire(hub);
        hub.conector_publish(|| {
            hub.conector_fence(&self.bot, |_| {
                self.closed.store(true, Ordering::Release);
                self.signal();
                let active = self.active.locked().clone();
                if let Some(active) = active {
                    let (_, entries) = ledger::change(&hub.store.db.locked(), &active.id, |_, op| {
                        if !op.state.terminal() {
                            op.state = if op.state == Estado::EmEnvio { Estado::Incerta } else { Estado::Cancelada };
                            op.completed_at = Some(crate::store::now_ms());
                        }
                        Ok(None)
                    })?;
                    Ok(((), entries))
                } else {
                    Ok(((), vec![]))
                }
            })
        })?;
        let namespace = hub.store.guard_runtime_namespace()?;
        mcp::runtime::release(&namespace, &self.source.instance, &self.id, Some(&self.id));
        Ok(())
    }
    async fn wait(&self, hub: &Hub, session: &mut Session<HubMediation>, op: &Tentativa, clock: Instant) -> Result<()> {
        let mut changes = self.changed.subscribe();
        loop {
            let state = hub.store.conector_attempt(&op.id)?.ok_or_else(|| anyhow!("Tentativa ausente"))?.state;
            if state == Estado::Autorizada {
                return self.within_card(&op.id);
            }
            ensure!(state == Estado::Aguardando, "Pedido encerrado sem envio");
            if self.closed.load(Ordering::Acquire) {
                bail!("Canal encerrado");
            }
            tokio::select! {
                ()=tokio::time::sleep_until(clock+Duration::from_secs(90)) => {
                    finish(hub,&op.id,Estado::Expirada,None)?;
                    bail!("Cartão expirado; nada enviado");
                }
                change=changes.changed() => { change.map_err(|_| anyhow!("Canal encerrado"))?; }
                event=session.next_legacy(), if session.has_legacy() => {
                    // A observação escolhida permanece viva até terminar; não há select
                    // descartando um frame/futuro idle por um clique ou ping concorrente.
                    session.idle(event?).await?;
                }
            }
        }
    }
    pub(super) async fn frame(&self, hub: &Arc<Hub>, sequence: u64, frame: Value) -> Result<Value> {
        let clock = Instant::now();
        let t0 = crate::store::now_ms();
        let method = mcp::http::method(&frame)?.to_owned();
        ensure!(
            matches!(
                method.as_str(),
                "initialize" | "notifications/initialized" | "ping" | "tools/list" | "tools/call"
            ),
            "Método fora do piloto"
        );
        let mut session = self.session.try_lock().map_err(|_| anyhow!("Um pedido já está pendente neste canal"))?;
        let context = self.capture(hub)?;
        *self.request.locked() = Some(context.clone());
        // Session exclusiva: um concorrente nunca instala contexto sobre o pedido em voo.
        let _request = RequestScope { slot: self.request.clone() };
        if method != "tools/call" {
            return match session.exchange(frame.clone()).await {
                Ok(reply) => Ok(reply.unwrap_or(Value::Null)),
                Err(mcp::http::Failure::Incompatible(error)) => {
                    // Recibo/#125 já gravados: erro técnico fixo e canal encerrado antes de outro pedido.
                    self.retire(hub);
                    Ok(json!({"jsonrpc":"2.0","id":frame["id"],"error":error.json()}))
                }
                Err(_) => Err(anyhow!("Retorno retido")),
            };
        }
        // Validar acesso/argumentos antes de target, cofre, cartão e qualquer HTTP da chamada.
        let private = self.source.request(&self.conta, &frame)?;
        ensure!(self.discovered.load(Ordering::Acquire), "Schema remoto ainda não conferido");
        let ingress = format!("{}:{sequence}", self.id);
        let previous = self.transaction(hub, &context, |c| Ok((ledger::ingress(c, &ingress)?, vec![])))?;
        if let Some(previous) = previous {
            ensure!(previous.original_digest == private.request_digest, "Ingresso repetido com corpo conflitante");
            return Ok(json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32004,
                "message":"Pedido já registrado; consulte o recibo. Não repetir.","data":previous.receipt()}}));
        }
        let mut op = Tentativa {
            id: operacao::id(),
            ingress,
            original_digest: private.request_digest.clone(),
            request_digest: private.request_digest.clone(),
            snapshot_id: operacao::id(),
            card_id: operacao::id(),
            rev: 1,
            state: Estado::Aguardando,
            source: self.source.clone(),
            context: (*context).clone(),
            t0,
            card_deadline: 0,
            global_deadline: 0,
            admitted_at: None,
            completed_at: None,
            decision: None,
            decision_digest: None,
            caller: None,
            delivery: "nao_liberada".into(),
        };
        op.card_deadline = op.t0 + 90_000;
        op.global_deadline = op.t0 + 270_000;
        let plain = Zeroizing::new(serde_json::to_string(&private)?);
        let sealed = crate::market::vault::seal(&hub.store, &op.slot(), &plain)?;
        ensure!(plain.len() <= 64 * 1024 && sealed.len() <= 64 * 1024, "Dados privados acima de 64 KiB");
        // A ativação de V1 nunca vem de JSON. Não chamar unlock/keyring em fonte ausente.
        let op = self.transaction(hub, &context, |c| {
            let (op, entries) = ledger::prepare(c, op, &sealed, |_| Ok(()))?;
            *self.active.locked() = Some(Active { id: op.id.clone(), tool: private.tool.clone(), clock });
            Ok((op, entries))
        })?;
        let weak = Arc::downgrade(hub);
        let timer_id = op.id.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep_until(clock + Duration::from_secs(90)).await;
            if let Some(hub) = weak.upgrade()
                && expire_card(&hub, &timer_id).is_err()
            {
                tracing::error!("Falha ao expirar cartão de conector");
            }
            tokio::time::sleep_until(clock + Duration::from_secs(260)).await;
            if let Some(hub) = weak.upgrade()
                && finish(&hub, &timer_id, Estado::Cancelada, None).is_err()
            {
                tracing::error!("Falha ao fechar conector no prazo global");
            }
        });
        let pending = Pending { hub: hub.clone(), id: op.id.clone(), active: self.active.clone(), timer };
        self.wait(hub, &mut session, &op, clock).await?;
        let (_, private) = self.private(hub, &op.id)?;
        let private = private.ok_or_else(|| anyhow!("Privado indisponível"))?;
        let mut changes = self.changed.subscribe();
        let exchange = session.exchange(private.frame(&frame["id"]));
        tokio::pin!(exchange);
        let result = loop {
            tokio::select! {
                result=&mut exchange => break result,
                change=changes.changed() => {
                    change.map_err(|_| anyhow!("Canal encerrado"))?;
                    ensure!(!self.closed.load(Ordering::Acquire),"Canal encerrado durante envio; confira recibo");
                }
                ()=tokio::time::sleep_until(clock+Duration::from_secs(260)) => {
                    finish(hub,&op.id,Estado::Cancelada,None)?;
                    bail!("Prazo global encerrado; confira recibo");
                }
            }
        };
        if matches!(result, Err(mcp::http::Failure::Denied)) {
            finish(hub, &op.id, Estado::Cancelada, None)?;
            drop(pending);
            return Ok(
                json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32001,"message":"Chamada MCP recusada: regra ou política indisponível"}}),
            );
        }
        if let Ok(Some(reply)) = result {
            self.transaction(hub, &context, |c| {
                ledger::change(c, &op.id, |_, op| {
                    op.delivery = "liberada".into();
                    Ok(None)
                })
            })?;
            drop(pending);
            Ok(reply)
        } else {
            finish(hub, &op.id, Estado::Cancelada, None)?;
            drop(pending);
            bail!("Retorno retido; confira o recibo antes de repetir")
        }
    }
}
struct RequestScope {
    slot: Arc<Mutex<Option<Arc<ContextoExecucao>>>>,
}
impl Drop for RequestScope {
    fn drop(&mut self) {
        *self.slot.locked() = None;
    }
}
struct Pending {
    hub: Arc<Hub>,
    id: String,
    active: Arc<Mutex<Option<Active>>>,
    timer: tokio::task::JoinHandle<()>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.timer.abort();
        // Cancelar/drop/EOF depois do CAS é incerto. Só o correlated_result fecha prova.
        if finish(&self.hub, &self.id, Estado::Cancelada, None).is_err() {
            tracing::error!("Falha ao fechar tentativa de conector; startup deve recuperar o ledger");
        }
        *self.active.locked() = None;
    }
}
pub(super) fn finish(hub: &Hub, id: &str, state: Estado, delivery: Option<&str>) -> Result<Tentativa> {
    hub.conector_publish(|| {
        ledger::change(&hub.store.db.locked(), id, |_, op| {
            if !op.state.terminal() {
                op.state =
                    if op.state == Estado::EmEnvio && !matches!(state, Estado::Concluida | Estado::FalhaComprovada) {
                        Estado::Incerta
                    } else {
                        state
                    };
                op.completed_at = Some(crate::store::now_ms());
            }
            if let Some(delivery) = delivery {
                op.delivery = delivery.into();
            }
            Ok(None)
        })
    })
}

fn expire_card(hub: &Hub, id: &str) -> Result<()> {
    hub.conector_publish(|| {
        ledger::change(&hub.store.db.locked(), id, |_, op| {
            if matches!(op.state, Estado::Aguardando | Estado::Autorizada) {
                op.state = Estado::Expirada;
                op.completed_at = Some(crate::store::now_ms());
            }
            Ok(None)
        })
    })
    .map(|_| ())
}

struct HubMediation {
    hub: Weak<Hub>,
    source: SnapshotAcesso,
    conta: ContaSaida,
    request: Arc<Mutex<Option<Arc<ContextoExecucao>>>>,
    active: Arc<Mutex<Option<Active>>>,
    closed: Arc<AtomicBool>,
    discovered: Arc<AtomicBool>,
    bridge: String,
}
impl HubMediation {
    fn validate<T>(&self, action: impl FnOnce(&Hub, &Connection, &Source) -> Result<T>) -> Result<T> {
        ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
        let context = self.request.locked().clone().ok_or_else(|| anyhow!("Contexto ausente"))?;
        self.validate_context(&context, action)
    }
    fn validate_context<T>(
        &self,
        context: &ContextoExecucao,
        action: impl FnOnce(&Hub, &Connection, &Source) -> Result<T>,
    ) -> Result<T> {
        ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
        hub.conector_fence(&context.bot, |runtime| {
            super::source(&hub, &self.source.instance, |source| {
                available(source)?;
                ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
                ensure!(source.same(&self.source, &self.conta), "Concessão alterada");
                let c = hub.store.db.locked();
                source.verify_engine(&c)?;
                context.check(&hub, runtime, &c)?;
                action(&hub, &c, source)
            })
        })
    }
    fn record_observation(
        &self,
        context: &ContextoExecucao,
        origin: &mcp::contexto::Context,
        event: &Value,
    ) -> Result<()> {
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "curated_observe_commit");
        let _boundary = hub.store.memory_boundary.locked();
        hub.conector_fence(&context.bot, |runtime| {
            super::source(&hub, &self.source.instance, |source| {
                available(source)?;
                ensure!(!self.closed.load(Ordering::Acquire), "Canal encerrado");
                ensure!(source.same(&self.source, &self.conta), "Concessão alterada");
                let mut db = hub.store.db.locked();
                let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                crate::store::Store::guard_record_on(&tx, Some(origin), event, false, |c| {
                    source.verify_engine(c)?;
                    context.check(&hub, runtime, c)?;
                    ensure!(origin.binding == context.session_binding, "Vínculo original alterado");
                    Ok(())
                })?;
                tx.commit()?;
                Ok(())
            })
        })
    }
    fn remaining(&self, cap: u64) -> Duration {
        self.active.locked().as_ref().map_or(Duration::from_secs(cap), |a| {
            (a.clock + Duration::from_secs(260)).saturating_duration_since(Instant::now()).min(Duration::from_secs(cap))
        })
    }
}
impl Mediation for HubMediation {
    fn target(&mut self, _method: &str, _stale: bool) -> impl Future<Output = Result<Target>> {
        futures::future::lazy(move |_| {
            #[cfg(test)]
            if let Some(hub) = self.hub.upgrade() {
                crate::fork::sessoes::call_tests::before_effect(&hub.store, "curated_target");
            }
            let (url, headers) = self.validate(|_, _, source| Ok((source.target.clone(), source.headers.clone())))?;
            let url = reqwest::Url::parse(&url).map_err(|_| anyhow!("Alvo indisponível"))?;
            let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
            let namespace = hub.store.guard_runtime_namespace()?;
            let mut secrets = Vec::new();
            for value in headers.values().filter_map(|v| v.to_str().ok()) {
                mcp::runtime::remote_secret(value, &mut secrets);
            }
            for (_, value) in url.query_pairs() {
                mcp::runtime::remote_secret(&value, &mut secrets);
            }
            mcp::runtime::remember(&namespace, &self.source.instance, &self.bridge, &self.bridge, &secrets);
            self.validate(|_, _, _| Ok(()))?;
            Ok(Target { url, headers, streamable_only: false })
        })
    }
    fn before_send(&mut self, frame: &Value, method: &str) -> Result<()> {
        if matches!(method, "tools/call" | "resources/read" | "prompts/get") {
            let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
            let context = self.request.locked().clone().ok_or_else(|| anyhow!("Sem contexto"))?;
            let _boundary = hub.store.memory_boundary.locked();
            match crate::fork::rules::mcp::check(
                &hub.store,
                &context.bot,
                &format!("donna-curado-{}", self.source.instance),
                method,
                &frame["params"],
            ) {
                Ok(false) => {}
                Ok(true) => return Err(mcp::http::PolicyDenied.into()),
                Err(error) => {
                    tracing::error!(%error, "política MCP indisponível; chamada recusada");
                    return Err(mcp::http::PolicyDenied.into());
                }
            }
        }
        if method != "tools/call" {
            return self.validate(|_, _, _| Ok(()));
        }
        let active = self.active.locked().clone().ok_or_else(|| anyhow!("Sem autorização nativa"))?;
        let request = self.source.request(&self.conta, frame)?;
        ensure!(self.discovered.load(Ordering::Acquire), "Schema não conferido");
        ensure!(
            Instant::now() < active.clock + Duration::from_secs(90) && self.remaining(60) > Duration::ZERO,
            "Orçamento expirado"
        );
        // Ordem emit → runtime → fonte → DB, compatível com bot_json.
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
        let context = self.request.locked().clone().ok_or_else(|| anyhow!("Sem contexto"))?;
        hub.conector_publish(|| {
            hub.conector_fence(&context.bot, |runtime| {
                super::source(&hub, &self.source.instance, |source| {
                    available(source)?;
                    ensure!(
                        !self.closed.load(Ordering::Acquire) && source.same(&self.source, &self.conta),
                        "Concessão revogada"
                    );
                    let c = hub.store.db.locked();
                    source.verify_engine(&c)?;
                    context.check(&hub, runtime, &c)?;
                    ledger::change(&c, &active.id, |_, op| {
                        ensure!(
                            op.state == Estado::Autorizada
                                && op.request_digest == request.request_digest
                                && op.context == *context,
                            "Autorização já consumida ou corpo alterado"
                        );
                        ensure!(Instant::now() < active.clock + Duration::from_secs(90), "Cartão expirado");
                        // Encaixe específico do conector: revalidar binding/revisão e
                        // consumo da prova Times/Atividades (05b) antes deste CAS.
                        // Este ledger registra efeitos; não produz HumanReviewProof.
                        op.state = Estado::EmEnvio;
                        op.admitted_at = Some(crate::store::now_ms());
                        op.delivery = "retida".into();
                        Ok(None)
                    })
                })
            })
        })
        .map(|_| ())
    }

    fn correlated_result(&mut self, frame: &Value, method: &str) -> Result<()> {
        if method != "tools/call" {
            return Ok(());
        }
        let active = self.active.locked().clone().ok_or_else(|| anyhow!("Tentativa ausente"))?;
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
        let state = self.source.classify(&active.tool, frame);
        // Evidência antes de Guard, inclusive resposta no GET antes do ACK do POST.
        // Não exige contexto ainda vivo para preservar um efeito já reportado.
        finish(&hub, &active.id, state, Some("retida"))?;
        Ok(())
    }
    fn observe(&mut self, mut frame: Value, method: &str) -> impl Future<Output = Result<Option<Value>>> {
        futures::future::lazy(move |_| {
            let context = self.request.locked().clone().ok_or_else(|| anyhow!("Contexto ausente"))?;
            self.validate_context(&context, |_, _, _| Ok(()))?;
            if method == "tools/list" {
                self.discovered.store(false, Ordering::Release);
                let tools = frame["result"]["tools"].as_array().ok_or_else(|| anyhow!("Descoberta inválida"))?;
                let mut filtered = Vec::new();
                let mut seen = std::collections::BTreeSet::new();
                for tool in tools {
                    if let Some(rule) = tool["name"].as_str().and_then(|name| self.source.rules.get(name)) {
                        ensure!(seen.insert(tool["name"].as_str()), "Descoberta com ferramenta duplicada");
                        ensure!(operacao::digest(&tool["inputSchema"]) == rule.schema_digest, "Schema remoto alterado");
                        if tool["description"].as_str() != Some(rule.descricao.as_str()) {
                            tracing::warn!(
                                ferramenta = tool["name"].as_str().unwrap_or("?"),
                                conector = %self.source.instance,
                                "descrição remota diferente da aprovada; lista de ferramentas recusada"
                            );
                            bail!("Descrição remota alterada");
                        }
                        // O modelo recebe só o que foi aprovado: título, anotações e saída do fornecedor não passam.
                        filtered.push(
                            json!({"name":tool["name"],"description":rule.descricao,"inputSchema":tool["inputSchema"]}),
                        );
                    }
                }
                ensure!(
                    filtered.len() == self.source.rules.len() && frame["result"].get("nextCursor").is_none(),
                    "Descoberta curada incompleta"
                );
                frame["result"]["tools"] = json!(filtered);
            }
            let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado"))?;
            let namespace = hub.store.guard_runtime_namespace()?;
            let secrets = mcp::runtime::snapshot(&namespace, &self.source.instance, &self.bridge, Some(&self.bridge));
            // Adaptador só de metadados para o recibo comum; NÃO chama Context::resolve
            // (o legado exige BotConfig.connectors e não é autoridade curada).
            let origin = mcp::contexto::Context {
                id: self.bridge.clone(),
                bot: context.bot.clone(),
                connector: self.source.instance.clone(),
                lane: context.lane.chat.clone(),
                run: None,
                binding: context.session_binding.clone(),
            };
            let body = json!({"frame":frame,"method":method,"connector":self.source.instance,"context":self.bridge});
            let result = mcp::observe_checked(
                mcp::Observation::Curated(&origin),
                &body,
                secrets.as_deref().map(Vec::as_slice),
                || self.validate_context(&context, |_, _, _| Ok(())),
                |event| self.record_observation(&context, &origin, event),
            )?;
            if method == "tools/list" && result["message"]["result"]["tools"].is_array() {
                self.discovered.store(true, Ordering::Release);
            }
            Ok(result.get("message").filter(|m| m.is_object()).cloned())
        })
    }
    fn post_timeout(&self) -> Duration {
        self.remaining(60)
    }
    fn exchange_timeout(&self) -> Duration {
        self.remaining(90)
    }
}

/// Ponte stdio sem alvo nem segredo remoto. O HTTP local mantém o pedido vivo
/// enquanto stdin lê cancel/EOF; a liberação é um RPC independente.
pub(crate) async fn serve(port: u16, context: String) -> Result<()> {
    let token = Zeroizing::new(crate::fork::ponte_credencial::ler_token()?);
    relay(tokio::io::stdin(), tokio::io::stdout(), port, context, token).await
}

pub(super) async fn relay(
    input: impl tokio::io::AsyncRead + Unpin,
    mut output: impl tokio::io::AsyncWrite + Unpin,
    port: u16,
    context: String,
    token: Zeroizing<String>,
) -> Result<()> {
    use crate::guard_hub::mcp::stdio::{Frame, Frames};
    use tokio::io::AsyncWriteExt;
    let client = crate::fork::bridge_http::BridgeHttp::new(port)?;
    let mut input = Frames::with_limit(input, 64 * 1024);
    let mut sequence = 0_u64;
    let mut queued: Option<Value> = None;
    let result = async {
        loop {
            let frame = if let Some(frame) = queued.take() {
                frame
            } else {
                let Some(incoming) = input.read(&[]).await? else {
                    return Ok::<(), anyhow::Error>(());
                };
                let Frame::Message(frame) = incoming else { bail!("Frame inválido"); };
                frame
            };
            sequence = sequence.checked_add(1).ok_or_else(|| anyhow!("Sequência esgotada"))?;
            let request_id = frame.get("id").cloned();
            let can_queue = mcp::http::method(&frame).is_ok_and(|method| method == "notifications/initialized");
            let request = client
                .post("donnaConectorFrame")
                .bearer_auth(token.trim())
                .json(&json!({"context":context,"sequence":sequence,"frame":frame}))
                .timeout(Duration::from_secs(270))
                .send();
            // Um único POST permanece vivo até status e corpo completos.
            let operation = async {
                let response: reqwest::Response = request.await.map_err(|_| anyhow!("Hub indisponível"))?;
                ensure!(response.status().is_success(), "Hub reteve o pedido; consulte o recibo");
                let reply: Value = response.json().await?;
                Ok::<_, anyhow::Error>(reply)
            };
            tokio::pin!(operation);
            let reply = loop {
                tokio::select! {
                    result=&mut operation => break result?,
                    incoming=input.read(&[]) => match incoming? {
                        None => return Ok(()),
                        Some(Frame::Message(frame)) if frame["method"]=="notifications/cancelled"
                            && (request_id.as_ref().is_some_and(|id| Some(id)==frame["params"].get("requestId"))
                                || queued.as_ref().and_then(|q| q.get("id")).is_some_and(|id| Some(id)==frame["params"].get("requestId"))) => return Ok(()),
                        Some(Frame::Message(frame)) if can_queue && queued.is_none()
                            && mcp::http::method(&frame).is_ok_and(|method| method=="tools/list") => {
                            // Só descoberta após notificação: nunca uma fila de efeitos.
                            queued=Some(frame);
                        }
                        Some(_) => bail!("Pedido concorrente recusado; sem fila de efeitos"),
                    }
                }
            };
            if !reply.is_null() {
                let mut bytes = serde_json::to_vec(&reply)?;
                bytes.push(b'\n');
                output.write_all(&bytes).await?;
                output.flush().await?;
            }
        }
    }
    .await;
    let _ = client
        .post("donnaConectorRelease")
        .bearer_auth(token.trim())
        .json(&json!({"context":context}))
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    result
}
