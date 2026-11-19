//! R2a: o bot solicitante consulta o estado registrado do próprio pedido, na mesma conversa.
//! Só leitura de metadados do R1: nenhum texto, ID de conteúdo/aviso, digest, proveniência,
//! binding, token ou alvo sai daqui, e `entries` não é lida. Não grava, não enfileira, não
//! acorda ator, não abre turno nem reenvia. Identidade só de Sessões; o modelo dá só o ID.
use super::{Decisao, Desfecho, MotivoFim, VERSAO};
use crate::LockExt;
use crate::fork::sessoes::{ExecutionRef, bindings, store::SessionRef};
use crate::fork::spaces::PERSONAL;
use crate::hub::Hub;
use anyhow::{Result, anyhow, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

/// Nome da ferramenta no MCP `team`.
pub(crate) const FERRAMENTA: &str = "bot_request_status";
/// Pedido de outra conversa, de outro bot ou inexistente: a mesma recusa, sem revelar existência.
pub(crate) const INDISPONIVEL: &str =
    "retorno_pedido_indisponivel: nenhum pedido seu com esse requestId nesta conversa";
/// Sem execução atual, conversa de Space, grupo ou fora do Pessoal.
pub(crate) const FORA_DO_ALCANCE: &str =
    "retorno_consulta_fora_do_alcance: só na conversa própria do bot no Pessoal, durante a execução atual";
const ARGUMENTO: &str =
    "retorno_argumento_invalido: informe somente requestId, como devolvido por ask_bot ou message_bot";
/// Versão/estado desconhecido ou incoerente: falha fechada, sem fallback ao histórico.
const INVALIDO: &str = "retorno_registro_invalido: o registro deste pedido não pôde ser lido com segurança";
const VENCIDA: &str = "chamada vencida: a execução atual mudou durante a consulta";

// Notas fixas do Hub por estado; nunca dados do pedido.
const N_ACEITO: &str = "Aceito e ainda não enviado ao destinatário; não indica que o trabalho começou.";
const N_EM_EXECUCAO: &str = "O envio ao destinatário é possível e não há conclusão registrada. Não prova que o bot está trabalhando nem que algo foi feito; não repita por isso.";
const N_CONCLUIU: &str = "O turno do destinatário terminou normalmente. Isso não confirma entrega, leitura nem que o combinado foi cumprido, e esta consulta não traz a resposta.";
const N_SEM_SUCESSO: &str = "O turno do destinatário terminou sem sucesso (falha, parada ou incompleto). Trabalho parcial pode ter ocorrido; confira antes de repetir.";
const N_ANTES_DO_ENVIO: &str =
    "Encerrado antes do envio ao destinatário; não há registro de envio. Repetir é um pedido novo.";
const N_REINICIO_ANTES: &str =
    "Interrompido por reinício do Hub antes do envio; não há registro de envio. Nada é reenviado automaticamente.";
const N_REINICIO_INCERTO: &str = "Interrompido por reinício do Hub depois de um envio possível: o resultado é incerto e não será concluído por este registro. Confira a conversa do destinatário antes de repetir.";
const N_DESLIGADO: &str =
    "Interrompido porque o ator do destinatário foi desligado; o envio pode ter ocorrido. Confira antes de repetir.";
const N_CANCELADO: &str =
    "Cancelado ou encerrado sem conclusão registrada; trabalho parcial pode ter ocorrido. Confira antes de repetir.";

/// Motivo de um pedido encerrado sem conclusão do motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decidido {
    Decisao(Decisao),
    /// Reinício com o pedido ainda `accepted`: não há registro de envio.
    ReinicioAntesDoEnvio,
    /// Reinício com o pedido `running`: envio possível, resultado desconhecido.
    ReinicioIncerto,
}
impl Decidido {
    fn de(sql: &str) -> Result<Self> {
        Ok(match sql {
            "cancelled" => Self::Decisao(Decisao::Cancelado),
            "timeout" => Self::Decisao(Decisao::Prazo),
            "recipient_stopped" => Self::Decisao(Decisao::DestinatarioParou),
            "not_started" => Self::Decisao(Decisao::NaoIniciado),
            "host_stopped" => Self::Decisao(Decisao::HostParou),
            "start_failed" => Self::Decisao(Decisao::FalhaInicio),
            "restart_before_start" => Self::ReinicioAntesDoEnvio,
            "restart_uncertain" => Self::ReinicioIncerto,
            _ => bail!(INVALIDO),
        })
    }
    pub(crate) fn sql(self) -> &'static str {
        match self {
            Self::Decisao(d) => d.sql(),
            Self::ReinicioAntesDoEnvio => "restart_before_start",
            Self::ReinicioIncerto => "restart_uncertain",
        }
    }
    pub(crate) fn estado(self) -> &'static str {
        match self {
            Self::Decisao(d) => d.estado(),
            Self::ReinicioAntesDoEnvio | Self::ReinicioIncerto => "interrupted",
        }
    }
    fn nota(self) -> &'static str {
        match self {
            Self::Decisao(Decisao::NaoIniciado | Decisao::FalhaInicio) => N_ANTES_DO_ENVIO,
            Self::ReinicioAntesDoEnvio => N_REINICIO_ANTES,
            Self::ReinicioIncerto => N_REINICIO_INCERTO,
            Self::Decisao(Decisao::HostParou) => N_DESLIGADO,
            Self::Decisao(Decisao::Cancelado | Decisao::Prazo | Decisao::DestinatarioParou) => N_CANCELADO,
        }
    }
}

