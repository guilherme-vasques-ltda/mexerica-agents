//! R1 de Retornos: registro durável dos pedidos entre bots e das conclusões reais do ator.
//! Só metadados e referências a conteúdo já persistido; nenhum texto de pedido, resposta,
//! título ou erro. Não publica, não consome, não reenvia e não abre turno.
//! Funções `*_na` usam somente a `Connection` recebida; sem Store, Cloud, await ou ID aleatório.
//! Os adaptadores sem sufixo abrem a própria Tx IMMEDIATE e nunca são chamados com o DB travado.
use crate::LockExt;
use crate::fork::cadeias::{admissao::digest, modelo::Recibo};
use crate::fork::sessoes::{
    ExecutionRef, bindings,
    store::{self as sessoes, SessionRef},
};
use crate::store::{Store, now_ms};
use anyhow::{Result, anyhow, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// R2a: consulta só leitura do estado do próprio pedido pelo solicitante.
pub(crate) mod consulta;
pub(crate) mod entrega;
/// Leitura canônica dos pedidos R1 (Mission Board, VG-03b).
pub(crate) mod pendentes;
pub(crate) mod privacidade;

/// Versão do contrato comum gravada em cada linha. Leitores recusam outra versão.
pub(crate) const VERSAO: i64 = 1;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_session_requests(
           id TEXT PRIMARY KEY, schema_version INTEGER NOT NULL CHECK(schema_version=1),
           kind TEXT NOT NULL CHECK(kind='inter_bot'), mode TEXT NOT NULL CHECK(mode IN ('wait','async')),
           requester_bot_id TEXT NOT NULL, requester_chat_id TEXT NOT NULL, requester_session_id TEXT NOT NULL,
           requester_execution_ref TEXT NOT NULL,
           target_bot_id TEXT NOT NULL, target_chat_id TEXT NOT NULL, target_session_id TEXT NOT NULL,
           chain_id TEXT NOT NULL, admission_id TEXT NOT NULL,
           origin_entry_id TEXT NOT NULL, target_entry_id TEXT NOT NULL,
           attempt INTEGER NOT NULL CHECK(attempt=1), fence INTEGER NOT NULL CHECK(fence>=1),
           state TEXT NOT NULL CHECK(state IN ('accepted','running','finished','cancelled','interrupted')),
           end_reason TEXT CHECK(end_reason IN ('cancelled','timeout','recipient_stopped','not_started',
             'host_stopped','start_failed','restart_before_start','restart_uncertain')),
           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
           CHECK((state IN ('cancelled','interrupted')) = (end_reason IS NOT NULL)));
         CREATE INDEX IF NOT EXISTS donna_session_requests_open
           ON donna_session_requests(state) WHERE state IN ('accepted','running');
         CREATE TABLE IF NOT EXISTS donna_session_results(
           id TEXT PRIMARY KEY, request_id TEXT NOT NULL REFERENCES donna_session_requests(id),
           schema_version INTEGER NOT NULL CHECK(schema_version=1),
           attempt INTEGER NOT NULL, fence INTEGER NOT NULL, producer_execution_ref TEXT,
           outcome TEXT NOT NULL CHECK(outcome IN ('completed','failed','stopped','incomplete')),
           end_reason TEXT NOT NULL CHECK(end_reason IN ('end_turn','engine_error','cancelled','max_tokens',
             'max_turn_requests','refusal','subscription_limit','start_failed','other')),
           content TEXT NOT NULL CHECK(content IN ('final_entry','unreferenced','none','unverified')),
           final_entry_id TEXT, digest TEXT NOT NULL CHECK(length(digest)=64), created_at INTEGER NOT NULL,
           UNIQUE(request_id, attempt),
           CHECK((content='final_entry') = (final_entry_id IS NOT NULL)),
           CHECK(content<>'final_entry' OR (outcome='completed' AND producer_execution_ref IS NOT NULL)));",
    )?;
    privacidade::migrate_na(&tx)?;
    entrega::migrate(&tx)?;
    tx.commit()?;
    Ok(())
}

/// `ask_bot` espera a resposta; `message_bot` segue sem o solicitante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modo {
    Espera,
    Assincrono,
}
impl Modo {
    fn sql(self) -> &'static str {
        match self {
            Self::Espera => "wait",
            Self::Assincrono => "async",
        }
    }
}

