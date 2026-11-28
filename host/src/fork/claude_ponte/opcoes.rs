//! Pedido de sessão ACP (`session/new`/`session/load`) → linha de comando do programa oficial no modo sem
//! tela. Só as opções que o Hub envia hoje são aceitas; qualquer outra, ou um valor com tipo errado, recusa a sessão
//! (falha fechada), para que nada seja ignorado em silêncio.

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PastaAdicional {
    caminho: PathBuf,
    #[cfg(unix)]
    identidade: (u64, u64),
}

impl PastaAdicional {
    fn conferir(original: &Path) -> Result<Self> {
        ensure!(original.is_absolute(), "pasta adicional inválida");
        let caminho = original.canonicalize().context("pasta adicional inválida")?;
        let meta = caminho.symlink_metadata().context("pasta adicional inválida")?;
        ensure!(meta.is_dir(), "pasta adicional inválida");
        Ok(Self {
            caminho,
            #[cfg(unix)]
            identidade: {
                use std::os::unix::fs::MetadataExt as _;
                (meta.dev(), meta.ino())
            },
        })
    }
}

/// Servidor MCP da própria ponte (pedido de permissão). O Hub não pode usar este nome.
pub(super) const SERVIDOR_PERMISSAO: &str = "mexerica_ponte";
pub(super) const FERRAMENTA_PERMISSAO: &str = "permissao";
/// Ferramenta do Claude Code que pergunta à pessoa no meio do turno.
const FERRAMENTA_PERGUNTA: &str = "AskUserQuestion";

pub(super) fn nome_ferramenta_permissao() -> String {
    format!("mcp__{SERVIDOR_PERMISSAO}__{FERRAMENTA_PERMISSAO}")
}

/// Como o prompt de sistema chega ao programa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PromptSistema {
    /// Mantém o prompt do Claude Code e acrescenta este texto (`--append-system-prompt-file`).
    Acrescentar(String),
    /// Substitui o prompt inteiro (`--system-prompt-file`).
    Substituir(String),
}

#[derive(Clone, Debug)]
pub(super) struct Opcoes {
    pub cwd: PathBuf,
    /// `{"nome": configuração}` no formato do `--mcp-config`.
    pub mcp: Map<String, Value>,
    pub prompt: Option<PromptSistema>,
    pub settings: Option<Value>,
    pub setting_sources: Vec<String>,
    pub negadas: Vec<String>,
    pub permitidas: Vec<String>,
    pub ferramentas: Option<Vec<String>>,
    pub persistir: bool,
    pub max_turnos: Option<u64>,
    pub modelo: Option<String>,
    pub esforco: Option<String>,
    pastas: Vec<(PathBuf, PastaAdicional)>,
}

pub(super) const ESFORCOS: &[&str] = &["default", "low", "medium", "high", "xhigh", "max"];

/// Identificador de modelo aceito na linha de comando: apelido documentado ou nome completo, sem começar por traço.
pub(super) fn modelo_valido(id: &str) -> bool {
    id.len() <= 100
        && id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._[]".contains(&b))
}

fn lista(valor: &Value, campo: &str) -> Result<Vec<String>> {
    let Some(itens) = valor.as_array() else { bail!("opção {campo} precisa ser uma lista") };
    itens
        .iter()
        .map(|v| {
            let s = v.as_str().ok_or_else(|| anyhow::anyhow!("opção {campo} precisa ser uma lista de textos"))?;
            ensure!(!s.contains(['\n', '\0']), "opção {campo} com texto inválido");
            Ok(s.to_owned())
        })
        .collect()
}

/// Lista de ferramentas que vira um único argumento separado por vírgula: item com vírgula ou traço inicial recusa.
fn ferramentas(valor: &Value, campo: &str) -> Result<Vec<String>> {
    let itens = lista(valor, campo)?;
    ensure!(itens.iter().all(|i| !i.contains(',') && !i.starts_with('-')), "opção {campo} com item inválido");
    Ok(itens)
}

/// `[{name, value}]` do ACP; ausente vale vazio, qualquer outro formato recusa.
fn pares(valor: &Value, campo: &str) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    if valor.is_null() {
        return Ok(out);
    }
    let Some(itens) = valor.as_array() else { bail!("{campo} do servidor MCP inválido") };
    for item in itens {
        let (Some(nome), Some(v)) = (item["name"].as_str(), item["value"].as_str()) else {
            bail!("{campo} do servidor MCP inválido");
        };
        out.insert(nome.to_owned(), v.into());
    }
    Ok(out)
}

