use crate::{
    LockExt,
    store::{BotConfig, Lane, Store, now_ms},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_turn_usage(
          id TEXT PRIMARY KEY, bot_id TEXT NOT NULL, chat_id TEXT NOT NULL, session_id TEXT NOT NULL,
          parent_session_id TEXT, account_id TEXT, model TEXT, at INTEGER NOT NULL,
          input_tokens INTEGER, cached_read_tokens INTEGER, cached_write_tokens INTEGER, output_tokens INTEGER,
          amount REAL, currency TEXT, source TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS donna_turn_usage_month ON donna_turn_usage(at);
         CREATE INDEX IF NOT EXISTS donna_turn_usage_session ON donna_turn_usage(chat_id,session_id);
         CREATE TABLE IF NOT EXISTS donna_cost_cursors(
          bot_id TEXT NOT NULL, acp_session TEXT NOT NULL, amount REAL NOT NULL, currency TEXT NOT NULL, valid INTEGER NOT NULL DEFAULT 1,
          PRIMARY KEY(bot_id,acp_session)) WITHOUT ROWID;"
    )?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Cost {
    pub amount: f64,
    pub currency: String,
}
impl Cost {
    fn parse(v: &Value) -> Option<Self> {
        let amount = v["amount"].as_f64().filter(|n| n.is_finite() && *n >= 0.0)?;
        let currency = v["currency"].as_str().filter(|s| s.len() == 3 && s.bytes().all(|c| c.is_ascii_uppercase()))?;
        Some(Self { amount, currency: currency.into() })
    }
}

#[derive(Default, Clone)]
pub(crate) struct Tokens {
    pub input: Option<i64>,
    pub read: Option<i64>,
    pub write: Option<i64>,
    pub output: Option<i64>,
    /// Algum campo de uso presente mas não representável (não inteiro,
    /// negativo ou acima de `i64::MAX`). Nunca é ausência: `sem preço` sempre.
    pub invalid: bool,
}
impl Tokens {
    fn from_answer(answer: &Value) -> Self {
        let u = &answer["usage"];
        let campo = |k: &str| (u.get(k).is_some(), u[k].as_u64().and_then(|n| i64::try_from(n).ok()));
        let (has_in, input) = campo("inputTokens");
        let (has_read, read) = campo("cachedReadTokens");
        let (has_write, write) = campo("cachedWriteTokens");
        let (has_out, output) = campo("outputTokens");
        Self {
            input,
            read,
            write,
            output,
            invalid: (has_in && input.is_none())
                || (has_read && read.is_none())
                || (has_write && write.is_none())
                || (has_out && output.is_none()),
        }
    }
}

/// Único encaixe de preço (MX-159): o valor informado pelo motor manda
/// (`source` `motor`); o motor que informou custo no turno NUNCA é estimado —
/// `cost` presente (mesmo inválido) com diferença que não fecha (sessão sem
/// referência, regressão, troca de moeda) fica `sem preço`. Só sem campo
/// `cost` no turno, um modelo com linha na tabela versionada (`precos.json`)
/// e contagens conhecidas estima em USD (`source` `estimado`). Somente novos
/// turnos; nunca atualiza o histórico.
fn price(
    tokens: &Tokens,
    model: Option<&str>,
    motor: Option<Cost>,
    informou: bool,
    politica: super::precos::PoliticaCache,
) -> Option<(Cost, &'static str)> {
    if let Some(c) = motor {
        return Some((c, "motor"));
    }
    if informou {
        return None;
    }
    let estimado = super::precos::estimar(model?, tokens, politica)?;
    Some((estimado, "estimado"))
}

/// MX-145: trabalho de fundo (títulos, memória, resumo do dia) entra no ledger com
/// `bot_id` = o bot do trabalho, `chat_id` = a principal dele e `session_id` =
/// `background:<job>` — uma "conversa" própria no relatório, sem sessão ACP real
/// nem pai. Sem preço informado pelo motor: `amount`/`currency` NULL (o teto de
/// dinheiro fica não conferido); sem retorno de uso: tokens NULL.
pub(crate) const BACKGROUND_SESSION_PREFIX: &str = "background:";

/// Uso devolvido pelo motor de fundo: contagens de cache `None` = ausentes
/// (não zero); `invalid` = algum campo presente mas não representável
/// (`sem preço` sempre, como no turno normal).
#[derive(Default)]
pub(crate) struct UsoFundo {
    pub input: u64,
    pub output: u64,
    pub read: Option<u64>,
    pub write: Option<u64>,
    pub invalid: bool,
    /// Backend do motor selecionado e `agentInfo.version` do handshake: a
    /// política de cache ausente é por adaptador na versão pinada.
    pub backend: String,
    pub adapter_versao: Option<String>,
}

/// Envio e uso numa chamada só (testes do relatório e do teto).
#[cfg(test)]
pub(crate) fn record_background(
    store: &Store,
    bot: &str,
    job: &str,
    model: Option<&str>,
    account: Option<&str>,
    tokens: Option<UsoFundo>,
) -> Result<()> {
    let id = begin_background(store, bot, job, model, account)?;
    complete_background(store, &id, model, tokens)
}

/// Evidência do ENVIO: a linha nasce antes do `session/prompt` (tokens NULL =
/// não conferido). Cancelar o trabalho depois disso (timeout, heartbeat, queda)
/// nunca tira o envio do ledger; `complete_background` só acrescenta o uso.
pub(crate) fn begin_background(
    store: &Store,
    bot: &str,
    job: &str,
    model: Option<&str>,
    account: Option<&str>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    store.db.locked().execute(
        "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,parent_session_id,account_id,model,at,
         input_tokens,cached_read_tokens,cached_write_tokens,output_tokens,amount,currency,source)
         VALUES(?1,?2,?2,?3,NULL,?4,?5,?6,NULL,NULL,NULL,NULL,NULL,NULL,'fundo')",
        params![id, bot, format!("{BACKGROUND_SESSION_PREFIX}{job}"), account, model, now_ms()],
    )?;
    Ok(id)
}

