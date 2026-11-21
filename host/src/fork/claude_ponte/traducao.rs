//! Tradução do fluxo `stream-json` do Claude Code em `session/update` do ACP, no formato que o
//! Hub já consome do adaptador anterior: texto, pensamento, ferramentas (anúncio, entrada, resultado), plano,
//! ocupação de contexto, compactação e limite de uso. Nenhum segredo passa por aqui: só eventos da conversa.

use regex::{Captures, Regex};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

/// Janela usada até o resultado do turno informar a do modelo (`modelUsage.<modelo>.contextWindow`).
pub(super) const JANELA_PADRAO: u64 = 200_000;

/// Uma chamada de ferramenta que o Claude anunciou: nome e entrada (vazia até a mensagem consolidada).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Ferramenta {
    pub nome: String,
    pub entrada: Value,
    pub raiz: bool,
}

#[derive(Default)]
struct Amostra {
    entrada: u64,
    leitura: u64,
    escrita: u64,
    saida: u64,
}

impl Amostra {
    fn total(&self) -> u64 {
        self.entrada.saturating_add(self.leitura).saturating_add(self.escrita).saturating_add(self.saida)
    }

    fn atualizar(&mut self, uso: &Value) {
        for (campo, valor) in [
            ("input_tokens", &mut self.entrada),
            ("cache_read_input_tokens", &mut self.leitura),
            ("cache_creation_input_tokens", &mut self.escrita),
            ("output_tokens", &mut self.saida),
        ] {
            if let Some(n) = uso[campo].as_u64() {
                *valor = n;
            }
        }
    }
}

