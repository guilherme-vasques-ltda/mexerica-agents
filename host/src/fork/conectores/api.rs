//! RPCs administrativos Local; o body nunca habilita fonte, caller ou apresentação.
use super::{
    State,
    operacao::{self, Estado},
    proxy,
};
use crate::{api::devices::Caller, hub::Hub, store::conectores as ledger};
use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{ops::ControlFlow, sync::Arc};
use zeroize::Zeroizing;

pub(crate) const METHODS: &[&str] = &[
    "donnaConectorTentativa",
    "donnaConectorDecidir",
    "donnaConectorEditar",
    "donnaConectorFrame",
    "donnaConectorRelease",
    "donnaConectorCapability",
    "donnaConectorRascunho",
];
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Id {
    tentativa_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Context {
    context: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    context: String,
    sequence: u64,
    frame: Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Decision {
    protocol: u8,
    tentativa_id: String,
    snapshot_id: String,
    card_id: String,
    card_rev: i64,
    request_digest: String,
    decisao: String,
    source: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Edit {
    protocol: u8,
    tentativa_id: String,
    snapshot_id: String,
    card_id: String,
    card_rev: i64,
    request_digest: String,
    source: String,
    patch: Value,
}
fn local(caller: &Caller) -> Result<()> {
    ensure!(
        matches!(caller, Caller::Local),
        "Conectores curados exigem o operador Local; Device/voz/ACS4 não concedem autoridade"
    );
    Ok(())
}
fn channel(hub: &Hub, id: &str) -> Result<Arc<proxy::Channel>> {
    let op = hub.store.conector_attempt(id)?.ok_or_else(|| anyhow!("Tentativa não encontrada"))?;
    hub.conectores.channel(&op.context.channel)
}
fn protocol(protocol: u8, source: &str) -> Result<()> {
    // O protocolo 1 não exibia a conta de saída: app antigo não aprova cartão que a exige.
    ensure!(
        protocol == operacao::DECISION_PROTOCOL && source == "app",
        "Decisão nativa exige app compatível; voz e protocolo legado recusados"
    );
    Ok(())
}
/// Nativo identificado no mapa durável, inclusive se Entry/data já sumiu.
pub(crate) fn intercept(hub: &Hub, body: &Value) -> Result<()> {
    if let Some(id) = body["entryId"].as_str() {
        ensure!(!super::whatsapp_human::is_draft(&hub.store, id)?, "Rascunho não aceita decisão ou envio");
        ensure!(
            !hub.store.conector_card(id)?,
            "Cartão de conector não aceita respondPermission; use decisão nativa no app compatível"
        );
    }
    Ok(())
}
pub(crate) async fn call(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    body: Value,
) -> Result<ControlFlow<Value, Value>> {
    if method == "respondPermission" {
        intercept(hub, &body)?;
        return Ok(ControlFlow::Continue(body));
    }
    if !METHODS.contains(&method) {
        return Ok(ControlFlow::Continue(body));
    }
    // Caller original capturado antes do único await (frame). Sem resolver ACS4.
    if !matches!(caller, Caller::Ponte { .. }) || !matches!(method, "donnaConectorFrame" | "donnaConectorRelease") {
        local(caller)?;
    }
    let result = match method {
        "donnaConectorRascunho" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct ReadDraft {
                draft_id: String,
            }
            let request: ReadDraft = serde_json::from_value(body).map_err(|_| anyhow!("Rascunho inválido"))?;
            super::whatsapp_human::read(hub, &request.draft_id).await?
        }
        "donnaConectorCapability" => {
            ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "Campos desconhecidos");
            json!({"decisionProtocol":0,"available":false,"reason":"fonte_05_e_apresentacao_06_indisponiveis",
                "catalogo":super::catalogo::publico()})
        }
        "donnaConectorTentativa" => {
            let body: Id = serde_json::from_value(body)?;
            // Recibos terminais sobrevivem ao encerramento do canal/restart. Nenhum
            // conteúdo privado é aberto sem a concessão original/contexto ainda vivos.
            let op =
                hub.store.conector_attempt(&body.tentativa_id)?.ok_or_else(|| anyhow!("Tentativa não encontrada"))?;
            if op.state.terminal() {
                op.receipt()
            } else {
                let channel = channel(hub, &body.tentativa_id)?;
                let (op, private) = channel.private(hub, &body.tentativa_id)?;
                json!({"receipt":op.receipt(),"presentation":private.map(|p| p.presentation)})
            }
        }
        "donnaConectorDecidir" => {
            let decision_digest = operacao::digest(&body);
            let body: Decision = serde_json::from_value(body)?;
            let op =
                hub.store.conector_attempt(&body.tentativa_id)?.ok_or_else(|| anyhow!("Tentativa não encontrada"))?;
            // Replay idêntico de decisão já gravada (inclusive protocolo antigo) só relê o recibo.
            if op.decision_digest.as_ref() == Some(&decision_digest) {
                op.receipt()
            } else {
                protocol(body.protocol, &body.source)?;
                ensure!(matches!(body.decisao.as_str(), "aprovar_uma_vez" | "recusar"), "Decisão inválida");
                let channel = channel(hub, &body.tentativa_id)?;
                channel.within_card(&body.tentativa_id)?;
                let op = channel.transaction(hub, &op.context, |c| {
                    ledger::change(c, &body.tentativa_id, |_, op| {
                        ensure!(
                            op.context.channel == channel.id && op.state == Estado::Aguardando,
                            "Decisão conflitante ou pedido encerrado"
                        );
                        ensure!(
                            op.snapshot_id == body.snapshot_id
                                && op.card_id == body.card_id
                                && op.rev == body.card_rev
                                && op.request_digest == body.request_digest,
                            "Cartão/revisão/snapshot alterados"
                        );
                        channel.within_card(&op.id)?;
                        // Encaixe específico do conector: validar a prova do produtor único
                        // Times/Atividades (05b) antes deste CAS. Local/snapshot não são
                        // HumanReviewProof; não emitir prova própria neste ledger.
                        op.state =
                            if body.decisao == "aprovar_uma_vez" { Estado::Autorizada } else { Estado::Recusada };
                        op.decision = Some(body.decisao.clone());
                        op.decision_digest = Some(decision_digest);
                        op.caller = Some("local".into());
                        if op.state.terminal() {
                            op.completed_at = Some(crate::store::now_ms());
                        }
                        Ok(None)
                    })
                })?;
                channel.signal();
                op.receipt()
            }
        }
        "donnaConectorEditar" => {
            let body: Edit = serde_json::from_value(body)?;
            protocol(body.protocol, &body.source)?;
            let channel = channel(hub, &body.tentativa_id)?;
            channel.within_card(&body.tentativa_id)?;
            let (previous, private) = channel.private(hub, &body.tentativa_id)?;
            ensure!(previous.state == Estado::Aguardando, "Só é possível editar enquanto aguarda decisão");
            let private = private.ok_or_else(|| anyhow!("Privado ausente"))?.edit(
                &channel.source,
                &channel.conta,
                &body.patch,
            )?;
            let plain = Zeroizing::new(serde_json::to_string(&private)?);
            let sealed = crate::market::vault::seal(&hub.store, &previous.slot(), &plain)?;
            let op = channel.transaction(hub, &previous.context, |c| {
                ledger::change(c, &body.tentativa_id, |_, op| {
                    channel.within_card(&op.id)?;
                    ensure!(
                        op.state == Estado::Aguardando
                            && op.rev == body.card_rev
                            && op.card_id == body.card_id
                            && op.snapshot_id == body.snapshot_id
                            && op.request_digest == body.request_digest,
                        "Edição concorrente ou cartão antigo"
                    );
                    op.snapshot_id = operacao::id();
                    op.card_id = operacao::id();
                    op.request_digest = private.request_digest;
                    Ok(Some(sealed))
                })
            })?;
            channel.signal();
            op.receipt()
        }
        "donnaConectorFrame" => {
            let body: Envelope = serde_json::from_value(body)?;
            ensure!(body.sequence > 0, "Sequência inválida");
            // Handle ausente não vira grant, mesmo com source/protocol forjados.
            let channel = hub.conectores.channel(&body.context)?;
            channel.frame(hub, body.sequence, body.frame).await?
        }
        "donnaConectorRelease" => {
            let body: Context = serde_json::from_value(body)?;
            hub.conectores.release(hub, &body.context)?;
            json!({"released":true})
        }
        _ => bail!("Método desconhecido"),
    };
    Ok(ControlFlow::Break(result))
}

impl State {
    pub(super) fn release(&self, hub: &Hub, id: &str) -> Result<()> {
        use crate::LockExt;
        let channel = self.channels.locked().get(id).cloned();
        if let Some(channel) = channel {
            channel.retire(hub);
            self.channels.locked().remove(id);
            // Sem guard do mapa durante publicação/runtime/DB.
            channel.close(hub)?;
        }
        Ok(())
    }
}
