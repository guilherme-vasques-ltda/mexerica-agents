//! Chaves de API do Motor Mexerica: cadastro, troca, remoção e «Testar acesso».
//!
//! A chave é do dono do Hub e só serve aos bots do Space pessoal dele. Fica cifrada no cofre, no slot do fornecedor
//! (`motor-mexerica/<fornecedor>`), e nunca volta ao app, a registro, a log nem a mensagem de erro. Só este
//! computador cadastra, testa ou remove. «Testar acesso» abre o motor preso ao fornecedor e só lista os modelos que
//! a chave enxerga: nenhuma conversa, nenhuma cobrança. A lista fica guardada (sem segredo) para a escolha do modelo.
use super::{Engine, VaultSource, credential_slot};
use crate::{api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

/// Fornecedores por chave, na ordem da tela: (id, nome).
pub(crate) const FORNECEDORES: [(&str, &str); 6] = [
    ("anthropic", "Anthropic"),
    ("openai", "OpenAI"),
    ("google", "Google"),
    ("xai", "xAI"),
    ("openrouter", "OpenRouter"),
    ("meta", "Meta"),
];

/// Erros do «Testar acesso» que o app sabe mostrar; qualquer outro vira `fornecedor_falhou`.
const ERROS: [&str; 7] = [
    "chave_invalida",
    "sem_permissao",
    "limite",
    "fornecedor_falhou",
    "sem_resposta",
    "resposta_invalida",
    "sem_chave",
];

/// O motor tem 30 s para o teste inteiro; aqui sobra folga para abrir e fechar o processo.
const PRAZO_DO_TESTE: Duration = Duration::from_secs(45);
const MAX_CHAVE: usize = 4096;

fn cache_slot(fornecedor: &str) -> String {
    format!("motor-mexerica.modelos/{fornecedor}")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pedido {
    #[serde(default)]
    fornecedor: Option<String>,
    #[serde(default)]
    chave: Option<String>,
}

fn fornecedor(pedido: &Pedido) -> Result<&'static str> {
    let id = pedido.fornecedor.as_deref().ok_or_else(|| anyhow!("fornecedor_invalido: informe o fornecedor."))?;
    FORNECEDORES
        .iter()
        .find(|(f, _)| *f == id)
        .map(|(f, _)| *f)
        .ok_or_else(|| anyhow!("fornecedor_invalido: fornecedor desconhecido."))
}

/// A chave colada: sem espaços nas pontas, sem espaço ou controle no meio, de tamanho plausível. O erro nunca a repete.
pub(super) fn chave(texto: Option<&str>) -> Result<Zeroizing<String>> {
    let chave = texto.map(str::trim).unwrap_or_default();
    ensure!(
        (super::link::MIN_SECRET_LEN..=MAX_CHAVE).contains(&chave.len())
            && !chave.chars().any(|c| c.is_whitespace() || c.is_control()),
        "chave_formato: a chave colada não parece uma chave de API."
    );
    Ok(Zeroizing::new(chave.to_owned()))
}

/// Métodos do app. Só este computador, e só com o motor ligado.
pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::fork::engine_setup::require_local(caller)?;
    ensure!(super::enabled(), "motor_desligado: o Motor Mexerica está desligado neste computador.");
    let pedido: Pedido =
        serde_json::from_value(body).map_err(|_| anyhow!("pedido_invalido: pedido de chave inválido."))?;
    if method == "motorChaves" {
        return listar(&hub.store);
    }
    let fornecedor = fornecedor(&pedido)?;
    crate::market::vault::unlock(hub.clone()).await?;
    match method {
        "motorChaveTestar" => {
            let candidata = pedido.chave.as_deref().map(|texto| chave(Some(texto))).transpose()?;
            let guardada = candidata.is_none();
            let acesso = testar(store_source(hub, candidata), fornecedor, &[]).await?;
            if guardada {
                guardar_modelos(&hub.store, fornecedor, &acesso)?;
            }
            Ok(acesso)
        }
        "motorChaveSalvar" => {
            let candidata = chave(pedido.chave.as_deref())?;
            salvar(hub, fornecedor, candidata, &[]).await
        }
        "motorChaveRemover" => {
            let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
            remover(&hub.store, fornecedor)?;
            Ok(json!({"ok": true}))
        }
        _ => bail!("{method} não é um método de chave do motor"),
    }
}

/// Fonte do cofre do teste: a chave colada (ainda não salva) ou a que já está no cofre.
fn store_source(hub: &Arc<Hub>, candidata: Option<Zeroizing<String>>) -> VaultSource {
    match candidata {
        Some(chave) => Arc::new(move |_slot: &str| Ok(Some(chave.to_string()))),
        None => super::hub_vault(hub.clone()),
    }
}

/// Grava a chave só se o teste com ela passou; assim o cofre nunca guarda uma chave recusada pelo fornecedor.
pub(crate) async fn salvar(
    hub: &Arc<Hub>,
    fornecedor: &str,
    candidata: Zeroizing<String>,
    extra_env: &[(&str, &str)],
) -> Result<Value> {
    let valor = candidata.clone();
    let acesso = testar(Arc::new(move |_slot: &str| Ok(Some(valor.to_string()))), fornecedor, extra_env).await?;
    if acesso["ok"] != true {
        return Ok(acesso);
    }
    let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
    crate::market::vault::write(&hub.store, &credential_slot(fornecedor), &candidata)?;
    guardar_modelos(&hub.store, fornecedor, &acesso)?;
    Ok(acesso)
}