/// Encerramento sem conclusão do motor. Decide só pedidos `accepted`/`running`. Os motivos
/// de reinício são gravados só por [`recuperar_na`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decisao {
    /// Parada do solicitante, remoção de um dos bots ou recusa do dono na bandeja (MX-77).
    Cancelado,
    Prazo,
    /// O canal do destinatário fechou sem concluir.
    DestinatarioParou,
    NaoIniciado,
    HostParou,
    FalhaInicio,
}
impl Decisao {
    fn sql(self) -> &'static str {
        match self {
            Self::Cancelado => "cancelled",
            Self::Prazo => "timeout",
            Self::DestinatarioParou => "recipient_stopped",
            Self::NaoIniciado => "not_started",
            Self::HostParou => "host_stopped",
            Self::FalhaInicio => "start_failed",
        }
    }
    fn estado(self) -> &'static str {
        match self {
            Self::Cancelado | Self::Prazo | Self::DestinatarioParou | Self::NaoIniciado => "cancelled",
            Self::HostParou | Self::FalhaInicio => "interrupted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Desfecho {
    Concluiu,
    Falhou,
    Parou,
    Incompleto,
}
impl Desfecho {
    fn sql(self) -> &'static str {
        match self {
            Self::Concluiu => "completed",
            Self::Falhou => "failed",
            Self::Parou => "stopped",
            Self::Incompleto => "incomplete",
        }
    }
}

/// Motivo terminal tipado; o texto de erro do motor nunca é guardado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MotivoFim {
    FimDoTurno,
    ErroDoMotor,
    Cancelado,
    LimiteDeSaida,
    LimiteDePassos,
    Recusa,
    LimiteDaAssinatura,
    FalhaInicio,
    Outro,
}
impl MotivoFim {
    /// `stopReason` da resposta final do `session/prompt`.
    pub(crate) fn do_motor(stop_reason: &str) -> Self {
        match stop_reason {
            "end_turn" => Self::FimDoTurno,
            "cancelled" => Self::Cancelado,
            "max_tokens" => Self::LimiteDeSaida,
            "max_turn_requests" => Self::LimiteDePassos,
            "refusal" => Self::Recusa,
            _ => Self::Outro,
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::FimDoTurno => "end_turn",
            Self::ErroDoMotor => "engine_error",
            Self::Cancelado => "cancelled",
            Self::LimiteDeSaida => "max_tokens",
            Self::LimiteDePassos => "max_turn_requests",
            Self::Recusa => "refusal",
            Self::LimiteDaAssinatura => "subscription_limit",
            Self::FalhaInicio => "start_failed",
            Self::Outro => "other",
        }
    }
}

/// Onde está o resultado. `Entrada` é uma referência, nunca uma concessão de leitura.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Conteudo {
    /// Entrada `agent` já persistida na sessão destinatária.
    Entrada(String),
    /// Houve texto, mas sem uma entrada única (por exemplo `send_message`).
    SemReferencia,
    Nenhum,
}

/// Execução ACP do destinatário capturada antes de limpar o runtime.
#[derive(Clone, Debug)]
pub(crate) struct Produtor {
    acp_session: String,
    turn_token: String,
}
impl Produtor {
    /// Sessão ACP e token do turno ainda presentes no runtime; sem os dois, sem proveniência.
    pub(crate) fn de(acp_session: Option<String>, turn_token: Option<String>) -> Option<Self> {
        Some(Self { acp_session: acp_session?, turn_token: turn_token? })
    }
}

/// Pedido capturado na Tx da admissão Team.
pub(crate) struct NovoPedido<'a, E = &'a ExecutionRef> {
    pub id: &'a str,
    pub modo: Modo,
    pub de: &'a str,
    pub para: &'a str,
    pub execucao: E,
    pub recibo: &'a Recibo,
    pub aviso_origem: &'a str,
    pub aviso_destino: &'a str,
}

impl<'a, E> NovoPedido<'a, E> {
    pub(crate) fn com_execucao<T>(self, execucao: T) -> NovoPedido<'a, T> {
        NovoPedido {
            id: self.id,
            modo: self.modo,
            de: self.de,
            para: self.para,
            execucao,
            recibo: self.recibo,
            aviso_origem: self.aviso_origem,
            aviso_destino: self.aviso_destino,
        }
    }
}

/// Como o turno do destinatário terminou, a partir da resposta final do `session/prompt`.
pub(crate) struct Encerramento {
    pub desfecho: Desfecho,
    pub motivo: MotivoFim,
    pub conteudo: Conteudo,
    pub produtor: Option<Produtor>,
}

