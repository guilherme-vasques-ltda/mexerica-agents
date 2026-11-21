//! Chaves de API do Motor Mexerica: cadastro, troca, remoção e «Testar acesso».
//!
//! A chave é do dono do Hub e só serve aos bots do Space pessoal dele. Fica cifrada no cofre, no slot do fornecedor
//! (`motor-mexerica/<fornecedor>`), e nunca volta ao app, a registro, a log nem a mensagem de erro. Só este
//! computador cadastra, testa ou remove. «Testar acesso» abre o motor preso ao fornecedor e só lista os modelos que
//! a chave enxerga: nenhuma conversa, nenhuma cobrança. A lista fica guardada (sem segredo) para a escolha do modelo.
use super::proprio::{self, Formato, Registro};
use super::{Engine, LimitsSource, VaultSource, credential_slot};
use crate::{api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Context, Result, anyhow, bail, ensure};
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
const ERROS: [&str; 9] = [
    "sem_lista",
    "destino_recusado",
    "chave_invalida",
    "sem_permissao",
    "limite",
    "fornecedor_falhou",
    "sem_resposta",
    "resposta_invalida",
    "sem_chave",
];

/// Uma operação de chave por vez (salvar, remover, testar a chave guardada): um teste lento não pode gravar por cima de
/// uma remoção ou de uma troca feitas enquanto ele esperava o fornecedor.
static OPERACAO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// O motor tem 30 s para o teste inteiro; aqui sobra folga para abrir e fechar o processo.
const PRAZO_DO_TESTE: Duration = Duration::from_secs(45);
const MAX_CHAVE: usize = 4096;
/// Limites de cada modelo que o «Testar acesso» guarda e a conversa usa: contexto e teto de saída, em tokens.
const LIMITES: [&str; 2] = ["contextWindow", "maxTokens"];
const MAX_LIMITE: u64 = 100_000_000;

fn cache_slot(fornecedor: &str) -> String {
    format!("motor-mexerica.modelos/{fornecedor}")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Pedido {
    #[serde(default)]
    pub(super) fornecedor: Option<String>,
    #[serde(default)]
    pub(super) chave: Option<String>,
    /// Endereço próprio (`motorEnderecoSalvar`): id ao trocar um existente, nome, endereço, formato, se é local e o
    /// modelo digitado para serviço que não lista modelos.
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) nome: Option<String>,
    #[serde(default)]
    pub(super) url: Option<String>,
    #[serde(default)]
    pub(super) formato: Option<Formato>,
    #[serde(default)]
    pub(super) local: Option<bool>,
    #[serde(default)]
    pub(super) modelo: Option<String>,
    /// O app manda o Space aberto em todo pedido (já conferido pela guarda de Spaces). A chave é do computador e do
    /// dono, então o Space não muda nada aqui.
    #[serde(default, rename = "spaceId")]
    _space_id: Option<Value>,
}

/// Um fornecedor por chave da lista ou, quando `proprios` vale, um endereço próprio cadastrado.
fn fornecedor(store: &Store, pedido: &Pedido, proprios: bool) -> Result<String> {
    let id = pedido.fornecedor.as_deref().ok_or_else(|| anyhow!("fornecedor_invalido: informe o fornecedor."))?;
    let conhecido = FORNECEDORES.iter().any(|(f, _)| *f == id) || (proprios && proprio::existe(store, id)?);
    ensure!(conhecido, "fornecedor_invalido: fornecedor desconhecido.");
    Ok(id.to_owned())
}

