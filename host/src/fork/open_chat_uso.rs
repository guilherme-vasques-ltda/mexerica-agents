use crate::store::{BotConfig, Lane, Store};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Contagem {
    #[serde(rename = "inputTokens")]
    pub(super) entrada: u64,
    #[serde(rename = "cachedInputTokens")]
    pub(super) cache_lido: u64,
    #[serde(default, rename = "cacheWriteInputTokens")]
    pub(super) cache_gravado: u64,
    #[serde(rename = "outputTokens")]
    pub(super) saida: u64,
    #[serde(rename = "reasoningOutputTokens")]
    pub(super) raciocinio: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Uso {
    pub(super) entrada_sem_cache: u64,
    pub(super) cache_lido: u64,
    pub(super) cache_gravado: u64,
    pub(super) saida: u64,
}

#[derive(Deserialize, Serialize)]
struct Amostra {
    thread: String,
    total: Contagem,
}

pub(super) struct Cursor<'a> {
    chave: &'a str,
    anterior: Option<Contagem>,
}

impl<'a> Cursor<'a> {
    pub(super) fn iniciar(store: &Store, bot: &str, chave: &'a str, thread: &str, nova: bool) -> Result<Self> {
        let anterior = if nova {
            Some(Contagem::default())
        } else {
            store
                .kv_read(chave)?
                .and_then(|json| serde_json::from_str::<Amostra>(&json).ok())
                .filter(|amostra| amostra.thread == thread)
                .map(|amostra| amostra.total)
        };
        ensure!(store.kv_set_if_bot(bot, chave, "null")?, "open_chat_cancelled");
        Ok(Self { chave, anterior })
    }

    pub(super) fn gravar(
        self,
        store: &Store,
        cfg: &BotConfig,
        lane: &Lane,
        turno: i64,
        thread: String,
        total: Option<Contagem>,
    ) -> Result<()> {
        let atual = total.filter(|total| {
            total.depois_de(Contagem::default()).is_some_and(|uso| {
                [total.entrada, total.saida].into_iter().all(|n| i64::try_from(n).is_ok()) && uso != Uso::default()
            })
        });
        let medido = self
            .anterior
            .zip(atual)
            .and_then(|(base, total)| total.depois_de(base))
            .filter(|uso| *uso != Uso::default());
        let recibo = medido.map(|uso| {
            json!({ "usage": {
            "inputTokens": uso.entrada_sem_cache,
            "cachedReadTokens": uso.cache_lido,
            "cachedWriteTokens": uso.cache_gravado,
            "outputTokens": uso.saida,
        } })
        });
        let mut pendente =
            crate::fork::custo::Pending::begin(store, cfg, lane, &thread, false, None)?.with_turn(Some(turno));
        pendente.dispatched(store)?;
        pendente.finish(store, recibo.as_ref())?;
        if let Some(total) = atual.filter(|_| self.anterior.is_none() || recibo.is_some()) {
            store.kv_set_if_bot(&cfg.id, self.chave, &serde_json::to_string(&Amostra { thread, total })?)?;
        }
        Ok(())
    }
}

impl Contagem {
    pub(super) fn do_protocolo(value: &Value) -> Option<Self> {
        let contagem: Self = serde_json::from_value(value.clone()).ok()?;
        (contagem.entrada.checked_add(contagem.saida)? == value["totalTokens"].as_u64()?).then_some(contagem)
    }

    pub(super) fn depois_de(self, anterior: Self) -> Option<Uso> {
        let entrada = self.entrada.checked_sub(anterior.entrada)?;
        let cache_lido = self.cache_lido.checked_sub(anterior.cache_lido)?;
        let cache_gravado = self.cache_gravado.checked_sub(anterior.cache_gravado)?;
        let saida = self.saida.checked_sub(anterior.saida)?;
        let raciocinio = self.raciocinio.checked_sub(anterior.raciocinio)?;
        Some(Uso {
            entrada_sem_cache: entrada.checked_sub(cache_lido.checked_add(cache_gravado)?)?,
            cache_lido,
            cache_gravado,
            saida: (raciocinio <= saida).then_some(saida)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Contagem, Uso};

    #[test]
    fn uso_do_turno_separa_cache_sem_somar_raciocinio_duas_vezes() {
        let anterior = Contagem { entrada: 100, cache_lido: 40, cache_gravado: 10, saida: 30, raciocinio: 3 };
        let depois = Contagem { entrada: 160, cache_lido: 60, cache_gravado: 15, saida: 43, raciocinio: 8 };
        assert_eq!(
            depois.depois_de(anterior),
            Some(Uso { entrada_sem_cache: 35, cache_lido: 20, cache_gravado: 5, saida: 13 }),
        );
        assert_eq!(depois.depois_de(depois), Some(Uso::default()));
        assert!(anterior.depois_de(depois).is_none());
    }

    #[test]
    fn uso_incoerente_nao_vira_preco() {
        let base = Contagem::default();
        assert!(Contagem { entrada: 3, cache_lido: 4, ..base }.depois_de(base).is_none());
        assert!(Contagem { saida: 2, raciocinio: 3, ..base }.depois_de(base).is_none());
    }
}
