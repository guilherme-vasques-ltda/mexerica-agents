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
/// `mexerica_vinculo_epoca`: contador por bot e conta que avança cada vez que a conta sai do bot; uma confirmação só vale
/// para a época que a pessoa viu.
/// `mexerica_ferramenta_liberacoes`: o que a pessoa ligou, por bot, preso ao digest.
/// `mexerica_ferramenta_recibos`: liberações e recusas.
pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS mexerica_conta_ferramentas(
            conta TEXT NOT NULL, nome TEXT NOT NULL, digest TEXT NOT NULL, dica INTEGER NOT NULL DEFAULT 0,
            visto_em INTEGER NOT NULL, PRIMARY KEY(conta, nome));
         CREATE TABLE IF NOT EXISTS mexerica_conta_inventario(conta TEXT PRIMARY KEY, completo INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS mexerica_conta_geracao(conta TEXT PRIMARY KEY, n INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS mexerica_vinculo_epoca(bot TEXT NOT NULL, conta TEXT NOT NULL, n INTEGER NOT NULL, PRIMARY KEY(bot, conta));
         CREATE TABLE IF NOT EXISTS mexerica_ferramenta_liberacoes(
            bot TEXT NOT NULL, conta TEXT NOT NULL, nome TEXT NOT NULL, digest TEXT NOT NULL, em INTEGER NOT NULL,
            PRIMARY KEY(bot, conta, nome));
         CREATE TABLE IF NOT EXISTS mexerica_ferramenta_recibos(
            id INTEGER PRIMARY KEY AUTOINCREMENT, em INTEGER NOT NULL, conta TEXT NOT NULL, bot TEXT NOT NULL,
            ferramenta TEXT NOT NULL, evento TEXT NOT NULL, motivo TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS mexerica_ferramenta_mudancas(
            bot TEXT NOT NULL, conta TEXT NOT NULL, nome TEXT NOT NULL, partes TEXT, em INTEGER NOT NULL,
            PRIMARY KEY(bot, conta, nome));",
    )?;
    // Colunas do texto que a pessoa lê. Linha anterior a elas fica sem texto (ainda não lido: nada dela passa e ninguém a
    // liga) e a conta perde o inventário até a próxima lista completa reclassificar tudo. As liberações continuam presas
    // ao digest, que já cobria a descrição, e voltam a valer quando a lista confirmar o mesmo digest. Colunas e
    // invalidação entram juntas: uma partida interrompida no meio não deixa colunas sem a invalidação.
    c.execute_batch("SAVEPOINT ferramentas_texto")?;
    let feito = (|| -> Result<()> {
        let mut atualizou = false;
        for (tabela, coluna, tipo) in [
            ("mexerica_conta_ferramentas", "descricao", "TEXT"),
            ("mexerica_conta_ferramentas", "campos", "TEXT"),
            ("mexerica_conta_ferramentas", "apresentacao", "TEXT"),
            ("mexerica_conta_ferramentas", "descricao_invalida", "INTEGER NOT NULL DEFAULT 0"),
            ("mexerica_conta_ferramentas", "partes", "TEXT"),
            ("mexerica_ferramenta_liberacoes", "partes", "TEXT"),
            ("mexerica_ferramenta_liberacoes", "apresentacao", "TEXT"),
        ] {
            let existe: bool = c.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{tabela}') WHERE name=?1)"),
                [coluna],
                |r| r.get(0),
            )?;
            if !existe {
                c.execute_batch(&format!("ALTER TABLE {tabela} ADD COLUMN {coluna} {tipo}"))?;
                atualizou = true;
            }
        }
        if atualizou {
            let contas: Vec<String> = c
                .prepare("SELECT conta FROM mexerica_conta_inventario")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for conta in contas {
                invalidar_em(c, &conta)?;
            }
        }
        Ok(())
    })();
    match feito {
        Ok(()) => c.execute_batch("RELEASE ferramentas_texto")?,
        Err(error) => {
            c.execute_batch("ROLLBACK TO ferramentas_texto; RELEASE ferramentas_texto")?;
            return Err(error);
        }
    }
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
    /// A descrição passa do teto ou não é texto: não chega ao modelo e nem a pessoa a liga.
    DescricaoInvalida,
    /// O Hub ainda não leu o texto que o modelo receberia: nada passa e ninguém a liga até a próxima lista completa.
    DescricaoDesconhecida,
}

impl Motivo {
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::Perigosa => "perigosa",
            Self::Alterada => "alterada",
            Self::ForaDaFicha => "fora_da_ficha",
            Self::SemInventario => "sem_inventario",
            Self::DescricaoInvalida => "descricao_invalida",
            Self::DescricaoDesconhecida => "descricao_desconhecida",
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
    pub descricao_invalida: bool,
    /// Digest de cada parte (`PARTES`), em JSON: diz à pessoa o que o fornecedor mudou.
    pub partes: String,
}

/// As partes da ferramenta que o digest cobre além do nome, na ordem em que a tela as cita.
const PARTES: [&str; 5] = ["descricao", "entrada", "titulo", "saida", "anotacoes"];

