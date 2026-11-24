//! D395 (DON-917): o idioma escolhido no app por cada pessoa/aparelho, para o Berga responder nele.
//!
//! O app manda `donnaAppLanguageSave {language}` ao conectar e a cada troca. O Hub valida contra os seis
//! idiomas do app e guarda uma chave por chamador: `local` (o computador do Hub, que é o idioma do dono) e
//! uma por aparelho. Cada chamador grava só o próprio idioma; um aparelho sem Control também, porque é
//! preferência dele e não dá poder sobre a Donna. Ferramentas de bot (Ponte), rotinas e pareamento não
//! gravam. Na mensagem humana, o Hub carimba `appLanguage` a partir do idioma guardado de quem mandou
//! (nunca do corpo do pedido). No turno do Berga, vale o carimbo da última mensagem humana; sem ele
//! (canal, `WhatsApp`, rotina, remetente desconhecido), o idioma do dono; sem esse, português do Brasil.
use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

pub(crate) const METHOD: &str = "donnaAppLanguageSave";

/// Os idiomas do app (`apps/desktop/src/shared/fork-idioma.ts`), com o nome que vai ao modelo.
pub(crate) const IDIOMAS: [(&str, &str); 6] = [
    ("pt-BR", "português do Brasil"),
    ("en", "English"),
    ("es", "español"),
    ("fr", "français"),
    ("zh-CN", "简体中文"),
    ("ar-AE", "العربية"),
];
const PADRAO: &str = "pt-BR";
const LOCAL: &str = "donna.appLanguage.local";
/// Prefixo da chave de cada aparelho; apagada junto com o aparelho (`Store::remove_device`).
pub(crate) const APARELHO: &str = "donna.appLanguage.device.";

fn valido(raw: &str) -> Option<&'static str> {
    IDIOMAS.iter().map(|(codigo, _)| *codigo).find(|codigo| *codigo == raw)
}

fn chave(caller: &Caller) -> Option<String> {
    match caller {
        Caller::Local => Some(LOCAL.to_owned()),
        Caller::Device { key, .. } => Some(format!("{APARELHO}{key}")),
        Caller::Ponte { .. } | Caller::Pairing { .. } | Caller::Bootstrap { .. } => None,
    }
}

/// `donnaAppLanguageSave`: grava só o idioma do próprio chamador, sob a autoridade original do
/// transporte e na mesma Tx da conferência; o aparelho grava junto a geração da concessão.
pub(crate) fn call(hub: &Hub, caller: &Caller, body: &Value) -> Result<Value> {
    let chave = chave(caller).ok_or_else(|| anyhow!("forbidden: só o app grava o próprio idioma"))?;
    let campos = body.as_object().ok_or_else(|| anyhow!("invalid_app_language"))?;
    ensure!(campos.keys().all(|k| k == "language" || k == "spaceId"), "invalid_app_language");
    let idioma = body["language"].as_str().and_then(valido).ok_or_else(|| anyhow!("invalid_app_language"))?;
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "app_language_write");
    crate::fork::autoridade::Autoridade::proprio_with_db(hub, caller, |tx, geracao| {
        let valor = json!({"language": idioma, "generation": geracao}).to_string();
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            [chave.as_str(), valor.as_str()],
        )?;
        Ok(())
    })?;
    Ok(json!({"language": idioma}))
}

/// O valor guardado: idioma válido e a geração com que foi gravado.
fn guardado(store: &Store, chave: &str) -> Option<(&'static str, Option<i64>)> {
    let raw = store.kv_read(chave).ok().flatten()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    Some((v["language"].as_str().and_then(valido)?, v["generation"].as_i64()))
}

/// O idioma guardado de quem chama. Do aparelho, só com a geração ATUAL da concessão: revogado ou
/// concessão nova na mesma chave não herdam o valor antigo.
pub(crate) fn do_chamador(store: &Store, caller: &Caller) -> Option<&'static str> {
    match caller {
        Caller::Local => guardado(store, LOCAL).map(|(idioma, _)| idioma),
        Caller::Device { key, .. } => do_aparelho(store, key),
        Caller::Ponte { .. } | Caller::Pairing { .. } | Caller::Bootstrap { .. } => None,
    }
}

/// O idioma guardado de um aparelho (`device_key`), só com a geração ATUAL da concessão: revogado,
/// desconhecido ou concessão nova na mesma chave não herdam o valor antigo. Mesma regra de
/// `do_chamador` para `Caller::Device`; usada também pelo push, que conhece o aparelho pelo ticket.
pub(crate) fn do_aparelho(store: &Store, key: &str) -> Option<&'static str> {
    let (idioma, geracao) = guardado(store, &format!("{APARELHO}{key}"))?;
    let atual = {
        let db = store.db.locked();
        crate::fork::autoridade::Autoridade::geracao_atual(&db, key)
    };
    (store.device(key).is_some() && geracao.is_some() && geracao == atual).then_some(idioma)
}

/// D395: carimba na mensagem humana o idioma guardado de quem mandou; nunca o do corpo (um
/// `appLanguage` que tenha vindo junto sai). Os envios (`send`, `donnaConversationSend`) passam por aqui
/// antes de gravar; a resposta ao cartão carimba com `do_chamador` no `extra` server-owned.
pub(crate) fn carimbar(store: &Store, caller: &Caller, data: &mut Value) {
    if let Some(campos) = data.as_object_mut() {
        campos.remove("appLanguage");
    }
    if let Some(idioma) = do_chamador(store, caller) {
        data["appLanguage"] = json!(idioma);
    }
}

/// Mensagem humana para o idioma: a escrita pelo app e a resposta ao cartão de escolha.
fn humana(e: &crate::store::Entry) -> bool {
    e.is_user_authored()
        || (e.kind == crate::store::EntryKind::User.as_str() && crate::fork::genui::is_choice_response(&e.data))
}

/// O idioma do dono: o do app no computador do Hub; sem ele, português do Brasil.
pub(crate) fn do_dono(store: &Store) -> &'static str {
    guardado(store, LOCAL).map_or(PADRAO, |(idioma, _)| idioma)
}

/// O idioma de um turno da Donna. `canal`: turno de canal/grupo, que segue o dono (D395, regra 3).
pub(crate) fn do_turno(store: &Store, entries: &[String], canal: bool) -> &'static str {
    let ultima = (!canal).then(|| entries.iter().rev().filter_map(|id| store.entry(id)).find(humana));
    ultima.flatten().and_then(|e| e.data["appLanguage"].as_str().and_then(valido)).unwrap_or_else(|| do_dono(store))
}

/// A linha do turno, só para o Berga (D395); os outros bots seguem como antes.
pub(crate) fn turno(store: &Store, cfg: &BotConfig, entries: &[String], canal: bool) -> String {
    if !crate::fork::donna_fixa::is_donna_here(store, cfg) {
        return String::new();
    }
    let codigo = do_turno(store, entries, canal);
    let nome = IDIOMAS.iter().find(|(c, _)| *c == codigo).map_or(codigo, |(_, n)| *n);
    format!("<system-reminder>\nIdioma da resposta neste turno: {nome} ({codigo}).\n</system-reminder>\n\n")
}
