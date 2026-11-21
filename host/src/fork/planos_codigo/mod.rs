//! Planos de código ligados pela chave colada, com o OpenCode como motor.
//!
//! A chave do plano é do dono do Hub e fica cifrada no cofre, no slot do plano (`plano-codigo/<plano>`). Ela nunca
//! volta ao app, a registro, a log nem a mensagem de erro. «Testar acesso» só lê a lista de modelos do plano: nenhuma
//! conversa, nenhuma cobrança. Na hora de conversar, a chave NÃO entra no processo do OpenCode: ele recebe um código
//! de sessão e o endereço do repasse local do Hub (`repasse.rs`), que troca o código pela chave ao falar com o
//! fornecedor. O processo roda num perfil próprio do plano (`perfil.rs`), só para bot do Space pessoal cujo modelo é do
//! plano.
mod acesso;
mod cerca;
mod mascara;
mod perfil;
pub(crate) use mascara::{mascarar_codigo, mascarar_json, mascarar_saida};
pub(crate) mod repasse;
#[cfg(test)]
mod repasse_tests;
#[cfg(test)]
mod tests;

pub(crate) use perfil::{ambiente_do_bot, aplicar_ambiente};
pub(crate) use repasse::{Concessao, Repasse};

use crate::{LockExt, api::devices::Caller, hub::Hub, store::Store};
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
    /// Os únicos caminhos que o repasse encaminha, com o endereço fixo do fornecedor para cada um: `(caminho local,
    /// endereço)`. Um `*` casa um segmento (o modelo) e vale também no endereço.
    pub(crate) rotas: &'static [(&'static str, &'static str)],
    /// Avisos que a tela mostra junto do plano.
    pub(crate) avisos: &'static [&'static str],
}

pub(crate) const PLANOS: &[Plano] = &[Plano {
    id: "opencode-go",
    nome: "OpenCode Go",
    provedor: "opencode-go",
    variavel: "OPENCODE_API_KEY",
    lista: "https://opencode.ai/inference/go/v1/models",
    rotas: &[
        ("chat/completions", "https://opencode.ai/inference/go/openai/v1/chat/completions"),
        ("responses", "https://opencode.ai/inference/go/openai/v1/responses"),
        ("messages", "https://opencode.ai/inference/go/anthropic/v1/messages"),
        ("models/*:generateContent", "https://opencode.ai/inference/go/google/v1beta/models/*:generateContent"),
        (
            "models/*:streamGenerateContent",
            "https://opencode.ai/inference/go/google/v1beta/models/*:streamGenerateContent",
        ),
    ],
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

/// Uma operação de chave por vez: um teste lento não grava por cima de outra gravação feita enquanto esperava.
static OPERACAO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const MAX_CHAVE: usize = 512;
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
            bail!("plano_chave_ja_salva: Trocar ou remover a chave do plano ainda não está disponível nesta versão.")
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

/// Grava a chave só se o fornecedor a aceitou; assim o cofre nunca guarda uma chave recusada. Com chave já salva no
/// plano, colar outra é recusado sem tocar em nada do que está salvo (nem testar a chave nova na rede): trocar ou
/// remover ainda não está disponível nesta versão.
async fn salvar(hub: &Arc<Hub>, plano: &'static Plano, lista: &str, candidata: Zeroizing<String>) -> Result<Value> {
    ensure!(!cfg!(windows), "plano_indisponivel: os planos de código só funcionam em Linux e macOS nesta versão.");
    let _operacao = OPERACAO.lock().await;
    // Falha rápido, antes de qualquer teste na rede: com chave salva, colar outra não muda nada.
    ensure!(
        hub.store.kv_read(&slot_chave(plano))?.is_none(),
        "plano_chave_ja_salva: Trocar ou remover a chave do plano ainda não está disponível nesta versão."
    );
    let acesso = resultado(acesso::listar_modelos(plano.provedor, lista, &candidata).await);
    if acesso["ok"] != true {
        return Ok(acesso);
    }
    crate::market::vault::unlock(hub.clone()).await?;
    let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
    // A mesma checagem outra vez, já com o cofre e a guarda da mutação: uma chave gravada no meio não é pisada.
    ensure!(
        hub.store.kv_read(&slot_chave(plano))?.is_none(),
        "plano_chave_ja_salva: Trocar ou remover a chave do plano ainda não está disponível nesta versão."
    );
    gravar(&hub.store, plano, &candidata, &acesso)?;
    Ok(acesso)
}

/// Chave, lista de modelos e revisão na mesma transação: uma falha no meio não deixa uma chave nova com a revisão velha.
fn gravar(store: &Store, plano: &Plano, chave: &str, acesso: &Value) -> Result<()> {
    let selada = crate::market::vault::seal(store, &slot_chave(plano), chave)?;
    let modelos = json!({"modelos": acesso["modelos"], "conferidoEm": crate::store::now_ms()}).to_string();
    let proxima = (revisao(store, plano)? + 1).to_string();
    let mut db = store.db.locked();
    let transacao = db.transaction()?;
    crate::market::vault::write_sealed(&transacao, &slot_chave(plano), &selada)?;
    for (slot, valor) in [(slot_modelos(plano), &modelos), (slot_revisao(plano), &proxima)] {
        transacao.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
            rusqlite::params![slot, valor],
        )?;
    }
    transacao.commit()?;
    Ok(())
}

/// O repasse local deste Hub, iniciado na primeira vez que alguém precisa dele.
pub(crate) async fn repasse_do_hub(hub: &Arc<Hub>) -> Result<Arc<Repasse>> {
    Ok(hub.planos.get_or_try_init(Repasse::iniciar).await?.clone())
}

/// Número que muda a cada gravação da chave: entra no carimbo da sessão e reinicia o processo do OpenCode.
fn revisao(store: &Store, plano: &Plano) -> Result<u64> {
    Ok(store.kv_read(&slot_revisao(plano))?.and_then(|v| v.parse().ok()).unwrap_or(0))
}

/// Atualiza só a lista de modelos (um novo «Testar acesso» com a chave guardada); a chave e a revisão não mudam.
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
