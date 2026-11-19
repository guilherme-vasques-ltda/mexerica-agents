//! MX-264 (opção A do dono, 12/11/2026): "Ligar em todos os bots". A escolha por
//! bot segue a base (D288/c1, MX-130 e `updateBot`); este produtor só acrescenta
//! a prévia de UMA lista e a aplicação de tudo-ou-nada numa transação, sob a
//! autoridade original do transporte (`fork::autoridade`). Não é catálogo, grant
//! nem executor: a instalação é conferida pelo catálogo/MCP já existentes e a
//! escrita repete as mesmas condições do `updateBot`/seleção de uma célula
//! (D91/D306, regras da principal). O padrão "novos bots já vêm com…" existe só
//! para skills (`extensaoNovosBotsDefinir` + kv `donna.extensoes_novos_bots.v1`),
//! por decisão do dono sobre a D91: conectores, contas e logins seguem desligados
//! até escolha explícita ("Ligar em todos os bots" ou bot a bot).
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{autoridade::Autoridade, spaces},
    hub::Hub,
    store::next_rev,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};

pub(crate) const METHODS: &[&str] = &["extensaoEmTodosPrevia", "extensaoEmTodosAplicar", "extensaoNovosBotsDefinir"];
const MOTIVO_SEM_CONTAS: &str = "principal_sem_contas";
pub(crate) const CONFLICT: &str = "extensoes_em_todos_conflito";
pub(crate) const INDISPONIVEL: &str = "extensoes_em_todos_indisponivel";
pub(crate) const UNSUPPORTED: &str = "extensoes_em_todos_unsupported";
/// MX-264/C: o padrão "novos bots já vêm com…" existe só para skills; conectores
/// e contas seguem a D91 (desligados até escolha explícita).
pub(crate) const NOVOS_BOTS_RECUSADO: &str = "extensoes_novos_bots_recusado";
const FORMATO: u8 = 1;
/// kv do Hub: `{"skills": ["<id>", …]}` — ids de skill que bots novos já recebem.
const KV_NOVOS_BOTS: &str = "donna.extensoes_novos_bots.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Tipo {
    Skill,
    Conector,
}

