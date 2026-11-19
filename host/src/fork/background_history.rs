//! Histórico limitado do trabalhador; não guarda prompts, respostas ou erros brutos.
use crate::{
    LockExt,
    store::{Store, now_ms},
};
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

const SLOT: &str = "fork.background-history.v1";
const LIMIT: usize = 50;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Function {
    Memory,
    Titles,
    /// MB-3: resumo do dia na assinatura Claude do próprio bot.
    DailySummary,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    ResponseReceived,
    Applied,
    Unchanged,
    Proposed,
    Deferred,
    Failed,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureStage {
    Selection,
    Account,
    Usage,
    Execution,
    Revalidation,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    #[serde(default)]
    id: String,
    function: Function,
    #[serde(default)]
    bot_id: String,
    status: Status,
    at: i64,
    #[serde(default)]
    effective_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
    #[serde(default)]
    failure_stage: Option<FailureStage>,
}

/// Forma interna. `None` = campo ausente na resposta do motor (não zero);
/// a distinção é preservada no armazenamento pelas flags `cacheLidoAusente`/
/// `cacheEscritoAusente` de `UsageDisco` — os campos legados seguem numéricos
/// (`0` quando ausente) para não quebrar um Hub anterior que lê o mesmo slot.
#[derive(Clone, Serialize, Deserialize)]
#[serde(from = "UsageDisco", into = "UsageDisco")]
pub(crate) struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached_input: Option<u64>,
    pub cached_write: Option<u64>,
    /// Campo de cache presente mas não representável (sem preço, nunca zero).
    pub cache_invalid: bool,
}

/// Forma persistida no slot `fork.background-history.v1`, compatível com o
/// Hub anterior: `cachedInputTokens`/`cachedWriteTokens` sempre números
/// (`0` quando ausente, como a versão antiga gravava) e as flags novas
/// opcionais — `Usage` antigo não tem `deny_unknown_fields`, então o Hub da
/// main as ignora; um Hub novo lê a flag e reconstrói o `None`. Registro
/// antigo sem flag → `Some(valor legado)`, como antes.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageDisco {
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cached_write_tokens: u64,
    #[serde(default)]
    cache_lido_ausente: bool,
    #[serde(default)]
    cache_escrito_ausente: bool,
    #[serde(default)]
    cache_invalid: bool,
}

impl From<Usage> for UsageDisco {
    fn from(u: Usage) -> Self {
        Self {
            input_tokens: u.input,
            output_tokens: u.output,
            cached_input_tokens: u.cached_input.unwrap_or(0),
            cached_write_tokens: u.cached_write.unwrap_or(0),
            cache_lido_ausente: u.cached_input.is_none(),
            cache_escrito_ausente: u.cached_write.is_none(),
            cache_invalid: u.cache_invalid,
        }
    }
}

impl From<UsageDisco> for Usage {
    fn from(d: UsageDisco) -> Self {
        Self {
            input: d.input_tokens,
            output: d.output_tokens,
            cached_input: (!d.cache_lido_ausente).then_some(d.cached_input_tokens),
            cached_write: (!d.cache_escrito_ausente).then_some(d.cached_write_tokens),
            cache_invalid: d.cache_invalid,
        }
    }
}

/// Forma da resposta ao desktop: idêntica a `Entry`, mas com as contagens de
/// cache como números — o parser do app rejeita `null`. Ausente/interno
/// `None` vira `0` apenas aqui.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryTela {
    id: String,
    function: Function,
    bot_id: String,
    status: Status,
    at: i64,
    effective_models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsageTela>,
    failure_stage: Option<FailureStage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageTela {
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cached_write_tokens: u64,
    cache_invalid: bool,
}

pub(crate) fn read(store: &Store) -> Result<Vec<Entry>> {
    let raw = store.kv_read(SLOT)?;
    raw.as_deref().map_or(Ok(Vec::new()), |s| Ok(serde_json::from_str(s)?))
}

/// Histórico para a resposta RPC ao desktop: cache ausente/interno `None`
/// vira `0` (o parser do app exige número; o Hub nunca emite `null` aqui).
pub(crate) fn read_para_tela(store: &Store) -> Result<Vec<EntryTela>> {
    Ok(read(store)?
        .into_iter()
        .map(|e| EntryTela {
            id: e.id,
            function: e.function,
            bot_id: e.bot_id,
            status: e.status,
            at: e.at,
            effective_models: e.effective_models,
            usage: e.usage.map(|u| UsageTela {
                input_tokens: u.input,
                output_tokens: u.output,
                cached_input_tokens: u.cached_input.unwrap_or(0),
                cached_write_tokens: u.cached_write.unwrap_or(0),
                cache_invalid: u.cache_invalid,
            }),
            failure_stage: e.failure_stage,
        })
        .collect())
}

