//! Planos de código ligados pela chave colada, com o OpenCode como motor.
//!
//! A chave do plano é do dono do Hub e fica cifrada no cofre, no slot do plano (`plano-codigo/<plano>`). Ela nunca
//! volta ao app, a registro, a log nem a mensagem de erro. «Testar acesso» só lê a lista de modelos do plano: nenhuma
//! conversa, nenhuma cobrança. Na hora de conversar, a chave é entregue só ao processo do OpenCode, em um perfil
//! próprio do plano (`perfil.rs`), e só a bot do Space pessoal cujo modelo é do plano.
mod acesso;
mod perfil;
#[cfg(test)]
mod tests;

pub(crate) use perfil::ambiente_do_bot;

use crate::{api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Métodos do app para os planos de código. Só este computador (`LOCAL_ONLY` e `require_local`).
pub(crate) const METHODS: [&str; 4] = ["planosCodigo", "planoCodigoTestar", "planoCodigoSalvar", "planoCodigoRemover"];

/// Um plano de código que o OpenCode conduz com a chave colada.
pub(crate) struct Plano {
    pub(crate) id: &'static str,
    pub(crate) nome: &'static str,
    /// Identificador do plano dentro do OpenCode: os modelos dele são `<provedor>/<modelo>`.
    pub(crate) provedor: &'static str,
    /// Variável por onde o OpenCode recebe a chave deste plano.
    pub(crate) variavel: &'static str,
    /// Endereço fixo (nunca vem do app) da lista de modelos que a chave enxerga.
    pub(crate) lista: &'static str,
    /// Avisos que a tela mostra junto do plano.
    pub(crate) avisos: &'static [&'static str],
}

pub(crate) const PLANOS: &[Plano] = &[Plano {
    id: "opencode-go",
    nome: "OpenCode Go",
    provedor: "opencode-go",
    variavel: "OPENCODE_API_KEY",
    lista: "https://opencode.ai/inference/go/v1/models",
    avisos: &["saldo_extra"],
}];

/// O plano a que um modelo do OpenCode (`provedor/modelo`) pertence.
pub(crate) fn plano_do_modelo(modelo: &str) -> Option<&'static Plano> {
    let (provedor, restante) = modelo.split_once('/')?;
    PLANOS.iter().find(|plano| plano.provedor == provedor && !restante.is_empty())
}

fn plano(id: &str) -> Result<&'static Plano> {
    PLANOS.iter().find(|plano| plano.id == id).ok_or_else(|| anyhow!("plano_invalido: plano desconhecido."))
}

fn slot_chave(plano: &Plano) -> String {
    format!("plano-codigo/{}", plano.id)
}

fn slot_modelos(plano: &Plano) -> String {
    format!("plano-codigo.modelos/{}", plano.id)
}

fn slot_revisao(plano: &Plano) -> String {
    format!("plano-codigo.revisao/{}", plano.id)
}

/// Uma operação de chave por vez: um teste lento não grava por cima de uma remoção ou troca feitas enquanto esperava.
static OPERACAO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const MAX_CHAVE: usize = 4096;
const MIN_CHAVE: usize = 8;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pedido {
    #[serde(default)]
    plano: Option<String>,
    #[serde(default)]
    chave: Option<String>,
    /// O app manda o Space aberto em todo pedido. A chave é do computador e do dono, então não muda nada aqui.
    #[serde(default, rename = "spaceId")]
    _space_id: Option<Value>,
}

/// A chave colada: sem espaços nas pontas, só ASCII visível, sem aspas nem barra invertida (assim ela aparece igual em
/// JSON e a máscara a acha), de tamanho plausível. O erro nunca a repete.
fn chave(texto: Option<&str>) -> Result<Zeroizing<String>> {
    let chave = texto.map(str::trim).unwrap_or_default();
    ensure!(
        (MIN_CHAVE..=MAX_CHAVE).contains(&chave.len())
            && chave.chars().all(|c| c.is_ascii_graphic() && c != '"' && c != '\\'),
        "chave_formato: a chave colada não parece uma chave de API."
    );
    Ok(Zeroizing::new(chave.to_owned()))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::fork::engine_setup::require_local(caller)?;
    let pedido: Pedido =
        serde_json::from_value(body).map_err(|_| anyhow!("pedido_invalido: pedido de plano de código inválido."))?;
    if method == "planosCodigo" {
        return listar(&hub.store);
    }
    let plano = plano(pedido.plano.as_deref().unwrap_or_default())?;
    match method {
        "planoCodigoTestar" => {
            if let Some(texto) = pedido.chave.as_deref() {
                return Ok(resultado(acesso::listar_modelos(plano.provedor, plano.lista, &chave(Some(texto))?).await));
            }
            let _operacao = OPERACAO.lock().await;
            crate::market::vault::unlock(hub.clone()).await?;
            let Some(guardada) = crate::market::vault::read(&hub.store, &slot_chave(plano))?.map(Zeroizing::new) else {
                return Ok(json!({"ok": false, "erro": "sem_chave", "estado": "nao_verificado"}));
            };
            let acesso = resultado(acesso::listar_modelos(plano.provedor, plano.lista, &guardada).await);
            guardar_modelos(&hub.store, plano, &acesso)?;
            Ok(acesso)
        }
        "planoCodigoSalvar" => salvar(hub, plano, plano.lista, chave(pedido.chave.as_deref())?).await,
        "planoCodigoRemover" => {
            let _operacao = OPERACAO.lock().await;
            let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
            remover(&hub.store, plano)?;
            Ok(json!({"ok": true}))
        }
        _ => bail!("{method} não é um método de plano de código"),
    }
}

