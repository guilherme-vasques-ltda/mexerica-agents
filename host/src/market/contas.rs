//! Marketplace, fatia 1a: contas nomeadas por Space, conexão pela ficha, renomear, desconectar, vínculo bot × conta
//! e a escolha da conta em cada pedido. Desenho: `docs/propostas/marketplace-20261113/conexoes-contas-skills.md`.
//!
//! Regras que o Hub aplica sozinho (a tela só mostra):
//! - toda conta tem um Space, conferido na criação, no vínculo com o bot e na execução;
//! - nome único por serviço e Space; a 2ª conta de um serviço exige nome para a nova e para a 1ª;
//! - escopos, URL e liberações vêm da ficha do catálogo, nunca do pedido do app;
//! - a conta de cada pedido é escolhida pelo Hub a partir da mensagem da PESSOA, nunca de texto externo.
use super::{
    Connector,
    contas_recibos::{self, Acao},
    oauth, reserved_connector_ids, unique_id, update_connectors_com,
};
use crate::fork::conectores::catalogo_servicos::{self as catalogo, Auth, Servico, TipoConexao};
use crate::fork::conectores::oauth_binding::Binding;
use crate::fork::spaces::PERSONAL;
use crate::store::{BotConfig, Store};
use anyhow::{Result, anyhow, bail, ensure};
use chrono::NaiveDate;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const NOME_MAX: usize = 60;

// MARK: Space

/// O Space do pedido. O `guard` do dispatch já confere `spaceId` e `hubId`; sem `spaceId` vale o Pessoal.
/// Aqui só se repete a existência, porque o chamador pode não ter passado pelo dispatch (testes, TUI).
pub(crate) fn space_do_pedido(store: &Store, body: &Value) -> Result<String> {
    let space = match body.get("spaceId") {
        None => PERSONAL,
        Some(v) => v.as_str().ok_or_else(|| anyhow!("space_invalido"))?,
    };
    store.require_space(space)?;
    Ok(space.to_owned())
}

/// Recusa estável (`principal_sem_contas`) de qualquer ligação de conta de serviço no bot principal. O app a lê pelo código.
pub(crate) const PRINCIPAL_SEM_CONTAS: &str = "principal_sem_contas: o bot principal do Space não usa contas.";

/// O principal do Space: o Berga do Pessoal (`team.settings`) ou o principal de um Space (`principalBotId`). Sempre pelo
/// id gravado no Hub, nunca pelo nome.
fn eh_principal(store: &Store, id: &str) -> Result<bool> {
    Ok(store.team_settings()?.primary_bot_id.as_deref() == Some(id)
        || store.spaces()?.spaces.iter().any(|s| s.principal_bot_id.as_deref() == Some(id)))
}

fn conta_de_outro_space() -> anyhow::Error {
    anyhow!("conta_de_outro_space: essa conta pertence a outro Space.")
}

// MARK: nomes

/// Forma comparável de um nome: sem acento, caixa nem pontuação ("Gmail – Pessoal" = "gmail pessoal").
pub(crate) fn dobrar(texto: &str) -> String {
    let limpo: String = texto
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    limpo.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn nome_valido(bruto: &str) -> Result<String> {
    let nome = bruto.trim();
    ensure!(
        !nome.is_empty() && nome.chars().count() <= NOME_MAX && !nome.chars().any(char::is_control),
        "nome_invalido: use de 1 a {NOME_MAX} caracteres, sem quebra de linha."
    );
    ensure!(!dobrar(nome).is_empty(), "nome_invalido: o nome precisa ter letras ou números.");
    Ok(nome.to_owned())
}

/// Nome único por serviço e Space (`servico: None` = MCPs do próprio usuário). `exceto` é a conta que está
/// sendo renomeada.
pub(crate) fn nome_livre(
    all: &[Connector],
    servico: Option<&str>,
    space: &str,
    nome: &str,
    exceto: Option<&str>,
) -> Result<()> {
    let quer = dobrar(nome);
    let repetido = all.iter().any(|c| {
        c.servico.as_deref() == servico && c.space == space && Some(c.id.as_str()) != exceto && dobrar(&c.name) == quer
    });
    ensure!(
        !repetido,
        "nome_repetido: There's already a connector named “{}”. Give this one a name that tells them apart.",
        nome.trim()
    );
    Ok(())
}

fn do_servico<'a>(all: &'a [Connector], servico: &str, space: &str) -> Vec<&'a Connector> {
    all.iter().filter(|c| c.servico.as_deref() == Some(servico) && c.space == space).collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Nomes {
    nova: String,
    /// `(id, novo nome)` da 1ª conta, quando a 2ª chega e ela ainda usa o nome do serviço.
    primeira: Option<(String, String)>,
}

