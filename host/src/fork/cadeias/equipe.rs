//! R0: D220 nos pedidos entre bots (`ask_bot`/`message_bot`). Roda dentro da Tx da
//! admissão Team com a Connection recebida; sem Store, Cloud, await ou ID aleatório.
use super::{
    admissao,
    modelo::{Escopo, Origem, Pedido, Recibo, Recusa, Tipo},
    store,
};
use crate::fork::sessoes::{ExecutionRef, bindings, store::SessionRef};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};

/// Preparada fora da Tx: IDs candidatos e digest da intenção. O recibo anterior vem
/// do ator (repasse herdado ou Run de Rotina), nunca de argumento do modelo.
pub(crate) struct Entrada {
    de: String,
    para: String,
    pedido: String,
    /// `ask_bot` espera a resposta; o registro R1 guarda o modo, não o texto.
    espera: bool,
    intencao: String,
    anterior: Option<Recibo>,
    cadeia: String,
    raiz: String,
    repasse: String,
}

impl Entrada {
    pub(crate) fn nova(
        de: &str,
        para: &str,
        pedido: &str,
        ferramenta: &str,
        mensagem: &str,
        contrato: Option<&serde_json::Value>,
        anterior: Option<Recibo>,
    ) -> Result<Self> {
        let id = || uuid::Uuid::new_v4().to_string();
        Ok(Self {
            de: de.into(),
            para: para.into(),
            pedido: pedido.into(),
            espera: ferramenta == "ask_bot",
            // T2a: o contrato entra estruturado; sem ele, a tupla e o digest de antes.
            intencao: match contrato {
                None => admissao::digest(&("team", ferramenta, de, para, mensagem))?,
                Some(contrato) => admissao::digest(&("team", ferramenta, de, para, mensagem, contrato))?,
            },
            anterior,
            cadeia: id(),
            raiz: id(),
            repasse: id(),
        })
    }
    pub(crate) fn de(&self) -> &str {
        &self.de
    }
    pub(crate) fn para(&self) -> &str {
        &self.para
    }
    pub(crate) fn pedido(&self) -> &str {
        &self.pedido
    }
    pub(crate) fn espera(&self) -> bool {
        self.espera
    }
}

/// Recusa D220 de um pedido entre bots. Nada foi gravado, publicado ou enfileirado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecusaEquipe(pub Recusa);

impl std::fmt::Display for RecusaEquipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Recusa::Nivel => {
                "Limite de 5 níveis desta cadeia de pedidos entre bots atingido. Nada foi enviado. Conclua com o que já tem ou peça ao usuário para iniciar um novo pedido."
            }
            Recusa::Execucoes => {
                "Limite de 50 execuções desta cadeia de pedidos entre bots atingido. Nada foi enviado. Conclua com o que já tem ou peça ao usuário para iniciar um novo pedido."
            }
            Recusa::Conflito | Recusa::ReciboInvalido | Recusa::EnvioPossivel => {
                "Não foi possível conferir a origem deste pedido entre bots. Nada foi enviado."
            }
        })
    }
}
impl std::error::Error for RecusaEquipe {}

/// Dentro da Tx IMMEDIATE da admissão Team, entre as checagens de origem.
/// `Ok(None)`: produtor interno legado, sem execução nem cadeia herdada; segue só
/// com os saltos em memória, como antes.
pub(crate) fn admitir_na(
    c: &Connection,
    e: &Entrada,
    space_id: Option<&str>,
    execution: Option<&ExecutionRef>,
) -> Result<Option<Recibo>> {
    let anterior = match (&e.anterior, execution) {
        (Some(r), _) => r.clone(),
        (None, Some(execution)) => raiz(c, e, space_id, execution)?,
        (None, None) => return Ok(None),
    };
    Ok(Some(admitir_filho(c, e, space_id, anterior)?))
}

pub(crate) fn admitir_open_na(
    c: &Connection,
    e: &Entrada,
    space: &str,
    session: &SessionRef,
    reference: &str,
) -> Result<Recibo> {
    ensure!(e.anterior.is_none() && e.espera, "retorno_origem_invalida");
    let escopo = Escopo::TurnoHubOpen {
        bot_id: e.de.clone(),
        space_id: space.into(),
        chat_id: session.chat_id.clone(),
        session_id: session.id.clone(),
    };
    let anterior = admitir_raiz(c, e, reference.to_owned(), escopo)?;
    admitir_filho(c, e, Some(space), anterior)
}

fn admitir_filho(c: &Connection, e: &Entrada, space_id: Option<&str>, anterior: Recibo) -> Result<Recibo> {
    // O repasse conserva a fonte real da raiz (Rotina agendada/manual ou turno interno).
    let sql = "SELECT source FROM donna_chain_roots WHERE id=?1 AND version=1";
    let source: Option<String> = c.query_row(sql, [&anterior.chain_id], |row| row.get(0)).optional()?;
    let origem = source.as_deref().and_then(Origem::from_sql).ok_or(RecusaEquipe(Recusa::ReciboInvalido))?;
    let pedido = Pedido {
        tipo: Tipo::Delegacao,
        evento: format!("team:{}", e.pedido),
        intencao: e.intencao.clone(),
        anterior: Some(anterior),
        candidato: Recibo {
            chain_id: e.cadeia.clone(),
            admission_id: e.repasse.clone(),
            run_id: e.pedido.clone(),
            bot_id: e.para.clone(),
            level: 1,
            ordinal: 1,
        },
        rotina: None,
        origem,
        escopo: Escopo::PedidoEntreBots {
            bot_id: e.para.clone(),
            space_id: space_id.map(Into::into),
            de: e.de.clone(),
        },
    };
    Ok(store::admit(c, &pedido)?.map_err(RecusaEquipe)?)
}