impl Encerramento {
    fn falha_inicio() -> Self {
        Self { desfecho: Desfecho::Falhou, motivo: MotivoFim::FalhaInicio, conteudo: Conteudo::Nenhum, produtor: None }
    }
}

/// Término conferido contra tentativa e cerca atuais.
pub(crate) struct Termino {
    pub pedido: String,
    pub bot: String,
    pub attempt: i64,
    pub fence: i64,
    pub fim: Encerramento,
}

pub(crate) fn referencia(binding: &str, token: &str) -> Result<String> {
    ensure!(!token.is_empty(), "retorno_execucao_invalida");
    Ok(format!("acp:{binding}:{}", digest(&("retornos-turno", token))?))
}

/// Grava o pedido na Tx da admissão. Fonte e alvo vêm só de Sessões; a principal do
/// destinatário é o alvo do percurso atual. Falha desfaz a admissão inteira.
pub(crate) fn aceitar_na(c: &Connection, p: &NovoPedido<'_>) -> Result<()> {
    let origem = bindings::assert_execution_live(c, p.execucao)?;
    let binding = p.execucao.acp_binding().ok_or_else(|| anyhow!("chamada vencida"))?;
    ensure!(bindings::resolve(c, binding, p.de)?.session_ref == origem, "retorno_origem_invalida");
    aceitar_validado_na(c, p, &origem, &referencia(binding, p.execucao.turn_token())?)
}

pub(crate) fn aceitar_open_na(c: &Connection, p: &NovoPedido<'_, (&SessionRef, &str)>) -> Result<()> {
    let (origem, referencia) = p.execucao;
    ensure!(
        origem.id == p.de && origem.chat_id == p.de && referencia.starts_with("hub-open:"),
        "retorno_origem_invalida"
    );
    aceitar_validado_na(c, p, origem, referencia)
}

fn aceitar_validado_na<E>(c: &Connection, p: &NovoPedido<'_, E>, origem: &SessionRef, reference: &str) -> Result<()> {
    let alvo = sessoes::get(c, p.para)?
        .filter(|s| s.principal && s.chat_id == p.para && s.state == "open" && s.runs_on.is_none())
        .map(|s| SessionRef { id: s.id, chat_id: s.chat_id })
        .ok_or_else(|| anyhow!("retorno_destino_indisponivel"))?;
    ensure!(&sessoes::session_ref_of_entry(c, &origem.chat_id, p.aviso_origem)? == origem, "retorno_aviso_invalido");
    ensure!(sessoes::session_ref_of_entry(c, &alvo.chat_id, p.aviso_destino)? == alvo, "retorno_aviso_invalido");
    ensure!(p.recibo.bot_id == p.para && p.recibo.run_id == p.id, "retorno_cadeia_invalida");
    let now = now_ms();
    c.execute(
        "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,
           requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,
           admission_id,origin_entry_id,target_entry_id,attempt,fence,state,created_at,updated_at)
         VALUES(?1,?2,'inter_bot',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,1,1,'accepted',?15,?15)",
        params![
            p.id,
            VERSAO,
            p.modo.sql(),
            p.de,
            origem.chat_id,
            origem.id,
            reference,
            p.para,
            alvo.chat_id,
            alvo.id,
            p.recibo.chain_id,
            p.recibo.admission_id,
            p.aviso_origem,
            p.aviso_destino,
            now
        ],
    )?;
    Ok(())
}

/// Antes da permissão e do `session/prompt`: sem esta gravação não há envio. `None`: o
/// pedido já foi decidido (cancelado/interrompido) e não deve ser enviado.
pub(crate) fn despachar_na(c: &Connection, id: &str) -> Result<Option<i64>> {
    let changed = c.execute(
        "UPDATE donna_session_requests SET state='running',fence=fence+1,updated_at=?2
         WHERE id=?1 AND schema_version=1 AND state='accepted'",
        params![id, now_ms()],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(Some(c.query_row("SELECT fence FROM donna_session_requests WHERE id=?1", [id], |r| r.get(0))?))
}

/// Encerra só pedido ainda aberto. `false`: já havia conclusão ou decisão, que prevalece.
pub(crate) fn encerrar_na(c: &Connection, id: &str, decisao: Decisao) -> Result<bool> {
    if decisao == Decisao::NaoIniciado {
        let changed = c.execute(
            "UPDATE donna_session_requests SET state='cancelled',end_reason='not_started',fence=fence+1,updated_at=?2
             WHERE id=?1 AND schema_version=1 AND state='accepted'",
            params![id, now_ms()],
        )?;
        return Ok(changed == 1);
    }
    let changed = c.execute(
        "UPDATE donna_session_requests SET state=?2,end_reason=?3,fence=fence+1,updated_at=?4
         WHERE id=?1 AND schema_version=1 AND state IN ('accepted','running')",
        params![id, decisao.estado(), decisao.sql(), now_ms()],
    )?;
    Ok(changed == 1)
}

/// Linha do pedido lida para conferir a conclusão.
struct Aberto {
    state: String,
    fence: i64,
    attempt: i64,
    bot: String,
    alvo: SessionRef,
    delivery_state: Option<String>,
}

/// Resultado da gravação de uma conclusão.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Conclusao {
    /// Linha gravada agora ou repetição idêntica: o mesmo recibo.
    Gravada(String),
    /// Conclusão gravada, sem corpo no canal nem em avisos finais.
    CorpoRetido,
    /// Tentativa/cerca vencida ou pedido já decidido: nada foi alterado.
    Vencida,
}