/// Nome técnico e `schema_digest` do que a pessoa vê e do que o fornecedor promete: título, descrição, entrada, saída
/// e anotações. Mudou qualquer parte, a autorização antiga deixa de valer. A comparação é exata: um espaço ou uma
/// maiúscula a mais na descrição também é mudança. O texto bruto nunca é guardado (ver `descricoes_mediadas`).
pub(crate) fn ler_ferramenta(tool: &Value) -> Option<Visto> {
    let nome = tool["name"].as_str()?.to_owned();
    let dicas = &tool["annotations"];
    let digest = operacao::digest(&json!({
        "nome": nome, "titulo": tool["title"], "descricao": tool["description"], "entrada": tool["inputSchema"],
        "saida": tool["outputSchema"], "anotacoes": dicas
    }));
    let dica = dicas["destructiveHint"] == true || dicas["readOnlyHint"] == false;
    let descricao_invalida = match tool.get("description") {
        None => false,
        Some(Value::String(texto)) => texto.len() > operacao::DESCRICAO_MAX,
        Some(_) => true,
    };
    let campo = |parte: &str| match parte {
        "descricao" => &tool["description"],
        "entrada" => &tool["inputSchema"],
        "titulo" => &tool["title"],
        "saida" => &tool["outputSchema"],
        _ => dicas,
    };
    let partes: BTreeMap<&str, String> = PARTES.iter().map(|p| (*p, operacao::digest(campo(p)))).collect();
    Some(Visto { nome, digest, dica, descricao_invalida, partes: json!(partes).to_string() })
}

/// Teto do texto do schema de entrada que a pessoa lê (nomes de campos e todo texto que sobra na projeção), somado.
const CAMPOS_MAX: usize = 16 * 1024;

/// Tipos JSON Schema: vocabulário fechado, não é texto do fornecedor.
const TIPOS: &[&str] = &["string", "number", "integer", "boolean", "object", "array", "null"];

/// O schema de entrada que vai ao modelo, reduzido a uma lista fechada de palavras-chave. Fica só a estrutura e o texto
/// que a tela mostra (`campos_do_schema_limitado`); qualquer outra palavra-chave do fornecedor sai antes de chegar
/// ao modelo.
pub(crate) fn projetar_schema(schema: &Value) -> Value {
    projetar_schema_com(schema, &propriedades_declaradas(schema), false)
}