/// Estado da tradução de uma sessão. Sobrevive entre turnos e entre processos do programa.
#[derive(Default)]
pub(super) struct Traducao {
    cwd: String,
    modelo: Option<String>,
    janelas: HashMap<String, u64>,
    amostra: Option<Amostra>,
    /// Mensagem de nível superior em streaming e o texto/pensamento já enviados por ela.
    mensagem: Option<String>,
    transmitido: HashMap<(String, &'static str), String>,
    pub(super) ferramentas: HashMap<String, Ferramenta>,
    anunciadas: HashSet<String>,
    concluidas: HashSet<String>,
    planos: HashSet<String>,
    compactacao: Option<String>,
}

fn texto(atualizacao: &str, conteudo: &str) -> Value {
    json!({"sessionUpdate": atualizacao, "content": {"type": "text", "text": conteudo}})
}

fn meta(nome: &str) -> Value {
    json!({"claudeCode": {"toolName": nome}})
}

/// Ferramentas de lista de tarefas: viram `plan`, não cartão de ferramenta.
fn e_plano(nome: &str) -> bool {
    nome == "TodoWrite"
}

fn caminho_exibido(caminho: &str, cwd: &str) -> String {
    if !cwd.is_empty()
        && let Ok(relativo) = Path::new(caminho).strip_prefix(cwd)
        && !relativo.as_os_str().is_empty()
    {
        return relativo.display().to_string();
    }
    caminho.to_owned()
}

/// Título, tipo, locais e conteúdo (diff) de uma chamada, a partir do nome e da entrada.
pub(super) fn descrever(nome: &str, entrada: &Value, cwd: &str) -> (String, &'static str, Value, Value) {
    let s = |k: &str| entrada[k].as_str().filter(|v| !v.is_empty());
    let caminho = s("file_path").or_else(|| s("notebook_path"));
    let locais = caminho.map_or_else(|| json!([]), |p| json!([{"path": p}]));
    let exibido = caminho.map(|p| caminho_exibido(p, cwd));
    match nome {
        "Bash" | "PowerShell" => (s("command").unwrap_or("Terminal").to_owned(), "execute", json!([]), json!([])),
        "Read" => {
            let linha = entrada["offset"].as_u64().unwrap_or(1);
            let local = caminho.map_or_else(|| json!([]), |p| json!([{"path": p, "line": linha}]));
            (exibido.map_or_else(|| "Read File".to_owned(), |p| format!("Read {p}")), "read", local, json!([]))
        }
        "Write" => {
            let diff = caminho.map_or_else(
                || json!([]),
                |p| json!([{"type": "diff", "path": p, "oldText": null, "newText": s("content").unwrap_or_default()}]),
            );
            (exibido.map_or_else(|| "Write".to_owned(), |p| format!("Write {p}")), "edit", locais, diff)
        }
        "Edit" | "MultiEdit" => {
            let diff = match caminho {
                Some(p) if s("old_string").is_some() || s("new_string").is_some() => json!([{"type": "diff",
                    "path": p, "oldText": s("old_string").unwrap_or_default(), "newText": s("new_string").unwrap_or_default()}]),
                _ => json!([]),
            };
            (exibido.map_or_else(|| "Edit".to_owned(), |p| format!("Edit {p}")), "edit", locais, diff)
        }
        "NotebookEdit" => {
            (exibido.map_or_else(|| "NotebookEdit".to_owned(), |p| format!("Edit {p}")), "edit", locais, json!([]))
        }
        "Glob" => {
            let mut titulo = "Find".to_owned();
            for chave in ["path", "pattern"] {
                if let Some(v) = s(chave) {
                    let _ = write!(titulo, " `{v}`");
                }
            }
            let local = s("path").map_or_else(|| json!([]), |p| json!([{"path": p}]));
            (titulo, "search", local, json!([]))
        }
        "Grep" => {
            let mut titulo = "grep".to_owned();
            if let Some(p) = s("pattern") {
                let _ = write!(titulo, " \"{p}\"");
            }
            if let Some(p) = s("path") {
                titulo.push(' ');
                titulo.push_str(p);
            }
            (titulo, "search", json!([]), json!([]))
        }
        "WebFetch" => {
            (s("url").map_or_else(|| "Fetch".to_owned(), |u| format!("Fetch {u}")), "fetch", json!([]), json!([]))
        }
        "WebSearch" => (
            s("query").map_or_else(|| "Web search".to_owned(), |q| format!("Search \"{q}\"")),
            "fetch",
            json!([]),
            json!([]),
        ),
        "Agent" | "Task" => (s("description").unwrap_or("Task").to_owned(), "think", json!([]), json!([])),
        "Skill" => (
            s("skill").map_or_else(|| "Load skill".to_owned(), |k| format!("Load skill: {k}")),
            "other",
            json!([]),
            json!([]),
        ),
        _ => (nome.to_owned(), "other", json!([]), json!([])),
    }
}

/// Limite do texto da projeção de uma ferramenta genérica, abaixo do corte do cartão no app (2000): com a
/// projeção dentro do limite, o aviso de corte do app nunca precisa aparecer.
const LIMITE_DETALHE: usize = 1800;

/// Linha final quando o corpo sanitizado não cabe: sem conferir tudo, só negar é possível.
const AVISO_LONGO: &str = "argumentos longos demais para conferir: só é possível negar";

/// Chaves cujo valor não aparece na projeção.
const CHAVES_SENSIVEIS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "senha",
    "authorization",
    "api_key",
    "apikey",
    "api-key",
    "cookie",
    "credential",
    "bearer",
    "private_key",
    "auth",
    "session",
];

/// Prefixos de credenciais conhecidas: a palavra que começa com um deles vira «oculto».
const PREFIXOS_SEGREDO: &[&str] =
    &["sk-", "sk_live_", "sk-ant-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "AKIA", "AIza", "eyJ"];

/// Ferramentas com projeção própria: o cartão já mostra comando, caminho ou diff. Todo o resto (MCP genérica
/// ou desconhecida) sai da ponte só pela projeção segura.
fn tem_projecao_propria(nome: &str) -> bool {
    matches!(
        nome,
        "Bash"
            | "PowerShell"
            | "Read"
            | "Write"
            | "Edit"
            | "MultiEdit"
            | "NotebookEdit"
            | "Glob"
            | "Grep"
            | "WebFetch"
            | "WebSearch"
            | "Agent"
            | "Task"
            | "Skill"
            | "TodoWrite"
    )
}

/// Servidor e nome real da ferramenta: `mcp__servidor__ferramenta` separa os dois; o resto é o nome inteiro.
fn servidor_ferramenta(nome: &str) -> (Option<&str>, &str) {
    match nome.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
        Some((servidor, ferramenta)) => (Some(servidor), ferramenta),
        None => (None, nome),
    }
}

fn e_sensivel(nome: &str) -> bool {
    let minuscula = nome.to_lowercase();
    CHAVES_SENSIVEIS.iter().any(|s| minuscula.contains(s))
}