/// Regra de nomes do dono (16/11/2026): 1ª conta do serviço no Space sem nome (usa o do serviço); na 2ª, nome para
/// a nova E para a 1ª; da 3ª em diante, sempre nome. Sempre únicos por serviço e Space.
fn planejar_nomes(
    all: &[Connector],
    ficha: &Servico,
    space: &str,
    nome: Option<&str>,
    nome_primeira: Option<&str>,
) -> Result<Nomes> {
    let existentes = do_servico(all, ficha.id, space);
    let nome = nome.map(nome_valido).transpose()?;
    match existentes.as_slice() {
        [] => {
            let nova = nome.unwrap_or_else(|| ficha.nome.to_owned());
            nome_livre(all, Some(ficha.id), space, &nova, None)?;
            Ok(Nomes { nova, primeira: None })
        }
        [primeira] => {
            let nova = nome.ok_or_else(|| {
                anyhow!("nome_obrigatorio: dê um nome à nova conta de {} para diferenciá-la da primeira.", ficha.nome)
            })?;
            let ainda_padrao = dobrar(&primeira.name) == dobrar(ficha.nome);
            let primeira_nome = match (nome_primeira.map(nome_valido).transpose()?, ainda_padrao) {
                (Some(escolhido), _) => Some(escolhido),
                (None, true) => bail!(
                    "nome_primeira_obrigatorio: dê também um nome à primeira conta de {}, hoje chamada só “{}”.",
                    ficha.nome,
                    primeira.name
                ),
                (None, false) => None,
            };
            if let Some(novo) = &primeira_nome {
                nome_livre(all, Some(ficha.id), space, novo, Some(&primeira.id))?;
                ensure!(
                    dobrar(novo) != dobrar(&nova),
                    "nome_repetido: as duas contas de {} precisam de nomes diferentes.",
                    ficha.nome
                );
            }
            nome_livre(all, Some(ficha.id), space, &nova, None)?;
            Ok(Nomes { primeira: primeira_nome.map(|n| (primeira.id.clone(), n)), nova })
        }
        _ => {
            let nova = nome.ok_or_else(|| {
                anyhow!("nome_obrigatorio: dê um nome à nova conta de {} para diferenciá-la das outras.", ficha.nome)
            })?;
            nome_livre(all, Some(ficha.id), space, &nova, None)?;
            Ok(Nomes { nova, primeira: None })
        }
    }
}

// MARK: conectar pela ficha

pub(crate) struct Pedido<'a> {
    pub servico: &'a str,
    pub tipo: &'a str,
    pub space: &'a str,
    pub nome: Option<&'a str>,
    pub nome_primeira: Option<&'a str>,
    pub liberacoes: Option<Vec<String>>,
    pub aceita_credencial_com_escrita: bool,
}

/// Recusa estável de `conectar`/login de uma ficha com app próprio (Google) quando o Space ainda não cadastrou o app.
/// O app lê o código `app_proprio_ausente` e abre o guia de 6 passos.
pub(crate) const APP_PROPRIO_AUSENTE: &str =
    "app_proprio_ausente: cadastre o app Google deste Space (guia de 6 passos) antes de conectar.";

