//! Marketplace, fatia 2: filtro geral das ferramentas de uma conta do catálogo. Desenho:
//! `docs/propostas/marketplace-20261113/README.md` ("Ações perigosas bloqueadas antes de conectar").
//!
//! Regras que o Hub aplica sozinho, em `tools/list` e em cada `tools/call` (a tela só mostra e pede):
//! - ferramenta que escreve, envia, publica, apaga ou mexe em dinheiro nasce **desligada**, por conta e por bot, e só
//!   a pessoa a liga (autoridade do dono + confirmação), presa ao `schema_digest` que ela viu;
//! - ferramenta **nova** ou **alterada** pelo fornecedor (nome ou digest que a conta ainda não conhece) também nasce
//!   desligada (fail-closed): ligar de novo é um ato da pessoa;
//! - pagamentos (reembolso, cobrança, transferência) são sempre perigosos, mesmo que a ficha os liste como seguros;
//! - sem inventário (nenhuma lista completa vista) ou com ficha divergente, vencida ou indisponível, nada passa;
//! - cada liberação e cada recusa gera recibo, só com identificadores e o nome técnico da ferramenta.
//!
//! Só a ficha concede leitura automática, e só pelo par nome + digest que ela curou (`curadas` do grupo ligado), sem
//! verbo de escrita ou dinheiro e sem dica destrutiva do fornecedor. O que o fornecedor mostrar de diferente do curado
//! (outro digest, nome sem curadoria, nome que sumiu e voltou) nunca passa sozinho. O fornecedor pode restringir,
//! nunca conceder: nome fora da lista de um grupo que tem lista está fora do perfil (nem a pessoa o liga), e sem
//! curadoria no grupo nada passa sozinho (a pessoa liga, uma a uma, o que quiser dessa conta neste bot). O digest
//! cobre nome, título, descrição, entrada, saída e anotações: qualquer mudança desliga o que foi ligado e apaga a
//! liberação. Só entra no inventário o que a mediação do Guard deixou passar, e só uma lista completa (sem paginação)
//! substitui o inventário inteiro de uma vez; `notifications/tools/list_changed` o invalida.
//!
//! Geração da descoberta: cada conta tem um contador que avança a cada invalidação. O pedido `tools/list` anota o
//! contador ANTES de sair; a resposta só instala o inventário se o contador ainda for o mesmo na transação da
//! gravação. Uma resposta anterior ao aviso de mudança, entregue depois por outra ponte, não reabre o inventário.
//! Resposta mediada sem lista (erro, retenção total do Guard) também só invalida: nunca vale como lista vazia.
//!
//! FORA DESTE FILTRO, por decisão em aberto do produto: conta sem ficha (MCP do próprio usuário). Bloqueá-la por padrão
//! desligaria as ferramentas de quem já tem um conector próprio ligado; ver `recusar_chamada` e a descrição do PR.
use super::{Connector, contas};
use crate::{
    LockExt,
    fork::conectores::{
        catalogo_servicos::{self, TipoConexao},
        operacao,
    },
    store::Store,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{LazyLock, Mutex};

/// Verbos de leitura: uma ferramenta só parece segura se tiver um deles e nenhum verbo de escrita.
const LEITURA: &[&str] = &[
    "list",
    "get",
    "search",
    "find",
    "read",
    "fetch",
    "retrieve",
    "query",
    "describe",
    "view",
    "show",
    "count",
    "lookup",
    "check",
    "summarize",
    "inspect",
    "explain",
    "preview",
    "whoami",
    "ping",
    "extract",
];
/// Dinheiro: sempre perigoso, mesmo com verbo de leitura (`list_charges` também pede liberação).
const PAGAMENTO: &[&str] = &[
    "refund",
    "refunds",
    "charge",
    "charges",
    "payout",
    "payouts",
    "transfer",
    "transfers",
    "withdraw",
    "pay",
    "capture",
    "purchase",
    "checkout",
    "invoice",
    "invoices",
    "billing",
    "payment",
    "payments",
    "subscription",
    "subscriptions",
    "payroll",
    "deposit",
    "donate",
];
/// Raízes de dinheiro que contam mesmo coladas a outras palavras (`refundpayment`, `PayoutNow`): errar para o lado
/// do perigo só pede liberação da pessoa.
const RAIZES_DE_DINHEIRO: &[&str] = &[
    "refund", "payout", "payment", "charge", "transfer", "withdraw", "purchase", "checkout", "invoice", "billing",
    "payroll", "deposit", "donat",
];
const ESCRITA: &[&str] = &[
    "create",
    "update",
    "delete",
    "remove",
    "destroy",
    "drop",
    "send",
    "post",
    "publish",
    "deploy",
    "merge",
    "push",
    "write",
    "edit",
    "set",
    "add",
    "move",
    "rename",
    "archive",
    "close",
    "reopen",
    "assign",
    "unassign",
    "transition",
    "resolve",
    "cancel",
    "approve",
    "reject",
    "invite",
    "grant",
    "revoke",
    "rotate",
    "execute",
    "exec",
    "run",
    "invoke",
    "trigger",
    "upload",
    "import",
    "patch",
    "put",
    "reply",
    "comment",
    "forward",
    "schedule",
    "subscribe",
    "unsubscribe",
    "restore",
    "reset",
    "enable",
    "disable",
    "apply",
    "commit",
    "save",
    "submit",
    "share",
    "unshare",
    "attach",
    "detach",
    "link",
    "unlink",
    "mark",
    "label",
    "tag",
    "restart",
    "start",
    "stop",
    "kill",
    "terminate",
    "install",
    "uninstall",
    "provision",
];

/// Tabelas do filtro. `mexerica_conta_ferramentas`: o que a conta viu do fornecedor (`digest` atual, `dica` = o
/// fornecedor marca como destrutiva). `mexerica_conta_inventario`: `completo` = a lista inteira já foi vista.
/// `mexerica_conta_geracao`: contador que avança a cada invalidação e nunca volta (nem ao desconectar a conta).
/// `mexerica_ferramenta_liberacoes`: o que a pessoa ligou, por bot, preso ao digest.
/// `mexerica_ferramenta_recibos`: liberações e recusas.
pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS mexerica_conta_ferramentas(
            conta TEXT NOT NULL, nome TEXT NOT NULL, digest TEXT NOT NULL, dica INTEGER NOT NULL DEFAULT 0,
            visto_em INTEGER NOT NULL, PRIMARY KEY(conta, nome));
         CREATE TABLE IF NOT EXISTS mexerica_conta_inventario(conta TEXT PRIMARY KEY, completo INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS mexerica_conta_geracao(conta TEXT PRIMARY KEY, n INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS mexerica_ferramenta_liberacoes(
            bot TEXT NOT NULL, conta TEXT NOT NULL, nome TEXT NOT NULL, digest TEXT NOT NULL, em INTEGER NOT NULL,
            PRIMARY KEY(bot, conta, nome));
         CREATE TABLE IF NOT EXISTS mexerica_ferramenta_recibos(
            id INTEGER PRIMARY KEY AUTOINCREMENT, em INTEGER NOT NULL, conta TEXT NOT NULL, bot TEXT NOT NULL,
            ferramenta TEXT NOT NULL, evento TEXT NOT NULL, motivo TEXT NOT NULL);",
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motivo {
    /// Escreve, envia, publica, apaga ou mexe em dinheiro: só a pessoa liga.
    Perigosa,
    /// O digest que o fornecedor mostra não é o curado pela ficha (ou mudou desde que a pessoa a ligou).
    Alterada,
    /// Fora da ficha: a ficha não a oferece neste grupo, ou o nome não é aceitável. Nem a pessoa a liga.
    ForaDaFicha,
    /// Nenhum `tools/list` foi visto para esta ferramenta.
    SemInventario,
}

impl Motivo {
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::Perigosa => "perigosa",
            Self::Alterada => "alterada",
            Self::ForaDaFicha => "fora_da_ficha",
            Self::SemInventario => "sem_inventario",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Estado {
    Permitida,
    /// Perigosa, nova ou alterada, ligada pela pessoa neste bot para o digest atual.
    Ligada,
    Desligada(Motivo),
}

impl Estado {
    pub(crate) fn passa(self) -> bool {
        matches!(self, Self::Permitida | Self::Ligada)
    }
}

/// O que a ficha da conta diz sobre as ferramentas.
pub(crate) struct Politica {
    /// `Some`: só estes nomes (mais as `perigosas`) existem para a conta; `None`: o fornecedor impõe o limite.
    permitidas: Option<BTreeSet<&'static str>>,
    /// Pares nome → digest que a ficha curou: só estes passam sem a pessoa ligar.
    curadas: BTreeMap<String, String>,
    tipo: &'static TipoConexao,
}

/// `None`: conta sem ficha, fora deste filtro (decisão em aberto do produto). Erro: ficha sumiu, venceu, não confere ou
/// perdeu o grupo ligado, e o chamador fecha tudo.
pub(crate) fn politica(c: &Connector) -> Result<Option<Politica>> {
    let Some(tipo) = contas::tipo_da_conta(c)? else { return Ok(None) };
    let servico = c.servico.as_deref().and_then(catalogo_servicos::servico);
    ensure!(
        servico.is_some_and(|s| s.conferencia_vigente(catalogo_servicos::hoje())),
        "ficha_vencida: a ficha desta conta passou do prazo de conferência."
    );
    ensure!(
        tipo.perfil_disponivel(&c.liberacoes),
        "ficha_indisponivel: o perfil desta conta não está mais disponível."
    );
    let curadas =
        tipo.ferramentas_curadas(&c.liberacoes).into_iter().map(|(n, d)| (n.to_owned(), d.to_owned())).collect();
    Ok(Some(Politica { permitidas: tipo.ferramentas_permitidas(&c.liberacoes), curadas, tipo }))
}

fn palavras(nome: &str) -> Vec<String> {
    let mut saida = vec![];
    let mut atual = String::new();
    let mut anterior: Option<char> = None;
    for c in nome.chars() {
        let corte = !c.is_alphanumeric()
            || (c.is_uppercase() && anterior.is_some_and(|a| a.is_lowercase() || a.is_ascii_digit()));
        if corte && !atual.is_empty() {
            saida.push(std::mem::take(&mut atual));
        }
        if c.is_alphanumeric() {
            atual.extend(c.to_lowercase());
        }
        anterior = Some(c);
    }
    if !atual.is_empty() {
        saida.push(atual);
    }
    saida
}

fn nome_aceitavel(nome: &str) -> bool {
    (1..=64).contains(&nome.len())
        && nome.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn pagamento(nome: &str) -> bool {
    let minusculo = nome.to_lowercase();
    palavras(nome).iter().any(|p| PAGAMENTO.contains(&p.as_str()))
        || RAIZES_DE_DINHEIRO.iter().any(|r| minusculo.contains(r))
}

/// Heurística de nome e de dica do fornecedor. Só ADICIONA perigo: nunca desfaz o que a ficha marcou.
fn parece_perigosa(nome: &str, dica: bool) -> bool {
    let palavras = palavras(nome);
    let tem = |lista: &[&str]| palavras.iter().any(|p| lista.contains(&p.as_str()));
    dica || !tem(LEITURA) || tem(ESCRITA) || tem(PAGAMENTO) || pagamento(nome)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Classe {
    Fora,
    Perigosa,
    Segura,
}

/// O perfil da conta é o teto: com lista no grupo ligado, só os nomes dela existem (nem a pessoa liga outro). A
/// leitura automática exige o nome na lista da ficha, a curadoria (nome + digest) e nenhuma pista de escrita, dinheiro
/// ou dica destrutiva; o resto é perigoso (a pessoa liga por bot) e nada que o fornecedor mostre vira `Segura` sozinho.
fn classe(pol: &Politica, nome: &str, dica: bool) -> Classe {
    if !nome_aceitavel(nome) {
        return Classe::Fora;
    }
    let listada = pol.permitidas.as_ref().is_some_and(|p| p.contains(nome));
    if pol.permitidas.is_some() && !listada {
        return Classe::Fora;
    }
    let da_ficha = pol.tipo.perigosas.iter().any(|p| p.ferramenta == nome);
    if listada && pol.curadas.contains_key(nome) && !da_ficha && !parece_perigosa(nome, dica) {
        return Classe::Segura;
    }
    Classe::Perigosa
}

/// Uma ferramenta como o fornecedor a mostrou em `tools/list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Visto {
    pub nome: String,
    pub digest: String,
    pub dica: bool,
}

/// Nome técnico e `schema_digest` do que a pessoa vê e do que o fornecedor promete: título, descrição, entrada, saída
/// e anotações. Mudou qualquer parte, a autorização antiga deixa de valer. Só o hash é guardado, nunca o texto.
pub(crate) fn ler_ferramenta(tool: &Value) -> Option<Visto> {
    let nome = tool["name"].as_str()?.to_owned();
    let dicas = &tool["annotations"];
    let digest = operacao::digest(&json!({
        "nome": nome, "titulo": tool["title"], "descricao": tool["description"], "entrada": tool["inputSchema"],
        "saida": tool["outputSchema"], "anotacoes": dicas
    }));
    let dica = dicas["destructiveHint"] == true || dicas["readOnlyHint"] == false;
    Some(Visto { nome, digest, dica })
}

struct Linha {
    digest: String,
    dica: bool,
}

fn linhas(c: &Connection, conta: &str) -> Result<BTreeMap<String, Linha>> {
    let mut st = c.prepare("SELECT nome,digest,dica FROM mexerica_conta_ferramentas WHERE conta=?1")?;
    let rows = st
        .query_map([conta], |r| {
            Ok((r.get::<_, String>(0)?, Linha { digest: r.get(1)?, dica: r.get::<_, i64>(2)? != 0 }))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok(rows)
}

fn liberacoes(c: &Connection, bot: &str, conta: &str) -> Result<BTreeMap<String, String>> {
    let mut st = c.prepare("SELECT nome,digest FROM mexerica_ferramenta_liberacoes WHERE bot=?1 AND conta=?2")?;
    let rows = st
        .query_map([bot, conta], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok(rows)
}

fn estado(pol: &Politica, nome: &str, linha: &Linha, liberada: Option<&String>) -> Estado {
    let ligada = liberada == Some(&linha.digest);
    let fechada = |motivo| if ligada { Estado::Ligada } else { Estado::Desligada(motivo) };
    match classe(pol, nome, linha.dica) {
        Classe::Fora => Estado::Desligada(Motivo::ForaDaFicha),
        Classe::Perigosa => fechada(if liberada.is_some() { Motivo::Alterada } else { Motivo::Perigosa }),
        Classe::Segura if pol.curadas.get(nome) == Some(&linha.digest) => Estado::Permitida,
        Classe::Segura => fechada(Motivo::Alterada),
    }
}

/// Instala o inventário da conta a partir de um `tools/list`. `mediados` são os nomes que a mediação do Guard deixou
/// chegar ao bot (`None`: a resposta mediada não trouxe lista, como um erro): o que ela reteve nunca entra. `completa` =
/// lista inteira, sem paginação, de um pedido sem cursor. `geracao` = o contador anotado antes do pedido sair
/// (`None`: ninguém anotou). Só uma lista completa, mediada e de geração ainda vigente substitui o inventário (de
/// uma vez, numa transação). Qualquer outra o invalida e nada passa até uma lista completa nova. A primeira lista
/// não concede nada além do que a ficha curou.
pub(crate) fn registrar(
    store: &Store,
    conta: &Connector,
    tools: &[Value],
    mediados: Option<&BTreeSet<String>>,
    completa: bool,
    geracao: Option<i64>,
) -> Result<()> {
    if politica(conta)?.is_none() {
        return Ok(());
    }
    let vazio = BTreeSet::new();
    registrar_com(store, &conta.id, tools, mediados.unwrap_or(&vazio), completa && mediados.is_some(), geracao)
}

fn registrar_com(
    store: &Store,
    conta: &str,
    tools: &[Value],
    mediados: &BTreeSet<String>,
    completa: bool,
    geracao: Option<i64>,
) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let vigente = geracao.is_some_and(|g| geracao_em(&tx, conta).is_ok_and(|atual| atual == g));
    if !completa || !vigente {
        invalidar_em(&tx, conta)?;
        return Ok(tx.commit()?);
    }
    let mut vistos: BTreeMap<String, Option<Visto>> = BTreeMap::new();
    for visto in tools.iter().filter_map(ler_ferramenta) {
        // Nome repetido na mesma lista é ambíguo: nenhuma das duas passa. Nome que o Guard reteve nem entra.
        if !mediados.contains(&visto.nome) {
            continue;
        }
        let repetido = vistos.contains_key(&visto.nome);
        vistos.insert(visto.nome.clone(), if repetido { None } else { Some(visto) });
    }
    let vistos: BTreeMap<String, Visto> = vistos
        .into_iter()
        .filter_map(|(nome, visto)| visto.filter(|_| nome_aceitavel(&nome)).map(|v| (nome, v)))
        .collect();
    let existentes = linhas(&tx, conta)?;
    let agora = crate::store::now_ms();
    for nome in existentes.keys().filter(|nome| !vistos.contains_key(*nome)) {
        tx.execute("DELETE FROM mexerica_conta_ferramentas WHERE conta=?1 AND nome=?2", params![conta, nome])?;
        tx.execute("DELETE FROM mexerica_ferramenta_liberacoes WHERE conta=?1 AND nome=?2", params![conta, nome])?;
        recibo(&tx, conta, "*", nome, "invalidada", "retirada")?;
    }
    for (nome, visto) in &vistos {
        match existentes.get(nome) {
            Some(antiga) if antiga.digest == visto.digest => {
                tx.execute(
                    "UPDATE mexerica_conta_ferramentas SET dica=?3, visto_em=?4 WHERE conta=?1 AND nome=?2",
                    params![conta, nome, i64::from(visto.dica), agora],
                )?;
            }
            Some(_) => {
                tx.execute(
                    "UPDATE mexerica_conta_ferramentas SET digest=?3, dica=?4, visto_em=?5 WHERE conta=?1 AND nome=?2",
                    params![conta, nome, visto.digest, i64::from(visto.dica), agora],
                )?;
                let apagadas = tx.execute(
                    "DELETE FROM mexerica_ferramenta_liberacoes WHERE conta=?1 AND nome=?2",
                    params![conta, nome],
                )?;
                if apagadas > 0 {
                    recibo(&tx, conta, "*", nome, "invalidada", "alterada")?;
                }
            }
            None => {
                tx.execute(
                    "INSERT INTO mexerica_conta_ferramentas(conta,nome,digest,dica,visto_em) VALUES(?1,?2,?3,?4,?5)",
                    params![conta, nome, visto.digest, i64::from(visto.dica), agora],
                )?;
            }
        }
    }
    tx.execute(
        "INSERT INTO mexerica_conta_inventario(conta,completo) VALUES(?1,1) ON CONFLICT(conta) DO UPDATE SET completo=1",
        [conta],
    )?;
    tx.commit()?;
    Ok(())
}

/// O inventário da conta deixa de valer (lista parcial, paginada ou `tools/list_changed`): nada passa até o Hub ver
/// uma lista completa nova, pedida depois desta invalidação (a geração avança).
fn invalidar_em(c: &Connection, conta: &str) -> Result<()> {
    c.execute(
        "INSERT INTO mexerica_conta_inventario(conta,completo) VALUES(?1,0) ON CONFLICT(conta) DO UPDATE SET completo=0",
        [conta],
    )?;
    c.execute(
        "INSERT INTO mexerica_conta_geracao(conta,n) VALUES(?1,1) ON CONFLICT(conta) DO UPDATE SET n=n+1",
        [conta],
    )?;
    Ok(())
}

fn geracao_em(c: &Connection, conta: &str) -> Result<i64> {
    Ok(c.query_row("SELECT n FROM mexerica_conta_geracao WHERE conta=?1", [conta], |r| r.get(0))
        .optional()?
        .unwrap_or(0))
}

/// Pedidos `tools/list` em voo: a geração da conta anotada antes de o pedido sair, por ponte e conta.
static DESCOBERTAS: LazyLock<Mutex<HashMap<(String, String), i64>>> = LazyLock::new(Mutex::default);

pub(crate) fn geracao(store: &Store, conta: &str) -> Result<i64> {
    geracao_em(&store.db.locked(), conta)
}

/// Anota a geração da conta antes de a ponte mandar o `tools/list` ao fornecedor.
pub(crate) fn iniciar_descoberta(store: &Store, ponte: &str, conta: &str) -> Result<()> {
    let geracao = geracao(store, conta)?;
    DESCOBERTAS
        .lock()
        .map_err(|_| anyhow!("descobertas indisponíveis"))?
        .insert((ponte.to_owned(), conta.to_owned()), geracao);
    Ok(())
}

/// A geração anotada para a resposta que chegou, uma vez só. `None`: ninguém anotou, e a resposta não instala nada.
pub(crate) fn encerrar_descoberta(ponte: &str, conta: &str) -> Option<i64> {
    DESCOBERTAS.lock().ok()?.remove(&(ponte.to_owned(), conta.to_owned()))
}

pub(crate) fn invalidar(store: &Store, conta: &str) -> Result<()> {
    invalidar_em(&store.db.locked(), conta)
}

fn inventario_completo(c: &Connection, conta: &str) -> Result<bool> {
    Ok(c.query_row("SELECT completo FROM mexerica_conta_inventario WHERE conta=?1", [conta], |r| r.get::<_, i64>(0))
        .optional()?
        .is_some_and(|v| v != 0))
}

fn recibo(c: &Connection, conta: &str, bot: &str, ferramenta: &str, evento: &str, motivo: &str) -> Result<()> {
    let nome = if nome_aceitavel(ferramenta) { ferramenta } else { "?" };
    c.execute(
        "INSERT INTO mexerica_ferramenta_recibos(em,conta,bot,ferramenta,evento,motivo) VALUES(?1,?2,?3,?4,?5,?6)",
        params![crate::store::now_ms(), conta, bot, nome, evento, motivo],
    )?;
    // Um laço do modelo contra uma ferramenta desligada não enche o disco: só as últimas mil recusas ficam.
    if evento == "recusada" {
        c.execute(
            "DELETE FROM mexerica_ferramenta_recibos
             WHERE evento='recusada' AND id <= (SELECT MAX(id) - 1000 FROM mexerica_ferramenta_recibos)",
            [],
        )?;
    }
    Ok(())
}

fn decidir_chamada(store: &Store, bot: &str, conta: &str, pol: &Politica, nome: &str) -> Result<Estado> {
    let db = store.db.locked();
    // Sem uma lista completa vista, ou com a conta fora do bot, nenhuma decisão antiga vale.
    let no_bot = Store::bot_on(&db, bot)?.is_some_and(|b| !b.deleted && b.config.connectors.iter().any(|c| c == conta));
    if !no_bot || !inventario_completo(&db, conta)? {
        return Ok(Estado::Desligada(Motivo::SemInventario));
    }
    let linhas = linhas(&db, conta)?;
    let Some(linha) = linhas.get(nome) else { return Ok(Estado::Desligada(Motivo::SemInventario)) };
    Ok(estado(pol, nome, linha, liberacoes(&db, bot, conta)?.get(nome)))
}

/// Decide uma chamada e, se recusada, grava o recibo. `true` = recusar.
fn chamar(store: &Store, bot: &str, conta: &str, pol: Option<&Politica>, nome: &str) -> bool {
    let estado = pol
        .ok_or_else(|| anyhow!("ficha da conta indisponível"))
        .and_then(|pol| decidir_chamada(store, bot, conta, pol, nome))
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "filtro de ferramentas indisponível; chamada recusada");
            Estado::Desligada(Motivo::SemInventario)
        });
    let Estado::Desligada(motivo) = estado else { return false };
    if let Err(error) = recibo(&store.db.locked(), conta, bot, nome, "recusada", motivo.codigo()) {
        tracing::warn!(%error, "recibo de recusa não gravado");
    }
    true
}

/// `tools/call`: `true` = recusar. Qualquer falha também recusa. Cada recusa gera recibo.
///
/// FORA DO FILTRO (decisão em aberto do produto): conta sem ficha (MCP do próprio usuário, por link ou comando) segue como
/// antes. O mesmo padrão fail-closed desligaria TODAS as ferramentas de quem já tem um conector próprio ligado até
/// a pessoa liberar uma a uma; por isso fica de fora deste PR, só com o aviso "seu, não verificado".
pub(crate) fn recusar_chamada(store: &Store, bot: &str, conta: &Connector, method: &str, params: &Value) -> bool {
    if method != "tools/call" || conta.servico.is_none() {
        return false;
    }
    // Conta do catálogo nunca roda como comando local; ficha sumida ou divergente fecha tudo.
    let pol = if conta.command.is_none() { politica(conta).ok().flatten() } else { None };
    chamar(store, bot, &conta.id, pol.as_ref(), params["name"].as_str().unwrap_or_default())
}

/// Descoberta do setup (sem bot): o mesmo tratamento do caminho do bot (`filtrar_lista_com`), sem filtrar a resposta.
/// Erro do fornecedor ou frame sem lista invalida a conta antes do erro; só lista completa, mediada e de geração
/// vigente instala o inventário.
pub(crate) fn registrar_descoberta(
    store: &Store,
    conta: &Connector,
    pedido: &Value,
    bruto: &Value,
    mediado: &Value,
    geracao: Option<i64>,
) -> Result<()> {
    let tools = lista_do_frame(store, &conta.id, bruto)?;
    registrar(store, conta, tools, nomes_mediados(mediado).as_ref(), lista_completa(pedido, bruto), geracao)
}

/// As ferramentas do frame bruto de um `tools/list`. Erro do fornecedor ou frame sem `result.tools` não é lista: a conta
/// é invalidada antes do erro (a autorização da lista anterior não vale mais), preservando as liberações da pessoa.
/// Vale para o caminho do bot e para o do setup.
pub(crate) fn lista_do_frame<'a>(store: &Store, conta: &str, bruto: &'a Value) -> Result<&'a [Value]> {
    let Some(tools) = bruto["result"]["tools"].as_array() else {
        invalidar(store, conta)?;
        return Err(anyhow!("Descoberta inválida"));
    };
    Ok(tools)
}

/// Os nomes das ferramentas de uma resposta mediada de `tools/list` (o que o Guard deixou chegar ao bot). `None`: a
/// resposta mediada não tem lista (erro), o que nunca vale como lista vazia.
pub(crate) fn nomes_mediados(mediado: &Value) -> Option<BTreeSet<String>> {
    let lista = mediado["message"]["result"]["tools"].as_array()?;
    Some(lista.iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect())
}

/// A lista de um pedido `tools/list` é completa quando o pedido não trouxe cursor e a resposta não aponta outra
/// página. Qualquer outra coisa é parcial: nada passa até uma lista completa.
pub(crate) fn lista_completa(pedido: &Value, bruto: &Value) -> bool {
    pedido["cursor"].is_null() && bruto["result"].get("nextCursor").is_none_or(Value::is_null)
}

/// `tools/list`: instala o inventário (só o que o Guard deixou passar) e deixa na resposta mediada só as ferramentas
/// que passam para este bot. `bruto` é o frame original (o digest sai dele); `mediado` é o que iria ao bot.
pub(crate) fn filtrar_lista(
    store: &Store,
    bot: &str,
    conta: &Connector,
    pedido: &Value,
    bruto: &Value,
    mediado: &mut Value,
    geracao: Option<i64>,
) -> Result<()> {
    if conta.servico.is_none() {
        return Ok(());
    }
    let pol = politica(conta)?.ok_or_else(|| anyhow!("conta sem ficha"))?;
    filtrar_lista_com(store, bot, &conta.id, &pol, pedido, bruto, mediado, geracao)
}

#[allow(
    clippy::too_many_arguments,
    reason = "descoberta inteira: conta, política, pedido, resposta bruta, mediada e geração"
)]
fn filtrar_lista_com(
    store: &Store,
    bot: &str,
    conta: &str,
    pol: &Politica,
    pedido: &Value,
    bruto: &Value,
    mediado: &mut Value,
    geracao: Option<i64>,
) -> Result<()> {
    let tools = lista_do_frame(store, conta, bruto)?;
    let mediados = nomes_mediados(mediado);
    registrar_com(
        store,
        conta,
        tools,
        mediados.as_ref().unwrap_or(&BTreeSet::new()),
        lista_completa(pedido, bruto) && mediados.is_some(),
        geracao,
    )?;
    let mediados = mediados.unwrap_or_default();
    let db = store.db.locked();
    let completo = inventario_completo(&db, conta)?;
    let linhas = linhas(&db, conta)?;
    let liberadas = liberacoes(&db, bot, conta)?;
    let passam: BTreeSet<&str> = mediados
        .iter()
        .map(String::as_str)
        .filter(|n| completo && linhas.get(*n).is_some_and(|l| estado(pol, n, l, liberadas.get(*n)).passa()))
        .collect();
    if let Some(lista) = mediado["message"]["result"]["tools"].as_array_mut() {
        lista.retain(|t| t["name"].as_str().is_some_and(|n| passam.contains(n)));
    }
    Ok(())
}