/// Os nomes de campos declarados no schema todo, inclusive em ramos (`allOf`, `anyOf`, `oneOf`, `items`, `$defs`):
/// `required` só derruba o nome que nenhum ramo declara.
fn propriedades_declaradas(schema: &Value) -> BTreeSet<String> {
    fn andar(v: &Value, nomes: &mut BTreeSet<String>) {
        match v {
            Value::Array(itens) => itens.iter().for_each(|i| andar(i, nomes)),
            Value::Object(mapa) => {
                for (chave, valor) in mapa {
                    match chave.as_str() {
                        "properties" | "patternProperties" | "$defs" | "definitions" => {
                            for (nome, filho) in valor.as_object().into_iter().flatten() {
                                nomes.insert(nome.clone());
                                andar(filho, nomes);
                            }
                        }
                        "items" | "allOf" | "anyOf" | "oneOf" | "additionalProperties" | "not" => andar(valor, nomes),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let mut nomes = BTreeSet::new();
    andar(schema, &mut nomes);
    nomes
}

fn projetar_schema_com(schema: &Value, declaradas: &BTreeSet<String>, negado: bool) -> Value {
    let Value::Object(mapa) = schema else {
        return if schema.is_boolean() { schema.clone() } else { json!({}) };
    };
    let escalar = |v: &Value| matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null);
    let mut saida = serde_json::Map::new();
    for (chave, valor) in mapa {
        let projetado = match (chave.as_str(), valor) {
            ("properties" | "patternProperties" | "$defs" | "definitions", Value::Object(filhos)) => Value::Object(
                filhos.iter().map(|(n, f)| (n.clone(), projetar_schema_com(f, declaradas, negado))).collect(),
            ),
            ("items" | "anyOf" | "oneOf" | "allOf", Value::Array(itens)) => {
                Value::Array(itens.iter().map(|i| projetar_schema_com(i, declaradas, negado)).collect())
            }
            ("not", v) if v.is_object() || v.is_boolean() => projetar_schema_com(v, declaradas, true),
            ("items" | "additionalProperties", v) if v.is_object() || v.is_boolean() => {
                projetar_schema_com(v, declaradas, negado)
            }
            ("type", Value::String(t)) if TIPOS.contains(&t.as_str()) => valor.clone(),
            ("type", Value::Array(ts)) if ts.iter().all(|t| t.as_str().is_some_and(|t| TIPOS.contains(&t))) => {
                valor.clone()
            }
            // Dentro de `not`, o nome de `required` não depende de `properties` declarado: o Guard já validou o
            // identificador, e apagá-lo enfraqueceria a negação. Fora de `not`, nome que nenhum ramo declara não passa.
            ("required", Value::Array(nomes)) => Value::Array(
                nomes
                    .iter()
                    .filter(|n| if negado { n.is_string() } else { n.as_str().is_some_and(|n| declaradas.contains(n)) })
                    .cloned()
                    .collect(),
            ),
            ("enum", Value::Array(itens)) => Value::Array(itens.iter().filter(|i| escalar(i)).cloned().collect()),
            ("const", v) if escalar(v) => v.clone(),
            ("description" | "title" | "format" | "pattern" | "$ref", Value::String(_))
            | (
                "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf" | "minLength"
                | "maxLength" | "minItems" | "maxItems" | "minProperties" | "maxProperties",
                Value::Number(_),
            )
            | ("uniqueItems" | "nullable", Value::Bool(_)) => valor.clone(),
            _ => continue,
        };
        saida.insert(chave.clone(), projetado);
    }
    Value::Object(saida)
}

/// Todo texto de um schema já projetado, como `[campo, texto]`: nomes de campos (sozinhos, quando não há texto
/// abaixo deles) e todo valor de texto que não seja um tipo do vocabulário fechado. É a lista que a tela mostra.
/// Acima do teto somado (`limite`), para de andar na hora e devolve `None`: nada do resto é percorrido, serializado
/// nem digerido.
fn campos_do_schema_limitado(schema: &Value, limite: usize) -> Option<Vec<(String, String)>> {
    fn rotulo(caminho: &[String], chave: &str) -> String {
        let base = caminho.join(" › ");
        match (base.is_empty(), chave) {
            (_, "description") => base,
            (true, _) => chave.to_owned(),
            (false, _) => format!("{base} ({chave})"),
        }
    }
    fn texto(
        valor: &Value,
        caminho: &[String],
        chave: &str,
        saida: &mut Vec<(String, String)>,
        total: &mut usize,
        limite: usize,
    ) -> bool {
        match valor {
            Value::String(t) => {
                let rotulo = rotulo(caminho, chave);
                *total += rotulo.len() + t.len();
                if *total > limite {
                    return false;
                }
                saida.push((rotulo, t.clone()));
                true
            }
            Value::Array(itens) => itens.iter().all(|i| texto(i, caminho, chave, saida, total, limite)),
            _ => true,
        }
    }
    fn andar(
        schema: &Value,
        caminho: &mut Vec<String>,
        saida: &mut Vec<(String, String)>,
        total: &mut usize,
        limite: usize,
        negado: bool,
    ) -> bool {
        let Value::Object(mapa) = schema else { return true };
        for (chave, valor) in mapa {
            match chave.as_str() {
                "properties" | "patternProperties" | "$defs" | "definitions" => {
                    for (nome, filho) in valor.as_object().into_iter().flatten() {
                        caminho.push(nome.clone());
                        let antes = saida.len();
                        if !andar(filho, caminho, saida, total, limite, negado) {
                            caminho.pop();
                            return false;
                        }
                        if saida.len() == antes {
                            let rotulo = caminho.join(" › ");
                            *total += rotulo.len();
                            if *total > limite {
                                caminho.pop();
                                return false;
                            }
                            saida.push((rotulo, String::new()));
                        }
                        caminho.pop();
                    }
                }
                "items" | "anyOf" | "oneOf" | "allOf" => match valor {
                    Value::Array(itens) => {
                        for (i, item) in itens.iter().enumerate() {
                            caminho.push(format!("{chave}[{i}]"));
                            if !andar(item, caminho, saida, total, limite, negado) {
                                caminho.pop();
                                return false;
                            }
                            caminho.pop();
                        }
                    }
                    v => {
                        caminho.push(chave.clone());
                        if !andar(v, caminho, saida, total, limite, negado) {
                            caminho.pop();
                            return false;
                        }
                        caminho.pop();
                    }
                },
                "not" => {
                    caminho.push(chave.clone());
                    if !andar(valor, caminho, saida, total, limite, true) {
                        caminho.pop();
                        return false;
                    }
                    caminho.pop();
                }
                "additionalProperties" => {
                    caminho.push(chave.clone());
                    if !andar(valor, caminho, saida, total, limite, negado) {
                        caminho.pop();
                        return false;
                    }
                    caminho.pop();
                }
                // `required` dentro de `not` guarda nomes que nenhum `properties` declara; eles também precisam
                // chegar à tela, dentro do mesmo teto.
                "required" if negado => {
                    if !texto(valor, caminho, chave, saida, total, limite) {
                        return false;
                    }
                }
                "type" | "required" => {}
                _ => {
                    if !texto(valor, caminho, chave, saida, total, limite) {
                        return false;
                    }
                }
            }
        }
        true
    }
    let mut saida = Vec::new();
    let mut total = 0;
    if andar(schema, &mut Vec::new(), &mut saida, &mut total, limite, false) { Some(saida) } else { None }
}

/// Referência local (`$ref: "#..."`) resolvida contra o schema já projetado, andando só por palavras-chave
/// estruturais que a projeção mantém. Alvo que a projeção removeu (por exemplo `prefixItems`) não resolve.
fn ref_local_valida(referencia: &str, raiz: &Value) -> bool {
    let Some(ponteiro) = referencia.strip_prefix('#') else { return false };
    if ponteiro.is_empty() {
        return true;
    }
    let Some(caminho) = ponteiro.strip_prefix('/') else { return false };
    let mut partes = caminho.split('/');
    let mut alvo = raiz;
    while let Some(palavra) = partes.next() {
        alvo = match palavra {
            "properties" | "patternProperties" | "$defs" | "definitions" | "allOf" | "anyOf" | "oneOf" => {
                let Some(chave) = partes.next() else { return false };
                let Some(valor) = alvo.pointer(&format!("/{palavra}/{chave}")) else { return false };
                valor
            }
            "items" | "additionalProperties" | "not" => {
                let Some(valor) = alvo.get(palavra) else { return false };
                valor
            }
            _ => return false,
        };
    }
    alvo.is_object() || alvo.is_boolean()
}

/// Todo `$ref` do schema projetado resolve dentro dele. Um `$ref` que apontava para o que a projeção removeu deixaria
/// o modelo com uma referência sem alvo, então a ferramenta fica indisponível, como com o texto acima do teto.
/// Chaves de mapas de subschemas (`properties`, `patternProperties`, …) são nomes de campos ou padrões, e o conteúdo
/// de `const`/`enum` são valores: nenhum deles é palavra-chave, então nenhum `$ref` é procurado neles.
fn refs_locais_validos(schema: &Value, raiz: &Value) -> bool {
    match schema {
        Value::Object(mapa) => mapa.iter().all(|(chave, valor)| match chave.as_str() {
            "$ref" => valor.as_str().is_some_and(|r| ref_local_valida(r, raiz)),
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => {
                valor.as_object().is_some_and(|filhos| filhos.values().all(|filho| refs_locais_validos(filho, raiz)))
            }
            "const" | "enum" => true,
            _ => refs_locais_validos(valor, raiz),
        }),
        Value::Array(itens) => itens.iter().all(|i| refs_locais_validos(i, raiz)),
        _ => true,
    }
}

/// O texto de uma ferramenta como a mediação do Guard o entrega ao bot, já sem segredos e escapado, com o schema na
/// projeção fechada. A comparação exata continua sobre o frame bruto (o digest); a pessoa lê, a liberação guarda
/// (`apresentacao`) e o modelo recebe esta mesma projeção.
pub(crate) struct TextoMediado {
    descricao: String,
    /// `[campo, texto]` em JSON.
    campos: String,
    /// Digest da projeção que a pessoa vê: descrição e campos.
    apresentacao: String,
    grande: bool,
}

/// Sem o envelope do Guard, a ferramenta fica sem texto conhecido: nada dela passa e ninguém a liga.
pub(crate) fn descricoes_mediadas(mediado: &Value) -> BTreeMap<String, TextoMediado> {
    mediado["message"]["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let nome = t["name"].as_str()?.to_owned();
            let descricao = crate::guard_hub::mcp::sem_envelope(t["description"].as_str()?)?.to_owned();
            // Acima do teto ou com `$ref` apontando para o que a projeção removeu, nada do texto é montado nem
            // digerido: a ferramenta fica inválida, sem lista de campos.
            let projetado = projetar_schema(&t["inputSchema"]);
            let campos = refs_locais_validos(&projetado, &projetado)
                .then(|| campos_do_schema_limitado(&projetado, CAMPOS_MAX))
                .flatten();
            let Some(campos) = campos else {
                return Some((
                    nome,
                    TextoMediado {
                        descricao: String::new(),
                        campos: String::new(),
                        apresentacao: String::new(),
                        grande: true,
                    },
                ));
            };
            let campos = json!(campos.iter().map(|(r, x)| [r, x]).collect::<Vec<_>>());
            let apresentacao = operacao::digest(&json!({"descricao": descricao, "campos": campos}));
            Some((nome, TextoMediado { descricao, campos: campos.to_string(), apresentacao, grande: false }))
        })
        .collect()
}

/// As partes que mudaram entre o que a pessoa ligou e o que o fornecedor mostra agora. Vazia quando uma das duas
/// versões é anterior ao registro das partes: a tela diz só que a ação mudou. As chaves internas `_descricao_mediada`
/// e `_campos_mediado` valem só quando presentes nas duas versões: a redação do Guard ou a projeção mudarem sem o
/// bruto mudar conta como mudança de `descricao`/`entrada`, e linhas antigas, sem as chaves, não dão falso positivo.
fn partes_que_mudaram(aprovadas: Option<&str>, atuais: Option<&str>) -> Vec<&'static str> {
    let ler = |p: Option<&str>| p.and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(t).ok());
    let (Some(antes), Some(agora)) = (ler(aprovadas), ler(atuais)) else { return vec![] };
    let mediada_mudou = |chave: &str| antes.get(chave).zip(agora.get(chave)).is_some_and(|(a, b)| a != b);
    PARTES
        .into_iter()
        .filter(|p| match *p {
            "descricao" => antes.get("descricao") != agora.get("descricao") || mediada_mudou("_descricao_mediada"),
            "entrada" => antes.get("entrada") != agora.get("entrada") || mediada_mudou("_campos_mediado"),
            _ => antes.get(*p) != agora.get(*p),
        })
        .collect()
}

struct Linha {
    digest: String,
    dica: bool,
    descricao: Option<String>,
    campos: Option<String>,
    apresentacao: Option<String>,
    descricao_invalida: bool,
    partes: Option<String>,
}

fn linhas(c: &Connection, conta: &str) -> Result<BTreeMap<String, Linha>> {
    let mut st = c.prepare(
        "SELECT nome,digest,dica,descricao,campos,apresentacao,descricao_invalida,partes FROM mexerica_conta_ferramentas
         WHERE conta=?1",
    )?;
    let rows = st
        .query_map([conta], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Linha {
                    digest: r.get(1)?,
                    dica: r.get::<_, i64>(2)? != 0,
                    descricao: r.get(3)?,
                    campos: r.get(4)?,
                    apresentacao: r.get(5)?,
                    descricao_invalida: r.get::<_, i64>(6)? != 0,
                    partes: r.get(7)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok(rows)
}

/// As ferramentas que a pessoa tinha ligado neste bot e que o fornecedor mudou depois, com as partes que ela viu.
fn mudancas(c: &Connection, bot: &str, conta: &str) -> Result<BTreeMap<String, Option<String>>> {
    let mut st = c.prepare("SELECT nome,partes FROM mexerica_ferramenta_mudancas WHERE bot=?1 AND conta=?2")?;
    let rows = st
        .query_map([bot, conta], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
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
    let classe = classe(pol, nome, linha.dica);
    if classe != Classe::Fora && linha.descricao_invalida {
        return Estado::Desligada(Motivo::DescricaoInvalida);
    }
    if classe != Classe::Fora && linha.apresentacao.is_none() {
        return Estado::Desligada(Motivo::DescricaoDesconhecida);
    }
    match classe {
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
#[cfg(test)]
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

#[cfg(test)]
fn registrar_com(
    store: &Store,
    conta: &str,
    tools: &[Value],
    mediados: &BTreeSet<String>,
    completa: bool,
    geracao: Option<i64>,
) -> Result<()> {
    // Nos testes, o texto lido é o que o Guard de verdade entregaria para esta lista.
    let mediado = crate::guard_hub::mcp::mediar_para_teste(
        &json!({"jsonrpc": "2.0", "id": 0, "result": {"tools": tools}}),
        "tools/list",
        &[],
    );
    registrar_textos(store, conta, tools, mediados, &descricoes_mediadas(&mediado), completa, geracao)
}

fn registrar_textos(
    store: &Store,
    conta: &str,
    tools: &[Value],
    mediados: &BTreeSet<String>,
    textos: &BTreeMap<String, TextoMediado>,
    completa: bool,
    geracao: Option<i64>,
) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    instalar_em(&tx, conta, tools, mediados, textos, completa, geracao)?;
    Ok(tx.commit()?)
}

/// Instala o inventário da conta na transação do chamador (sem confirmar). `true`: esta resposta foi aceita (lista
/// completa, mediada e de geração vigente) e é, nesta transação, a fonte do inventário; `false`: a conta foi
/// invalidada e nada dela passa.
fn instalar_em(
    tx: &Connection,
    conta: &str,
    tools: &[Value],
    mediados: &BTreeSet<String>,
    textos: &BTreeMap<String, TextoMediado>,
    completa: bool,
    geracao: Option<i64>,
) -> Result<bool> {
    let vigente = geracao.is_some_and(|g| geracao_em(tx, conta).is_ok_and(|atual| atual == g));
    if !completa || !vigente {
        invalidar_em(tx, conta)?;
        return Ok(false);
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
    let existentes = linhas(tx, conta)?;
    let agora = crate::store::now_ms();
    for nome in existentes.keys().filter(|nome| !vistos.contains_key(*nome)) {
        tx.execute("DELETE FROM mexerica_conta_ferramentas WHERE conta=?1 AND nome=?2", params![conta, nome])?;
        tx.execute("DELETE FROM mexerica_ferramenta_liberacoes WHERE conta=?1 AND nome=?2", params![conta, nome])?;
        tx.execute("DELETE FROM mexerica_ferramenta_mudancas WHERE conta=?1 AND nome=?2", params![conta, nome])?;
        recibo(tx, conta, "*", nome, "invalidada", "retirada")?;
    }
    for (nome, visto) in &vistos {
        let mediado = textos.get(nome);
        let invalida = visto.descricao_invalida || mediado.is_some_and(|t| t.grande);
        let (texto, campos, apresentacao) = match mediado.filter(|_| !invalida) {
            Some(t) => (Some(&t.descricao), Some(&t.campos), Some(&t.apresentacao)),
            None => (None, None, None),
        };
        // As partes gravadas levam, junto das públicas, a impressão do texto mediado (chaves internas `_...`): a
        // redação do Guard ou a projeção mudarem sem o bruto mudar conta como mudança de `descricao`/`entrada`.
        // `partes_que_mudaram` só compara as chaves internas quando as duas versões as têm.
        let partes = match mediado.filter(|_| !invalida) {
            Some(t) => {
                let mut mapa: BTreeMap<String, String> = serde_json::from_str(&visto.partes).unwrap_or_default();
                mapa.insert("_descricao_mediada".to_owned(), operacao::digest(&Value::String(t.descricao.clone())));
                mapa.insert("_campos_mediado".to_owned(), operacao::digest(&Value::String(t.campos.clone())));
                json!(mapa).to_string()
            }
            None => visto.partes.clone(),
        };
        match existentes.get(nome) {
            Some(antiga) if antiga.digest == visto.digest => {
                tx.execute(
                    "UPDATE mexerica_conta_ferramentas SET dica=?3, visto_em=?4, descricao=?5, descricao_invalida=?6,
                     partes=?7, campos=?8, apresentacao=?9 WHERE conta=?1 AND nome=?2",
                    params![
                        conta,
                        nome,
                        i64::from(visto.dica),
                        agora,
                        texto,
                        i64::from(invalida),
                        partes,
                        campos,
                        apresentacao
                    ],
                )?;
                // Mesmo bruto, outra projeção (o filtro passou a mostrar outro texto): o que a pessoa viu não é mais
                // o que o modelo receberia, e a liberação cai como numa mudança do fornecedor.
                tx.execute(
                    "INSERT INTO mexerica_ferramenta_mudancas(bot,conta,nome,partes,em)
                     SELECT bot,conta,nome,partes,?3 FROM mexerica_ferramenta_liberacoes
                     WHERE conta=?1 AND nome=?2 AND apresentacao IS NOT NULL AND apresentacao IS NOT ?4
                     ON CONFLICT(bot,conta,nome) DO UPDATE SET partes=excluded.partes, em=excluded.em",
                    params![conta, nome, agora, apresentacao],
                )?;
                let caidas = tx.execute(
                    "DELETE FROM mexerica_ferramenta_liberacoes
                     WHERE conta=?1 AND nome=?2 AND apresentacao IS NOT NULL AND apresentacao IS NOT ?3",
                    params![conta, nome, apresentacao],
                )?;
                if caidas > 0 {
                    recibo(tx, conta, "*", nome, "invalidada", "alterada")?;
                }
                // Liberação de antes da projeção: o mesmo digest fixa nela o texto desta lista.
                tx.execute(
                    "UPDATE mexerica_ferramenta_liberacoes SET apresentacao=?3 WHERE conta=?1 AND nome=?2 AND digest=?4
                     AND apresentacao IS NULL AND ?3 IS NOT NULL",
                    params![conta, nome, apresentacao, visto.digest],
                )?;
                // Liberação de antes das partes: o mesmo digest confirma o que a pessoa ligou.
                tx.execute(
                    "UPDATE mexerica_ferramenta_liberacoes SET partes=?3 WHERE conta=?1 AND nome=?2 AND digest=?4
                     AND partes IS NULL",
                    params![conta, nome, partes, visto.digest],
                )?;
            }
            Some(_) => {
                tx.execute(
                    "UPDATE mexerica_conta_ferramentas SET digest=?3, dica=?4, visto_em=?5, descricao=?6,
                     descricao_invalida=?7, partes=?8, campos=?9, apresentacao=?10 WHERE conta=?1 AND nome=?2",
                    params![
                        conta,
                        nome,
                        visto.digest,
                        i64::from(visto.dica),
                        agora,
                        texto,
                        i64::from(invalida),
                        partes,
                        campos,
                        apresentacao
                    ],
                )?;
                // O que a pessoa tinha ligado fica anotado, com as partes que ela viu, para a tela dizer o que mudou.
                tx.execute(
                    "INSERT INTO mexerica_ferramenta_mudancas(bot,conta,nome,partes,em)
                     SELECT bot,conta,nome,partes,?3 FROM mexerica_ferramenta_liberacoes WHERE conta=?1 AND nome=?2
                     ON CONFLICT(bot,conta,nome) DO UPDATE SET partes=excluded.partes, em=excluded.em",
                    params![conta, nome, agora],
                )?;
                let apagadas = tx.execute(
                    "DELETE FROM mexerica_ferramenta_liberacoes WHERE conta=?1 AND nome=?2",
                    params![conta, nome],
                )?;
                if apagadas > 0 {
                    recibo(tx, conta, "*", nome, "invalidada", "alterada")?;
                }
            }
            None => {
                tx.execute(
                    "INSERT INTO mexerica_conta_ferramentas(conta,nome,digest,dica,visto_em,descricao,descricao_invalida,
                     partes,campos,apresentacao) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        conta,
                        nome,
                        visto.digest,
                        i64::from(visto.dica),
                        agora,
                        texto,
                        i64::from(invalida),
                        partes,
                        campos,
                        apresentacao
                    ],
                )?;
            }
        }
    }
    tx.execute(
        "INSERT INTO mexerica_conta_inventario(conta,completo) VALUES(?1,1) ON CONFLICT(conta) DO UPDATE SET completo=1",
        [conta],
    )?;
    Ok(true)
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
    // Nome que o bot manda numa chamada recusada só entra no recibo se a conta já o mostrou: texto livre do bot nunca vira
    // registro.
    let conhecida = evento != "recusada"
        || c.query_row(
            "SELECT 1 FROM mexerica_conta_ferramentas WHERE conta=?1 AND nome=?2",
            params![conta, ferramenta],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let nome = if conhecida && nome_aceitavel(ferramenta) { ferramenta } else { "?" };
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
    if conta.servico.is_none() {
        tracing::warn!(conta = %conta.id, "descoberta recusada: conta sem serviço");
        return Err(anyhow!("conta sem serviço"));
    }
    let Some(_pol) = politica(conta)? else {
        tracing::warn!(conta = %conta.id, "descoberta recusada: conta sem ficha");
        return Err(anyhow!("conta sem ficha"));
    };
    let mediados = nomes_mediados(mediado);
    let completa = lista_completa(pedido, bruto) && mediados.is_some();
    let textos = descricoes_mediadas(mediado);
    registrar_textos(store, &conta.id, tools, &mediados.unwrap_or_default(), &textos, completa, geracao)
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
    let mediados_ref = mediados.clone().unwrap_or_default();
    // O texto mediado é projetado ANTES de abrir o banco: a projeção dos schemas não segura o lock.
    let textos = descricoes_mediadas(mediado);
    // Instalar e decidir o que passa é uma transação só: só uma resposta aceita agora decide o que o bot vê, e só
    // pelos digests dela. Outra descoberta que reabra o inventário em seguida não empresta digest a esta resposta.
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let instalada = instalar_em(
        &tx,
        conta,
        tools,
        &mediados_ref,
        &textos,
        lista_completa(pedido, bruto) && mediados.is_some(),
        geracao,
    )?;
    let passam: BTreeSet<String> = if instalada {
        let linhas = linhas(&tx, conta)?;
        let liberadas = liberacoes(&tx, bot, conta)?;
        mediados_ref
            .iter()
            .filter(|n| linhas.get(*n).is_some_and(|l| estado(pol, n, l, liberadas.get(*n)).passa()))
            .cloned()
            .collect()
    } else {
        BTreeSet::new()
    };
    tx.commit()?;
    drop(db);
    if let Some(lista) = mediado["message"]["result"]["tools"].as_array_mut() {
        lista.retain(|t| t["name"].as_str().is_some_and(|n| passam.contains(n)));
        for tool in lista.iter_mut() {
            tool["inputSchema"] = projetar_schema(&tool["inputSchema"]);
        }
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
    // A época é lida ANTES do vínculo: uma retirada que aconteça depois só deixa a época velha, e a confirmação feita
    // com ela é recusada; nunca uma época nova para uma lista montada com o vínculo antigo.
    let epoca = epoca_em(&store.db.locked(), bot, conta_id)?;
    let cfg = store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    let conta = conta_do_bot(&super::connectors(store)?, &cfg, conta_id)?;
    let pol = politica(&conta)?.ok_or_else(|| anyhow!("conta sem ficha"))?;
    let db = store.db.locked();
    let linhas = linhas(&db, &conta.id)?;
    let liberadas = liberacoes(&db, bot, &conta.id)?;
    let mudancas = mudancas(&db, bot, &conta.id)?;
    let pronto = inventario_completo(&db, &conta.id)?;
    let itens: Vec<Value> = linhas
        .iter()
        .filter_map(|(nome, linha)| {
            let mudou =
                mudancas.get(nome).map(|aprovadas| partes_que_mudaram(aprovadas.as_deref(), linha.partes.as_deref()));
            let (estado, motivo) = match estado(&pol, nome, linha, liberadas.get(nome)) {
                Estado::Permitida => ("permitida", None),
                Estado::Ligada => ("ligada", None),
                Estado::Desligada(Motivo::ForaDaFicha) => return None,
                Estado::Desligada(Motivo::Perigosa) if mudou.is_some() => {
                    ("desligada", Some(Motivo::Alterada.codigo()))
                }
                Estado::Desligada(motivo) => ("desligada", Some(motivo.codigo())),
            };
            let mudou = mudou.filter(|_| estado == "desligada");
            let campos = linha.campos.as_deref().and_then(|c| serde_json::from_str::<Value>(c).ok());
            let rotulo = pol.tipo.perigosas.iter().find(|p| p.ferramenta == nome).map(|p| p.rotulo);
            Some(json!({"nome": nome, "rotulo": rotulo, "estado": estado, "motivo": motivo, "digest": linha.digest,
                "descricao": linha.descricao, "campos": campos, "apresentacao": linha.apresentacao, "mudou": mudou}))
        })
        .collect();
    Ok(json!({"botId": bot, "conta": conta.id, "inventarioPronto": pronto, "vinculo": epoca, "ferramentas": itens}))
}

pub(crate) struct PedidoLigar<'a> {
    pub bot: &'a str,
    pub conta: &'a str,
    pub ferramenta: &'a str,
    pub digest: &'a str,
}

const VINCULO_MUDOU: &str = "vinculo_mudou: a conta saiu deste bot e voltou desde que você abriu a lista; abra a lista de novo e confirme outra vez.";

const DESCRICAO_INVALIDA: &str =
    "descricao_invalida: o texto desta ação é grande demais ou não é texto; ela não pode ser ligada.";

const DESCRICAO_DESCONHECIDA: &str =
    "descricao_desconhecida: o Hub ainda não leu o texto desta ação; verifique a conta e abra a lista de novo.";

const APP_DESATUALIZADO: &str = "app_desatualizado: atualize o app para ver o texto desta ação antes de ligá-la.";

/// O app que confirma devolve a impressão do texto que mostrou (`apresentacao` de `listar`). Sem ela, o pedido vem
/// de um app que não mostra o texto e é recusado; diferente da atual, o texto mudou desde que a pessoa o leu. Roda na
/// transação que grava a liberação.
pub(crate) fn conferir_apresentacao(tx: &Connection, pedido: &PedidoLigar<'_>, vista: Option<&str>) -> Result<()> {
    let Some(vista) = vista else {
        tracing::warn!(
            bot = pedido.bot,
            conta = pedido.conta,
            ferramenta = pedido.ferramenta,
            "confirmação de ferramenta sem a apresentação vista pelo usuário; pedido recusado"
        );
        return Err(anyhow!("{APP_DESATUALIZADO}"));
    };
    let atual: Option<String> = tx
        .query_row(
            "SELECT apresentacao FROM mexerica_conta_ferramentas WHERE conta=?1 AND nome=?2",
            params![pedido.conta, pedido.ferramenta],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if atual.as_deref() != Some(vista) {
        tracing::warn!(
            bot = pedido.bot,
            conta = pedido.conta,
            ferramenta = pedido.ferramenta,
            "confirmação de ferramenta com apresentação divergente da atual; pedido recusado"
        );
        return Err(anyhow!("{FERRAMENTA_MUDOU}"));
    }
    Ok(())
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
    if linha.descricao_invalida {
        tracing::warn!(
            bot = pedido.bot,
            conta = pedido.conta,
            ferramenta = pedido.ferramenta,
            "ligação recusada: descrição inválida"
        );
        return Err(anyhow!("{DESCRICAO_INVALIDA}"));
    }
    if linha.apresentacao.is_none() {
        tracing::warn!(
            bot = pedido.bot,
            conta = pedido.conta,
            ferramenta = pedido.ferramenta,
            "ligação recusada: descrição ainda não lida"
        );
        return Err(anyhow!("{DESCRICAO_DESCONHECIDA}"));
    }
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
    let atual: Option<(String, bool, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT digest,descricao_invalida,apresentacao,partes FROM mexerica_conta_ferramentas
             WHERE conta=?1 AND nome=?2",
            params![pedido.conta, pedido.ferramenta],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (digest, invalida, apresentacao, partes) = atual.ok_or_else(|| anyhow!("{FERRAMENTA_MUDOU}"))?;
    ensure!(digest == pedido.digest, "{FERRAMENTA_MUDOU}");
    if invalida {
        tracing::warn!(
            bot = pedido.bot,
            conta = pedido.conta,
            ferramenta = pedido.ferramenta,
            "gravação de liberação recusada: descrição inválida"
        );
        return Err(anyhow!("{DESCRICAO_INVALIDA}"));
    }
    let Some(apresentacao) = apresentacao else {
        tracing::warn!(
            bot = pedido.bot,
            conta = pedido.conta,
            ferramenta = pedido.ferramenta,
            "gravação de liberação recusada: descrição ainda não lida"
        );
        return Err(anyhow!("{DESCRICAO_DESCONHECIDA}"));
    };
    tx.execute(
        "INSERT INTO mexerica_ferramenta_liberacoes(bot,conta,nome,digest,em,partes,apresentacao)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(bot,conta,nome) DO UPDATE SET digest=excluded.digest, em=excluded.em, partes=excluded.partes,
         apresentacao=excluded.apresentacao",
        params![
            pedido.bot,
            pedido.conta,
            pedido.ferramenta,
            pedido.digest,
            crate::store::now_ms(),
            partes,
            apresentacao
        ],
    )?;
    tx.execute(
        "DELETE FROM mexerica_ferramenta_mudancas WHERE bot=?1 AND conta=?2 AND nome=?3",
        params![pedido.bot, pedido.conta, pedido.ferramenta],
    )?;
    recibo(tx, pedido.conta, pedido.bot, pedido.ferramenta, "ligada", "confirmada")
}

fn epoca_em(c: &Connection, bot: &str, conta: &str) -> Result<i64> {
    Ok(c.query_row("SELECT n FROM mexerica_vinculo_epoca WHERE bot=?1 AND conta=?2", params![bot, conta], |r| r.get(0))
        .optional()?
        .unwrap_or(0))
}

/// A confirmação carrega a época do vínculo bot–conta que a pessoa viu; se a conta saiu do bot desde então (mesmo que
/// tenha voltado), a confirmação não vale. Roda na transação que grava a liberação.
pub(crate) fn conferir_vinculo(tx: &Connection, bot: &str, conta: &str, vinculo: i64) -> Result<()> {
    ensure!(epoca_em(tx, bot, conta)? == vinculo, "{VINCULO_MUDOU}");
    Ok(())
}

pub(crate) fn gravar_desligamento(tx: &Connection, bot: &str, conta: &str, ferramenta: &str) -> Result<()> {
    let n = tx.execute(
        "DELETE FROM mexerica_ferramenta_liberacoes WHERE bot=?1 AND conta=?2 AND nome=?3",
        params![bot, conta, ferramenta],
    )?;
    tx.execute(
        "DELETE FROM mexerica_ferramenta_mudancas WHERE bot=?1 AND conta=?2 AND nome=?3",
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
        c.execute("DELETE FROM mexerica_ferramenta_mudancas WHERE bot=?1 AND conta=?2", params![bot, conta])?;
        c.execute(
            "INSERT INTO mexerica_vinculo_epoca(bot,conta,n) VALUES(?1,?2,1) ON CONFLICT(bot,conta) DO UPDATE SET n=n+1",
            params![bot, conta],
        )?;
    }
    Ok(())
}

/// Desconectar a conta apaga o inventário e as liberações dela, em todos os bots, na mesma transação que a tira do
/// cofre. Os recibos ficam.
pub(crate) fn esquecer_conta(c: &Connection, conta: &str) -> Result<()> {
    for tabela in [
        "mexerica_conta_ferramentas",
        "mexerica_conta_inventario",
        "mexerica_ferramenta_liberacoes",
        "mexerica_ferramenta_mudancas",
    ] {
        c.execute(&format!("DELETE FROM {tabela} WHERE conta=?1"), [conta])?;
    }
    // A geração nunca volta atrás: um pedido anotado antes da desconexão não instala inventário na conta nova.
    invalidar_em(c, conta)
}

#[cfg(test)]
#[path = "ferramentas_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ferramentas_descricao_tests.rs"]
mod descricao_tests;