/// Só as configurações que o Hub envia hoje; o modo de permissão nunca sai do padrão e nenhum gancho entra.
fn settings(valor: &Value) -> Result<Value> {
    let Some(campos) = valor.as_object() else { bail!("settings precisa ser um objeto") };
    for (chave, v) in campos {
        match chave.as_str() {
            "includeGitInstructions" | "autoMemoryEnabled" | "autoDreamEnabled" | "disableAllHooks" => {
                ensure!(v.is_boolean(), "settings.{chave} inválido");
            }
            "attribution" => {
                let ok = v
                    .as_object()
                    .is_some_and(|a| a.iter().all(|(k, t)| matches!(k.as_str(), "commit" | "pr") && t.is_string()));
                ensure!(ok, "settings.attribution inválido");
            }
            "permissions" => {
                let Some(p) = v.as_object() else { bail!("settings.permissions inválido") };
                for (k, regra) in p {
                    match k.as_str() {
                        "defaultMode" => ensure!(regra == "default", "a ponte só abre no modo de permissão padrão"),
                        "allow" | "deny" => {
                            lista(regra, "settings.permissions")?;
                        }
                        _ => bail!("settings.permissions.{k} não suportado"),
                    }
                }
            }
            _ => bail!("settings.{chave} não suportado pela ponte"),
        }
    }
    Ok(valor.clone())
}

/// Servidores MCP do pedido ACP no formato do `--mcp-config`.
fn servidores(lista_acp: &Value) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    if lista_acp.is_null() {
        return Ok(out);
    }
    let Some(itens) = lista_acp.as_array() else { bail!("mcpServers precisa ser uma lista") };
    for s in itens {
        let nome = s["name"]
            .as_str()
            .filter(|n| !n.is_empty() && n.len() <= 128)
            .ok_or_else(|| anyhow::anyhow!("servidor MCP sem nome"))?;
        ensure!(nome != SERVIDOR_PERMISSAO, "nome de servidor MCP reservado: {nome}");
        ensure!(!out.contains_key(nome), "servidor MCP repetido: {nome}");
        ensure!(s["type"].is_null() || s["type"].is_string(), "tipo de servidor MCP inválido");
        let campos: &[&str] = match s["type"].as_str() {
            None | Some("stdio") => &["name", "type", "command", "args", "env"],
            _ => &["name", "type", "url", "headers"],
        };
        if let Some(k) = s.as_object().and_then(|o| o.keys().find(|k| !campos.contains(&k.as_str()))) {
            bail!("campo de servidor MCP não suportado: {k}");
        }
        let config = match s["type"].as_str() {
            None | Some("stdio") => {
                let comando = s["command"]
                    .as_str()
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("servidor MCP sem comando"))?;
                let args = if s["args"].is_null() { Vec::new() } else { lista(&s["args"], "args")? };
                json!({"type": "stdio", "command": comando, "args": args, "env": pares(&s["env"], "env")?})
            }
            Some(tipo @ ("http" | "sse")) => {
                let url = s["url"]
                    .as_str()
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("servidor MCP sem endereço"))?;
                json!({"type": tipo, "url": url, "headers": pares(&s["headers"], "headers")?})
            }
            Some(outro) => bail!("tipo de servidor MCP não suportado: {outro}"),
        };
        out.insert(nome.to_owned(), config);
    }
    Ok(out)
}

