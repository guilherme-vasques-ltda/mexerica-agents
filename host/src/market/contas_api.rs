//! Marketplace 1a: métodos do Hub para o app (catálogo, ficha, contas, conectar, renomear, desconectar e vínculo por
//! bot). O app só renderiza e manda intenção; nome, Space, escopos e liberações são decididos e conferidos aqui.
//! Contrato para a tela (fatia 1b): `docs/features/marketplace.md`, seção "API do Marketplace".
use super::contas::{self, Pedido};
use crate::agent::bot::Cmd;
use crate::api::devices::Caller;
use crate::fork::conectores::catalogo_servicos as catalogo;
use crate::hub::Hub;
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub(crate) const METHODS: &[&str] = &[
    "marketCatalogo",
    "marketServico",
    "marketContas",
    "marketConectar",
    "marketPersonalizado",
    "marketContaRenomear",
    "marketContaDesconectar",
    "marketBotContas",
    "marketBotContasDefinir",
    "marketFerramentas",
    "marketFerramentaLigar",
    "marketFerramentaDesligar",
    "marketGoogleApp",
    "marketGoogleAppSalvar",
    "marketGoogleAppRemover",
];

fn texto<'a>(b: &'a Value, campo: &str) -> Result<&'a str> {
    b[campo].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("`{campo}` is required"))
}

/// Campo opcional de texto: ausente ou `null` valem "não informado"; outro tipo é erro (nada de adivinhar).
fn texto_opcional<'a>(b: &'a Value, campo: &str) -> Result<Option<&'a str>> {
    match b.get(campo) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => bail!("`{campo}` must be text"),
    }
}

/// A ficha como o app a lê: os dados do catálogo mais o que depende do momento e do Space.
fn ficha(
    servico: &catalogo::Servico,
    hoje: chrono::NaiveDate,
    contas: Option<&[super::Connector]>,
    space: &str,
) -> Value {
    let mut v = serde_json::to_value(servico).unwrap_or(Value::Null);
    v["conectavel"] = json!(servico.conectavel() && servico.conferencia_vigente(hoje));
    v["conferenciaVigente"] = json!(servico.conferencia_vigente(hoje));
    v["diasDesdeConferencia"] = json!(catalogo::dias_desde_conferencia(servico.conferida_em, hoje));
    v["contas"] = json!(contas.map(|todas| {
        todas
            .iter()
            .filter(|c| c.servico.as_deref() == Some(servico.id) && c.space == space)
            .map(|c| json!({"id": c.id, "name": c.name, "identidade": c.identidade}))
            .collect::<Vec<_>>()
    }));
    v
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, b: &Value) -> Result<Option<Value>> {
    if !METHODS.contains(&method) {
        return Ok(None);
    }
    let store = &hub.store;
    let space = contas::space_do_pedido(store, b)?;
    let hoje = catalogo::hoje();
    Ok(Some(match method {
        // O catálogo é dado público; a lista de contas do Space só aparece com o cofre aberto.
        "marketCatalogo" => {
            let todas = super::connectors(store).ok();
            let catalogo = catalogo::publico();
            json!({
                "versao": catalogo.versao,
                "prazoConferenciaDias": catalogo.prazo_conferencia_dias,
                "categorias": catalogo.categorias,
                "hoje": hoje.to_string(),
                "spaceId": space,
                // O app OAuth da própria pessoa, por Space: só o ID, nunca a chave. Cofre trancado = `null`.
                "appsProprios": {"google": super::google_app::publico(store, &space)},
                "servicos": catalogo.servicos.iter().map(|s| ficha(s, hoje, todas.as_deref(), &space)).collect::<Vec<_>>(),
            })
        }
        "marketServico" => {
            let servico = catalogo::servico(texto(b, "id")?).ok_or_else(|| anyhow!("servico_desconhecido"))?;
            let todas = super::connectors(store).ok();
            json!({"servico": ficha(servico, hoje, todas.as_deref(), &space), "spaceId": space})
        }
        "marketContas" => contas::listar(store, &space)?,
        "marketConectar" => {
            let liberacoes = match b.get("liberacoes") {
                None | Some(Value::Null) => None,
                Some(Value::Array(itens)) => Some(
                    itens
                        .iter()
                        .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| anyhow!("`liberacoes` must be text")))
                        .collect::<Result<Vec<_>>>()?,
                ),
                Some(_) => bail!("`liberacoes` must be a list"),
            };
            let pedido = Pedido {
                servico: texto(b, "servico")?,
                tipo: texto(b, "tipo")?,
                space: &space,
                nome: texto_opcional(b, "nome")?,
                nome_primeira: texto_opcional(b, "nomePrimeira")?,
                liberacoes,
            };
            contas::conectar(store, &pedido)?
        }
        // MCP do próprio usuário, por link ou comando: o mesmo caminho do `installConnector` (Space do pedido, nome único
        // por Space, nasce desligado em todos os bots), sem ficha, marcado "seu, não verificado". O bloqueio das ações
        // perigosas dele é da fatia 2.
        "marketPersonalizado" => {
            ensure!(b.get("registryName").is_none_or(Value::is_null), "{}", super::REGISTRY_UNAVAILABLE);
            let conta = super::add_custom_connector(store, b).await?;
            let proximo = if conta["auth"] == "none" { "verificar" } else { "entrar" };
            json!({"connector": conta, "proximoPasso": proximo})
        }
        "marketGoogleApp" => super::google_app::publico(store, &space)
            .ok_or_else(|| anyhow!("cofre_trancado: destrave o cofre para ver o app Google."))?,
        // A chave secreta entra aqui uma vez, vinda do cartão seguro do app, e a resposta só devolve o ID.
        "marketGoogleAppSalvar" => {
            let resposta = super::google_app::salvar(store, &space, texto(b, "clientId")?, texto(b, "clientSecret")?)?;
            atualizar_bots_do_space(hub, &space);
            resposta
        }
        "marketGoogleAppRemover" => {
            let resposta = super::google_app::remover(store, &space)?;
            atualizar_bots_do_space(hub, &space);
            resposta
        }
        "marketContaRenomear" => contas::renomear(store, texto(b, "id")?, texto(b, "nome")?, &space)?,
        "marketContaDesconectar" => {
            let resultado = contas::desconectar(store, texto(b, "id")?, &space)?;
            for bot in resultado["bots"].as_array().into_iter().flatten().filter_map(|v| v["id"].as_str()) {
                if let Err(error) = hub.send_cmd(bot, Cmd::RefreshTools) {
                    tracing::warn!(%error, bot, "conta desconectada; atualização das ferramentas do bot pendente");
                }
            }
            resultado
        }
        "marketBotContas" => contas::vinculos(store, texto(b, "botId")?)?,
        "marketBotContasDefinir" => definir_vinculo(hub, caller, b).await?,
        "marketFerramentas" => super::ferramentas::listar(store, texto(b, "botId")?, texto(b, "conta")?)?,
        "marketFerramentaLigar" => ligar_ferramenta(hub, caller, b)?,
        "marketFerramentaDesligar" => desligar_ferramenta(hub, caller, b)?,
        _ => unreachable!(),
    }))
}