/// Conectar pela ficha do catálogo.
pub(crate) fn conectar(store: &Store, pedido: &Pedido<'_>) -> Result<Value> {
    let ficha = catalogo::servico(pedido.servico)
        .ok_or_else(|| anyhow!("servico_desconhecido: não há ficha para `{}`.", pedido.servico))?;
    conectar_em(store, ficha, pedido, catalogo::hoje())
}

/// Mesma conexão sobre uma ficha dada e um "hoje" dado (testes de vencimento e de chave).
pub(crate) fn conectar_em(
    store: &Store,
    ficha: &'static Servico,
    pedido: &Pedido<'_>,
    hoje: NaiveDate,
) -> Result<Value> {
    let tipo = ficha.exigir_conectavel(pedido.tipo, hoje)?;
    let liberacoes = tipo.liberacoes_validas(pedido.liberacoes.as_deref())?;
    ensure!(
        !tipo.exige_filtro(&liberacoes) || pedido.aceita_credencial_com_escrita,
        "credencial_com_escrita: use uma versão do app que mostre o aviso de que esta conta pode alterar dados."
    );
    store.require_space(pedido.space)?;
    let mut conta = Connector {
        id: String::new(),
        name: String::new(),
        description: ficha.descricao.to_owned(),
        registry_name: None,
        command: None,
        args: vec![],
        env: std::collections::BTreeMap::default(),
        url: (tipo.tipo != catalogo::Tipo::Local).then(|| tipo.url_efetiva(&liberacoes).to_owned()),
        headers: std::collections::BTreeMap::default(),
        oauth: None,
        oauth_binding: None,
        servico: Some(ficha.id.to_owned()),
        tipo: Some(tipo.id.to_owned()),
        liberacoes,
        space: pedido.space.to_owned(),
        identidade: None,
    };
    let proximo = match &tipo.auth {
        Auth::Oauth { .. } => {
            conta.oauth = Some(oauth::OAuth::default());
            "entrar"
        }
        Auth::Chave { destino: "header", nome, .. } => {
            // O valor nunca passa por aqui: o app o entrega ao cofre pelo cartão seguro.
            conta.headers.insert((*nome).to_owned(), String::new());
            "chave"
        }
        Auth::Chave { .. } => bail!("tipo_indisponivel: só chave em cabeçalho é aceita nesta versão."),
        // O app OAuth é o da própria pessoa: sem ele cadastrado neste Space o app abre o guia. Só o ID e a chave do Space
        // entram na conta no login (`oauth::start`), nunca aqui.
        Auth::AppProprio { provedor: "google", .. } => {
            ensure!(super::google_app::ler(store, pedido.space)?.is_some(), "{APP_PROPRIO_AUSENTE}");
            conta.oauth = Some(oauth::OAuth::default());
            "entrar"
        }
        Auth::AppProprio { .. } => {
            use super::google_app::{APP_DA_FICHA_AUSENTE, AppDaFicha};
            let app = AppDaFicha::novo(ficha, tipo).ok_or_else(|| anyhow!("tipo_indisponivel"))?;
            ensure!(app.ler(store, pedido.space)?.is_some(), "{APP_DA_FICHA_AUSENTE}");
            conta.oauth = Some(oauth::OAuth::default());
            "entrar"
        }
    };
    let (publico, _) = update_connectors_com(
        store,
        |all| {
            // Recalculado sob o `connector_lock`: duas conexões simultâneas não furam a regra de nomes.
            let nomes = planejar_nomes(all, ficha, pedido.space, pedido.nome, pedido.nome_primeira)?;
            if let Some((id, novo)) = &nomes.primeira
                && let Some(primeira) = all.iter_mut().find(|c| &c.id == id)
            {
                primeira.name.clone_from(novo);
            }
            conta.name = nomes.nova;
            conta.id = unique_id(ficha.id, &reserved_connector_ids(store, all)?);
            let binding = Binding::new();
            let instancia = binding.instance().to_owned();
            conta.oauth_binding = Some(binding);
            let publico = conta.public();
            all.push(conta);
            Ok((publico, instancia))
        },
        // O recibo nasce na mesma transação que sela a conta no cofre.
        |tx, (_, instancia)| {
            contas_recibos::registrar_em(tx, Acao::Conectar, instancia, Some(ficha.id), pedido.space, "ok")
        },
    )?;
    Ok(json!({"connector": publico, "proximoPasso": proximo}))
}