/// Conclusão real com CAS por estado, tentativa e cerca. Repetição idêntica devolve o mesmo
/// recibo; divergente conflita. Callback vencido ou pedido decidido não altera nada.
pub(crate) fn concluir_na(c: &Connection, t: &Termino) -> Result<Conclusao> {
    concluir_com_produtor_na(c, t, None)
}

pub(crate) fn concluir_open_na(
    c: &Connection,
    t: &Termino,
    produtor: &crate::fork::open_tool_delegation::OpenProducer<'_>,
) -> Result<Conclusao> {
    let reference = produtor.reference_on(c, t)?;
    concluir_com_produtor_na(c, t, Some(reference))
}

fn concluir_com_produtor_na(c: &Connection, t: &Termino, open: Option<String>) -> Result<Conclusao> {
    let aberto = c
        .query_row(
            "SELECT r.state,r.fence,r.attempt,r.target_bot_id,r.target_chat_id,r.target_session_id,p.delivery_state
             FROM donna_session_requests r LEFT JOIN donna_session_privacy p ON p.request_id=r.id
             WHERE r.id=?1 AND r.schema_version=1",
            [&t.pedido],
            |r| {
                Ok(Aberto {
                    state: r.get(0)?,
                    fence: r.get(1)?,
                    attempt: r.get(2)?,
                    bot: r.get(3)?,
                    alvo: SessionRef { chat_id: r.get(4)?, id: r.get(5)? },
                    delivery_state: r.get(6)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("retorno_desconhecido"))?;
    ensure!(t.bot == aberto.bot, "retorno_produtor_invalido");
    if aberto.delivery_state.as_deref().is_some_and(|state| state != "retained")
        || aberto.attempt != t.attempt
        || (aberto.state != "finished" && (aberto.state != "running" || aberto.fence != t.fence))
    {
        return Ok(Conclusao::Vencida);
    }
    let alvo = aberto.alvo;
    let produtor = match &t.fim.produtor {
        Some(p) => bindings::for_acp_on(c, &t.bot, &p.acp_session)
            .ok()
            .filter(|b| b.session_ref == alvo)
            .map(|b| referencia(b.id.as_str(), &p.turn_token))
            .transpose()?,
        None => open,
    };
    let (conteudo, entrada) = match &t.fim.conteudo {
        Conteudo::Entrada(id) => {
            let kind: Option<String> =
                c.query_row("SELECT kind FROM entries WHERE id=?1", [id], |r| r.get(0)).optional()?;
            ensure!(kind.as_deref() == Some("agent"), "retorno_entrada_invalida");
            ensure!(
                sessoes::session_ref_of_entry(c, &alvo.chat_id, id).is_ok_and(|s| s == alvo),
                "retorno_entrada_invalida"
            );
            ensure!(t.fim.desfecho == Desfecho::Concluiu, "retorno_entrada_invalida");
            // Sem proveniência comprovada do produtor, a referência não vira sucesso de conteúdo.
            if produtor.is_some() { ("final_entry", Some(id.as_str())) } else { ("unverified", None) }
        }
        Conteudo::SemReferencia if produtor.is_none() => ("unverified", None),
        Conteudo::SemReferencia => ("unreferenced", None),
        Conteudo::Nenhum => ("none", None),
    };
    let resumo = digest(&(
        "retornos-resultado",
        t.attempt,
        t.fence,
        t.fim.desfecho.sql(),
        t.fim.motivo.sql(),
        conteudo,
        entrada,
        &produtor,
    ))?;
    let id = format!("{}#{}", t.pedido, t.attempt);
    let registrada = |id| {
        if aberto.delivery_state.is_some() { Conclusao::CorpoRetido } else { Conclusao::Gravada(id) }
    };
    if aberto.state == "finished" {
        let anterior: Option<String> =
            c.query_row("SELECT digest FROM donna_session_results WHERE id=?1", [&id], |r| r.get(0)).optional()?;
        return match anterior {
            Some(anterior) if anterior == resumo => Ok(registrada(id)),
            Some(_) => bail!("retorno_conflito"),
            None => Ok(Conclusao::Vencida),
        };
    }
    c.execute(
        "INSERT INTO donna_session_results(id,request_id,schema_version,attempt,fence,producer_execution_ref,
           outcome,end_reason,content,final_entry_id,digest,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            id,
            t.pedido,
            VERSAO,
            t.attempt,
            t.fence,
            produtor,
            t.fim.desfecho.sql(),
            t.fim.motivo.sql(),
            conteudo,
            entrada,
            resumo,
            now_ms()
        ],
    )?;
    let changed = c.execute(
        "UPDATE donna_session_requests SET state='finished',fence=fence+1,updated_at=?3
         WHERE id=?1 AND state='running' AND fence=?2",
        params![t.pedido, t.fence, now_ms()],
    )?;
    ensure!(changed == 1, "retorno_vencido");
    Ok(registrada(id))
}

/// Startup, antes dos atores: nada aberto sobrevive como em andamento. Sem prova de envio,
/// `restart_before_start`; com possível envio, `restart_uncertain`. Nunca reenvia nem conclui.
pub(crate) fn recuperar_na(c: &Connection) -> Result<usize> {
    let now = now_ms();
    let antes = c.execute(
        "UPDATE donna_session_requests SET state='interrupted',end_reason='restart_before_start',fence=fence+1,
           updated_at=?1 WHERE schema_version=1 AND state='accepted'",
        [now],
    )?;
    let incertos = c.execute(
        "UPDATE donna_session_requests SET state='interrupted',end_reason='restart_uncertain',fence=fence+1,
           updated_at=?1 WHERE schema_version=1 AND state='running'",
        [now],
    )?;
    super::cadeias::equipe::recuperar_open_na(c)?;
    super::open_tool_delegation::recover_notices_on(c)?;
    entrega::recover_on(c)?;
    Ok(antes + incertos)
}

/// Andamento em memória do pedido no ator destinatário; a verdade é a tabela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Andamento {
    /// Chamada interna legada sem `ExecutionRef`: só o canal vivo, sem identidade inventada.
    SemRegistro,
    Aceito,
    /// Cerca da tentativa gravada antes do `session/prompt`.
    EmExecucao(i64),
}

/// Como o ator encerrou o pedido.
pub(crate) enum Fim {
    /// Resposta final correlacionada do `session/prompt` (ou fim local equivalente do turno).
    Turno(Encerramento),
    FalhaInicio,
    Desligado,
}

/// Situação do registro durável, informada antes do canal em memória.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Registro {
    Gravado,
    /// Conclusão gravada com qualificação de privacidade; o canal não recebe corpo.
    CorpoRetido,
    /// Canal vivo legado, sem pedido durável.
    Legado,
    /// Decisão anterior ou tentativa vencida prevalece; nada foi alterado.
    Decidido,
    /// A gravação falhou: o pedido continua aberto e o reinício o marca incerto.
    ErroDeGravacao,
}