fn conta_do_bot(todas: &[Connector], bot: &crate::store::BotConfig, id: &str) -> Result<Connector> {
    let conta = todas.iter().find(|c| c.id == id).ok_or_else(|| anyhow!("Unknown connector"))?;
    ensure!(conta.servico.is_some(), "conta_sem_ficha: só contas do catálogo têm ações perigosas controladas.");
    contas::exigir_mesmo_space(conta, &bot.space_id)?;
    ensure!(bot.connectors.contains(&conta.id), "conta_desligada_no_bot: ligue a conta neste bot antes.");
    Ok(conta.clone())
}

/// Ferramentas da conta como a tela as mostra, para um bot. Só o que a ficha admite (`fora_da_ficha` não aparece).
pub(crate) fn listar(store: &Store, bot: &str, conta_id: &str) -> Result<Value> {
    let cfg = store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    let conta = conta_do_bot(&super::connectors(store)?, &cfg, conta_id)?;
    let pol = politica(&conta)?.ok_or_else(|| anyhow!("conta sem ficha"))?;
    let db = store.db.locked();
    let linhas = linhas(&db, &conta.id)?;
    let liberadas = liberacoes(&db, bot, &conta.id)?;
    let pronto = inventario_completo(&db, &conta.id)?;
    let itens: Vec<Value> = linhas
        .iter()
        .filter_map(|(nome, linha)| {
            let (estado, motivo) = match estado(&pol, nome, linha, liberadas.get(nome)) {
                Estado::Permitida => ("permitida", None),
                Estado::Ligada => ("ligada", None),
                Estado::Desligada(Motivo::ForaDaFicha) => return None,
                Estado::Desligada(motivo) => ("desligada", Some(motivo.codigo())),
            };
            let rotulo = pol.tipo.perigosas.iter().find(|p| p.ferramenta == nome).map(|p| p.rotulo);
            Some(json!({"nome": nome, "rotulo": rotulo, "estado": estado, "motivo": motivo, "digest": linha.digest}))
        })
        .collect();
    Ok(json!({"botId": bot, "conta": conta.id, "inventarioPronto": pronto, "ferramentas": itens}))
}