// MARK: a ficha que a conta usa

/// O jeito de conectar da ficha de uma conta. `None` = MCP do próprio usuário (sem ficha). Conta de ficha que sumiu do
/// catálogo falha fechado: nunca cai no comportamento "peça tudo".
pub(crate) fn tipo_da_conta(c: &Connector) -> Result<Option<&'static TipoConexao>> {
    let Some(servico) = c.servico.as_deref() else { return Ok(None) };
    let tipo = c.tipo.as_deref().ok_or_else(|| anyhow!("ficha_indisponivel: conta sem jeito de conectar."))?;
    let tipo = catalogo::servico(servico)
        .and_then(|ficha| ficha.tipo(tipo))
        .ok_or_else(|| anyhow!("ficha_indisponivel: a ficha dessa conta não está mais no catálogo."))?;
    ensure!(
        if tipo.tipo == catalogo::Tipo::Local {
            crate::fork::conectores::catalogo_local::da_conta(c).is_some()
        } else {
            c.url.as_deref() == Some(tipo.url_efetiva(&c.liberacoes))
        },
        "ficha_indisponivel: o endereço da conta não confere com a ficha."
    );
    Ok(Some(tipo))
}

pub(crate) fn desktop_catalog(c: &Connector) -> bool {
    c.command.is_none() && tipo_da_conta(c).is_ok_and(|tipo| tipo.is_some())
}

pub(crate) fn auth_da_conta(c: &Connector) -> Result<Option<&'static Auth>> {
    Ok(tipo_da_conta(c)?.map(|tipo| &tipo.auth))
}

/// Escopos OAuth a pedir no login: só os dos grupos ligados. `None` = conta sem ficha (caminho antigo).
pub(crate) fn escopos_da_conta(c: &Connector) -> Result<Option<Vec<String>>> {
    Ok(tipo_da_conta(c)?.map(|tipo| tipo.escopos_efetivos(&c.liberacoes)))
}

/// Escopos do login: conta de ficha pede só os dos grupos ligados; conta sem ficha segue o caminho antigo
/// (desafio do servidor ou a lista dele).
pub(crate) fn escopos_do_login(c: &Connector, desafio: Option<Vec<String>>, documento: &Value) -> Result<Vec<String>> {
    match escopos_da_conta(c)? {
        Some(da_ficha) => crate::fork::conectores::oauth_scopes::da_ficha(&da_ficha, documento),
        None => crate::fork::conectores::oauth_scopes::selecionar(desafio, documento),
    }
}

/// Ferramentas elegíveis pelos grupos ligados (`None`: nenhum limite de nomes declarado, ou conta sem ficha).
pub(crate) fn ferramentas_da_conta(c: &Connector) -> Result<Option<BTreeSet<&'static str>>> {
    Ok(tipo_da_conta(c)?.and_then(|tipo| tipo.ferramentas_permitidas(&c.liberacoes)))
}

// MARK: renomear e desconectar