/// Remove a chave e a lista de modelos. O próximo pedido do motor já não acha chave.
pub(crate) fn remover(store: &Store, fornecedor: &str) -> Result<()> {
    store.kv_delete(&credential_slot(fornecedor))?;
    store.kv_delete(&cache_slot(fornecedor))
}

/// Abre o motor preso a `fornecedor` com a fonte dada e pede só a lista de modelos. O resultado é refeito campo a
/// campo: nada do que o motor mandou passa adiante sem ser conferido.
pub(crate) async fn testar(fonte: VaultSource, fornecedor: &str, extra_env: &[(&str, &str)]) -> Result<Value> {
    let mut engine = Engine::spawn_with(fonte, fornecedor, extra_env)?;
    let resposta = tokio::time::timeout(PRAZO_DO_TESTE, engine.list_models()).await;
    let _ = engine.kill().await;
    let dados = match resposta {
        Err(_) => return Ok(json!({"ok": false, "erro": "sem_resposta"})),
        Ok(resposta) => resposta?,
    };
    if dados["ok"] != true {
        let erro = dados["erro"].as_str().filter(|erro| ERROS.contains(erro)).unwrap_or("fornecedor_falhou");
        let mut falha = json!({"ok": false, "erro": erro});
        if let Some(status) = dados["status"].as_u64().filter(|s| (100..600).contains(s)) {
            falha["status"] = status.into();
        }
        return Ok(falha);
    }
    let modelos: Vec<Value> = dados["modelos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id =
                m["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control))?;
            let nome: String = m["name"].as_str().unwrap_or(id).chars().filter(|c| !c.is_control()).take(120).collect();
            Some(json!({"id": format!("{fornecedor}/{id}"), "name": nome}))
        })
        .take(2000)
        .collect();
    Ok(json!({"ok": true, "modelos": modelos}))
}

fn guardar_modelos(store: &Store, fornecedor: &str, acesso: &Value) -> Result<()> {
    if acesso["ok"] != true {
        return Ok(());
    }
    let guardado = json!({"modelos": acesso["modelos"], "conferidoEm": crate::store::now_ms()});
    store.kv_set(&cache_slot(fornecedor), &guardado.to_string())
}

fn modelos_guardados(store: &Store, fornecedor: &str) -> Result<Option<Value>> {
    Ok(store.kv_read(&cache_slot(fornecedor))?.and_then(|raw| serde_json::from_str(&raw).ok()))
}

/// O que o app vê: quais fornecedores têm chave e os modelos do último teste. Nunca a chave, nem parte dela.
pub(crate) fn listar(store: &Store) -> Result<Value> {
    let mut lista = Vec::new();
    for (id, nome) in FORNECEDORES {
        let configurada = store.kv_read(&credential_slot(id))?.is_some();
        let guardado = if configurada { modelos_guardados(store, id)? } else { None };
        lista.push(json!({
            "id": id,
            "nome": nome,
            "configurada": configurada,
            "modelos": guardado.as_ref().map(|g| g["modelos"].clone()),
            "conferidoEm": guardado.as_ref().map(|g| g["conferidoEm"].clone()),
        }));
    }
    Ok(json!({"fornecedores": lista}))
}

/// Catálogo da escolha de modelo do bot (`agentModels` do backend `mexerica`): os modelos do último teste de cada
/// fornecedor com chave, no formato `fornecedor/modelo`.
pub(crate) fn catalogo(store: &Store) -> Result<Value> {
    let mut modelos = Vec::new();
    for (id, _) in FORNECEDORES {
        if store.kv_read(&credential_slot(id))?.is_none() {
            continue;
        }
        for modelo in modelos_guardados(store, id)?.and_then(|g| g["modelos"].as_array().cloned()).unwrap_or_default() {
            modelos
                .push(json!({"id": modelo["id"], "name": modelo["name"], "description": Value::Null, "options": []}));
        }
    }
    Ok(json!({"models": modelos, "currentModelId": Value::Null, "fastMode": false}))
}

/// O cofre do motor para um bot: a chave do dono só sai para bot do Space pessoal, conferido a cada leitura (o bot
/// pode ter mudado de Space ou sido apagado desde que a conexão abriu).
pub(crate) fn owner_vault(hub: Arc<Hub>, bot: &str) -> VaultSource {
    let bot = bot.to_owned();
    Arc::new(move |slot: &str| {
        if !bot_pessoal(&hub.store, &bot)? {
            return Ok(None);
        }
        crate::market::vault::read(&hub.store, slot)
    })
}

/// O bot existe, não foi apagado e é do Space pessoal do dono, fora de contexto de grupo ou rotina de outro Space.
pub(super) fn bot_pessoal(store: &Store, bot: &str) -> Result<bool> {
    Ok(store.bot(bot)?.is_some_and(|row| pessoal(&row)))
}

pub(super) fn pessoal(row: &crate::store::BotRow) -> bool {
    !row.deleted && row.config.space_id == crate::fork::spaces::PERSONAL && row.config.context.is_none()
}
