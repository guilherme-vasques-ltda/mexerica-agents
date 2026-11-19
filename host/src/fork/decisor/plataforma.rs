//! Modo Cloud: política e teto mensal vivem no Hub (servidor), nunca vêm do cliente. Os valores
//! (limite, custo por decisão) são parâmetro interno injetado pelo backend: não há número no código.
use super::{Alvo, ProvedorDecisor, config::modelo_valido};
use crate::store::Store;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

const SLOT_POLITICA: &str = "fork.decisor.plataforma.v1";
const SLOT_USO: &str = "fork.decisor.uso.v1";
const TENTATIVAS_CAS: usize = 64;

/// Escrita só pelo caminho server-side do Hub/backend Cloud; não há rota de API nem flag de cliente.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Politica {
    /// Ligada: o decisor usa a conta `OpenRouter` da plataforma e ignora a escolha/chave do usuário.
    pub ativa: bool,
    /// Modelo de decisão da plataforma (padrão definido pelo benchmark, fatia posterior).
    pub modelo: String,
    /// Teto do período, em micro-unidades de custo. Reservado antes de cada chamada.
    pub limite_micro: u64,
    pub custo_por_decisao_micro: u64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Uso {
    periodo: String,
    usado_micro: u64,
}

pub(crate) fn definir_politica(store: &Store, politica: &Politica) -> Result<()> {
    if politica.ativa {
        ensure!(modelo_valido(&politica.modelo), "Modelo do decisor da plataforma inválido.");
        ensure!(politica.custo_por_decisao_micro > 0, "Custo por decisão deve ser positivo.");
    }
    store.kv_set(SLOT_POLITICA, &serde_json::to_string(politica)?)
}

/// Ausente ou ilegível = inativa (o decisor segue a escolha do usuário, que por padrão é desligada).
pub(crate) fn politica(store: &Store) -> Politica {
    store.kv_get(SLOT_POLITICA).and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}

pub(crate) fn alvo(politica: &Politica) -> Alvo {
    Alvo { provedor: ProvedorDecisor::OpenRouter, modelo: politica.modelo.clone() }
}

/// Período de contagem, `AAAA-MM` em UTC. O chamador passa o período para o teste poder virar o mês.
pub(crate) fn periodo_atual() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Reserva {
    Ok,
    TetoAtingido,
}

/// Reserva o custo da próxima decisão por compare-and-swap no KV, então duas decisões concorrentes
/// nunca passam do teto. Período novo zera a contagem. Erro de leitura/gravação = teto atingido
/// (fail-closed). A reserva não é devolvida se a chamada falhar, e não há retry.
pub(crate) fn reservar(store: &Store, politica: &Politica, periodo: &str) -> Reserva {
    for _ in 0..TENTATIVAS_CAS {
        let Ok(bruto) = store.kv_read(SLOT_USO) else { return Reserva::TetoAtingido };
        let uso: Uso = match bruto.as_deref().map(serde_json::from_str) {
            None => Uso::default(),
            Some(Ok(uso)) => uso,
            Some(Err(_)) => return Reserva::TetoAtingido,
        };
        let base = if uso.periodo == periodo { uso.usado_micro } else { 0 };
        let Some(novo) = base.checked_add(politica.custo_por_decisao_micro).filter(|n| *n <= politica.limite_micro)
        else {
            return Reserva::TetoAtingido;
        };
        let Ok(valor) = serde_json::to_string(&Uso { periodo: periodo.to_owned(), usado_micro: novo }) else {
            return Reserva::TetoAtingido;
        };
        match store.kv_compare_exchange(SLOT_USO, bruto.as_deref(), &valor) {
            Ok(true) => return Reserva::Ok,
            Ok(false) => {}
            Err(_) => return Reserva::TetoAtingido,
        }
    }
    Reserva::TetoAtingido
}