impl Tipo {
    fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Conector => "conector",
        }
    }
    /// Lista do `BotConfig` que a extensão ocupa (mesmos campos do `updateBot`).
    fn campo(self) -> &'static str {
        match self {
            Self::Skill => "skills",
            Self::Conector => "connectors",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviaPedido {
    protocol: u8,
    tipo: Tipo,
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AplicarPedido {
    protocol: u8,
    tipo: Tipo,
    id: String,
    bot_ids: Vec<String>,
    versao: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NovosBotsPedido {
    protocol: u8,
    tipo: Tipo,
    id: String,
    ligado: bool,
}

/// A extensão vista na leitura que originou a prévia. `fingerprint` entra no
/// digest da versão; `detalhe` carrega o que a escrita reconfere na Tx.
struct Alvo {
    tipo: Tipo,
    id: String,
    fingerprint: Value,
    detalhe: Detalhe,
}

enum Detalhe {
    /// Identidade da instalação conferida pelo `skill_catalog`.
    Skill { installation_id: String },
    /// Raws do cofre (`connectors`) a reconferir byte a byte na Tx:
    /// qualquer escrita, até rotação de cifra, invalida a prévia.
    Conector { cofres: Vec<(String, Option<String>)>, space: String, servico: bool },
}

struct Recebedor {
    id: String,
    nome: String,
    /// Lista já validada pelo parser canônico (`skill_selection::lista_estrita`).
    lista: Vec<String>,
}

struct Avaliacao {
    recebedores: Vec<Recebedor>,
    excluidos: Vec<Value>,
    versao: String,
}

pub(crate) async fn call(hub: Arc<Hub>, caller: Caller, method: &str, params: Value) -> Result<Value> {
    match method {
        "extensaoEmTodosPrevia" => previa(hub, caller, params).await,
        "extensaoEmTodosAplicar" => aplicar(hub, caller, params).await,
        "extensaoNovosBotsDefinir" => novos_bots_definir(hub, caller, params).await,
        _ => Err(anyhow!("unknown_method")),
    }
}

async fn previa(hub: Arc<Hub>, caller: Caller, params: Value) -> Result<Value> {
    let pedido: PreviaPedido = serde_json::from_value(params).map_err(|_| anyhow!(UNSUPPORTED))?;
    ensure!(pedido.protocol == FORMATO, UNSUPPORTED);
    ensure!(!pedido.id.trim().is_empty() && pedido.id.len() <= 4096, UNSUPPORTED);
    // A leitura também é do dono: a autoridade ORIGINAL é capturada antes da
    // fila e reconferida depois da espera — revogação no meio falha fechado.
    let owner = Autoridade::capturar(&hub, &caller)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    // `spawn_blocking` não herda o task-local do pedido: a origem viaja
    // explícita para que gravações internas dos resolvedores reconferiem a
    // autoridade na própria transação, sem cair em "operação interna".
    let origem = crate::fork::donna_fixa::pedido::atual();
    let (tipo, id) = (pedido.tipo, pedido.id);
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let trabalho = || -> Result<Value> {
            // Fronteira do dono antes do resolvedor: `skill_catalog`/cofre podem
            // gravar migrações internas e nenhuma escrita acontece com a autoridade
            // já revogada.
            crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
            owner.with_db(&hub, None, |_| Ok(()))?;
            let alvo = resolver(&hub, tipo, &id)?;
            // A avaliação roda dentro de uma TxImmediate: snapshot coerente dos bots
            // e reconferência da autoridade em volta da leitura.
            let aval = owner.with_db(&hub, None, |c| avaliar(c, &alvo))?;
            Ok(json!({
                "tipo": alvo.tipo.as_str(),
                "id": alvo.id,
                "versao": aval.versao,
                "recebedores": aval.recebedores.iter().map(|r| json!({"botId": r.id, "nome": r.nome})).collect::<Vec<_>>(),
                "excluidos": aval.excluidos,
            }))
        };
        match origem {
            Some(origem) => crate::fork::donna_fixa::pedido::com_origem(origem, trabalho),
            None => trabalho(),
        }
    })
    .await?
}

async fn aplicar(hub: Arc<Hub>, caller: Caller, params: Value) -> Result<Value> {
    let pedido: AplicarPedido = serde_json::from_value(params).map_err(|_| anyhow!(UNSUPPORTED))?;
    ensure!(pedido.protocol == FORMATO, UNSUPPORTED);
    ensure!(!pedido.id.trim().is_empty() && pedido.id.len() <= 4096, UNSUPPORTED);
    ensure!(
        !pedido.bot_ids.is_empty()
            && pedido.bot_ids.iter().all(|id| !id.trim().is_empty() && id.len() <= 256)
            && pedido.versao.len() == 64
            && pedido.versao.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        UNSUPPORTED
    );
    // A autoridade é a ORIGINAL do transporte, capturada antes da fila e dos
    // locks: a espera não reautoriza nem degrada (D544/R2). Ponte/Pairing/
    // Bootstrap e aparelho sem Control são recusados aqui e em `permit`.
    let owner = Autoridade::capturar(&hub, &caller)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    // Mesma ponte da prévia: a origem explícita alcança as gravações internas
    // do resolvedor dentro do `spawn_blocking`.
    let origem = crate::fork::donna_fixa::pedido::atual();
    crate::chat::manage::recover_live(&hub);
    let (tipo, id, bot_ids, versao) = (pedido.tipo, pedido.id, pedido.bot_ids, pedido.versao);
    let aplicados = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let trabalho = || aplicar_em_transacao(&hub, &owner, tipo, &id, &bot_ids, &versao);
        match origem {
            Some(origem) => crate::fork::donna_fixa::pedido::com_origem(origem, trabalho),
            None => trabalho(),
        }
    })
    .await??;
    Ok(json!({"aplicados": aplicados}))
}