pub(crate) fn record(
    store: &Store,
    function: Function,
    bot_id: &str,
    status: Status,
    effective_models: Vec<String>,
    usage: Option<Usage>,
    failure_stage: Option<FailureStage>,
) -> Result<String> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let raw: Option<String> = tx.query_row("SELECT v FROM kv WHERE k = ?1", [SLOT], |row| row.get(0)).optional()?;
    let mut history: Vec<Entry> = raw.as_deref().map_or(Ok(Vec::new()), serde_json::from_str)?;
    let id = uuid::Uuid::new_v4().to_string();
    history.push(Entry {
        id: id.clone(),
        function,
        bot_id: bot_id.into(),
        status,
        at: now_ms(),
        effective_models,
        usage,
        failure_stage,
    });
    let excess = history.len().saturating_sub(LIMIT);
    history.drain(..excess);
    tx.execute(
        "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        rusqlite::params![SLOT, serde_json::to_string(&history)?],
    )?;
    tx.commit()?;
    Ok(id)
}

/// MEX-1355: a mesma falha de seleção repetida só atualiza o horário da linha
/// existente. `false` se a linha já saiu do histórico (limite de 50).
pub(crate) fn touch(store: &Store, id: &str) -> Result<bool> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let raw: Option<String> = tx.query_row("SELECT v FROM kv WHERE k = ?1", [SLOT], |row| row.get(0)).optional()?;
    let mut history: Vec<Entry> = raw.as_deref().map_or(Ok(Vec::new()), serde_json::from_str)?;
    let Some(entry) = history.iter_mut().find(|entry| {
        entry.id == id
            && matches!(entry.status, Status::Failed)
            && matches!(entry.failure_stage, Some(FailureStage::Selection))
    }) else {
        return Ok(false);
    };
    entry.at = now_ms();
    tx.execute("UPDATE kv SET v = ?2 WHERE k = ?1", rusqlite::params![SLOT, serde_json::to_string(&history)?])?;
    tx.commit()?;
    Ok(true)
}

pub(crate) fn finish(store: &Store, id: &str, status: Status) -> Result<()> {
    finish_with_stage(store, id, status, None)
}