fn desfecho(sql: &str) -> Result<Desfecho> {
    Ok(match sql {
        "completed" => Desfecho::Concluiu,
        "failed" => Desfecho::Falhou,
        "stopped" => Desfecho::Parou,
        "incomplete" => Desfecho::Incompleto,
        _ => bail!(INVALIDO),
    })
}

fn motivo_fim(sql: &str) -> Result<MotivoFim> {
    Ok(match sql {
        "end_turn" => MotivoFim::FimDoTurno,
        "engine_error" => MotivoFim::ErroDoMotor,
        "cancelled" => MotivoFim::Cancelado,
        "max_tokens" => MotivoFim::LimiteDeSaida,
        "max_turn_requests" => MotivoFim::LimiteDePassos,
        "refusal" => MotivoFim::Recusa,
        "subscription_limit" => MotivoFim::LimiteDaAssinatura,
        "start_failed" => MotivoFim::FalhaInicio,
        "other" => MotivoFim::Outro,
        _ => bail!(INVALIDO),
    })
}

/// Estado tipado do pedido, como registrado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Estado {
    /// Prova só que o envio ainda não começou.
    Aceito,
    /// Envio possível, sem conclusão registrada; não prova bot ocupado nem efeito.
    EmExecucao,
    /// Fim do turno do destinatário; não é entrega, consumo ou cumprimento do combinado.
    Encerrado(Desfecho, MotivoFim),
    Decidido(Decidido),
}

impl Estado {
    fn json(self, pedido: &str) -> Value {
        let (state, reason, outcome, nota) = match self {
            Self::Aceito => ("accepted", None, Value::Null, N_ACEITO),
            Self::EmExecucao => ("running", None, Value::Null, N_EM_EXECUCAO),
            Self::Encerrado(d, m) => {
                let nota = if d == Desfecho::Concluiu { N_CONCLUIU } else { N_SEM_SUCESSO };
                ("finished", None, json!({"result": d.sql(), "reason": m.sql()}), nota)
            }
            Self::Decidido(m) => (m.estado(), Some(m.sql()), Value::Null, m.nota()),
        };
        json!({"requestId": pedido, "state": state, "reason": reason, "outcome": outcome, "note": nota})
    }
}

/// Colunas lidas: nada de conteúdo, avisos, digest, proveniência ou alvo.
pub(super) struct Linha {
    pub versao: i64,
    pub kind: String,
    pub state: String,
    pub end_reason: Option<String>,
    pub attempt: i64,
    pub resultados: i64,
    pub res_versao: Option<i64>,
    pub outcome: Option<String>,
    pub res_motivo: Option<String>,
}

/// Leitor único dos estados do R1 (R2a e pendentes): combinação desconhecida falha fechada.
pub(super) fn estado(l: &Linha) -> Result<Estado> {
    ensure!(l.versao == VERSAO && l.kind == "inter_bot" && l.attempt == 1, INVALIDO);
    let resultado = match (l.res_versao, l.outcome.as_deref(), l.res_motivo.as_deref()) {
        (None, None, None) => None,
        (Some(VERSAO), Some(d), Some(m)) => Some((desfecho(d)?, motivo_fim(m)?)),
        _ => bail!(INVALIDO),
    };
    ensure!(l.resultados == i64::from(resultado.is_some()), INVALIDO);
    Ok(match (l.state.as_str(), l.end_reason.as_deref(), resultado) {
        ("accepted", None, None) => Estado::Aceito,
        ("running", None, None) => Estado::EmExecucao,
        ("finished", None, Some((d, m))) => Estado::Encerrado(d, m),
        (state @ ("cancelled" | "interrupted"), Some(motivo), None) => {
            let motivo = Decidido::de(motivo)?;
            ensure!(motivo.estado() == state, INVALIDO);
            Estado::Decidido(motivo)
        }
        _ => bail!(INVALIDO),
    })
}

