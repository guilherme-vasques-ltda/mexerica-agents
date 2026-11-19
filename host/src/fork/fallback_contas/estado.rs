//! F6 fatia B (MEX-1391): estado durável do fallback entre contas do mesmo motor.
//!
//! Um registro só (`fork.fallback-contas.v1`) com as ordens de reserva, o descanso por conta, o bloqueio de 1 h do
//! destino que falhou, a permissão de uso extra e o diário das trocas; mais uma sobreposição pequena por conversa
//! (`fork.fallback-contas.sessao.<id>`), lida dentro de `engine_choice::read`. Toda gravação lê, muda e grava sob o
//! mesmo trava do SQLite: duas trocas concorrentes não perdem uma à outra.
use crate::{LockExt, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const KEY: &str = "fork.fallback-contas.v1";
const SESSAO: &str = "fork.fallback-contas.sessao.";
/// Até 3 reservas depois da conta do bot (plano, regra (a)).
pub(crate) const MAX_RESERVAS: usize = 3;
/// Destino que falhou ao assumir não recebe nova troca por 1 h (regra (o).3).
pub(crate) const BLOQUEIO_MS: i64 = 3_600_000;
/// Conta esgotada sem horário de volta descansa 1 h e é lida de novo (plano, "Volta").
pub(crate) const DESCANSO_SEM_LEITURA_MS: i64 = 3_600_000;
const MAX_TROCAS: usize = 100;

/// Por que a conta descansa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Motivo {
    /// Leitura >= 90% da sessão ou da semana (regra (m)).
    PertoDoLimite,
    /// Cota, limite ou crédito esgotado (regra (b)).
    Esgotada,
    /// A conta de origem voltou a ser a primeira livre; só aparece no diário de trocas.
    Voltou,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Descanso {
    /// Instante em que a conta volta a valer (reset informado, ou 1 h sem leitura).
    pub ate: i64,
    pub motivo: Motivo,
}

/// A conversa roda na conta `atual` enquanto a escolha do dono (`origem`) descansa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Sobreposicao {
    pub space: String,
    pub bot: String,
    pub origem: String,
    pub atual: String,
    pub desde: i64,
    pub motivo: Motivo,
}

/// Uma troca feita, para o cartão do Mission Board e o recibo (regra (o).2 e (d)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Troca {
    pub id: String,
    pub em: i64,
    pub space: String,
    pub bot: String,
    pub sessao: String,
    pub de: String,
    pub para: String,
    pub motivo: Motivo,
    /// Quando a conta de origem volta (reset informado), se conhecido.
    pub volta_em: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Estado {
    /// `"<space>|<motor>"` → contas reserva, na ordem. Vale para os bots do Space.
    pub ordens: BTreeMap<String, Vec<String>>,
    /// `"<bot>|<motor>"` → ordem só deste bot, no lugar da do Space.
    pub ordens_bot: BTreeMap<String, Vec<String>>,
    pub descanso: BTreeMap<String, Descanso>,
    /// Conta → instante até o qual não recebe nova troca.
    pub bloqueio: BTreeMap<String, i64>,
    /// Conta → reset até o qual o dono permitiu uso extra (regra (m).2).
    pub uso_extra: BTreeMap<String, i64>,
    pub trocas: Vec<Troca>,
}

pub(crate) fn chave_ordem(space: &str, backend: &str) -> String {
    format!("{space}|{backend}")
}

pub(crate) fn chave_ordem_bot(bot: &str, backend: &str) -> String {
    format!("{bot}|{backend}")
}