pub(crate) struct PedidoLigar<'a> {
    pub bot: &'a str,
    pub conta: &'a str,
    pub ferramenta: &'a str,
    pub digest: &'a str,
}

const FERRAMENTA_MUDOU: &str =
    "ferramenta_mudou: a ferramenta mudou desde que você a viu; abra a lista de novo e confirme outra vez.";

/// Valida a liberação contra o estado de agora. Recusa fechada: sem confirmação, conta de outro Space ou desligada no
/// bot, ferramenta que a ficha não admite, ou digest diferente do que a pessoa viu. Lê o cofre, por isso roda antes
/// da transação do dono.
pub(crate) fn conferir_ligar(store: &Store, pedido: &PedidoLigar<'_>, confirmado: bool) -> Result<()> {
    ensure!(
        confirmado,
        "confirmacao_obrigatoria: confirme que quer ligar esta ação; ela pode escrever, enviar, publicar ou mexer em dinheiro."
    );
    let cfg = store.bot(pedido.bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    ensure!(!cfg.is_group(), "grupo não usa contas de serviço");
    let conta = conta_do_bot(&super::connectors(store)?, &cfg, pedido.conta)?;
    let pol = politica(&conta)?.ok_or_else(|| anyhow!("conta sem ficha"))?;
    let db = store.db.locked();
    ensure!(
        inventario_completo(&db, &conta.id)?,
        "ferramenta_desconhecida: essa conta ainda não mostrou a lista completa de ferramentas; verifique a conta."
    );
    let linhas = linhas(&db, &conta.id)?;
    let linha = linhas.get(pedido.ferramenta).ok_or_else(|| {
        anyhow!("ferramenta_desconhecida: essa conta ainda não mostrou esta ferramenta; verifique a conta.")
    })?;
    ensure!(
        classe(&pol, pedido.ferramenta, linha.dica) != Classe::Fora,
        "ferramenta_fora_da_ficha: a ficha desta conta não oferece essa ferramenta."
    );
    ensure!(linha.digest == pedido.digest, "{FERRAMENTA_MUDOU}");
    Ok(())
}

/// Grava a liberação na transação do dono, refazendo nela o que muda de verdade: o vínculo da conta com o bot (uma
/// retirada concorrente não deixa liberação para trás), o inventário completo e o digest que a pessoa viu.
pub(crate) fn gravar_liberacao(tx: &Connection, pedido: &PedidoLigar<'_>) -> Result<()> {
    let no_bot = Store::bot_on(tx, pedido.bot)?
        .is_some_and(|b| !b.deleted && b.config.connectors.iter().any(|c| c == pedido.conta));
    ensure!(no_bot, "conta_desligada_no_bot: ligue a conta neste bot antes.");
    ensure!(
        inventario_completo(tx, pedido.conta)?,
        "ferramenta_desconhecida: essa conta ainda não mostrou a lista completa de ferramentas; verifique a conta."
    );
    let atual: Option<String> = tx
        .query_row(
            "SELECT digest FROM mexerica_conta_ferramentas WHERE conta=?1 AND nome=?2",
            params![pedido.conta, pedido.ferramenta],
            |r| r.get(0),
        )
        .optional()?;
    ensure!(atual.as_deref() == Some(pedido.digest), "{FERRAMENTA_MUDOU}");
    tx.execute(
        "INSERT INTO mexerica_ferramenta_liberacoes(bot,conta,nome,digest,em) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(bot,conta,nome) DO UPDATE SET digest=excluded.digest, em=excluded.em",
        params![pedido.bot, pedido.conta, pedido.ferramenta, pedido.digest, crate::store::now_ms()],
    )?;
    recibo(tx, pedido.conta, pedido.bot, pedido.ferramenta, "ligada", "confirmada")
}

pub(crate) fn gravar_desligamento(tx: &Connection, bot: &str, conta: &str, ferramenta: &str) -> Result<()> {
    let n = tx.execute(
        "DELETE FROM mexerica_ferramenta_liberacoes WHERE bot=?1 AND conta=?2 AND nome=?3",
        params![bot, conta, ferramenta],
    )?;
    if n > 0 {
        recibo(tx, conta, bot, ferramenta, "desligada", "pela_pessoa")?;
    }
    Ok(())
}

/// Tirar a conta do bot apaga o que a pessoa tinha ligado nele: ligar de novo pede nova confirmação. Roda NA MESMA
/// transação que grava o bot (`store::bots::save_bot`): se a gravação falha, nada some; se vale, nada fica para trás.
pub(crate) fn esquecer_liberacoes_do_bot(c: &Connection, bot: &str, contas: &[String]) -> Result<()> {
    for conta in contas {
        c.execute("DELETE FROM mexerica_ferramenta_liberacoes WHERE bot=?1 AND conta=?2", params![bot, conta])?;
    }
    Ok(())
}

/// Desconectar a conta apaga o inventário e as liberações dela, em todos os bots, na mesma transação que a tira do
/// cofre. Os recibos ficam.
pub(crate) fn esquecer_conta(c: &Connection, conta: &str) -> Result<()> {
    for tabela in ["mexerica_conta_ferramentas", "mexerica_conta_inventario", "mexerica_ferramenta_liberacoes"] {
        c.execute(&format!("DELETE FROM {tabela} WHERE conta=?1"), [conta])?;
    }
    // A geração nunca volta atrás: um pedido anotado antes da desconexão não instala inventário na conta nova.
    invalidar_em(c, conta)
}

#[cfg(test)]
#[path = "ferramentas_tests.rs"]
mod tests;