/// Renomeia qualquer conta sem mudar o `id` (bots, recibos e ferramentas continuam apontando para ela).
pub(crate) fn renomear(store: &Store, id: &str, nome: &str, space: &str) -> Result<Value> {
    let nome = nome_valido(nome)?;
    let (publico, _, _) = update_connectors_com(
        store,
        |all| {
            let atual = all.iter().find(|c| c.id == id).ok_or_else(|| anyhow!("Unknown connector"))?;
            ensure!(atual.space == space, conta_de_outro_space());
            nome_livre(all, atual.servico.as_deref(), space, &nome, Some(id))?;
            let conta = all.iter_mut().find(|c| c.id == id).ok_or_else(|| anyhow!("Unknown connector"))?;
            conta.name.clone_from(&nome);
            let instancia = conta.oauth_binding.as_ref().map(|b| b.instance().to_owned()).unwrap_or_default();
            Ok((conta.public(), instancia, conta.servico.clone()))
        },
        |tx, (_, instancia, servico)| {
            contas_recibos::registrar_em(tx, Acao::Renomear, instancia, servico.as_deref(), space, "ok")
        },
    )?;
    Ok(json!({"connector": publico}))
}

/// Desconecta UMA conta: apaga só o registro dela no cofre (tokens, chave, cadastro). As outras contas, do mesmo
/// serviço ou não, seguem como estavam. Devolve os bots que a usavam, para a tela avisar.
pub(crate) fn desconectar(store: &Store, id: &str, space: &str) -> Result<Value> {
    let usavam: Vec<Value> = store
        .bots()?
        .into_iter()
        .filter(|b| !b.deleted && b.config.connectors.iter().any(|c| c == id))
        .map(|b| json!({"id": b.config.id, "name": b.config.name}))
        .collect();
    update_connectors_com(
        store,
        |all| {
            let conta = all.iter().find(|c| c.id == id).ok_or_else(|| anyhow!("Unknown connector"))?;
            ensure!(conta.space == space, conta_de_outro_space());
            let instancia = conta.oauth_binding.as_ref().map(|b| b.instance().to_owned()).unwrap_or_default();
            let servico = conta.servico.clone();
            all.retain(|c| c.id != id);
            Ok((instancia, servico))
        },
        // O recibo nasce na mesma transação que apaga a conta do cofre.
        |tx, (instancia, servico)| {
            super::ferramentas::esquecer_conta(tx, id)?;
            contas_recibos::registrar_em(tx, Acao::Desconectar, instancia, servico.as_deref(), space, "ok")
        },
    )?;
    store.guard_connector_removed(id)?;
    Ok(json!({"removed": id, "bots": usavam, "revogadoNoFornecedor": false}))
}

/// Contas de um Space, com os bots do mesmo Space e o estado do vínculo de cada um.
pub(crate) fn listar(store: &Store, space: &str) -> Result<Value> {
    let todas = super::connectors(store)?;
    let bots: Vec<BotConfig> = store
        .bots()?
        .into_iter()
        .filter(|b| !b.deleted && !b.config.is_group() && b.config.space_id == space)
        .map(|b| b.config)
        .collect();
    let itens: Vec<Value> = todas
        .iter()
        .filter(|c| c.space == space)
        .map(|c| {
            let mut v = c.public();
            v["bots"] = json!(
                bots.iter()
                    .map(|b| json!({
                        "id": b.id, "name": b.name,
                        "ligada": b.connectors.contains(&c.id),
                        "padraoDoServico": c.servico.as_deref().is_some_and(|s| b.conta_padrao.get(s) == Some(&c.id)),
                    }))
                    .collect::<Vec<_>>()
            );
            v
        })
        .collect();
    Ok(json!({"spaceId": space, "items": itens}))
}

// MARK: vínculo bot × conta