fn prompt_sistema(params: &Value) -> Result<Option<PromptSistema>> {
    let acp = &params["systemPrompt"];
    if !acp.is_null() {
        let texto = acp["content"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("systemPrompt sem conteúdo"))?
            .iter()
            .map(|b| if b["type"] == "text" { b["text"].as_str().map(str::to_owned) } else { None })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| anyhow::anyhow!("systemPrompt com bloco que não é texto"))?
            .join("\n\n");
        return Ok(Some(match acp["mode"].as_str() {
            Some("append") => PromptSistema::Acrescentar(texto),
            Some("override") => PromptSistema::Substituir(texto),
            _ => bail!("modo de systemPrompt não suportado"),
        }));
    }
    Ok(match &params["_meta"]["systemPrompt"] {
        Value::Null => None,
        Value::String(s) => Some(PromptSistema::Substituir(s.clone())),
        Value::Object(o) => {
            ensure!(
                o.get("type").is_none_or(|t| t == "preset") && o.get("preset").is_none_or(|p| p == "claude_code"),
                "systemPrompt de _meta não suportado"
            );
            ensure!(
                o.keys().all(|k| matches!(k.as_str(), "type" | "preset" | "append")),
                "systemPrompt de _meta com campo desconhecido"
            );
            match o.get("append") {
                None => None,
                Some(Value::String(s)) => Some(PromptSistema::Acrescentar(s.clone())),
                Some(_) => bail!("systemPrompt de _meta inválido"),
            }
        }
        _ => bail!("systemPrompt de _meta inválido"),
    })
}

/// Lê e valida um pedido `session/new` ou `session/load`.
pub(super) fn ler(params: &Value) -> Result<Opcoes> {
    let Some(campos) = params.as_object() else { bail!("pedido de sessão inválido") };
    ensure!(
        campos.keys().all(|k| matches!(
            k.as_str(),
            "cwd" | "mcpServers" | "additionalDirectories" | "systemPrompt" | "_meta" | "sessionId"
        )),
        "campo do pedido de sessão não suportado"
    );
    let cwd = PathBuf::from(params["cwd"].as_str().unwrap_or_default());
    ensure!(cwd.is_absolute() && cwd.is_dir(), "pasta de trabalho inválida");
    let mut pastas = Vec::new();
    if !params["additionalDirectories"].is_null() {
        for p in lista(&params["additionalDirectories"], "additionalDirectories")? {
            let p = PathBuf::from(p);
            let conferida = PastaAdicional::conferir(&p)?;
            pastas.push((p, conferida));
        }
    }
    let mut opcoes = Opcoes {
        cwd,
        mcp: servidores(&params["mcpServers"])?,
        prompt: prompt_sistema(params)?,
        settings: None,
        setting_sources: Vec::new(),
        negadas: Vec::new(),
        permitidas: Vec::new(),
        ferramentas: None,
        persistir: true,
        max_turnos: None,
        modelo: None,
        esforco: None,
        pastas,
    };
    let meta = &params["_meta"];
    ensure!(meta.is_null() || meta.is_object(), "_meta inválido");
    if let Some(chaves) = meta.as_object() {
        ensure!(
            chaves.keys().all(|k| matches!(k.as_str(), "systemPrompt" | "claudeCode")),
            "campo de _meta não suportado"
        );
    }
    let claude = &meta["claudeCode"];
    ensure!(claude.is_null() || claude.is_object(), "_meta.claudeCode inválido");
    if let Some(chaves) = claude.as_object() {
        ensure!(chaves.keys().all(|k| k == "options"), "campo de _meta.claudeCode não suportado");
    }
    ensure!(claude["options"].is_null() || claude["options"].is_object(), "_meta.claudeCode.options inválido");
    for (chave, valor) in claude["options"].as_object().into_iter().flatten() {
        match chave.as_str() {
            "settings" => opcoes.settings = Some(settings(valor)?),
            "settingSources" => {
                opcoes.setting_sources = lista(valor, chave)?;
                ensure!(
                    opcoes.setting_sources.iter().all(|s| matches!(s.as_str(), "user" | "project" | "local")),
                    "settingSources desconhecido"
                );
            }
            "strictMcpConfig" => ensure!(valor == true, "a ponte sempre isola os servidores MCP"),
            "disallowedTools" => opcoes.negadas = ferramentas(valor, chave)?,
            "allowedTools" => opcoes.permitidas = ferramentas(valor, chave)?,
            "tools" => opcoes.ferramentas = Some(ferramentas(valor, chave)?),
            "permissionMode" => ensure!(valor == "default", "a ponte só abre no modo de permissão padrão"),
            "allowDangerouslySkipPermissions" => ensure!(valor == false, "a ponte nunca pula permissões"),
            "persistSession" => {
                opcoes.persistir = valor.as_bool().ok_or_else(|| anyhow::anyhow!("persistSession inválido"))?;
            }
            "maxTurns" => {
                opcoes.max_turnos =
                    Some(valor.as_u64().filter(|n| *n > 0).ok_or_else(|| anyhow::anyhow!("maxTurns inválido"))?);
            }
            "model" => {
                if !valor.is_null() {
                    let m = valor
                        .as_str()
                        .filter(|m| modelo_valido(m))
                        .ok_or_else(|| anyhow::anyhow!("modelo inválido"))?;
                    opcoes.modelo = Some(m.to_owned());
                }
            }
            "effort" => {
                let e = valor
                    .as_str()
                    .filter(|e| ESFORCOS.contains(e))
                    .ok_or_else(|| anyhow::anyhow!("esforço inválido"))?;
                opcoes.esforco = Some(e.to_owned());
            }
            outra => bail!("opção não suportada pelo programa oficial: {outra}"),
        }
    }
    Ok(opcoes)
}