/// Par nome/valor (`env`, `headers`…): o nome vem de `name`/`key`/`nome` e o valor de `value`/`valor`; nome
/// sensível oculta o valor.
fn par_com_nome_sensivel(mapa: &serde_json::Map<String, Value>) -> bool {
    let Some(nome) = ["name", "key", "nome"].into_iter().find_map(|c| mapa.get(c)).and_then(Value::as_str) else {
        return false;
    };
    ["value", "valor"].into_iter().any(|c| mapa.get(c).is_some()) && e_sensivel(nome)
}

/// URL dentro de um texto: a consulta e o fragmento podem carregar segredo.
static RE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b[a-z][a-z0-9+.-]*://[^\s"'<>`]+"#).expect("regex de URL da projeção"));

/// Uma URL sem userinfo, consulta e fragmento: só esquema, host e caminho; `?…` marca a consulta removida.
fn url_sem_segredo(url: &str) -> String {
    let sem_fragmento = url.split('#').next().unwrap_or(url);
    let (base, consulta) = match sem_fragmento.split_once('?') {
        Some((antes, _)) => (antes, true),
        None => (sem_fragmento, false),
    };
    let sem_userinfo = match base.split_once("://") {
        Some((esquema, depois)) => {
            let fim = depois.find('/').unwrap_or(depois.len());
            let hospedeiro = &depois[..fim];
            let limpo = hospedeiro.rsplit_once('@').map_or(hospedeiro, |(_, h)| h);
            format!("{esquema}://{limpo}{}", &depois[fim..])
        }
        None => base.to_owned(),
    };
    if consulta { format!("{sem_userinfo}?…") } else { sem_userinfo }
}

/// Palavra com cara de segredo: prefixo de credencial conhecido, ou sequência contígua de 32 ou mais
/// caracteres de `[A-Za-z0-9_+/=-]` misturando letras e dígitos.
fn palavra_e_segredo(palavra: &str) -> bool {
    if PREFIXOS_SEGREDO.iter().any(|p| palavra.starts_with(p)) {
        return true;
    }
    let sequencia = |tamanho: usize, letra: bool, digito: bool| tamanho >= 32 && letra && digito;
    let (mut tamanho, mut letra, mut digito) = (0usize, false, false);
    for c in palavra.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '/' | '=' | '-') {
            tamanho += 1;
            letra |= c.is_ascii_alphabetic();
            digito |= c.is_ascii_digit();
        } else {
            if sequencia(tamanho, letra, digito) {
                return true;
            }
            (tamanho, letra, digito) = (0, false, false);
        }
    }
    sequencia(tamanho, letra, digito)
}

/// Segredos soltos, palavra por palavra: prefixos conhecidos, sequências longas e o token depois de «Bearer».
fn ocultar_segredos_solto(texto: &str) -> String {
    const OCULTO: &str = "«oculto»";
    let mut out = String::with_capacity(texto.len());
    let mut bearer_anterior = false;
    for pedaco in texto.split_inclusive(char::is_whitespace) {
        let (palavra, espaco) = match pedaco.find(char::is_whitespace) {
            Some(i) => (&pedaco[..i], &pedaco[i..]),
            None => (pedaco, ""),
        };
        let oculto = (bearer_anterior || palavra_e_segredo(palavra)) && !palavra.is_empty();
        bearer_anterior = palavra.eq_ignore_ascii_case("bearer");
        out.push_str(if oculto { OCULTO } else { palavra });
        out.push_str(espaco);
    }
    out
}

/// Entrada sanitizada para a projeção, antes de qualquer medida: chaves sensíveis, pares nome/valor, URLs e
/// segredos soltos viram «oculto».
fn sanitizar(valor: &Value) -> Value {
    match valor {
        Value::Object(mapa) => {
            let par_sensivel = par_com_nome_sensivel(mapa);
            Value::Object(
                mapa.iter()
                    .map(|(k, v)| {
                        let oculto = e_sensivel(k) || (par_sensivel && matches!(k.as_str(), "value" | "valor"));
                        (k.clone(), if oculto { json!("«oculto»") } else { sanitizar(v) })
                    })
                    .collect(),
            )
        }
        Value::Array(itens) => Value::Array(itens.iter().map(sanitizar).collect()),
        Value::String(s) => {
            json!(ocultar_segredos_solto(&RE_URL.replace_all(s, |c: &Captures| url_sem_segredo(&c[0]))))
        }
        outro => outro.clone(),
    }
}