impl Estado {
    pub(crate) fn carregar(c: &Connection) -> Result<Self> {
        let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [KEY], |r| r.get(0)).optional()?;
        raw.map_or_else(
            || Ok(Self::default()),
            |raw| {
                serde_json::from_str(&raw).map_err(|_| anyhow!("fallback_state_invalid: estado do fallback ilegível."))
            },
        )
    }

    fn gravar(&self, c: &Connection) -> Result<()> {
        c.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
            params![KEY, serde_json::to_string(self)?],
        )?;
        Ok(())
    }

    /// A ordem de reserva que vale para o bot: a dele, se existir, senão a do Space. Nunca devolve mais de 3.
    pub(crate) fn ordem(&self, space: &str, bot: &str, backend: &str) -> Vec<String> {
        let lista = self
            .ordens_bot
            .get(&chave_ordem_bot(bot, backend))
            .or_else(|| self.ordens.get(&chave_ordem(space, backend)))
            .cloned()
            .unwrap_or_default();
        lista.into_iter().take(MAX_RESERVAS).collect()
    }

    /// Descanso ainda em vigor em `agora` (vencido não conta).
    pub(crate) fn descansando(&self, conta: &str, agora: i64) -> Option<&Descanso> {
        self.descanso.get(conta).filter(|d| d.ate > agora)
    }

    pub(crate) fn bloqueada(&self, conta: &str, agora: i64) -> bool {
        self.bloqueio.get(conta).is_some_and(|ate| *ate > agora)
    }

    pub(crate) fn uso_extra_vigente(&self, conta: &str, agora: i64) -> bool {
        self.uso_extra.get(conta).is_some_and(|ate| *ate > agora)
    }

    /// Tira o que já venceu, para o registro não crescer.
    fn limpar(&mut self, agora: i64) {
        self.descanso.retain(|_, d| d.ate > agora);
        self.bloqueio.retain(|_, ate| *ate > agora);
        self.uso_extra.retain(|_, ate| *ate > agora);
        if self.trocas.len() > MAX_TROCAS {
            let sobra = self.trocas.len() - MAX_TROCAS;
            self.trocas.drain(..sobra);
        }
    }
}

/// Lê, muda e grava o registro sob o mesmo trava do SQLite. Só grava se algo mudou.
pub(crate) fn alterar<T>(store: &Store, mudar: impl FnOnce(&mut Estado) -> Result<T>) -> Result<T> {
    let c = store.db.locked();
    alterar_em(&c, mudar)
}

pub(crate) fn alterar_em<T>(c: &Connection, mudar: impl FnOnce(&mut Estado) -> Result<T>) -> Result<T> {
    let antes = Estado::carregar(c)?;
    let mut depois = antes.clone();
    let valor = mudar(&mut depois)?;
    depois.limpar(crate::store::now_ms());
    if depois != antes {
        depois.gravar(c)?;
    }
    Ok(valor)
}

pub(crate) fn ler(store: &Store) -> Result<Estado> {
    Estado::carregar(&store.db.locked())
}

fn chave_sessao(sessao: &str) -> String {
    format!("{SESSAO}{sessao}")
}

pub(crate) fn sobreposicao(c: &Connection, sessao: &str) -> Result<Option<Sobreposicao>> {
    let raw: Option<String> =
        c.query_row("SELECT v FROM kv WHERE k=?1", [chave_sessao(sessao)], |r| r.get(0)).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(|_| anyhow!("fallback_overlay_invalid"))).transpose()
}

pub(crate) fn gravar_sobreposicao(c: &Connection, sessao: &str, valor: Option<&Sobreposicao>) -> Result<()> {
    match valor {
        Some(valor) => {
            ensure!(valor.origem != valor.atual, "fallback_overlay_invalid");
            c.execute(
                "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
                params![chave_sessao(sessao), serde_json::to_string(valor)?],
            )?;
        }
        None => {
            c.execute("DELETE FROM kv WHERE k=?1", [chave_sessao(sessao)])?;
        }
    }
    Ok(())
}

/// As conversas que hoje rodam na conta `conta` por causa do fallback (para a remoção da conta recusar).
pub(crate) fn sessoes_em(c: &Connection, conta: &str) -> Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT k, v FROM kv WHERE k LIKE ?1")?;
    let linhas: Vec<(String, String)> =
        stmt.query_map([format!("{SESSAO}%")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut sessoes = Vec::new();
    for (chave, raw) in linhas {
        let valor: Sobreposicao = serde_json::from_str(&raw).map_err(|_| anyhow!("fallback_overlay_invalid"))?;
        if valor.atual == conta || valor.origem == conta {
            sessoes.push(chave.trim_start_matches(SESSAO).to_owned());
        }
    }
    Ok(sessoes)
}

/// Todas as sobreposições (para o painel e o cartão).
pub(crate) fn todas_sobreposicoes(c: &Connection) -> Result<Vec<(String, Sobreposicao)>> {
    let mut stmt = c.prepare("SELECT k, v FROM kv WHERE k LIKE ?1")?;
    let linhas: Vec<(String, String)> =
        stmt.query_map([format!("{SESSAO}%")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    linhas
        .into_iter()
        .map(|(chave, raw)| {
            let valor = serde_json::from_str(&raw).map_err(|_| anyhow!("fallback_overlay_invalid"))?;
            Ok((chave.trim_start_matches(SESSAO).to_owned(), valor))
        })
        .collect()
}