fn aplicar_em_transacao(
    hub: &Arc<Hub>,
    owner: &Autoridade,
    tipo: Tipo,
    id: &str,
    bot_ids: &[String],
    versao: &str,
) -> Result<Vec<String>> {
    // Ponto de gravação do dono (mesmo gancho de `updateBot`/`createBot`): a
    // revogação entre a captura e a escrita é exercida pelo teste-porta D368.
    // A reconferência vem ANTES do resolvedor: as leituras do catálogo/cofre
    // podem gravar migrações internas e nenhuma delas roda com a autoridade
    // já revogada.
    crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
    owner.with_db(hub, None, |_| Ok(()))?;
    let alvo = resolver(hub, tipo, id).map_err(|e| {
        // Extensão que sumiu ou perdeu a verificação depois da prévia é conflito
        // de estado (a tela refaz a prévia), não indisponibilidade de entrada.
        if e.to_string() == INDISPONIVEL { anyhow!(CONFLICT) } else { e }
    })?;
    let aplicados = match &alvo.detalhe {
        Detalhe::Skill { installation_id } => {
            let esperado = vec![(alvo.id.clone(), installation_id.clone())];
            crate::fork::skill_catalog::with_installations(&hub.store, &esperado, true, |vista| {
                let _boundary = hub.store.memory_boundary.locked();
                let _emit = hub.emit_lock.locked();
                owner.with_db(hub, None, |c| {
                    vista.conferir(c)?;
                    escrever_em_lote(c, &alvo, bot_ids, versao)
                })
            })
            .map_err(conflito_se_instalacao)?
        }
        Detalhe::Conector { cofres, .. } => {
            // Mesma ordem do ramo de skills e dos caminhos de memória:
            // boundary → emit → DB. Invertido, with_db seguraria o DB pedindo
            // boundary e deadlockaria com quem faz boundary → DB.
            let _boundary = hub.store.memory_boundary.locked();
            let _emit = hub.emit_lock.locked();
            owner.with_db(hub, None, |c| {
                conferir_cofres(c, cofres)?;
                escrever_em_lote(c, &alvo, bot_ids, versao)
            })?
        }
    };
    // Efeitos só depois do commit: nenhum Reconfigure/aviso sai de lote parcial.
    for bot_id in &aplicados {
        match hub.store.bot(bot_id) {
            Ok(Some(row)) if !row.config.is_group() => {
                if let Err(error) =
                    hub.send_cmd(bot_id, crate::agent::bot::Cmd::Reconfigure(Box::new(row.config.clone())))
                {
                    tracing::warn!(%error, bot = %bot_id, "extensão em lote salva; reconfiguração do ator pendente");
                }
                hub.emit_bot(bot_id);
            }
            Ok(Some(_)) => hub.emit_bot(bot_id),
            _ => {}
        }
    }
    tracing::info!(tipo = alvo.tipo.as_str(), extensao = %alvo.id, bots = aplicados.len(), "extensão aplicada em todos os bots");
    Ok(aplicados)
}

/// STALE/CHANGED/UNAVAILABLE da instalação significam "estado mudou": o app
/// deve refazer a prévia, então viram o conflito tipado do lote.
fn conflito_se_instalacao(e: anyhow::Error) -> anyhow::Error {
    if e.chain().any(|m| m.to_string().starts_with("skill_installation_")) { anyhow!(CONFLICT) } else { e }
}

/// Teste: gancho por chamada — o resolvedor avisa `(tipo, id)` na entrada,
/// depois da captura e da reconferência inicial, antes dos locks internos.
/// Assinantes são independentes (testes em paralelo não se interferem) e saem
/// por ticket. Os testes de corrida usam como barreira para revogar a
/// autoridade numa ordem garantida.
#[cfg(test)]
pub(crate) mod sonda {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    type Gancho = Box<dyn Fn(&str, &str) + Send>;
    static HOOKS: Mutex<Vec<(u64, Gancho)>> = Mutex::new(Vec::new());
    static TICKET: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn disparar(tipo: &str, id: &str) {
        for (_, f) in HOOKS.lock().unwrap().iter() {
            f(tipo, id);
        }
    }