/// Projeção segura de uma ferramenta genérica para o cartão e o histórico.
pub(super) struct Projecao {
    /// Texto mostrado: cabeçalho, resumo dos argumentos inteiros e corpo, no máximo `LIMITE_DETALHE` caracteres.
    pub(super) texto: String,
    /// `true` só quando o corpo sanitizado inteiro cabe no texto.
    pub(super) cabe: bool,
    /// SHA-256 da entrada integral: a decisão vale para ela, não para o trecho mostrado.
    pub(super) sha256: String,
}

/// Projeção de uma ferramenta sem projeção própria (MCP genérica ou desconhecida): servidor, nome real e os
/// argumentos sanitizados que vão ser executados. Nada da entrada integral sai por aqui; se o corpo não cabe
/// inteiro, o texto termina avisando e a chamada não pode ser aprovada.
pub(super) fn projecao(nome: &str, entrada: &Value) -> Projecao {
    use sha2::{Digest, Sha256};
    let (servidor, ferramenta) = servidor_ferramenta(nome);
    let digest = Sha256::digest(entrada.to_string().as_bytes()).iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let corpo = serde_json::to_string_pretty(&sanitizar(entrada)).unwrap_or_default();
    let mut texto = String::new();
    if let Some(servidor) = servidor {
        let _ = writeln!(texto, "Servidor: {servidor}");
    }
    let _ = writeln!(texto, "Ferramenta: {ferramenta}");
    let _ = writeln!(texto, "Argumentos (SHA-256 dos argumentos inteiros: {digest}):");
    if texto.chars().count() + corpo.chars().count() <= LIMITE_DETALHE {
        texto.push_str(&corpo);
        return Projecao { texto, cabe: true, sha256: digest };
    }
    let aviso = format!("\n{AVISO_LONGO}");
    let aviso_chars = aviso.chars().count();
    if texto.chars().count() + aviso_chars > LIMITE_DETALHE {
        // Nome de ferramenta enorme: corta o próprio cabeçalho para o aviso continuar no fim do texto.
        texto = texto.chars().take(LIMITE_DETALHE - aviso_chars).collect();
    }
    let sobra = LIMITE_DETALHE.saturating_sub(texto.chars().count() + aviso_chars);
    texto.extend(corpo.chars().take(sobra));
    texto.push_str(&aviso);
    Projecao { texto, cabe: false, sha256: digest }
}

/// `rawInput` de uma ferramenta genérica: só a projeção, sem nenhuma chave que o cartão trate como ação.
fn raw_input_de_projecao(nome: &str, p: &Projecao) -> Value {
    let (servidor, ferramenta) = servidor_ferramenta(nome);
    json!({"servidor": servidor, "ferramenta": ferramenta, "resumo": p.texto, "completo": p.cabe,
        "sha256": p.sha256})
}

/// `content` de uma ferramenta genérica: o texto da projeção.
fn conteudo_de_projecao(p: &Projecao) -> Value {
    json!([{"type": "content", "content": {"type": "text", "text": p.texto}}])
}