/// Fatia 2: a PESSOA liga uma ação perigosa (ou nova, ou alterada) de uma conta num bot, com confirmação, presa ao
/// digest que ela viu. Só o dono, com a autoridade original reconferida na transação; o bot e o conteúdo externo
/// nunca chegam aqui (`Caller::Ponte` não tem autoridade de controle).
fn ligar_ferramenta(hub: &Arc<Hub>, caller: &Caller, b: &Value) -> Result<Value> {
    crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    let pedido = super::ferramentas::PedidoLigar {
        bot: texto(b, "botId")?,
        conta: texto(b, "conta")?,
        ferramenta: texto(b, "ferramenta")?,
        digest: texto(b, "digest")?,
    };
    let vinculo = b["vinculo"]
        .as_i64()
        .ok_or_else(|| anyhow!("vinculo_obrigatorio: abra a lista de ações de novo e confirme outra vez."))?;
    super::ferramentas::conferir_ligar(&hub.store, &pedido, b["confirmo"] == true)?;
    crate::fork::donna_fixa::pedido::gravar(&hub.store, |tx| {
        super::ferramentas::conferir_vinculo(tx, pedido.bot, pedido.conta, vinculo)?;
        super::ferramentas::gravar_liberacao(tx, &pedido)
    })?;
    if let Err(error) = hub.send_cmd(pedido.bot, Cmd::RefreshTools) {
        tracing::warn!(%error, bot = pedido.bot, "ação ligada; atualização das ferramentas do bot pendente");
    }
    super::ferramentas::listar(&hub.store, pedido.bot, pedido.conta)
}