    /// Assinante de UMA chamada: `rx` recebe quando o resolvedor daquela
    /// extensão entra; o Drop remove a inscrição.
    pub(crate) struct Espera {
        ticket: u64,
        pub(crate) rx: std::sync::mpsc::Receiver<()>,
    }
    impl Drop for Espera {
        fn drop(&mut self) {
            HOOKS.lock().unwrap().retain(|(t, _)| *t != self.ticket);
        }
    }

    pub(crate) fn assinar(tipo: &'static str, id: String) -> Espera {
        let (tx, rx) = std::sync::mpsc::channel();
        let ticket = TICKET.fetch_add(1, Ordering::Relaxed);
        HOOKS.lock().unwrap().push((
            ticket,
            Box::new(move |t, i| {
                if t == tipo && i == id {
                    let _ = tx.send(());
                }
            }),
        ));
        Espera { ticket, rx }
    }
}

fn resolver(hub: &Arc<Hub>, tipo: Tipo, id: &str) -> Result<Alvo> {
    #[cfg(test)]
    sonda::disparar(tipo.as_str(), id);
    match tipo {
        Tipo::Skill => resolver_skill(hub, id),
        Tipo::Conector => resolver_conector(hub, id),
    }
}

fn resolver_skill(hub: &Arc<Hub>, id: &str) -> Result<Alvo> {
    let lista = crate::fork::skill_catalog::list(&hub.store, || Ok(()))?;
    let item = lista["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["id"].as_str() == Some(id))
        .ok_or_else(|| anyhow!(INDISPONIVEL))?;
    ensure!(item["contentStatus"] == "verified", INDISPONIVEL);
    let installation_id =
        item["installationId"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!(INDISPONIVEL))?.to_owned();
    Ok(Alvo {
        tipo: Tipo::Skill,
        id: id.to_owned(),
        fingerprint: item.clone(),
        detalhe: Detalhe::Skill { installation_id },
    })
}

fn resolver_conector(hub: &Arc<Hub>, id: &str) -> Result<Alvo> {
    // Snapshot coerente do cofre: `connector_lock` é a mesma fronteira das
    // escritas de conectores; dentro dela, os loaders internos (que
    // não reentram o lock) e os raws do kv formam UM estado — na ordem
    // connector_lock → DB, como nas escritas.
    let _guard = hub.store.connector_lock.locked();
    // Leituras que podem gravar (migração do cofre, vínculo OAuth) ANTES da
    // impressão e da captura dos raws: o fingerprint e os cofres valem para o
    // estado já assentado.
    let mcp = crate::market::connectors_locked(&hub.store)?;
    let mut itens = Vec::new();
    let mut encontrado = false;
    let mut space_da_conta = spaces::PERSONAL.to_owned();
    let mut conta_de_servico = false;
    for c in &mcp {
        let mut public = c.public();
        if let Some(o) = public.as_object_mut() {
            o.remove("auth");
        }
        itens.push(public);
        if c.id == id && c.configured() {
            encontrado = true;
            space_da_conta.clone_from(&c.space);
            conta_de_servico = c.servico.is_some();
        }
    }
    ensure!(encontrado, INDISPONIVEL);
    let cofres = ["connectors"]
        .iter()
        .map(|slot| Ok(((*slot).to_owned(), hub.store.kv_read(slot)?)))
        .collect::<Result<Vec<_>>>()?;
    // A versão cobre também os registros privados (argumentos, env, headers,
    // dados de autenticação) por digest — sem expor valores. Trocar um valor
    // secreto mantém a projeção pública idêntica, mas muda a versão.
    let mut digestor = Sha256::new();
    for (slot, raw) in &cofres {
        digestor.update(slot.as_bytes());
        digestor.update([0u8]);
        digestor.update(raw.as_deref().unwrap_or_default().as_bytes());
        digestor.update([0u8]);
    }
    let cofres_digest = hex(&digestor.finalize());
    Ok(Alvo {
        tipo: Tipo::Conector,
        id: id.to_owned(),
        fingerprint: json!({"itens": itens, "cofres": cofres_digest}),
        detalhe: Detalhe::Conector { cofres, space: space_da_conta, servico: conta_de_servico },
    })
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
    }
    out
}