/// Texto seguro para o canal existente quando a conclusão não foi gravada.
pub(crate) const ERRO_DE_GRAVACAO: &str = "A conclusão deste pedido não foi registrada de forma durável e o resultado não foi entregue por aqui. Confira a conversa do destinatário antes de tentar de novo.";

/// Traduz o fim do turno do ator, nas mesmas regras da resposta ao vivo. Só um desfecho
/// `Concluiu` com texto pode referenciar conteúdo; sem entrada única, fica sem referência.
pub(crate) fn fim_do_turno(
    parou: bool,
    limite: bool,
    erro_do_motor: bool,
    stop_reason: &str,
    texto: Option<&str>,
    entrada: Option<String>,
    produtor: Option<Produtor>,
) -> Fim {
    let (desfecho, motivo) = if parou {
        (Desfecho::Parou, MotivoFim::Cancelado)
    } else if limite {
        (Desfecho::Incompleto, MotivoFim::LimiteDaAssinatura)
    } else if erro_do_motor {
        (Desfecho::Falhou, MotivoFim::ErroDoMotor)
    } else if stop_reason != "end_turn" {
        (Desfecho::Incompleto, MotivoFim::do_motor(stop_reason))
    } else {
        (Desfecho::Concluiu, MotivoFim::FimDoTurno)
    };
    let conteudo = match entrada {
        _ if desfecho != Desfecho::Concluiu || texto.is_none_or(|t| t.trim().is_empty()) => Conteudo::Nenhum,
        Some(id) => Conteudo::Entrada(id),
        None => Conteudo::SemReferencia,
    };
    Fim::Turno(Encerramento { desfecho, motivo, conteudo, produtor })
}