/// Conferido na gravação do bot (`updateBot`, criação, pedido de conexão): o principal não recebe conta nova; conta nova só entra em bot do Space dela;
/// o padrão precisa ser uma conta do serviço, do Space e ligada; padrão cuja conta saiu da lista é apagado.
/// Só abre o cofre quando algo relevante mudou.
pub(crate) fn validar_vinculo(store: &Store, antes: Option<&BotConfig>, cfg: &mut BotConfig) -> Result<()> {
    // Conta que sai do bot leva junto o que a pessoa tinha ligado nela, mas só quando a gravação vale: o apagamento
    // roda na transação do `save_bot`, não aqui (esta conferência pode ser seguida de recusa ou de outra falha).
    let novas: Vec<String> =
        cfg.connectors.iter().filter(|id| antes.is_none_or(|a| !a.connectors.contains(id))).cloned().collect();
    // Padrões novos ou trocados nesta gravação (os antigos só são conferidos se a conta saiu da lista).
    let novos_padroes: Vec<(String, String)> = cfg
        .conta_padrao
        .iter()
        .filter(|(servico, conta)| antes.is_none_or(|a| a.conta_padrao.get(*servico) != Some(*conta)))
        .map(|(servico, conta)| (servico.clone(), conta.clone()))
        .collect();
    for (_, conta) in &novos_padroes {
        ensure!(
            cfg.connectors.contains(conta),
            "conta_padrao_invalida: ligue a conta neste bot antes de torná-la a padrão."
        );
    }
    cfg.perguntar_conta.retain(|servico, quer| *quer && !servico.trim().is_empty() && servico.len() <= 64);
    // Padrão cuja conta saiu da lista de ligadas deixa de existir.
    cfg.conta_padrao.retain(|_, conta| cfg.connectors.contains(conta));
    if novas.is_empty() && novos_padroes.is_empty() {
        return Ok(());
    }
    let todas = super::connectors(store)?;
    let por_id = |id: &str| todas.iter().find(|c| c.id == id);
    // O principal não usa contas de serviço: conta nova da ficha (ou padrão novo) nele é recusada em todo caminho que
    // grava o bot (`marketBotContasDefinir`, `updateBot`, aprovação de pedido de conexão). Tirar conta continua
    // permitido, e os MCPs próprios do dono seguem pela regra da `donna_fixa` (só o dono mexe nas ferramentas do Berga).
    if !novos_padroes.is_empty() || novas.iter().any(|id| por_id(id).is_some_and(|c| c.servico.is_some())) {
        ensure!(!eh_principal(store, &cfg.id)?, "{PRINCIPAL_SEM_CONTAS}");
    }
    for id in &novas {
        // Id desconhecido continua aceito como antes (sem conta não há ferramenta); conta de outro Space, nunca.
        if let Some(conta) = por_id(id) {
            ensure!(conta.space == cfg.space_id, conta_de_outro_space());
        }
    }
    for (servico, id) in &novos_padroes {
        let conta = por_id(id).ok_or_else(|| anyhow!("conta_padrao_invalida: conta desconhecida."))?;
        ensure!(conta.space == cfg.space_id, conta_de_outro_space());
        ensure!(
            conta.servico.as_deref() == Some(servico.as_str()),
            "conta_padrao_invalida: a conta não é desse serviço."
        );
    }
    Ok(())
}

/// Visão `{conta, ligada, padraoDoServico}` que o app lê, mais o "perguntar qual conta" por serviço.
pub(crate) fn vinculos(store: &Store, bot: &str) -> Result<Value> {
    let cfg = store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    let contas: Vec<Value> = super::connectors(store)?
        .iter()
        .filter(|c| c.space == cfg.space_id && c.servico.is_some())
        .map(|c| {
            json!({
                "conta": c.id, "nome": c.name, "servico": c.servico,
                "ligada": cfg.connectors.contains(&c.id),
                "padraoDoServico": c.servico.as_deref().is_some_and(|s| cfg.conta_padrao.get(s) == Some(&c.id)),
            })
        })
        .collect();
    Ok(json!({"botId": cfg.id, "spaceId": cfg.space_id, "contas": contas, "perguntarConta": cfg.perguntar_conta}))
}

/// Execução: a conta só vale no Space do bot. Recusa fechada (conta de outro Space, ou sem conta).
pub(crate) fn exigir_mesmo_space(conta: &Connector, space_do_bot: &str) -> Result<()> {
    ensure!(conta.space == space_do_bot, conta_de_outro_space());
    Ok(())
}