/// Avalia TODOS os bots sobre a mesma `Connection` (prévia solta ou Tx da
/// aplicação) e devolve quem receberia a extensão, quem fica de fora e o digest
/// do estado. As condições repetem as do `updateBot` e da seleção de uma célula:
/// apagado, grupo, oculto, espaço não-pessoal para skills (D306) e a regra fixa
/// da principal de Space para conectores (`skills`/`connectors` são `DO_DONO` e
/// `FIXED_FIELDS`).
fn avaliar(c: &Connection, alvo: &Alvo) -> Result<Avaliacao> {
    let mut st = c.prepare("SELECT id, config, deleted, donna_skills_version FROM bots ORDER BY id")?;
    let linhas = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(st);
    // O retrato dos Spaces é lido uma vez; a principal de Space nunca recebe
    // conector (persona/poderes fixos) e bot fora do Pessoal nunca recebe skill.
    let spaces = (alvo.tipo == Tipo::Conector).then(|| spaces::store::read(c)).transpose()?;
    // Marketplace: a conta só vale para bots do Space dela (o Hub recusa ligar conta de outro Space).
    let space_da_conta = match &alvo.detalhe {
        Detalhe::Conector { space, .. } => Some(space.as_str()),
        Detalhe::Skill { .. } => None,
    };
    // O Berga do Pessoal (`team.settings`) não recebe conta de serviço do Marketplace, como o principal de um Space
    // (`principal_sem_contas`); MCP próprio do dono continua valendo para ele.
    let berga_pessoal = match &alvo.detalhe {
        Detalhe::Conector { servico: true, .. } => crate::store::read_team_settings(c)?.primary_bot_id,
        _ => None,
    };
    let mut entradas = Vec::with_capacity(linhas.len());
    let mut recebedores = Vec::new();
    let mut excluidos = Vec::new();
    for (id, raw, deleted, versao_skills) in linhas {
        let Ok(config) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let kind = config["kind"].as_str().unwrap_or("agent").to_owned();
        let hidden = config["hidden"].as_bool().unwrap_or(false);
        let space = config["spaceId"].as_str().unwrap_or(spaces::PERSONAL).to_owned();
        let nome = config["name"].as_str().unwrap_or_default().to_owned();
        // Mesma régua da seleção de uma célula: lista malformada recusa o estado
        // inteiro; entradas inválidas nunca são descartadas em silêncio.
        let lista =
            crate::fork::skill_selection::lista_estrita(&config[alvo.tipo.campo()]).map_err(|_| anyhow!(CONFLICT))?;
        let tem = lista.iter().any(|v| v == &alvo.id);
        let principal = spaces.as_ref().is_some_and(|s| {
            s.spaces.iter().any(|sp| sp.id != spaces::PERSONAL && sp.principal_bot_id.as_deref() == Some(id.as_str()))
        });
        let sem_contas = berga_pessoal.as_deref() == Some(id.as_str());
        entradas.push(json!([
            id,
            deleted,
            kind,
            hidden,
            space,
            tem,
            versao_skills,
            principal,
            config[alvo.tipo.campo()].clone(),
            sem_contas,
        ]));
        let motivo = if deleted {
            Some("apagado")
        } else if kind == "group" {
            Some("grupo")
        } else if hidden {
            Some("oculto")
        } else if alvo.tipo == Tipo::Skill && space != spaces::PERSONAL {
            Some("fora_do_pessoal")
        } else if alvo.tipo == Tipo::Conector && principal {
            Some("principal_de_space")
        } else if sem_contas {
            Some(MOTIVO_SEM_CONTAS)
        } else if space_da_conta.is_some_and(|conta| conta != space) {
            Some("space_diferente")
        } else if tem {
            Some("ja_tem")
        } else {
            None
        };
        // Para skill, quem receberia precisa da mesma condição válida que a
        // célula única exige (`valid_version`): ausente/inválida recusa o estado.
        if motivo.is_none() && alvo.tipo == Tipo::Skill {
            ensure!(crate::fork::skill_selection::valid_version(versao_skills.clone()).is_some(), CONFLICT);
        }
        match motivo {
            Some(m) => excluidos.push(json!({"botId": id, "motivo": m})),
            None => recebedores.push(Recebedor { id, nome, lista }),
        }
    }
    Ok(Avaliacao { recebedores, excluidos, versao: versao_do_estado(alvo, &entradas) })
}