/// O que o app vê do teste: `ok` com os modelos que a chave enxerga, ou o erro e o estado. A lista de modelos prova que
/// o fornecedor respondeu à chave, não que há saldo, assinatura ou permissão de conversar com cada modelo.
fn resultado(lista: Result<Vec<acesso::Modelo>, acesso::Falha>) -> Value {
    match lista {
        Ok(modelos) => json!({"ok": true, "estado": "modelos_no_catalogo", "modelos": modelos}),
        Err(falha) => json!({"ok": false, "erro": falha.codigo(), "estado": falha.estado()}),
    }
}

/// Grava a chave só se o fornecedor a aceitou; assim o cofre nunca guarda uma chave recusada.
async fn salvar(hub: &Arc<Hub>, plano: &'static Plano, lista: &str, candidata: Zeroizing<String>) -> Result<Value> {
    let _operacao = OPERACAO.lock().await;
    let acesso = resultado(acesso::listar_modelos(plano.provedor, lista, &candidata).await);
    if acesso["ok"] != true {
        return Ok(acesso);
    }
    crate::market::vault::unlock(hub.clone()).await?;
    let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
    crate::market::vault::write(&hub.store, &slot_chave(plano), &candidata)?;
    guardar_modelos(&hub.store, plano, &acesso)?;
    nova_revisao(&hub.store, plano)?;
    Ok(acesso)
}

/// Remove a chave, a lista de modelos e o perfil do plano. O próximo turno já não acha chave.
fn remover(store: &Store, plano: &Plano) -> Result<()> {
    store.kv_delete(&slot_chave(plano))?;
    store.kv_delete(&slot_modelos(plano))?;
    nova_revisao(store, plano)?;
    perfil::apagar(store, plano);
    Ok(())
}

/// Número que muda a cada troca ou remoção da chave: entra no carimbo da sessão e reinicia o processo do OpenCode.
fn revisao(store: &Store, plano: &Plano) -> Result<u64> {
    Ok(store.kv_read(&slot_revisao(plano))?.and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn nova_revisao(store: &Store, plano: &Plano) -> Result<()> {
    store.kv_set(&slot_revisao(plano), &(revisao(store, plano)? + 1).to_string())
}

fn guardar_modelos(store: &Store, plano: &Plano, acesso: &Value) -> Result<()> {
    if acesso["ok"] != true {
        return Ok(());
    }
    let guardado = json!({"modelos": acesso["modelos"], "conferidoEm": crate::store::now_ms()});
    store.kv_set(&slot_modelos(plano), &guardado.to_string())
}

fn modelos_guardados(store: &Store, plano: &Plano) -> Result<Option<Value>> {
    Ok(store.kv_read(&slot_modelos(plano))?.and_then(|raw| serde_json::from_str(&raw).ok()))
}

/// Os planos para a tela «Adicionar conexão», no grupo «Planos». Nunca a chave, nem parte dela.
pub(crate) fn listar(store: &Store) -> Result<Value> {
    let mut lista = Vec::new();
    for plano in PLANOS {
        let configurada = store.kv_read(&slot_chave(plano))?.is_some();
        let guardado = if configurada { modelos_guardados(store, plano)? } else { None };
        lista.push(json!({
            "id": plano.id,
            "nome": plano.nome,
            "grupo": "planos",
            "avisos": plano.avisos,
            "configurada": configurada,
            "programaInstalado": crate::agent::backends::which("opencode").is_some(),
            "modelos": guardado.as_ref().map(|g| g["modelos"].clone()),
            "conferidoEm": guardado.as_ref().map(|g| g["conferidoEm"].clone()),
        }));
    }
    Ok(json!({"planos": lista}))
}

/// Junta os modelos dos planos com chave ao catálogo do OpenCode (`agentModels`), sem repetir os que ele já lista.
pub(crate) fn acrescentar_modelos(store: &Store, catalogo: &mut Value) -> Result<()> {
    let Some(modelos) = catalogo["models"].as_array_mut() else { return Ok(()) };
    for plano in PLANOS {
        if store.kv_read(&slot_chave(plano))?.is_none() {
            continue;
        }
        for modelo in
            modelos_guardados(store, plano)?.and_then(|g| g["modelos"].as_array().cloned()).unwrap_or_default()
        {
            if !modelos.iter().any(|existente| existente["id"] == modelo["id"]) {
                modelos.push(
                    json!({"id": modelo["id"], "name": modelo["name"], "description": Value::Null, "options": []}),
                );
            }
        }
    }
    Ok(())
}