/// Uso devolvido pelo motor para o envio já registrado; sem uso, a linha fica NULL.
/// MX-159: com modelo na tabela e tokens conhecidos, estima o custo (`source`
/// vira `estimado`); sem isso, a linha segue `fundo` sem preço — o teto de
/// dinheiro continua não conferido.
pub(crate) fn complete_background(
    store: &Store,
    id: &str,
    model: Option<&str>,
    tokens: Option<UsoFundo>,
) -> Result<()> {
    // Contagem acima de `i64::MAX` não é representável: fica NULL e marca
    // `invalid` (presente ≠ ausente; `sem preço` sempre, como no turno normal).
    let obrigatorio = |n: u64| (i64::try_from(n).ok(), i64::try_from(n).is_err());
    let opcional =
        |n: Option<u64>| (n.and_then(|v| i64::try_from(v).ok()), n.is_some_and(|v| i64::try_from(v).is_err()));
    let (uso_backend, uso_versao) =
        tokens.as_ref().map_or_else(|| (String::new(), None), |u| (u.backend.clone(), u.adapter_versao.clone()));
    let (input, output, read, write, invalid) = match tokens {
        Some(uso) => {
            let (input, in_bad) = obrigatorio(uso.input);
            let (output, out_bad) = obrigatorio(uso.output);
            let (read, read_bad) = opcional(uso.read);
            let (write, write_bad) = opcional(uso.write);
            (input, output, read, write, uso.invalid || in_bad || out_bad || read_bad || write_bad)
        }
        None => (None, None, None, None, false),
    };
    let tokens = Tokens { input, read, write, output, invalid };
    let politica = super::precos::politica_cache(&uso_backend, uso_versao.as_deref());
    let estimado = model.and_then(|m| super::precos::estimar(m, &tokens, politica));
    store.db.locked().execute(
        "UPDATE donna_turn_usage SET model=COALESCE(?2,model), input_tokens=?3, cached_read_tokens=?4,
         cached_write_tokens=?5, output_tokens=?6, amount=?7, currency=?8,
         source=CASE WHEN ?7 IS NULL THEN 'fundo' ELSE 'estimado' END
         WHERE id=?1 AND source='fundo'",
        params![
            id,
            model,
            input,
            read,
            write,
            output,
            estimado.as_ref().map(|c| c.amount),
            estimado.as_ref().map(|c| &c.currency)
        ],
    )?;
    Ok(())
}

pub(crate) struct Pending {
    id: String,
    bot: String,
    lane: Lane,
    parent: Option<String>,
    account: Option<String>,
    model: Option<String>,
    acp: String,
    backend: String,
    /// `agentInfo.version` do handshake ACP: a política de cache ausente só
    /// vale na versão do adaptador medida; outra versão → fail-closed.
    adapter_versao: Option<String>,
    before: Option<Cost>,
    fresh: bool,
    sent: bool,
    /// Custo observado no turno. Presente ≠ válido: campo `cost` com valor
    /// inválido não apaga o último válido, mas marca o turno como não
    /// verificável (`sem preço`, nunca estima — o último válido ainda alimenta
    /// o cursor, conhecimento acumulado não é perdido).
    custo: CustoObservado,
}