fn versao_do_estado(alvo: &Alvo, entradas: &[Value]) -> String {
    let digest = json!({
        "formato": FORMATO,
        "tipo": alvo.tipo.as_str(),
        "extensao": alvo.id,
        "estado": alvo.fingerprint,
        "bots": entradas,
    });
    let bytes = serde_json::to_vec(&digest).unwrap_or_default();
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(&bytes) {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
    }
    out
}

fn conferir_cofres(c: &Connection, esperados: &[(String, Option<String>)]) -> Result<()> {
    for (slot, esperado) in esperados {
        let atual: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [slot], |r| r.get(0)).optional()?;
        ensure!(atual == *esperado, CONFLICT);
    }
    Ok(())
}

/// Aplica a extensão só nos bots que a prévia confirmou. `botIds` precisa ser
/// subconjunto dos recebedores do estado atual: qualquer diferença (bot novo,
/// extensão mexida, lista trocada) é conflito e nada é escrito.
fn escrever_em_lote(c: &Connection, alvo: &Alvo, bot_ids: &[String], versao: &str) -> Result<Vec<String>> {
    let aval = avaliar(c, alvo)?;
    ensure!(aval.versao == versao, CONFLICT);
    // Pedir o principal explicitamente recusa com o código estável do Marketplace, não com "conflito".
    let de_servico = matches!(alvo.detalhe, Detalhe::Conector { servico: true, .. });
    let principal_pedido = bot_ids.iter().any(|id| {
        aval.excluidos.iter().any(|e| {
            e["botId"] == id.as_str()
                && (e["motivo"] == MOTIVO_SEM_CONTAS || (de_servico && e["motivo"] == "principal_de_space"))
        })
    });
    ensure!(!principal_pedido, "{}", crate::market::contas::PRINCIPAL_SEM_CONTAS);
    let alvos: BTreeSet<&str> = aval.recebedores.iter().map(|r| r.id.as_str()).collect();
    let pedidos: BTreeSet<&str> = bot_ids.iter().map(String::as_str).collect();
    ensure!(pedidos.iter().all(|id| alvos.contains(id)), CONFLICT);
    let mut aplicados = Vec::new();
    // D368: intenção do dono pelo app; os gatilhos DO_DONO aceitam a marca.
    crate::fork::donna_fixa::como_dono(c, |c| {
        for id in &pedidos {
            let Some(bot) = aval.recebedores.iter().find(|r| r.id == *id) else { continue };
            let campo = alvo.tipo.campo();
            let mut lista = bot.lista.clone();
            if !lista.iter().any(|s| s == &alvo.id) {
                lista.push(alvo.id.clone());
            }
            let rev = next_rev(c)?;
            c.execute(
                "UPDATE bots SET config = json_set(config, ?1, json(?2)), rev = ?3 WHERE id = ?4",
                rusqlite::params![format!("$.{campo}"), serde_json::to_string(&lista)?, rev, id],
            )?;
            aplicados.push(id.to_string());
        }
        Ok(())
    })?;
    Ok(aplicados)
}