/// Uma raiz por execução ACP: chamadas irmãs do mesmo turno reencontram o mesmo
/// recibo pelo replay idempotente. Token de turno só entra como digest.
fn raiz(c: &Connection, e: &Entrada, space_id: Option<&str>, execution: &ExecutionRef) -> Result<Recibo> {
    let session = bindings::assert_execution_live(c, execution)?;
    let binding = execution.acp_binding().ok_or_else(|| anyhow!("chamada vencida"))?;
    let ocorrencia = format!("team-turn:{}", admissao::digest(&("team-turn", binding, execution.turn_token()))?);
    let escopo = Escopo::TurnoMcp {
        bot_id: e.de.clone(),
        space_id: space_id.map(Into::into),
        chat_id: session.chat_id,
        session_id: session.id,
    };
    admitir_raiz(c, e, ocorrencia, escopo)
}

fn admitir_raiz(c: &Connection, e: &Entrada, ocorrencia: String, escopo: Escopo) -> Result<Recibo> {
    let pedido = Pedido {
        tipo: Tipo::Raiz,
        evento: ocorrencia.clone(),
        intencao: admissao::digest(&("team-turn", &e.de, &escopo))?,
        anterior: None,
        candidato: Recibo {
            chain_id: e.cadeia.clone(),
            admission_id: e.raiz.clone(),
            run_id: ocorrencia,
            bot_id: e.de.clone(),
            level: 1,
            ordinal: 1,
        },
        rotina: None,
        origem: Origem::Interna,
        escopo,
    };
    Ok(store::admit(c, &pedido)?.map_err(RecusaEquipe)?)
}

pub(crate) fn despachar_open_na(c: &Connection, r: &Recibo) -> Result<()> {
    ensure!(store::receipt(c, &r.admission_id)?.as_ref() == Some(r), Recusa::ReciboInvalido);
    let changed = c.execute(
        "UPDATE donna_chain_admissions AS a SET phase='sending'
         WHERE a.id=?1 AND a.phase='reserved' AND a.kind='delegation' AND a.routine_id IS NULL
           AND json_extract(a.scope,'$.kind')='pedido_entre_bots'
           AND EXISTS(SELECT 1 FROM donna_chain_roots root JOIN donna_session_requests req ON req.chain_id=root.id
               WHERE root.id=a.chain_id AND root.version=1 AND root.source='internal'
                 AND json_extract(root.scope,'$.kind')='turno_hub_open'
                 AND req.schema_version=1 AND req.state='accepted' AND req.attempt=1 AND req.fence=1
                 AND req.requester_execution_ref=root.run_id AND req.requester_bot_id=root.bot_id
                 AND req.requester_bot_id=json_extract(a.scope,'$.de')
                 AND req.id=a.run_id AND req.admission_id=a.id AND req.target_bot_id=a.bot_id)",
        [&r.admission_id],
    )?;
    ensure!(changed == 1, Recusa::EnvioPossivel);
    Ok(())
}

pub(crate) fn recuperar_open_na(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE donna_chain_admissions AS a SET phase='finished'
         WHERE a.phase IN ('reserved','sending') AND a.kind='delegation'
           AND EXISTS(SELECT 1 FROM donna_chain_roots root JOIN donna_session_requests req ON req.chain_id=root.id
               WHERE root.id=a.chain_id AND root.version=1
                 AND json_extract(root.scope,'$.kind')='turno_hub_open'
                 AND req.schema_version=1 AND req.id=a.run_id AND req.admission_id=a.id
                 AND req.target_bot_id=a.bot_id AND req.state='interrupted'
                 AND req.end_reason IN ('restart_before_start','restart_uncertain'))",
        [],
    )?;
    Ok(())
}

/// Predecessor aceito no ramo MCP: recibo coerente de Rotina/raiz (consulta original
/// do produtor) ou repasse MCP exato. Sempre do próprio remetente; nunca de Rotina nova.
pub(super) fn predecessor_valido(c: &Connection, r: &Recibo, p: &Pedido) -> Result<bool> {
    let Escopo::PedidoEntreBots { bot_id, de, .. } = &p.escopo else { return Ok(false) };
    if p.tipo != Tipo::Delegacao
        || r.bot_id != *de
        || de == bot_id
        || p.candidato.bot_id != *bot_id
        || p.rotina.is_some()
    {
        return Ok(false);
    }
    if let Some(context) = store::contexto(c, r)? {
        return Ok(!matches!(context.escopo, Escopo::TurnoHubOpen { .. })
            || (p.origem == Origem::Interna && r.run_id.starts_with("hub-open:")));
    }
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_chain_admissions a
         JOIN donna_chain_roots root ON root.id=a.chain_id AND root.version=1
         WHERE a.id=?1 AND a.chain_id=?2 AND a.run_id=?3 AND a.bot_id=?4 AND a.level=?5 AND a.ordinal=?6
           AND a.kind='delegation' AND a.routine_id IS NULL
           AND json_extract(root.scope,'$.kind')<>'turno_hub_open'
           AND json_extract(a.scope,'$.kind')='pedido_entre_bots' AND json_extract(a.scope,'$.bot_id')=a.bot_id)",
        params![r.admission_id, r.chain_id, r.run_id, r.bot_id, r.level, r.ordinal],
        |row| row.get(0),
    )?)
}