/// Grava o término antes do oneshot/avisos finais. Erro é registrado sem texto do usuário.
pub(crate) fn registrar_fim(
    store: &Store,
    pedido: &str,
    bot: &str,
    andamento: Andamento,
    fim: Fim,
    corpo: Option<&str>,
) -> Registro {
    let encerrar = |decisao| {
        encerrar(store, pedido, decisao).map(|gravado| if gravado { Registro::Gravado } else { Registro::Decidido })
    };
    let gravado = match (andamento, fim) {
        (Andamento::SemRegistro, _) => return Registro::Legado,
        (_, Fim::Desligado) => encerrar(Decisao::HostParou),
        (Andamento::Aceito, Fim::FalhaInicio) => encerrar(Decisao::FalhaInicio),
        (Andamento::Aceito, Fim::Turno(_)) => encerrar(Decisao::NaoIniciado).map(|_| Registro::Decidido),
        (Andamento::EmExecucao(fence), Fim::FalhaInicio) => {
            gravar(store, pedido, bot, fence, Encerramento::falha_inicio(), None)
        }
        (Andamento::EmExecucao(fence), Fim::Turno(fim)) => gravar(store, pedido, bot, fence, fim, corpo),
    };
    gravado.unwrap_or_else(|_| {
        tracing::error!(request = pedido, error_code = "retorno_gravacao_falhou", "conclusão do pedido não registrada");
        Registro::ErroDeGravacao
    })
}

fn gravar(
    store: &Store,
    pedido: &str,
    bot: &str,
    fence: i64,
    fim: Encerramento,
    corpo: Option<&str>,
) -> Result<Registro> {
    let termino = Termino { pedido: pedido.into(), bot: bot.into(), attempt: 1, fence, fim };
    com_tx(store, |c| {
        Ok(match concluir_na(c, &termino)? {
            Conclusao::CorpoRetido => Registro::CorpoRetido,
            Conclusao::Gravada(_) => {
                let corpo = corpo.filter(|_| termino.fim.desfecho == Desfecho::Concluiu);
                entrega::stage_on(c, pedido, corpo)?;
                Registro::Gravado
            }
            Conclusao::Vencida => Registro::Decidido,
        })
    })
}

pub(crate) fn com_tx<T>(store: &Store, change: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let out = change(&tx)?;
    tx.commit()?;
    Ok(out)
}

/// Produção grava `running` dentro da Tx do permit (`despachar_na`); testes usam a Tx própria.
#[cfg(test)]
pub(crate) fn despachar(store: &Store, id: &str) -> Result<Option<i64>> {
    com_tx(store, |c| despachar_na(c, id))
}
/// Pedido ainda `accepted` (não decidido nem em execução).
pub(crate) fn aceito(store: &Store, id: &str) -> Result<bool> {
    let state: Option<String> = store
        .db
        .locked()
        .query_row("SELECT state FROM donna_session_requests WHERE id=?1 AND schema_version=1", [id], |r| r.get(0))
        .optional()?;
    Ok(state.as_deref() == Some("accepted"))
}
pub(crate) fn encerrar(store: &Store, id: &str, decisao: Decisao) -> Result<bool> {
    com_tx(store, |c| encerrar_na(c, id, decisao))
}
#[cfg(test)]
pub(crate) fn concluir(store: &Store, termino: &Termino) -> Result<Conclusao> {
    com_tx(store, |c| concluir_na(c, termino))
}
pub(crate) fn recuperar(store: &Store) -> Result<usize> {
    com_tx(store, recuperar_na)
}

#[cfg(test)]
mod consulta_testes;
#[cfg(test)]
mod pendentes_testes;
#[cfg(test)]
pub(crate) mod provas_privacidade_testes;
#[cfg(test)]
pub(crate) mod testes;