/// Texto de um bloco `tool_result`: string ou lista de blocos de texto.
fn texto_resultado(conteudo: &Value) -> String {
    match conteudo {
        Value::String(s) => s.clone(),
        Value::Array(blocos) => blocos
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn entradas_do_plano(entrada: &Value) -> Value {
    let itens = entrada["todos"].as_array().into_iter().flatten().filter_map(|t| {
        let status @ ("pending" | "in_progress" | "completed") = t["status"].as_str()? else { return None };
        let conteudo = if status == "in_progress" {
            t["activeForm"].as_str().or_else(|| t["content"].as_str())
        } else {
            t["content"].as_str()
        }?;
        Some(json!({"content": conteudo, "status": status, "priority": "medium"}))
    });
    Value::Array(itens.collect())
}

impl Traducao {
    pub(super) fn new(cwd: &str) -> Self {
        Self { cwd: cwd.to_owned(), ..Self::default() }
    }

    pub(super) fn modelo(&self) -> Option<&str> {
        self.modelo.as_deref()
    }

    fn janela(&self) -> u64 {
        self.modelo.as_ref().and_then(|m| self.janelas.get(m)).copied().unwrap_or(JANELA_PADRAO)
    }

    /// `usage_update` com a última amostra (se houver) e campos extras.
    pub(super) fn uso(&self, extra: Option<(&str, Value)>, meta_extra: Option<(&str, Value)>) -> Value {
        let mut u = json!({"sessionUpdate": "usage_update"});
        if let Some(amostra) = &self.amostra {
            u["used"] = amostra.total().into();
            u["size"] = self.janela().into();
        }
        if let Some((k, v)) = extra {
            u[k] = v;
        }
        let mut m = serde_json::Map::new();
        if let Some(modelo) = &self.modelo {
            m.insert("_claude/model".into(), modelo.clone().into());
        }
        if let Some((k, v)) = meta_extra {
            m.insert(k.into(), v);
        }
        if !m.is_empty() {
            u["_meta"] = Value::Object(m);
        }
        u
    }

    /// Janelas por modelo informadas pelo resultado do turno.
    pub(super) fn janelas(&mut self, model_usage: &Value) {
        for (modelo, uso) in model_usage.as_object().into_iter().flatten() {
            if let Some(janela) = uso["contextWindow"].as_u64().filter(|j| *j > 0) {
                self.janelas.insert(modelo.clone(), janela);
            }
        }
    }

    fn anunciar(&mut self, id: &str, nome: &str, raiz: bool, out: &mut Vec<Value>) {
        if id.is_empty() || self.anunciadas.contains(id) {
            return;
        }
        self.anunciadas.insert(id.to_owned());
        self.ferramentas.entry(id.to_owned()).or_insert_with(|| Ferramenta {
            nome: nome.to_owned(),
            entrada: json!({}),
            raiz,
        });
        if e_plano(nome) {
            self.planos.insert(id.to_owned());
            return;
        }
        let (titulo, tipo, _, _) = descrever(nome, &json!({}), &self.cwd);
        out.push(json!({"sessionUpdate": "tool_call", "toolCallId": id, "status": "pending", "title": titulo,
            "kind": tipo, "rawInput": {}, "content": [], "_meta": meta(nome)}));
    }

    /// A chamada já terminou (resultado ou recusa registrados).
    pub(super) fn concluida(&self, id: &str) -> bool {
        self.concluidas.contains(id)
    }

    /// O `toolCall` de `session/request_permission` e se a aprovação pode ser oferecida: ferramenta genérica
    /// sem a entrada inteira à vista não traz a opção de aprovar.
    pub(super) fn chamada(&self, id: &str, nome: &str, entrada: &Value) -> (Value, bool) {
        let (titulo, tipo, locais, conteudo) = descrever(nome, entrada, &self.cwd);
        if tem_projecao_propria(nome) {
            return (
                json!({"toolCallId": id, "status": "pending", "title": titulo, "kind": tipo,
                "rawInput": entrada, "content": conteudo, "locations": locais, "_meta": meta(nome)}),
                true,
            );
        }
        let p = projecao(nome, entrada);
        (
            json!({"toolCallId": id, "status": "pending", "title": titulo, "kind": tipo,
            "rawInput": raw_input_de_projecao(nome, &p), "content": conteudo_de_projecao(&p),
            "locations": locais, "_meta": meta(nome)}),
            p.cabe,
        )
    }

    /// Um evento do programa (exceto `result`, tratado pelo turno) vira zero ou mais atualizações.
    pub(super) fn evento(&mut self, ev: &Value) -> Vec<Value> {
        let mut out = Vec::new();
        let raiz = ev["parent_tool_use_id"].is_null();
        match ev["type"].as_str().unwrap_or_default() {
            "system" => self.sistema(ev, &mut out),
            "stream_event" => self.parcial(&ev["event"], raiz, &mut out),
            "assistant" => self.assistente(&ev["message"], raiz, &mut out),
            "user" => self.usuario(&ev["message"], &mut out),
            "rate_limit_event" if ev["rate_limit_info"].is_object() => {
                out.push(self.uso(None, Some(("_claude/rateLimit", ev["rate_limit_info"].clone()))));
            }
            _ => {}
        }
        out
    }

    fn sistema(&mut self, ev: &Value, out: &mut Vec<Value>) {
        match ev["subtype"].as_str().unwrap_or_default() {
            "init" => {
                if let Some(modelo) = ev["model"].as_str().filter(|m| !m.is_empty()) {
                    self.modelo = Some(modelo.to_owned());
                }
            }
            "status" if ev["status"] == "compacting" && self.compactacao.is_none() => {
                let id = uuid::Uuid::new_v4().to_string();
                out.push(json!({"sessionUpdate": "compaction_update", "compactionId": id, "status": "in_progress"}));
                self.compactacao = Some(id);
            }
            "compact_boundary" => {
                let depois = ev["compact_metadata"]["post_tokens"].as_u64().unwrap_or(0);
                self.amostra = Some(Amostra { entrada: depois, ..Amostra::default() });
                out.push(self.uso(None, None));
                let id = self.compactacao.take().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                out.push(json!({"sessionUpdate": "compaction_update", "compactionId": id, "status": "completed"}));
            }
            "permission_denied" => {
                if let Some(id) = ev["tool_use_id"].as_str().filter(|id| self.anunciadas.contains(*id)) {
                    let nome = self.ferramentas.get(id).map(|f| f.nome.clone()).unwrap_or_default();
                    let motivo = ev["message"].as_str().unwrap_or("Permissão negada");
                    self.concluidas.insert(id.to_owned());
                    out.push(json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "failed",
                        "content": [{"type": "content", "content": {"type": "text", "text": motivo}}], "_meta": meta(&nome)}));
                }
            }
            _ => {}
        }
    }

    fn parcial(&mut self, ev: &Value, raiz: bool, out: &mut Vec<Value>) {
        match ev["type"].as_str().unwrap_or_default() {
            "message_start" if raiz => {
                let msg = &ev["message"];
                self.mensagem = msg["id"].as_str().map(str::to_owned);
                if let Some(modelo) = msg["model"].as_str().filter(|m| !m.is_empty()) {
                    self.modelo = Some(modelo.to_owned());
                }
                if msg["usage"].is_object() {
                    let mut amostra = Amostra::default();
                    amostra.atualizar(&msg["usage"]);
                    self.amostra = Some(amostra);
                    out.push(self.uso(None, None));
                }
            }
            "message_delta" if raiz && ev["usage"].is_object() => {
                self.amostra.get_or_insert_with(Amostra::default).atualizar(&ev["usage"]);
                out.push(self.uso(None, None));
            }
            "content_block_start" => {
                let bloco = &ev["content_block"];
                if matches!(bloco["type"].as_str(), Some("tool_use" | "server_tool_use" | "mcp_tool_use"))
                    && let (Some(id), Some(nome)) = (bloco["id"].as_str(), bloco["name"].as_str())
                {
                    self.anunciar(id, nome, raiz, out);
                }
            }
            "content_block_delta" if raiz => {
                let delta = &ev["delta"];
                let (tipo, chave) = match delta["type"].as_str() {
                    Some("text_delta") => ("agent_message_chunk", "text"),
                    Some("thinking_delta") => ("agent_thought_chunk", "thinking"),
                    _ => return,
                };
                let Some(pedaco) = delta[chave].as_str().filter(|t| !t.is_empty()) else { return };
                if let Some(msg) = &self.mensagem {
                    self.transmitido.entry((msg.clone(), tipo)).or_default().push_str(pedaco);
                }
                out.push(texto(tipo, pedaco));
            }
            _ => {}
        }
    }

    fn assistente(&mut self, msg: &Value, raiz: bool, out: &mut Vec<Value>) {
        let id_msg = msg["id"].as_str().unwrap_or_default().to_owned();
        for bloco in msg["content"].as_array().into_iter().flatten() {
            match bloco["type"].as_str().unwrap_or_default() {
                tipo @ ("text" | "thinking") if raiz => {
                    let chave = if tipo == "text" { "text" } else { "thinking" };
                    let Some(t) = bloco[chave].as_str().filter(|t| !t.trim().is_empty()) else { continue };
                    let atualizacao = if tipo == "text" { "agent_message_chunk" } else { "agent_thought_chunk" };
                    if self.transmitido.get(&(id_msg.clone(), atualizacao)).is_some_and(|ja| ja.contains(t)) {
                        continue;
                    }
                    out.push(texto(atualizacao, t));
                }
                "web_search_tool_result" | "web_fetch_tool_result" => self.resultado(bloco, out),
                "tool_use" | "server_tool_use" | "mcp_tool_use" => {
                    let (Some(id), Some(nome)) = (bloco["id"].as_str(), bloco["name"].as_str()) else { continue };
                    self.anunciar(id, nome, raiz, out);
                    let entrada = bloco["input"].clone();
                    if let Some(f) = self.ferramentas.get_mut(id) {
                        f.entrada = entrada.clone();
                    }
                    if self.planos.contains(id) {
                        out.push(json!({"sessionUpdate": "plan", "entries": entradas_do_plano(&entrada)}));
                        continue;
                    }
                    let (titulo, tipo, locais, conteudo) = descrever(nome, &entrada, &self.cwd);
                    // Ferramenta genérica: nem o `rawInput` nem o `content` do histórico levam a entrada
                    // integral, só a projeção sanitizada.
                    let (raw, conteudo) = if tem_projecao_propria(nome) {
                        (entrada.clone(), conteudo)
                    } else {
                        let p = projecao(nome, &entrada);
                        (raw_input_de_projecao(nome, &p), conteudo_de_projecao(&p))
                    };
                    let mut u = json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "title": titulo,
                        "kind": tipo, "rawInput": raw, "_meta": meta(nome)});
                    if locais.as_array().is_some_and(|l| !l.is_empty()) {
                        u["locations"] = locais;
                    }
                    if conteudo.as_array().is_some_and(|c| !c.is_empty()) {
                        u["content"] = conteudo;
                    }
                    out.push(u);
                }
                _ => {}
            }
        }
    }

    fn usuario(&mut self, msg: &Value, out: &mut Vec<Value>) {
        for bloco in msg["content"].as_array().into_iter().flatten() {
            self.resultado(bloco, out);
        }
    }

    /// Resultado de uma chamada anunciada: de ferramenta local ou MCP (na mensagem do usuário) ou de ferramenta do
    /// servidor, como busca e leitura na web (na própria mensagem do assistente).
    fn resultado(&mut self, bloco: &Value, out: &mut Vec<Value>) {
        if !matches!(
            bloco["type"].as_str(),
            Some("tool_result" | "mcp_tool_result" | "web_search_tool_result" | "web_fetch_tool_result")
        ) {
            return;
        }
        let Some(id) = bloco["tool_use_id"].as_str() else { return };
        if self.planos.contains(id) || !self.anunciadas.contains(id) || self.concluidas.contains(id) {
            return;
        }
        self.concluidas.insert(id.to_owned());
        let nome = self.ferramentas.get(id).map(|f| f.nome.clone()).unwrap_or_default();
        let erro =
            bloco["is_error"] == true || bloco["content"]["type"].as_str().is_some_and(|t| t.ends_with("_error"));
        let status = if erro { "failed" } else { "completed" };
        let mut u = json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status,
                "rawOutput": bloco["content"].clone(), "_meta": meta(&nome)});
        let t = texto_resultado(&bloco["content"]);
        if !t.is_empty() {
            u["content"] = json!([{"type": "content", "content": {"type": "text", "text": t}}]);
        }
        out.push(u);
    }

    /// Chamadas ainda abertas ao fim de um turno sem resultado delas fecham como falha; uma compactação sem o marco de
    /// fim (turno cancelado no meio) também se encerra, para a próxima ser anunciada.
    pub(super) fn fechar_abertas(&mut self) -> Vec<Value> {
        let compactacao = self
            .compactacao
            .take()
            .map(|id| json!({"sessionUpdate": "compaction_update", "compactionId": id, "status": "failed"}));
        let abertas: Vec<String> = self
            .anunciadas
            .iter()
            .filter(|id| !self.concluidas.contains(*id) && !self.planos.contains(*id))
            .cloned()
            .collect();
        abertas
            .into_iter()
            .map(|id| {
                let nome = self.ferramentas.get(&id).map(|f| f.nome.clone()).unwrap_or_default();
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "failed", "_meta": meta(&nome)})
            })
            .chain(compactacao)
            .collect()
    }

    /// Ao fim do turno: esquece o que já terminou para não crescer sem limite.
    pub(super) fn encerrar_turno(&mut self) {
        self.anunciadas.clear();
        self.concluidas.clear();
        self.planos.clear();
        self.ferramentas.clear();
        self.transmitido.clear();
        self.mensagem = None;
    }
}