/// Desligar não precisa de confirmação (é o lado seguro), mas segue só do dono.
fn desligar_ferramenta(hub: &Arc<Hub>, caller: &Caller, b: &Value) -> Result<Value> {
    crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    let (bot, conta, ferramenta) = (texto(b, "botId")?, texto(b, "conta")?, texto(b, "ferramenta")?);
    crate::fork::donna_fixa::pedido::gravar(&hub.store, |tx| {
        super::ferramentas::gravar_desligamento(tx, bot, conta, ferramenta)
    })?;
    if let Err(error) = hub.send_cmd(bot, Cmd::RefreshTools) {
        tracing::warn!(%error, bot, "ação desligada; atualização das ferramentas do bot pendente");
    }
    super::ferramentas::listar(&hub.store, bot, conta)
}

/// Liga/desliga contas, escolhe a conta padrão e o "perguntar qual conta" de UM bot. Passa pelo caminho do dono
/// (`update_bot_by_user_authorized`): o Berga e os principais de Space seguem protegidos pelos gatilhos, e o Hub
/// recusa conta de outro Space (`validar_vinculo`).
async fn definir_vinculo(hub: &Arc<Hub>, caller: &Caller, b: &Value) -> Result<Value> {
    let bot = texto(b, "botId")?;
    let lista = |campo: &str| -> Result<Vec<String>> {
        match b.get(campo) {
            None | Some(Value::Null) => Ok(vec![]),
            Some(Value::Array(itens)) => itens
                .iter()
                .map(|v| {
                    v.as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| anyhow!("`{campo}` must be text"))
                })
                .collect(),
            Some(_) => bail!("`{campo}` must be a list"),
        }
    };
    let (ligar, desligar) = (lista("ligar")?, lista("desligar")?);
    ensure!(!ligar.iter().any(|id| desligar.contains(id)), "ligar_e_desligar: a mesma conta nas duas listas.");
    let objeto = |campo: &str| -> Result<Option<&Map<String, Value>>> {
        match b.get(campo) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(m)) => Ok(Some(m)),
            Some(_) => bail!("`{campo}` must be an object"),
        }
    };
    let (padrao, perguntar) = (objeto("padrao")?, objeto("perguntar")?);
    let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    let _guard = hub.store.team_lock.lock().await;
    crate::chat::manage::recover_live(hub);
    let row = hub.store.bot(bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
    ensure!(!row.config.is_group(), "grupo não usa contas de serviço");
    let mut conectores: Vec<String> =
        row.config.connectors.iter().filter(|id| !desligar.contains(id)).cloned().collect();
    for id in ligar {
        if !conectores.contains(&id) {
            conectores.push(id);
        }
    }
    let mut conta_padrao = row.config.conta_padrao.clone();
    for (servico, conta) in padrao.into_iter().flatten() {
        match conta {
            Value::Null => conta_padrao.remove(servico),
            Value::String(id) => conta_padrao.insert(servico.clone(), id.clone()),
            _ => bail!("`padrao` must map a service to an account id or null"),
        };
    }
    let mut perguntar_conta = row.config.perguntar_conta.clone();
    for (servico, quer) in perguntar.into_iter().flatten() {
        perguntar_conta.insert(
            servico.clone(),
            quer.as_bool().ok_or_else(|| anyhow!("`perguntar` must map a service to true or false"))?,
        );
    }
    let patch =
        json!({"id": bot, "connectors": conectores, "contaPadrao": conta_padrao, "perguntarConta": perguntar_conta});
    hub.update_bot_by_user_authorized(&patch, None, Some(&owner))?;
    if let Err(error) = hub.send_cmd(bot, Cmd::RefreshTools) {
        tracing::warn!(%error, bot, "vínculo salvo; atualização das ferramentas do bot pendente");
    }
    contas::vinculos(&hub.store, bot)
}

/// O app Google trocado ou removido encerrou o login das contas Google do Space: os bots do Space refazem a lista de
/// ferramentas (sem a conta entrando, as ferramentas dela somem na hora).
fn atualizar_bots_do_space(hub: &Arc<Hub>, space: &str) {
    let Ok(bots) = hub.store.bots() else { return };
    for bot in bots.into_iter().filter(|b| !b.deleted && !b.config.is_group() && b.config.space_id == space) {
        if let Err(error) = hub.send_cmd(&bot.config.id, Cmd::RefreshTools) {
            tracing::warn!(%error, bot = bot.config.id, "app Google alterado; atualização das ferramentas pendente");
        }
    }
}