/// Estado do campo `cost` no turno: ausente, válido (guarda o último valor
/// acumulado) ou já manchado por um valor inválido (o último válido segue
/// guardado para o cursor, mas o turno não é conferível).
#[derive(Default)]
enum CustoObservado {
    #[default]
    Nenhum,
    Valido(Cost),
    Invalido(Option<Cost>),
}
impl CustoObservado {
    /// O motor apresentou o campo `cost` (válido ou não) neste turno.
    fn presente(&self) -> bool {
        !matches!(self, Self::Nenhum)
    }
    /// Último custo válido recebido, se houver.
    fn ultimo(&self) -> Option<&Cost> {
        match self {
            Self::Valido(c) | Self::Invalido(Some(c)) => Some(c),
            _ => None,
        }
    }
}
impl Pending {
    pub(crate) fn begin(
        store: &Store,
        cfg: &BotConfig,
        lane: &Lane,
        acp: &str,
        fresh: bool,
        adapter_versao: Option<String>,
    ) -> Result<Self> {
        let db = store.db.locked();
        let before = db
            .query_row(
                "SELECT amount,currency FROM donna_cost_cursors WHERE bot_id=?1 AND acp_session=?2 AND valid=1",
                params![cfg.id, acp],
                |r| Ok(Cost { amount: r.get(0)?, currency: r.get(1)? }),
            )
            .optional()?;
        let parent = db
            .query_row(
                "SELECT parent_session FROM donna_bot_sessions WHERE id=?1 AND chat_id=?2",
                params![lane.session.as_deref().unwrap_or(&lane.chat), lane.chat],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            bot: cfg.id.clone(),
            lane: lane.clone(),
            parent,
            account: cfg.fork_editor.engine_account_id.clone(),
            model: cfg.model.clone(),
            acp: acp.into(),
            backend: cfg.backend.clone(),
            adapter_versao,
            before,
            fresh,
            sent: false,
            custo: CustoObservado::Nenhum,
        })
    }

    pub(crate) fn dispatched(&mut self, store: &Store) -> Result<()> {
        // Consumir sem commit de finish (Shutdown, crash ou erro SQL) é uma lacuna.
        // A pré-imagem deste turno fica em memória, mas o próximo nunca pode herdá-la.
        store.db.locked().execute(
            "UPDATE donna_cost_cursors SET valid=0 WHERE bot_id=?1 AND acp_session=?2",
            params![self.bot, self.acp],
        )?;
        self.sent = true;
        Ok(())
    }

    /// ACP cost é cumulativo por sessão, não o preço de cada notificação nem de cada turno.
    /// Campo `cost` presente com valor inválido não apaga o último válido
    /// recebido, mas marca o turno como não verificável (`sem preço`): o
    /// acumulado final ficou desconhecido.
    pub(crate) fn observe(&mut self, update: &Value) {
        if update.get("cost").is_some() {
            let valido = Cost::parse(&update["cost"]);
            let manchado = matches!(self.custo, CustoObservado::Invalido(_));
            let ultimo = self.custo.ultimo().cloned();
            self.custo = if let (false, Some(c)) = (manchado, valido.clone()) {
                CustoObservado::Valido(c)
            } else {
                // Uma vez manchado, o turno segue não verificável; o último
                // válido continua sendo guardado para o cursor.
                CustoObservado::Invalido(valido.or(ultimo))
            };
        }
    }

    pub(crate) fn finish(self, store: &Store, answer: Option<&Value>) -> Result<()> {
        if !self.sent {
            return Ok(());
        }
        let tokens = answer.map(Tokens::from_answer).unwrap_or_default();
        // Um `cost` inválido no turno torna o acumulado final desconhecido:
        // a diferença não é confiável → `sem preço`, e o cursor é invalidado
        // (o consumo escondido nunca é atribuído ao turno seguinte).
        let limpo = matches!(self.custo, CustoObservado::Valido(_));
        let ultimo = self.custo.ultimo();
        let motor = ultimo.filter(|_| limpo).and_then(|last| {
            let baseline = match &self.before {
                Some(before) if before.currency == last.currency && last.amount >= before.amount => before.amount,
                None if self.fresh => 0.0,
                _ => return None,
            };
            Some(Cost { amount: last.amount - baseline, currency: last.currency.clone() })
        });
        let priced = price(
            &tokens,
            self.model.as_deref(),
            motor,
            self.custo.presente(),
            super::precos::politica_cache(&self.backend, self.adapter_versao.as_deref()),
        );
        for n in [tokens.input, tokens.read, tokens.write, tokens.output].into_iter().flatten() {
            ensure!(n >= 0, "invalid_turn_tokens");
        }
        let mut db = store.db.locked();
        let tx = db.transaction()?;
        if let (true, Some(last)) = (limpo, ultimo) {
            tx.execute("INSERT INTO donna_cost_cursors(bot_id,acp_session,amount,currency) VALUES(?1,?2,?3,?4)
                ON CONFLICT(bot_id,acp_session) DO UPDATE SET amount=excluded.amount,currency=excluded.currency,valid=1",
                params![self.bot,self.acp,last.amount,last.currency])?;
        }
        if !(limpo && ultimo.is_some()) {
            // Uma lacuna não pode ser atribuída ao próximo turno que voltar a informar custo.
            tx.execute(
                "UPDATE donna_cost_cursors SET valid=0 WHERE bot_id=?1 AND acp_session=?2",
                params![self.bot, self.acp],
            )?;
        }
        {
            tx.execute(
                "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,parent_session_id,account_id,model,at,
                input_tokens,cached_read_tokens,cached_write_tokens,output_tokens,amount,currency,source)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    self.id,
                    self.bot,
                    self.lane.chat,
                    self.lane.session.as_deref().unwrap_or(&self.lane.chat),
                    self.parent,
                    self.account,
                    self.model,
                    now_ms(),
                    tokens.input,
                    tokens.read,
                    tokens.write,
                    tokens.output,
                    priced.as_ref().map(|(c, _)| c.amount),
                    priced.as_ref().map(|(c, _)| &c.currency),
                    priced.as_ref().map_or("sem preço", |(_, s)| *s)
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