/// Valida o único argumento: `requestId` no formato que o Hub emite (UUID minúsculo com hífens).
fn pedido(args: &Value) -> Result<&str> {
    let campos = args.as_object().ok_or_else(|| anyhow!(ARGUMENTO))?;
    ensure!(campos.len() == 1, ARGUMENTO);
    let id = campos.get("requestId").and_then(Value::as_str).ok_or_else(|| anyhow!(ARGUMENTO))?;
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| anyhow!(ARGUMENTO))?;
    ensure!(uuid.hyphenated().to_string() == id, ARGUMENTO);
    Ok(id)
}

/// Na Tx da leitura, só com a Connection recebida: identidade atual por Sessões e filtro
/// exato do solicitante no SQL antes de projetar qualquer coluna.
fn ler_na(c: &Connection, chamador: &str, execucao: &ExecutionRef, id: &str) -> Result<(SessionRef, Estado)> {
    let origem = bindings::assert_execution_live(c, execucao)?;
    let binding = execucao.acp_binding().ok_or_else(|| anyhow!(FORA_DO_ALCANCE))?;
    ensure!(bindings::resolve(c, binding, chamador)?.session_ref == origem, FORA_DO_ALCANCE);
    // Conversa própria do bot (principal ou adicional), nunca grupo ou conversa de Space.
    ensure!(origem.chat_id == chamador, FORA_DO_ALCANCE);
    let pessoal: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1 AND deleted=0
           AND COALESCE(json_extract(config,'$.kind'),'agent')='agent'
           AND COALESCE(json_extract(config,'$.spaceId'),?2)=?2)",
        params![chamador, PERSONAL],
        |r| r.get(0),
    )?;
    ensure!(pessoal, FORA_DO_ALCANCE);
    // Tipo de coluna inesperado também falha fechado, sem ecoar valores.
    let linha = c
        .query_row(
            "SELECT r.schema_version,r.kind,r.state,r.end_reason,r.attempt,
               (SELECT COUNT(*) FROM donna_session_results x WHERE x.request_id=r.id),
               s.schema_version,s.outcome,s.end_reason
             FROM donna_session_requests r
             LEFT JOIN donna_session_results s ON s.request_id=r.id AND s.attempt=r.attempt
             WHERE r.id=?1 AND r.requester_bot_id=?2 AND r.requester_chat_id=?3 AND r.requester_session_id=?4",
            params![id, chamador, origem.chat_id, origem.id],
            |r| {
                Ok(Linha {
                    versao: r.get(0)?,
                    kind: r.get(1)?,
                    state: r.get(2)?,
                    end_reason: r.get(3)?,
                    attempt: r.get(4)?,
                    resultados: r.get(5)?,
                    res_versao: r.get(6)?,
                    outcome: r.get(7)?,
                    res_motivo: r.get(8)?,
                })
            },
        )
        .optional()
        .map_err(|_| anyhow!(INVALIDO))?
        .ok_or_else(|| anyhow!(INDISPONIVEL))?;
    Ok((origem, estado(&linha)?))
}

/// Leitura numa Tx descartada (nada é gravado); revalidação canônica fora do lock, antes de
/// responder: execução viva, runtime e token do turno atuais, mesma sessão da leitura.
pub(crate) fn consultar(hub: &Hub, chamador: &str, args: &Value, execucao: Option<&ExecutionRef>) -> Result<Value> {
    let id = pedido(args)?;
    let execucao = execucao.ok_or_else(|| anyhow!(FORA_DO_ALCANCE))?;
    let (origem, estado) = {
        let mut c = hub.store.db.locked();
        let tx = c.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let lido = ler_na(&tx, chamador, execucao, id);
        drop(tx);
        lido?
    };
    ensure!(bindings::recheck(hub, execucao)? == origem, VENCIDA);
    let binding = execucao.acp_binding().ok_or_else(|| anyhow!(FORA_DO_ALCANCE))?;
    ensure!(bindings::capture(hub, chamador, binding)?.turn_token() == execucao.turn_token(), VENCIDA);
    Ok(estado.json(id))
}