/// Arquivos que a ponte gravou para o programa ler.
pub(super) struct Arquivos {
    pub mcp: PathBuf,
    pub settings: Option<PathBuf>,
    pub prompt: Option<PathBuf>,
}

/// Argumentos do programa (sem o executável). Valores livres vão sempre colados à opção (`--opcao=valor`) ou
/// como caminho de arquivo da própria ponte, nunca como argumento solto que possa virar outra opção.
pub(super) fn argumentos(
    o: &Opcoes,
    sessao: &str,
    retomar: bool,
    modelo: &str,
    esforco: &str,
    arquivos: &Arquivos,
) -> Result<Vec<String>> {
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-mode",
        "default",
        "--strict-mcp-config",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    a.push("--permission-prompt-tool".into());
    a.push(nome_ferramenta_permissao());
    a.push("--mcp-config".into());
    a.push(arquivos.mcp.display().to_string());
    a.push(format!("--setting-sources={}", o.setting_sources.join(",")));
    if let Some(s) = &arquivos.settings {
        a.push("--settings".into());
        a.push(s.display().to_string());
    }
    match (&o.prompt, &arquivos.prompt) {
        (Some(PromptSistema::Acrescentar(_)), Some(p)) => {
            a.push("--append-system-prompt-file".into());
            a.push(p.display().to_string());
        }
        (Some(PromptSistema::Substituir(_)), Some(p)) => {
            a.push("--system-prompt-file".into());
            a.push(p.display().to_string());
        }
        _ => {}
    }
    if let Some(f) = &o.ferramentas {
        a.push(format!("--tools={}", f.join(",")));
    }
    // Pergunta ao usuário dentro da ferramenta não tem quem responda pela ponte (o Hub não negocia formulário):
    // fica fora, como no adaptador anterior.
    let mut negadas = o.negadas.clone();
    if !negadas.iter().any(|n| n == "*" || n == FERRAMENTA_PERGUNTA) {
        negadas.push(FERRAMENTA_PERGUNTA.to_owned());
    }
    a.push(format!("--disallowedTools={}", negadas.join(",")));
    if !o.permitidas.is_empty() {
        a.push(format!("--allowedTools={}", o.permitidas.join(",")));
    }
    if modelo != "default" {
        a.push(format!("--model={modelo}"));
    }
    if esforco != "default" {
        a.push(format!("--effort={esforco}"));
    }
    if let Some(n) = o.max_turnos {
        a.push(format!("--max-turns={n}"));
    }
    for (original, esperada) in &o.pastas {
        let atual = PastaAdicional::conferir(original)?;
        ensure!(&atual == esperada, "pasta adicional mudou desde a abertura da sessão");
        let caminho = atual.caminho.to_str().context("caminho de pasta adicional não é UTF-8")?;
        a.push(format!("--add-dir={caminho}"));
    }
    if !o.persistir {
        a.push("--no-session-persistence".into());
    }
    a.push(if retomar { format!("--resume={sessao}") } else { format!("--session-id={sessao}") });
    Ok(a)
}

/// Configuração MCP: os servidores do Hub mais o servidor de permissão da ponte.
pub(super) fn config_mcp(o: &Opcoes, url: &str, segredo: &str) -> Value {
    let mut servidores = o.mcp.clone();
    servidores.insert(
        SERVIDOR_PERMISSAO.into(),
        json!({"type": "http", "url": url, "headers": {"Authorization": format!("Bearer {segredo}")}, "timeout": 86_400_000}),
    );
    json!({"mcpServers": servidores})
}