/// Lê o conjunto de skills-padrão para bots novos. Ausente ou corrompido vale
/// como vazio (fail-closed: nunca concede o que não dá para conferir).
pub(crate) fn novos_bots_skills(store: &crate::store::Store) -> BTreeSet<String> {
    let Ok(Some(raw)) = store.kv_read(KV_NOVOS_BOTS) else { return BTreeSet::new() };
    let Ok(state) = serde_json::from_str::<Value>(&raw) else {
        tracing::warn!("padrão de skills para bots novos ilegível; ignorado");
        return BTreeSet::new();
    };
    state["skills"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect()
}

/// `extensaoNovosBotsDefinir`: só o dono, só skills. Conector é recusado com erro
/// tipado próprio — a D91 segue valendo para conectores e contas.
async fn novos_bots_definir(hub: Arc<Hub>, caller: Caller, params: Value) -> Result<Value> {
    let pedido: NovosBotsPedido = serde_json::from_value(params).map_err(|_| anyhow!(UNSUPPORTED))?;
    ensure!(pedido.protocol == FORMATO, UNSUPPORTED);
    ensure!(!pedido.id.trim().is_empty() && pedido.id.len() <= 4096, UNSUPPORTED);
    ensure!(pedido.tipo == Tipo::Skill, NOVOS_BOTS_RECUSADO);
    // Autoridade antes do resolvedor: a leitura do catálogo pode gravar
    // migrações internas; nenhuma acontece com o transporte já revogado.
    let owner = Autoridade::capturar(&hub, &caller)?;
    // Só uma skill instalada e com conteúdo verificado pode virar padrão.
    let _ = resolver(&hub, Tipo::Skill, &pedido.id)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let (id, ligado) = (pedido.id, pedido.ligado);
    let resposta = json!({"id": id, "novosBots": ligado});
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        owner.with_db(&hub, None, |c| {
            let raw: Option<String> =
                c.query_row("SELECT v FROM kv WHERE k=?1", [KV_NOVOS_BOTS], |r| r.get(0)).optional()?;
            let mut skills: BTreeSet<String> = raw
                .and_then(|r| serde_json::from_str::<Value>(&r).ok())
                .and_then(|v| {
                    v["skills"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect::<BTreeSet<_>>())
                })
                .unwrap_or_default();
            if ligado {
                skills.insert(id.clone());
            } else {
                skills.remove(&id);
            }
            let state = json!({"skills": skills.iter().collect::<Vec<_>>()});
            c.execute(
                "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
                rusqlite::params![KV_NOVOS_BOTS, serde_json::to_string(&state)?],
            )?;
            Ok(())
        })
    })
    .await??;
    Ok(resposta)
}

/// MX-264/C na criação do `createBot` do dono: só quando o corpo NÃO trouxe
/// `skills` (ausente ou nulo — uma lista explícita, mesmo vazia, é a escolha do
/// dono e dispensa o padrão), soma os padrões instalados e verificados. Bot de
/// grupo, de Space não-pessoal ou com seleção explícita não recebe padrão; skill
/// desinstalada é ignorada (fail-closed). Bots criados por ferramenta de outro
/// bot/Times não passam por aqui: seguem a herança da seleção explícita do
/// criador, como antes.
pub(crate) fn padroes_novos_bot(
    hub: &Arc<Hub>,
    owner: &Autoridade,
    mut cfg: crate::store::BotConfig,
    skills: Option<crate::fork::skill_selection::CreationSelection>,
    skills_explicitos: bool,
) -> (crate::store::BotConfig, Option<crate::fork::skill_selection::CreationSelection>) {
    if skills_explicitos
        || skills.is_some()
        || cfg.is_group()
        || cfg.space_id != spaces::PERSONAL
        || !cfg.skills.is_empty()
    {
        return (cfg, skills);
    }
    let padroes = novos_bots_skills(&hub.store);
    if padroes.is_empty() {
        return (cfg, skills);
    }
    let mut expected = Vec::new();
    for id in padroes {
        // Só entra o que o catálogo confere agora: desinstalada/alterada fica de fora.
        if let Ok(alvo) = resolver_skill(hub, &id)
            && let Detalhe::Skill { installation_id } = alvo.detalhe
        {
            expected.push((id, installation_id));
        }
    }
    if expected.is_empty() {
        return (cfg, skills);
    }
    cfg.skills = expected.iter().map(|(id, _)| id.clone()).collect();
    (cfg, Some(crate::fork::skill_selection::CreationSelection::com_autoridade(expected, owner.clone())))
}

#[cfg(test)]
mod tests;