/// A chave colada: sem espaços nas pontas, sem espaço ou controle no meio, de tamanho plausível. O erro nunca a repete.
pub(super) fn chave(texto: Option<&str>) -> Result<Zeroizing<String>> {
    let chave = texto.map(str::trim).unwrap_or_default();
    // Só ASCII visível, sem aspas nem barra invertida: a chave aparece igual em JSON e o saneamento do motor a acha.
    ensure!(
        (super::link::MIN_SECRET_LEN..=MAX_CHAVE).contains(&chave.len())
            && chave.chars().all(|c| c.is_ascii_graphic() && c != '"' && c != '\\'),
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
    crate::market::vault::unlock(hub.clone()).await?;
    if method == "motorChaves" {
        return listar(&hub.store);
    }
    if method == "motorEnderecoSalvar" {
        return salvar_endereco(hub, pedido, &[]).await;
    }
    match method {
        "motorChaveTestar" => {
            let fornecedor = fornecedor(&hub.store, &pedido, true)?;
            if let Some(texto) = pedido.chave.as_deref() {
                // Chave colada só para os fornecedores da lista; o endereço próprio testa com o que está guardado.
                ensure!(!proprio::eh_proprio(&fornecedor), "pedido_invalido: use o cadastro do endereço próprio.");
                return testar(store_source(hub, Some(chave(Some(texto))?)), &fornecedor, &[]).await;
            }
            let _operacao = OPERACAO.lock().await;
            let acesso = testar(store_source(hub, None), &fornecedor, &[]).await?;
            guardar_modelos(&hub.store, &fornecedor, &acesso)?;
            Ok(acesso)
        }
        "motorChaveSalvar" => {
            let fornecedor = fornecedor(&hub.store, &pedido, false)?;
            let candidata = chave(pedido.chave.as_deref())?;
            salvar(hub, &fornecedor, candidata, &[]).await
        }
        "motorChaveRemover" => {
            let fornecedor = fornecedor(&hub.store, &pedido, true)?;
            let _operacao = OPERACAO.lock().await;
            let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
            remover(&hub.store, &fornecedor)?;
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
    let _operacao = OPERACAO.lock().await;
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

/// Remove a chave e a lista de modelos (e o endereço, se for próprio). O próximo pedido do motor já não acha chave.
pub(crate) fn remover(store: &Store, fornecedor: &str) -> Result<()> {
    if proprio::eh_proprio(fornecedor) {
        proprio::retirar(store, fornecedor)?;
    }
    store.kv_delete(&credential_slot(fornecedor))?;
    store.kv_delete(&cache_slot(fornecedor))
}

/// Cadastra ou troca um endereço próprio. Testa com o registro novo antes de gravar: salva quando o serviço lista
/// modelos, ou quando não tem lista e a pessoa digitou o modelo (acesso ainda não confirmado). Chave recusada, destino
/// recusado ou serviço que não respondeu: nada é gravado.
pub(super) async fn salvar_endereco(hub: &Arc<Hub>, pedido: Pedido, extra_env: &[(&str, &str)]) -> Result<Value> {
    let _operacao = OPERACAO.lock().await;
    let local = pedido.local.unwrap_or(false);
    let nome = proprio::conferir_nome(pedido.nome.as_deref().unwrap_or_default())?;
    let base_url = proprio::conferir_url(pedido.url.as_deref().unwrap_or_default(), local)?;
    let formato = pedido.formato.ok_or_else(|| anyhow!("formato_invalido: escolha o formato do serviço."))?;
    let modelo = pedido.modelo.as_deref().filter(|m| !m.trim().is_empty()).map(proprio::conferir_modelo).transpose()?;
    let (id, key) = match pedido.id {
        Some(id) => {
            ensure!(proprio::existe(&hub.store, &id)?, "fornecedor_invalido: endereço próprio desconhecido.");
            let key = if let Some(texto) = pedido.chave.as_deref() {
                chave(Some(texto))?
            } else {
                let registro = proprio::registro(&hub.store, &id)?.context("endereço próprio sem registro")?;
                // A chave guardada só serve ao destino para o qual ela foi aceita. Trocar o endereço (ou o
                // formato, ou o modo local) sem informar a chave de novo mandaria o segredo antigo ao
                // destino novo antes de qualquer gravação: recusado.
                ensure!(
                    registro.base_url == base_url && registro.formato == formato && registro.local == local,
                    "chave_obrigatoria: o destino mudou; informe a chave de novo."
                );
                Zeroizing::new(registro.key.clone())
            };
            (id, key)
        }
        None => (proprio::novo_id(), chave(pedido.chave.as_deref())?),
    };
    let raw = proprio::serializar(&Registro { key: key.to_string(), base_url, formato, local, nome })?;
    let fonte = raw.clone();
    let mut acesso = testar(Arc::new(move |_slot: &str| Ok(Some(fonte.to_string()))), &id, extra_env).await?;
    if acesso["ok"] != true {
        let Some(modelo) = modelo.filter(|_| acesso["erro"] == "sem_lista") else { return Ok(acesso) };
        acesso = json!({"ok": true, "estado": "acesso_nao_confirmado", "modelos": [{"id": format!("{id}/{modelo}"), "name": modelo}]});
    }
    let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
    proprio::incluir(&hub.store, &id)?;
    crate::market::vault::write(&hub.store, &credential_slot(&id), &raw)?;
    guardar_modelos(&hub.store, &id, &acesso)?;
    acesso["id"] = id.into();
    Ok(acesso)
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
        // Os estados da tela: só o fornecedor recusar a chave é «recusada»; sem lista, o acesso só não foi confirmado.
        let estado = match erro {
            "chave_invalida" => "recusada",
            "sem_lista" => "acesso_nao_confirmado",
            _ => "nao_verificado",
        };
        let mut falha = json!({"ok": false, "erro": erro, "estado": estado});
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
            let mut modelo = json!({"id": format!("{fornecedor}/{id}"), "name": nome});
            for campo in LIMITES {
                if let Some(n) = m[campo].as_u64().filter(|n| (1..=MAX_LIMITE).contains(n)) {
                    modelo[campo] = n.into();
                }
            }
            Some(modelo)
        })
        .take(2000)
        .collect();
    Ok(json!({"ok": true, "estado": "modelos_no_catalogo", "modelos": modelos}))
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

/// Os limites de `fornecedor/modelo` no último «Testar acesso» (`{}` se não há): sem eles o motor usaria os
/// padrões do fornecedor, e um teto de saída maior que o do modelo faz o fornecedor recusar o pedido.
pub(crate) fn limites(store: &Store, fornecedor: &str, modelo: &str) -> Result<Value> {
    let id = format!("{fornecedor}/{modelo}");
    let guardado = modelos_guardados(store, fornecedor)?;
    let achado = guardado.as_ref().and_then(|g| g["modelos"].as_array()?.iter().find(|m| m["id"] == id.as_str()));
    let mut limites = json!({});
    for campo in LIMITES {
        if let Some(n) = achado.and_then(|m| m[campo].as_u64()).filter(|n| (1..=MAX_LIMITE).contains(n)) {
            limites[campo] = n.into();
        }
    }
    Ok(limites)
}

/// A fonte de limites que o adaptador do motor consulta ao escolher o modelo.
pub(crate) fn fonte_de_limites(hub: Arc<Hub>) -> LimitsSource {
    Arc::new(move |fornecedor: &str, modelo: &str| limites(&hub.store, fornecedor, modelo))
}

/// O que o app vê: quais fornecedores têm chave e os modelos do último teste. Nunca a chave, nem parte dela.
pub(crate) fn listar(store: &Store) -> Result<Value> {
    let mut lista = Vec::new();
    let mut enderecos = Vec::new();
    for id in proprio::ids(store)? {
        let Some(registro) = proprio::registro(store, &id)? else { continue };
        let guardado = modelos_guardados(store, &id)?;
        let mut publico = proprio::publico(&id, &registro);
        publico["configurada"] = true.into();
        publico["modelos"] = guardado.as_ref().map(|g| g["modelos"].clone()).into();
        publico["conferidoEm"] = guardado.as_ref().map(|g| g["conferidoEm"].clone()).into();
        enderecos.push(publico);
    }
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
    Ok(json!({"fornecedores": lista, "enderecos": enderecos}))
}

/// Catálogo da escolha de modelo do bot (`agentModels` do backend `mexerica`): os modelos do último teste de cada
/// fornecedor com chave, no formato `fornecedor/modelo`.
pub(crate) fn catalogo(store: &Store, space: &str) -> Result<Value> {
    let mut modelos = Vec::new();
    // A chave é do dono e só serve ao Space pessoal.
    let pessoal = space == crate::fork::spaces::PERSONAL;
    let ids = FORNECEDORES.iter().map(|(id, _)| (*id).to_owned()).chain(proprio::ids(store)?);
    for id in ids.filter(|_| pessoal) {
        if store.kv_read(&credential_slot(&id))?.is_none() {
            continue;
        }
        for modelo in modelos_guardados(store, &id)?.and_then(|g| g["modelos"].as_array().cloned()).unwrap_or_default()
        {
            modelos
                .push(json!({"id": modelo["id"], "name": modelo["name"], "description": Value::Null, "options": []}));
        }
    }
    Ok(json!({"models": modelos, "currentModelId": Value::Null, "fastMode": false}))
}

/// O cofre do motor para uma execução do bot: a chave do dono só sai se esta execução é do Space pessoal, sem contexto
/// de outro Space ou conversa (o contexto existe só na configuração em memória, nunca no banco), e se o bot gravado
/// continua pessoal e não apagado, conferido a cada leitura.
pub(crate) fn owner_vault(hub: Arc<Hub>, cfg: &crate::store::BotConfig) -> VaultSource {
    let bot = cfg.id.clone();
    let execucao_pessoal = cfg.context.is_none() && cfg.space_id == crate::fork::spaces::PERSONAL;
    Arc::new(move |slot: &str| {
        if !execucao_pessoal || !bot_pessoal(&hub.store, &bot)? {
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