pub(crate) fn finish_with_stage(store: &Store, id: &str, status: Status, stage: Option<FailureStage>) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let raw: Option<String> = tx.query_row("SELECT v FROM kv WHERE k = ?1", [SLOT], |row| row.get(0)).optional()?;
    let mut history: Vec<Entry> = raw.as_deref().map_or(Ok(Vec::new()), serde_json::from_str)?;
    if let Some(entry) = history.iter_mut().find(|entry| entry.id == id) {
        entry.status = status;
        entry.failure_stage =
            if matches!(status, Status::Failed) { Some(stage.unwrap_or(FailureStage::Execution)) } else { None };
        tx.execute("UPDATE kv SET v = ?2 WHERE k = ?1", rusqlite::params![SLOT, serde_json::to_string(&history)?])?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn historico_limitado_preserva_modelo_confirmado_e_falhas() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for _ in 0..LIMIT {
            record(&store, Function::Memory, "bot", Status::ResponseReceived, vec![], None, None).unwrap();
        }
        record(
            &store,
            Function::Titles,
            "bot",
            Status::Failed,
            vec!["modelo-confirmado".into()],
            None,
            Some(FailureStage::Execution),
        )
        .unwrap();
        let value = serde_json::to_value(read(&store).unwrap()).unwrap();
        let entries = value.as_array().unwrap();
        assert_eq!(entries.len(), LIMIT);
        assert_eq!(entries[LIMIT - 1]["function"], "titles");
        assert_eq!(entries[LIMIT - 1]["status"], "failed");
        assert_eq!(entries[LIMIT - 1]["effectiveModels"][0], "modelo-confirmado");
        assert_eq!(entries[0]["effectiveModels"], serde_json::json!([]));
    }

    /// MX-159/P2-2 + P3 (rodadas 3-5): o armazenamento mantém os campos
    /// legados NUMÉRICOS (`0` quando ausente — compatível com o Hub anterior)
    /// e marca a verdade nas flags `cacheLidoAusente`/`cacheEscritoAusente`;
    /// só a resposta ao app (numérica por contrato) e a leitura interna usam
    /// esses dados — o Hub nunca emite `null` para o desktop.
    #[test]
    fn historico_guarda_flags_e_serializa_cache_ausente_como_zero_na_tela() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        record(
            &store,
            Function::Titles,
            "bot",
            Status::ResponseReceived,
            vec![],
            Some(Usage { input: 60, output: 50, cached_input: None, cached_write: None, cache_invalid: false }),
            None,
        )
        .unwrap();
        let stored = store.kv_read(SLOT).unwrap().unwrap();
        let stored_usage = &serde_json::from_str::<serde_json::Value>(&stored).unwrap()[0]["usage"];
        assert_eq!(stored_usage["cachedInputTokens"], serde_json::json!(0));
        assert_eq!(stored_usage["cachedWriteTokens"], serde_json::json!(0));
        assert_eq!(stored_usage["cacheLidoAusente"], serde_json::json!(true));
        assert_eq!(stored_usage["cacheEscritoAusente"], serde_json::json!(true));
        // A leitura interna reconstrói a ausência; a tela vê 0.
        let interno = read(&store).unwrap();
        assert_eq!(interno[0].usage.as_ref().unwrap().cached_input, None);
        let value = serde_json::to_value(read_para_tela(&store).unwrap()).unwrap();
        let usage = &value[0]["usage"];
        assert_eq!(usage["cachedInputTokens"], serde_json::json!(0));
        assert_eq!(usage["cachedWriteTokens"], serde_json::json!(0));
    }

    /// Rodada 5/P2: o struct `Usage` do Hub ANTERIOR (main, campos `u64`)
    /// deve desserializar o JSON gravado pelo Hub novo — flags novas são
    /// ignoradas (o struct antigo não tem `deny_unknown_fields`) e os campos
    /// legados seguem numéricos. Espelho fiel de
    /// `origin/main:host/src/fork/background_history.rs`.
    #[test]
    fn hub_anterior_le_o_historico_gravado_pelo_hub_novo() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct UsageAntigo {
            #[serde(rename = "inputTokens")]
            #[allow(dead_code)]
            input: u64,
            #[serde(rename = "outputTokens")]
            #[allow(dead_code)]
            output: u64,
            #[serde(rename = "cachedInputTokens")]
            cached_input: u64,
            #[serde(rename = "cachedWriteTokens")]
            cached_write: u64,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct EntryAntigo {
            #[serde(default)]
            usage: Option<UsageAntigo>,
        }
        let store = Store::open(Path::new(":memory:")).unwrap();
        record(
            &store,
            Function::Titles,
            "bot",
            Status::ResponseReceived,
            vec![],
            Some(Usage { input: 60, output: 50, cached_input: None, cached_write: Some(7), cache_invalid: false }),
            None,
        )
        .unwrap();
        let stored = store.kv_read(SLOT).unwrap().unwrap();
        let entries: Vec<EntryAntigo> = serde_json::from_str(&stored).unwrap();
        let usage = entries[0].usage.as_ref().unwrap();
        // Hub anterior vê números: 0 para ausente, o valor real para presente.
        assert_eq!(usage.cached_input, 0);
        assert_eq!(usage.cached_write, 7);
        // E o Hub novo recupera a verdade das flags.
        let interno = read(&store).unwrap();
        let usage_novo = interno[0].usage.as_ref().unwrap();
        assert_eq!(usage_novo.cached_input, None);
        assert_eq!(usage_novo.cached_write, Some(7));
    }

    #[test]
    fn falha_de_selecao_repetida_atualiza_a_mesma_linha() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = record(&store, Function::Memory, "bot", Status::Failed, vec![], None, Some(FailureStage::Selection))
            .unwrap();
        let ok = record(&store, Function::Memory, "bot", Status::ResponseReceived, vec![], None, None).unwrap();
        assert!(touch(&store, &id).unwrap());
        assert!(!touch(&store, &ok).unwrap(), "só linha de falha de seleção");
        assert!(!touch(&store, "sumiu").unwrap());
        assert_eq!(read(&store).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn historico_fica_no_canal_local() {
        use crate::{api::devices::Caller, hub::Hub, remote::identity::Identity, store::Scope};
        let dir = std::env::temp_dir().join(format!("donnabot-fundo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        record(&store, Function::Memory, "bot", Status::Failed, vec![], None, Some(FailureStage::Usage)).unwrap();
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
        let remote = Caller::Device { key: "outro".into(), scopes: vec![Scope::Control], autoridade: None };
        assert!(crate::api::dispatch(&hub, &remote, "backgroundHistory", serde_json::json!({})).await.is_err());
        let result =
            crate::api::dispatch(&hub, &Caller::Local, "backgroundHistory", serde_json::json!({})).await.unwrap();
        assert_eq!(result[0]["failureStage"], "usage");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
