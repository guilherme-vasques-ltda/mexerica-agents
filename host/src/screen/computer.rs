//! Bots operating the computer: the `computer` MCP tools, who is in control, and signing in.
//! Tools run on the computer-use driver (`cua.rs`); on Linux desktops it doesn't support, on
//! Codync Screen's portal input (`helper_tools.rs`).

#[cfg(target_os = "linux")]
use super::helper_tools;
use super::{AGENT_HOLD, Screen, cua, fork_observation::Attempt};
use crate::LockExt;
use crate::hub::Hub;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

const TYPE_LOGIN: &str = "type_login";

/// What runs the tools on this computer.
#[derive(Clone, Copy)]
enum Backend {
    Driver,
    #[cfg(target_os = "linux")]
    Helper,
}

#[cfg(not(target_os = "linux"))]
fn backend() -> Backend {
    Backend::Driver
}

#[cfg(target_os = "linux")]
fn backend() -> Backend {
    if cua::linux_session() && cua::executable().is_some() { Backend::Driver } else { Backend::Helper }
}

impl Screen {
    /// Whether bots may use the computer here: Windows needs nothing else; macOS and Linux run
    /// their part through Codync Screen, which runs while Remote screen is on.
    pub(super) fn computer_use(&self) -> bool {
        cfg!(windows) || self.enabled()
    }

    /// Marks `bot` as the one using the computer. Returns whether that's news.
    #[cfg(test)]
    fn claim(&self, bot: &str, read_only: bool) -> Result<bool> {
        let mut c = self.control.locked();
        if c.user && !read_only {
            bail!(
                "The user has taken over the screen from their phone. Wait until they hand it back, or ask them what to do."
            );
        }
        if let Some(other) = c.active_agent()
            && other != bot
        {
            bail!("Another bot is using the computer right now. Try again in a minute.");
        }
        let fresh = c.active_agent().is_none();
        c.agent = Some((bot.to_owned(), Instant::now()));
        Ok(fresh)
    }
}

/// The `computer` MCP tools on this computer.
pub async fn computer_tools(hub: &Arc<Hub>) -> Value {
    let screen = &hub.screen;
    let mut tools = match backend() {
        Backend::Driver => cua::tools(screen).await,
        #[cfg(target_os = "linux")]
        Backend::Helper => helper_tools::tools(),
    };
    if cfg!(not(windows)) {
        tools.push(type_login_tool());
    }
    for tool in &mut tools {
        let description = tool["description"].as_str().unwrap_or_default().to_owned();
        tool["description"] = format!("{description} {}", super::fork_observation::INSTRUCTIONS).into();
    }
    json!({"tools": tools})
}

pub(crate) fn observation_read_only(name: &str) -> bool {
    match backend() {
        Backend::Driver => cua::read_only(name),
        #[cfg(target_os = "linux")]
        Backend::Helper => helper_tools::read_only(name),
    }
}

/// A origem vem do dispatch; entradas internas sem captura são recusadas.
pub(crate) async fn computer(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    mut args: Value,
    original: Option<&Arc<Attempt>>,
) -> Result<Value> {
    if name == TYPE_LOGIN {
        crate::fork::secret_refs::reject_argument(&args, "login")?;
    }
    if let Some(args) = args.as_object_mut() {
        args.remove("screenReason");
    }
    let attempt = original.ok_or_else(|| anyhow!("Observação indisponível: origem original ausente"))?;
    attempt.verify()?;
    let result = computer_run(hub, bot, name, args, attempt).await;
    attempt.verify()?;
    result
}

/// Runs one `computer` tool for `bot`: `{content, isError?}` for the agent.
async fn computer_run(hub: &Arc<Hub>, bot: &str, name: &str, args: Value, attempt: &Arc<Attempt>) -> Result<Value> {
    if name == TYPE_LOGIN {
        crate::fork::secret_refs::reject_argument(&args, "login")?;
    }
    let row = hub.store.bot(bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
    if !row.config.computer {
        bail!("Computer use is turned off for this bot.");
    }
    let screen = &hub.screen;
    if !screen.computer_use() {
        bail!("Remote screen is turned off on this computer. Turn it on in Mexerica's menu there.");
    }
    let backend = backend();
    if matches!(backend, Backend::Driver) && name != TYPE_LOGIN && !cua::offered(name) {
        bail!("unknown computer tool `{name}`");
    }
    if attempt.claim()? {
        screen.emit();
        let screen = screen.clone();
        tokio::spawn(async move {
            // Say when the bot stops, so phones drop the live chip.
            loop {
                tokio::time::sleep(AGENT_HOLD).await;
                if screen.control.locked().active_agent().is_none() {
                    screen.emit();
                    return;
                }
            }
        });
    }
    if name == TYPE_LOGIN {
        return type_login(hub, bot, &row.config.logins, backend, args, attempt).await;
    }
    match backend {
        Backend::Driver => cua::call(screen, bot, name, args, attempt).await,
        #[cfg(target_os = "linux")]
        Backend::Helper => helper_tools::run(screen, name, args, attempt).await,
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum LoginField {
    Username,
    Password,
    /// Código de dois fatores pedido com `request_login_code`; uso único, sem campo seguro.
    Code,
}

#[derive(serde::Deserialize)]
struct LoginArgs {
    login: String,
    field: LoginField,
    pid: Option<i64>,
}

fn type_login_tool() -> Value {
    json!({
        "name": TYPE_LOGIN,
        "description": "Digita um login salvo ligado para você (list_logins / request_login) no campo em foco: clique no campo e passe o pid do app. Só funciona no site/app do login; password exige campo de senha. field \"code\" digita o código de dois fatores pedido com request_login_code e envia (Enter): o Hub só digita com prova positiva de campo curto e numérico (autocomplete de uso único, ou tipo/inputmode numérico; tamanho declarado precisa ser curto quando exposto) — nunca texto livre, busca, campo longo ou metadados ausentes; sem prova, ou sem código pronto e válido, recusa e pede para a pessoa digitar no site. Vale uma vez só, por 5 minutos; depois de digitado pode aparecer na tela. A marca reservada {{senha: em login, mesmo malformada, recusa antes do cofre ou preenchimento. O caminho protegido e a coleta privada ainda estão indisponíveis; não contorne a recusa.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "login": {"type": "string", "description": "ID de list_logins. Rótulo ou site também servem quando só um login corresponde; com várias contas, passe o ID indicado pela tarefa. Não aceita a marca reservada {{senha:, inclusive sem fechamento."},
                "field": {"type": "string", "enum": ["username", "password", "code"]},
                "pid": {"type": "integer", "description": "The app with the sign-in field (from list_windows); the frontmost app when left out."},
            },
            "required": ["login", "field"],
        },
    })
}

/// Types a saved login only into its own site or app, and a password only into a password
/// field, so a misled bot can't paste it into a page or chat that would show it. Codync Screen
/// checks the focused field (accessibility): which app, which web page, whether it's secure.
/// Windows has no Codync Screen to check it yet.
async fn type_login(
    hub: &Arc<Hub>,
    bot: &str,
    on: &[String],
    backend: Backend,
    args: Value,
    attempt: &Arc<Attempt>,
) -> Result<Value> {
    if cfg!(windows) {
        bail!("type_login isn't available on Windows yet. Ask the user to sign in themselves.");
    }
    let a: LoginArgs = serde_json::from_value(args).map_err(|e| anyhow!("invalid type_login call: {e}"))?;
    // D82: only the logins on for this bot, and it says which when several fit.
    let login = crate::market::logins::pick(&hub.store, on, &a.login)?;
    let screen = &hub.screen;
    let focus = screen.link()?.request_observed("focusedField", json!({"pid": a.pid}), attempt).await?;
    let app = focus["app"].as_str().unwrap_or_default();
    let url = focus["url"].as_str();
    if !crate::market::logins::matches(&login.site, url, app) {
        bail!(
            "The focused field is in {}, not {}. Open {} and click its sign-in field first.",
            url.unwrap_or(app),
            login.site,
            login.site
        );
    }
    let (text, secret, pid) = match a.field {
        LoginField::Username => (login.username, false, focus["pid"].clone()),
        LoginField::Password => {
            if focus["secure"] != true {
                bail!("Click into the password field first; type_login types a password only into a password field.");
            }
            (crate::market::passwords::resolve(&hub.store, &login.password).await?, true, focus["pid"].clone())
        }
        LoginField::Code => {
            let timing = CodeTiming { pid: a.pid, prepare: PREPARE_TIMEOUT, critical: CRITICAL_TIMEOUT };
            return type_login_code(hub, bot, &login, backend, attempt, &focus, timing).await;
        }
    };
    match backend {
        Backend::Driver => {
            // macOS: into the checked app, in the background; Linux: the active window it checked.
            let args = if cfg!(target_os = "macos") {
                json!({"pid": pid, "text": text})
            } else {
                json!({"scope": "desktop", "text": text})
            };
            // O driver's answer may quote what it typed: the agent gets only the outcome.
            // No caminho do segredo o erro externo também pode ecoar o valor: a
            // resposta é sempre a mensagem fixa, sem corpo nem cadeia de erro.
            let res = if secret {
                cua::call(screen, bot, "type_text", args, attempt).await.map_err(|_| driver_secret_error())?
            } else {
                cua::call(screen, bot, "type_text", args, attempt).await?
            };
            if res["isError"] == true {
                bail!("Não consegui digitar o valor no campo de {app}. Clique de novo e tente outra vez.");
            }
            Ok(
                json!({"content": [{"type": "text", "text": format!("O valor salvo foi digitado em {app}. Olhe a tela de novo para conferir se entrou.")}]}),
            )
        }
        #[cfg(target_os = "linux")]
        Backend::Helper => {
            if secret {
                // Segredo nunca volta em screenshot, árvore ou eco: resposta fixa.
                helper_tools::type_secret(screen, text, false, attempt).await
            } else {
                helper_tools::type_text(screen, text, attempt).await
            }
        }
    }
}

/// Prazo total para preparar/reservar o backend do segredo: estourou, recusa
/// sem consumir e sem reter MUTATION.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(5);
/// Prazo total do trecho prova final → consumo → digitação + Enter.
const CRITICAL_TIMEOUT: Duration = Duration::from_secs(10);

/// Parâmetros do caminho do código: pid pedido e os dois prazos curtos.
struct CodeTiming {
    pid: Option<i64>,
    prepare: Duration,
    critical: Duration,
}

/// Backend preparado para o segredo: driver com o daemon reservado (mutex
/// retido até o último envio) ou helper só com o link conferido.
enum Prepared<'a> {
    Driver(cua::Reservation<'a>),
    #[cfg(target_os = "linux")]
    Helper,
}

/// Prepara e reserva o backend ANTES da última prova de foco e sem reter
/// MUTATION: o `start`/ocupação do daemon acontece aqui, com prazo curto.
async fn prepare_secret<'a>(screen: &'a Screen, backend: Backend, attempt: &Arc<Attempt>) -> Result<Prepared<'a>> {
    match backend {
        Backend::Driver => Ok(Prepared::Driver(screen.driver.reserve(screen, attempt).await?)),
        #[cfg(target_os = "linux")]
        Backend::Helper => {
            // O helper não tem mutex de daemon: conferir o link já falha cedo.
            screen.link()?;
            Ok(Prepared::Helper)
        }
    }
}

/// `field: "code"`: prova positiva do campo, backend preparado e reservado,
/// depois — sob MUTATION e com prazo curto — última prova de foco, consumo do
/// código e digitação + Enter pelo caminho secreto, sem nova disputa pelo
/// backend. Estourou qualquer prazo: recusa sem consumir (ou mensagem fixa de
/// resultado incerto se o envio já tinha saído).
async fn type_login_code(
    hub: &Arc<Hub>,
    bot: &str,
    login: &crate::market::logins::Login,
    backend: Backend,
    attempt: &Arc<Attempt>,
    focus: &Value,
    timing: CodeTiming,
) -> Result<Value> {
    let screen = &hub.screen;
    // O domínio sozinho não distingue login de chat: exige prova positiva
    // de campo curto e numérico; metadados ausentes não são prova.
    match code_field(focus) {
        CodeField::Ok => {}
        CodeField::NoProof => bail!(
            "Não consegui confirmar que o campo em foco é curto e numérico de código. Digite o código no site você mesmo."
        ),
        CodeField::Multiline => {
            bail!("O campo em foco aceita texto livre, não um código curto. Digite o código no site você mesmo.")
        }
        CodeField::Long => {
            bail!("O campo em foco parece longo demais para um código. Digite o código no site você mesmo.")
        }
        CodeField::NonNumeric => {
            bail!("O campo em foco não parece numérico. Digite o código no site você mesmo.")
        }
    }
    let prepared =
        tokio::time::timeout(timing.prepare, prepare_secret(screen, backend, attempt)).await.map_err(|_| {
            anyhow!("Não consegui preparar o computador a tempo. Tente de novo ou digite o código no site.")
        })??;
    let critical = async {
        // A exclusão (requests::MUTATION) cobre prova final, consumo e envio:
        // cancelamento/publicação de cartão nunca intercala entre eles.
        let _guard = crate::market::requests::MUTATION.lock().await;
        let again = screen.link()?.request_observed("focusedField", json!({"pid": timing.pid}), attempt).await?;
        if !crate::market::logins::matches(
            &login.site,
            again["url"].as_str(),
            again["app"].as_str().unwrap_or_default(),
        ) || code_field(&again) != CodeField::Ok
        {
            bail!(
                "O campo em foco mudou antes da digitação. Clique no campo de código e tente de novo, ou digite o código no site."
            );
        }
        // O código só é consumido pela sessão que pediu o cartão, sob a mesma
        // exclusão da publicação e do cancelamento (requests::MUTATION).
        let origin = attempt
            .execution
            .acp_binding()
            .ok_or_else(|| anyhow!("Execução sem vínculo de sessão; não dá para confirmar a origem do código."))?;
        let code = match hub.login_codes.take(bot, &login.id, origin) {
            crate::market::login_codes::Take::Ready(code) => code,
            crate::market::login_codes::Take::Missing => bail!(
                "Não há código pronto e válido para este login; ele vale uma vez só, por 5 minutos. Peça outro com request_login_code ou digite o código no site."
            ),
            crate::market::login_codes::Take::Several => bail!(
                "Há mais de um código pronto para este login. Peça ao usuário para responder um cartão de código por vez, ou digite o código no site."
            ),
        };
        send_secret(hub, bot, backend, &prepared, code, attempt, &again).await
    };
    match tokio::time::timeout(timing.critical, critical).await {
        Ok(res) => res,
        Err(_) => bail!(
            "Não consegui confirmar a digitação do código a tempo. Confira a tela; se preciso, peça outro código com request_login_code."
        ),
    }
}

/// Envia o código e o Enter pelo backend já preparado/reservado: o driver usa a
/// reserva (mutex do daemon retido, sem nova disputa) e o helper o link direto.
/// Erros viram mensagens fixas em pt-BR, sem eco do valor.
async fn send_secret(
    // Só o braço Helper (Linux) usa o hub, para o link da digitação.
    #[cfg(target_os = "linux")] hub: &Arc<Hub>,
    #[cfg(not(target_os = "linux"))] _hub: &Arc<Hub>,
    bot: &str,
    backend: Backend,
    prepared: &Prepared<'_>,
    code: String,
    attempt: &Arc<Attempt>,
    again: &Value,
) -> Result<Value> {
    match (backend, prepared) {
        (Backend::Driver, Prepared::Driver(r)) => {
            // macOS: into the checked app, in the background; Linux: the active window it checked.
            let args = if cfg!(target_os = "macos") {
                json!({"pid": again["pid"], "text": code})
            } else {
                json!({"scope": "desktop", "text": code})
            };
            let res = r.call(bot, "type_text", args, attempt).await.map_err(|_| driver_secret_error())?;
            if res["isError"] == true {
                bail!("Não consegui digitar o valor no campo do app. Confira a tela e tente de novo.");
            }
            // Envia (Enter) na mesma ação, ainda sob a reserva do daemon.
            let key = if cfg!(target_os = "macos") {
                json!({"pid": again["pid"], "key": "return"})
            } else {
                json!({"scope": "desktop", "key": "return"})
            };
            let res = r.call(bot, "press_key", key, attempt).await.map_err(|_| {
                anyhow!(
                    "O código foi digitado, mas não consegui enviar o Enter. Confira a página antes de tentar de novo."
                )
            })?;
            if res["isError"] == true {
                bail!(
                    "O código foi digitado, mas não consegui enviar (Enter). Confira a página antes de tentar de novo."
                );
            }
            Ok(
                json!({"content": [{"type": "text", "text": "O código foi digitado e enviado. Confira pela tela se necessário."}]}),
            )
        }
        #[cfg(target_os = "linux")]
        (Backend::Helper, Prepared::Helper) => helper_tools::type_secret(&hub.screen, code, true, attempt).await,
        // No Linux os enums têm duas variantes: pares trocados não acontecem.
        #[cfg(target_os = "linux")]
        _ => bail!("backend de digitação indisponível"),
    }
}

/// Erro fixo do envio de segredo pelo driver: a resposta remota pode ecoar o
/// valor digitado, então nada dela — nem a cadeia de erro — chega ao agente.
fn driver_secret_error() -> anyhow::Error {
    anyhow!("Não consegui digitar o valor no campo do app. Confira a tela e tente de novo.")
}

/// Veredito da prova de que o campo em foco pede um código de verificação (OTP),
/// curto e numérico — nunca uma mensagem: `focusedField` só expõe o que a API de
/// acessibilidade entrega de verdade (app, URL, campo seguro, papel, título,
/// descrição, placeholder, autocomplete, multiline, maxlength/size, type,
/// inputmode, contenteditable). Sem prova positiva de campo curto e numérico,
/// ou com sinal de campo longo, de busca ou de texto livre, `type_login` recusa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodeField {
    Ok,
    /// Nem sinal de código nem sinal de texto livre: prova insuficiente.
    NoProof,
    /// Área de texto/multilinha ou contenteditable: nunca é campo de código.
    Multiline,
    /// maxlength/size maior que 10: longo demais para um código.
    Long,
    /// tipo/inputmode claramente não numérico.
    NonNumeric,
}

/// Papel de controle que aceita entrada de texto (o que `focusedField` expõe).
/// `searchfield` fica de fora: campo de busca nunca é prova de código.
fn text_entry_role(role: &str) -> bool {
    ["textfield", "textarea", "entry", "text", "edit", "spin", "combobox"].iter().any(|r| role.contains(r))
}

/// Sinais de multilinha/texto livre no papel ou em `multiline`/`contenteditable`.
fn multiline_field(focus: &Value, role: &str) -> bool {
    focus["multiline"] == true
        || focus["contenteditable"] == true
        // contenteditable="" também liga a edição; só "false" desliga.
        || focus["contenteditable"].as_str().is_some_and(|v| !v.eq_ignore_ascii_case("false"))
        || role == "text"
        || role.contains("textarea")
        || role.contains("text area")
        || role.contains("multiline")
}

/// maxlength/size declarados maiores que 10 caracteres.
fn long_field(focus: &Value) -> bool {
    ["maxlength", "max-length", "size"].iter().any(|key| {
        focus[*key]
            .as_f64()
            .or_else(|| focus[*key].as_str().and_then(|s| s.parse::<f64>().ok()))
            .is_some_and(|n| n > 10.0)
    })
}

/// inputmode explicitamente não numérico: texto livre, busca, e-mail etc.
fn non_numeric_field(focus: &Value) -> bool {
    const TYPES: &[&str] = &[
        "email",
        "url",
        "search",
        "password",
        "file",
        "hidden",
        "checkbox",
        "radio",
        "range",
        "color",
        "date",
        "datetime-local",
        "month",
        "time",
        "week",
        "button",
        "submit",
        "reset",
        "image",
    ];
    const INPUTMODES: &[&str] = &["none", "text", "search", "email", "url"];
    focus["type"].as_str().is_some_and(|t| TYPES.contains(&t.to_lowercase().as_str()))
        || focus["inputmode"].as_str().is_some_and(|mode| INPUTMODES.contains(&mode.to_lowercase().as_str()))
}

/// Prova positiva de campo curto e numérico: `autocomplete=one-time-code`, ou
/// `type` tel/number, ou `inputmode` numérico. Rótulos e palavras genéricas
/// ("code", "token", "verify") nunca autorizam sozinhas, e a falta de
/// metadados não é prova — a plataforma que não os expõe recusa.
fn numeric_code_field(focus: &Value) -> bool {
    if focus["autocomplete"].as_str() == Some("one-time-code") {
        return true;
    }
    focus["type"].as_str().is_some_and(|t| matches!(t.to_lowercase().as_str(), "tel" | "number"))
        || focus["inputmode"]
            .as_str()
            .is_some_and(|m| matches!(m.to_lowercase().as_str(), "numeric" | "tel" | "decimal"))
}

fn code_field(focus: &Value) -> CodeField {
    if focus["secure"] == true {
        return CodeField::NoProof;
    }
    // Papel ausente não prova nada; papel/subpapel de busca é texto livre.
    let role = focus["role"].as_str().unwrap_or_default().to_lowercase();
    let subrole = focus["subrole"].as_str().unwrap_or_default().to_lowercase();
    if role.is_empty() || role.contains("search") || subrole.contains("search") {
        return CodeField::NoProof;
    }
    // Quando o helper informa o papel do controle, ele precisa ser de entrada de texto.
    if !text_entry_role(&role) {
        return CodeField::NoProof;
    }
    // Sinais de texto livre vencem qualquer prova de código.
    if multiline_field(focus, &role) {
        return CodeField::Multiline;
    }
    if long_field(focus) {
        return CodeField::Long;
    }
    if non_numeric_field(focus) {
        return CodeField::NonNumeric;
    }
    if numeric_code_field(focus) { CodeField::Ok } else { CodeField::NoProof }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn user_takeover_blocks_bot_actions_but_not_looking() {
        let (tx, _) = broadcast::channel(8);
        let s = Screen::new(Some(true), tx);
        assert!(s.claim("a", false).unwrap());
        assert!(!s.claim("a", false).unwrap());
        assert!(s.claim("b", true).is_err(), "one bot at a time");
        s.takeover(true);
        assert!(s.claim("a", false).is_err());
        assert!(s.claim("a", true).is_ok());
    }

    #[test]
    fn type_login_names_its_target() {
        let t = type_login_tool();
        assert_eq!(t["inputSchema"]["required"], json!(["login", "field"]));
        assert!(t["inputSchema"]["properties"]["pid"].is_object());
        assert_eq!(t["inputSchema"]["properties"]["field"]["enum"], json!(["username", "password", "code"]));
        let a: LoginArgs = serde_json::from_value(json!({"login":"github-x","field":"code"})).unwrap();
        assert!(matches!(a.field, LoginField::Code));
    }

    #[test]
    fn code_field_requires_positive_proof() {
        use CodeField::*;
        // Prova positiva de campo curto e numérico: autocomplete de uso único
        // ou tipo/inputmode numérico — com papel de entrada de texto presente.
        for focus in [
            json!({"app":"Chrome","url":"https://github.com/login","secure":false,"role":"entry","autocomplete":"one-time-code","placeholder":"Verification code"}),
            json!({"app":"Chrome","secure":false,"role":"AXTextField","type":"tel","maxlength":6}),
            json!({"app":"Chrome","secure":false,"role":"entry","type":"number"}),
            json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"numeric"}),
            json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"tel","placeholder":"Código"}),
            json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"decimal","size":8}),
            // contenteditable="false" não é texto livre.
            json!({"app":"Chrome","secure":false,"role":"entry","contenteditable":"false","type":"tel"}),
        ] {
            assert_eq!(code_field(&focus), Ok, "{focus}");
        }
        // Sem prova positiva: papel ausente, só rótulo genérico, busca no mesmo
        // domínio, inputmode texto, maxlength ausente sem sinal forte — tudo recusa.
        for focus in [
            json!({"app":"Chrome","url":"https://github.com/login","secure":false,"autocomplete":"one-time-code"}),
            json!({"app":"Chrome","url":"https://github.com/login","secure":false}),
            json!({"app":"Chrome","url":"https://github.com/login","secure":false,"role":"AXTextField","title":"Message"}),
            json!({"app":"Chrome","url":"https://github.com/login","secure":false,"role":"entry","placeholder":"Search"}),
            json!({"app":"Chrome","url":"https://github.com/login","secure":false,"role":"AXTextField","placeholder":"Verification code"}),
            json!({"app":"Chrome","secure":false,"role":"entry","placeholder":"Verification code"}),
            json!({"app":"Chrome","secure":false,"role":"entry","title":"Digite o código do autenticador"}),
            json!({"app":"Chrome","secure":false,"role":"entry","placeholder":"Two-factor token"}),
            json!({"app":"Chrome","secure":false,"role":"searchfield","placeholder":"Search code"}),
            json!({"app":"Chrome","secure":false,"role":"AXTextField","subrole":"AXSearchField","placeholder":"Search code"}),
            json!({"app":"Chrome","secure":false,"role":"entry","subrole":"search","inputmode":"numeric"}),
            json!({"app":"Chrome","secure":false,"role":"entry","type":"text","placeholder":"Verification code"}),
            json!({"app":"Chrome","secure":false,"role":"AXButton","title":"Verify","type":"tel"}),
            json!({"app":"Chrome","url":"https://github.com/login","secure":true,"role":"AXSecureTextField","type":"tel"}),
        ] {
            assert_eq!(code_field(&focus), NoProof, "{focus}");
        }
        // Multilinha/texto livre recusa mesmo com prova numérica.
        for focus in [
            json!({"app":"Chrome","secure":false,"role":"AXTextArea","type":"tel"}),
            json!({"app":"Chrome","secure":false,"role":"textarea","type":"number"}),
            json!({"app":"Chrome","secure":false,"role":"text","inputmode":"numeric"}),
            json!({"app":"Chrome","secure":false,"role":"entry","multiline":true,"type":"tel"}),
            json!({"app":"Chrome","secure":false,"role":"entry","contenteditable":"","type":"tel"}),
        ] {
            assert_eq!(code_field(&focus), Multiline, "{focus}");
        }
        // Campo longo (maxlength/size > 10) recusa mesmo com prova numérica.
        for (focus, want) in [
            (json!({"app":"Chrome","secure":false,"role":"entry","type":"tel","maxlength":20}), Long),
            (json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"numeric","size":"42"}), Long),
            (json!({"app":"Chrome","secure":false,"role":"entry","autocomplete":"one-time-code","maxlength":11}), Long),
            (json!({"app":"Chrome","secure":false,"role":"entry","type":"tel","maxlength":6}), Ok),
            (json!({"app":"Chrome","secure":false,"role":"entry","type":"tel","size":10}), Ok),
        ] {
            assert_eq!(code_field(&focus), want, "{focus}");
        }
        // Tipo/inputmode explicitamente não numérico recusa; numéricos passam.
        for (focus, want) in [
            (json!({"app":"Chrome","secure":false,"role":"entry","type":"email","maxlength":6}), NonNumeric),
            (
                json!({"app":"Chrome","secure":false,"role":"entry","type":"url","placeholder":"Verification code"}),
                NonNumeric,
            ),
            (json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"search"}), NonNumeric),
            (
                json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"text","placeholder":"Verification code"}),
                NonNumeric,
            ),
            (json!({"app":"Chrome","secure":false,"role":"entry","inputmode":"email","type":"tel"}), NonNumeric),
            (json!({"app":"Chrome","secure":false,"role":"entry","type":"tel","placeholder":"Verification code"}), Ok),
        ] {
            assert_eq!(code_field(&focus), want, "{focus}");
        }
    }

    #[test]
    fn driver_secret_errors_are_fixed_and_never_echo() {
        // O driver pode devolver erro citando o valor digitado: o agente recebe
        // só a mensagem fixa, sem corpo nem cadeia de erro externa.
        let raw = Err::<Value, _>(anyhow!("driver failed typing 123456 into field"));
        let err = driver_secret_error();
        let shown = format!("{err:#}|{err:?}");
        assert!(!shown.contains("123456"), "{shown}");
        let mapped = raw.map_err(|_| driver_secret_error()).unwrap_err();
        assert!(!format!("{mapped:#}|{mapped:?}").contains("123456"));
        assert!(mapped.to_string().contains("Não consegui digitar"), "{mapped}");
    }
}

/// Fluxo real do backend Helper (Linux): `focusedField`/`input` respondidos por
/// um helper falso no socket; prova destino, revalidação, Enter e ausência de eco.
#[cfg(all(test, target_os = "linux"))]
mod code_flow {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::api::devices::Caller;
    use crate::fork::sessoes::bindings;
    use crate::remote::identity::Identity;
    use crate::store::{BotConfig, Store};
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::{Notify, mpsc};

    const CODE_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"AXTextField","placeholder":"Verification code","autocomplete":"one-time-code"}"#;
    const COMMENT_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"AXTextArea","placeholder":"Add a comment"}"#;
    const LONG_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"AXTextField","placeholder":"Verification code","autocomplete":"one-time-code","maxlength":20}"#;
    const EMAIL_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"AXTextField","type":"email"}"#;
    const PLAIN_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"AXTextField","placeholder":"Comment"}"#;
    const SEARCH_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"AXTextField","subrole":"AXSearchField","placeholder":"Search code"}"#;
    const NO_ROLE_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"placeholder":"Verification code"}"#;
    const INPUTMODE_TEXT_FIELD: &str = r#"{"app":"Chrome","pid":42,"url":"https://github.com/login","secure":false,"role":"entry","inputmode":"text","placeholder":"Verification code"}"#;

    struct Flow {
        hub: Arc<Hub>,
        dir: std::path::PathBuf,
        login: crate::market::logins::Login,
        attempt: Arc<Attempt>,
        origin: String,
        frames: mpsc::UnboundedReceiver<Value>,
    }

    impl Drop for Flow {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// `focuses`: respostas em sequência para cada `focusedField`; a última repete.
    /// `input_error`: mensagem de erro devolvida a cada `input` (ex.: eco do segredo).
    /// `hang`: depois de esgotar `focuses`, `focusedField` nunca responde (prazo estoura).
    async fn flow(focuses: &[&str]) -> Flow {
        flow_inner(focuses, None, false).await
    }

    async fn flow_inner(focuses: &[&str], input_error: Option<&str>, hang: bool) -> Flow {
        assert!(matches!(backend(), Backend::Helper), "este teste cobre só o backend Helper");
        let dir = std::env::temp_dir().join(format!("donna-code-flow-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let login = crate::market::logins::save(&hub.store, "github.com", "conta-a", "pw-x", "GitHub conta A").unwrap();
        let bot: BotConfig = serde_json::from_value(json!({
            "id":"bot","name":"Bot","computer":true,"logins":[login.id]
        }))
        .unwrap();
        hub.store.save_bot(&bot).unwrap();
        crate::fork::tela_por_computador::call(
            &hub,
            &Caller::Local,
            &json!({"botId":"bot","target":crate::fork::tela_por_computador::host_target(&hub),"enabled":true}),
        )
        .unwrap();
        *hub.screen.enabled.locked() = Some(true);
        // O helper precisa estar conectado ANTES da tentativa: ela guarda o Link
        // original e recusa se a conexão for substituída.
        let (host, peer) = UnixStream::pair().unwrap();
        tokio::spawn(crate::screen::helper::run_link(hub.screen.clone(), host));
        let (frames_tx, frames) = mpsc::unbounded_channel();
        let focuses: Vec<String> = focuses.iter().map(ToString::to_string).collect();
        let input_error = input_error.map(str::to_owned);
        tokio::spawn(async move {
            let (rd, mut wr) = peer.into_split();
            let status = json!({"jsonrpc":"2.0","method":"status","params":{"platform":"test","capture":true,"input":true,"displays":[crate::screen::tests::retina()]}});
            wr.write_all(format!("{status}\n").as_bytes()).await.unwrap();
            let mut lines = BufReader::new(rd).lines();
            let mut i = 0;
            while let Ok(Some(line)) = lines.next_line().await {
                let m: Value = serde_json::from_str(&line).unwrap();
                let _ = frames_tx.send(m.clone());
                if m["method"] == "focusedField" && hang && i >= focuses.len() {
                    continue;
                }
                let result = if m["method"] == "focusedField" {
                    let f = &focuses[i.min(focuses.len() - 1)];
                    i += 1;
                    serde_json::from_str::<Value>(f).unwrap()
                } else {
                    json!({})
                };
                let res = if m["method"] == "input"
                    && let Some(err) = &input_error
                {
                    json!({"jsonrpc":"2.0","id":m["id"],"error":{"message":err}})
                } else {
                    json!({"jsonrpc":"2.0","id":m["id"],"result":result})
                };
                wr.write_all(format!("{res}\n").as_bytes()).await.unwrap();
            }
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            while hub.screen.link.locked().is_none() || !hub.screen.status.locked().capture {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("helper falso não anunciou capture");
        let execution = bindings::fixture_execution(&hub, "bot");
        let attempt = Attempt::capture(&hub, &Caller::Local, "bot", &execution, false).unwrap();
        let origin = bindings::for_acp(&hub.store, "bot", "fixture-acp").unwrap().id.as_str().to_owned();
        Flow { hub, dir, login, attempt, origin, frames }
    }

    impl Flow {
        fn put_code(&self, origin: &str) {
            self.hub.login_codes.put("card-1", "bot", &self.login.id, origin, "123456".into()).unwrap();
        }
        async fn type_code(&self) -> Result<Value> {
            computer(&self.hub, "bot", TYPE_LOGIN, json!({"login":self.login.id,"field":"code"}), Some(&self.attempt))
                .await
        }
    }

    async fn drain(frames: &mut mpsc::UnboundedReceiver<Value>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(m) = frames.try_recv() {
            out.push(m);
        }
        // Pequena folga para os últimos frames.
        tokio::time::sleep(Duration::from_millis(50)).await;
        while let Ok(m) = frames.try_recv() {
            out.push(m);
        }
        out
    }

    #[tokio::test]
    async fn code_types_and_submits_without_echo() {
        let mut f = flow(&[CODE_FIELD]).await;
        f.put_code(&f.origin);
        let res = f.type_code().await.unwrap();
        let text = res["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect::<String>();
        assert!(!text.contains("123456"), "a resposta não devolve o código");
        assert!(res["content"].as_array().unwrap().iter().all(|c| c["type"] != "image"), "sem screenshot");
        let frames = drain(&mut f.frames).await;
        let methods: Vec<&str> = frames.iter().filter_map(|m| m["method"].as_str()).collect();
        assert_eq!(methods.iter().filter(|m| **m == "focusedField").count(), 2, "confere, revalida e digita");
        assert!(!methods.contains(&"screenshot"), "nunca pede screenshot no caminho do segredo");
        let inputs: Vec<&Value> = frames.iter().filter(|m| m["method"] == "input").collect();
        assert_eq!(inputs.len(), 2, "texto + Enter");
        assert_eq!(inputs[0]["params"]["event"]["text"], "123456");
        assert_eq!(inputs[1]["params"]["event"]["key"], "return");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Missing,
            "uso único"
        );
    }

    #[tokio::test]
    async fn comment_field_on_same_domain_is_refused() {
        let mut f = flow(&[COMMENT_FIELD]).await;
        f.put_code(&f.origin);
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("texto livre"), "{err}");
        assert!(err.contains("Digite o código no site"), "{err}");
        let frames = drain(&mut f.frames).await;
        assert!(frames.iter().all(|m| m["method"] != "input"), "nada é digitado");
        // O código não foi consumido pela recusa.
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into())
        );
    }

    #[tokio::test]
    async fn focus_change_before_typing_is_refused() {
        let mut f = flow(&[CODE_FIELD, COMMENT_FIELD]).await;
        f.put_code(&f.origin);
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("mudou"), "{err}");
        let frames = drain(&mut f.frames).await;
        assert!(frames.iter().all(|m| m["method"] != "input"), "nada é digitado");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into()),
            "o código não é queimado pela troca de foco"
        );
    }

    #[tokio::test]
    async fn unproven_long_and_non_numeric_fields_are_refused() {
        for (field, want) in [
            (PLAIN_FIELD, "Não consegui confirmar"),
            (NO_ROLE_FIELD, "Não consegui confirmar"),
            (SEARCH_FIELD, "Não consegui confirmar"),
            (INPUTMODE_TEXT_FIELD, "não parece numérico"),
            (LONG_FIELD, "longo demais"),
            (EMAIL_FIELD, "não parece numérico"),
        ] {
            let mut f = flow(&[field]).await;
            f.put_code(&f.origin);
            let err = f.type_code().await.unwrap_err().to_string();
            assert!(err.contains(want), "{field}: {err}");
            assert!(err.contains("Digite o código no site"), "{err}");
            let frames = drain(&mut f.frames).await;
            assert!(frames.iter().all(|m| m["method"] != "input"), "nada é digitado");
        }
    }

    #[tokio::test]
    async fn focus_change_while_waiting_for_mutation_is_refused() {
        // MUTATION segurada por outra tarefa: a segunda prova de foco só roda
        // depois da exclusão, e o foco já é de comentário — recusa sem consumir.
        let mut f = flow(&[CODE_FIELD, COMMENT_FIELD]).await;
        f.put_code(&f.origin);
        let guard = crate::market::requests::MUTATION.lock().await;
        let release = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(guard);
        };
        let (res, ()) = tokio::join!(f.type_code(), release);
        let err = res.unwrap_err().to_string();
        assert!(err.contains("mudou"), "{err}");
        let frames = drain(&mut f.frames).await;
        assert!(frames.iter().all(|m| m["method"] != "input"), "nada é digitado");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into()),
            "o código não é queimado pela troca de foco durante a espera"
        );
    }

    #[tokio::test]
    async fn helper_error_echoing_the_code_is_sanitized() {
        // O helper responde erro citando o texto recebido: o agente recebe só a
        // mensagem fixa — o código não aparece no erro final nem na cadeia.
        let f = flow_inner(&[CODE_FIELD], Some("falhou ao digitar 123456 no campo"), false).await;
        f.put_code(&f.origin);
        let err = f.type_code().await.unwrap_err();
        let shown = format!("{err:#}|{err:?}");
        assert!(!shown.contains("123456"), "{shown}");
        assert!(err.to_string().contains("Não consegui digitar"), "{err}");
    }

    #[tokio::test]
    async fn critical_timeout_releases_mutation_without_consuming() {
        // A prova final de foco nunca responde: o prazo crítico estoura, a
        // recusa é fixa em pt-BR, MUTATION é liberada e o código não é queimado.
        let mut f = flow_inner(&[], None, true).await;
        f.put_code(&f.origin);
        let focus: Value = serde_json::from_str(CODE_FIELD).unwrap();
        let timing = CodeTiming { pid: None, prepare: Duration::from_secs(5), critical: Duration::from_millis(300) };
        let err = type_login_code(&f.hub, "bot", &f.login, backend(), &f.attempt, &focus, timing).await.unwrap_err();
        assert!(err.to_string().contains("a tempo"), "{err}");
        assert!(crate::market::requests::MUTATION.try_lock().is_ok(), "MUTATION liberada no timeout");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into()),
            "o código não é queimado pelo timeout"
        );
        let frames = drain(&mut f.frames).await;
        assert!(frames.iter().all(|m| m["method"] != "input"), "nada é digitado");
    }

    #[tokio::test]
    async fn missing_expired_used_or_foreign_code_is_refused() {
        // Sem código pronto: recusa curta em pt-BR.
        let f = flow(&[CODE_FIELD]).await;
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("Não há código pronto e válido"), "{err}");
        assert!(err.contains("digite o código no site"), "{err}");
        // Código vencido: recusa, sem consumir nada.
        f.put_code(&f.origin);
        f.hub.login_codes.backdate("card-1", Duration::from_secs(6 * 60));
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("Não há código pronto e válido"), "{err}");
        // Código já usado: recusa.
        f.put_code(&f.origin);
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into())
        );
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("Não há código pronto e válido"), "{err}");
        // Código de OUTRA sessão, bot ou login: recusa, sem consumir.
        f.put_code("sessao-antiga");
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("Não há código pronto e válido"), "{err}");
        f.hub.login_codes.put("card-2", "outro-bot", &f.login.id, &f.origin, "654321".into()).unwrap();
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("Não há código pronto e válido"), "{err}");
        let outro = crate::market::logins::save(&f.hub.store, "exemplo.com", "conta-b", "pw-y", "Exemplo B").unwrap();
        f.hub.login_codes.put("card-3", "bot", &outro.id, &f.origin, "112233".into()).unwrap();
        let err = f.type_code().await.unwrap_err().to_string();
        assert!(err.contains("Não há código pronto e válido"), "{err}");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, "sessao-antiga"),
            crate::market::login_codes::Take::Ready("123456".into()),
            "o código da outra sessão não foi queimado"
        );
    }

    /// Driver falso no protocolo de linha do cua-driver: registra cada request e
    /// responde `{"ok":true,...}`; `hang_text` nunca responde `type_text` e
    /// `gate_enter` segura a resposta do `press_key` até `release` tocar.
    struct FakeDriver {
        endpoint: String,
        frames: mpsc::UnboundedReceiver<Value>,
        release: Arc<Notify>,
        dir: std::path::PathBuf,
    }

    impl Drop for FakeDriver {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fake_driver(hang_text: bool, gate_enter: bool) -> FakeDriver {
        let dir = std::env::temp_dir().join(format!("donna-fake-driver-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let endpoint = dir.join("cua.sock").to_string_lossy().into_owned();
        let listener = UnixListener::bind(&endpoint).unwrap();
        let (tx, frames) = mpsc::unbounded_channel();
        let release = Arc::new(Notify::new());
        let gate = release.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let tx = tx.clone();
                let gate = gate.clone();
                tokio::spawn(async move {
                    let (rd, mut wr) = stream.into_split();
                    let mut lines = BufReader::new(rd).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let Ok(m) = serde_json::from_str::<Value>(&line) else { break };
                        let _ = tx.send(m.clone());
                        let name = m["name"].as_str().unwrap_or_default();
                        if name == "type_text" && hang_text {
                            continue;
                        }
                        if name == "press_key" && gate_enter {
                            gate.notified().await;
                        }
                        let res = json!({"ok": true, "result": {"content": [{"type": "text", "text": "ok"}]}});
                        if wr.write_all(format!("{res}\n").as_bytes()).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        FakeDriver { endpoint, frames, release, dir }
    }

    /// Child vivo qualquer: o `try_wait` do reserve só confere que não morreu.
    fn fake_child() -> tokio::process::Child {
        tokio::process::Command::new("sleep").arg("300").kill_on_drop(true).spawn().unwrap()
    }

    fn driver_timing(prepare: Duration, critical: Duration) -> CodeTiming {
        CodeTiming { pid: None, prepare, critical }
    }

    fn code_focus() -> Value {
        serde_json::from_str(CODE_FIELD).unwrap()
    }

    #[tokio::test]
    async fn driver_reservation_covers_text_and_enter_without_interleave() {
        // Driver de pé (daemon semeado + socket fake): a reserva atravessa
        // type_text e press_key — uma chamada comum ao driver espera a reserva
        // cair, e só então é atendida.
        let f = flow(&[CODE_FIELD]).await;
        let mut drv = fake_driver(false, true);
        f.hub.screen.driver.seed_daemon_for_test(fake_child(), drv.endpoint.clone()).await;
        f.put_code(&f.origin);
        let focus = code_focus();
        let code = type_login_code(
            &f.hub,
            "bot",
            &f.login,
            Backend::Driver,
            &f.attempt,
            &focus,
            driver_timing(Duration::from_secs(2), Duration::from_secs(5)),
        );
        tokio::pin!(code);
        // Espera o driver ver texto + Enter; a resposta do Enter fica presa no gate.
        let mut seen = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), async {
            while seen.len() < 2 {
                tokio::select! {
                    r = &mut code => panic!("o código terminou antes do Enter: {r:?}"),
                    m = drv.frames.recv() => seen.push(m.unwrap()),
                }
            }
        })
        .await
        .expect("driver não recebeu type_text + press_key");
        assert_eq!(seen[0]["name"], "type_text");
        assert_eq!(seen[1]["name"], "press_key");
        // Com a reserva viva, a outra chamada ao driver não passa do mutex.
        let h = f.hub.clone();
        let att = f.attempt.clone();
        let mut other = tokio::spawn(async move { cua::call(&h.screen, "bot", "list_windows", json!({}), &att).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut other).await.is_err(),
            "observe intercalou com a reserva"
        );
        drv.release.notify_one();
        let res = code.await.unwrap();
        assert!(
            res["content"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| !c["text"].as_str().unwrap_or_default().contains("123456"))
        );
        other.await.unwrap().unwrap();
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Missing,
            "uso único"
        );
    }

    #[tokio::test]
    async fn driver_occupied_refuses_prepare_without_consuming() {
        // Mutex do daemon ocupado por outra chamada: a preparação estoura o
        // prazo curto, recusa sem consumir e sem tocar MUTATION.
        let f = flow(&[CODE_FIELD]).await;
        let drv = fake_driver(false, false);
        f.hub.screen.driver.seed_daemon_for_test(fake_child(), drv.endpoint.clone()).await;
        let _held = f.hub.screen.driver.hold_daemon_for_test().await;
        f.put_code(&f.origin);
        let focus = code_focus();
        let err = type_login_code(
            &f.hub,
            "bot",
            &f.login,
            Backend::Driver,
            &f.attempt,
            &focus,
            driver_timing(Duration::from_millis(200), Duration::from_secs(2)),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("preparar o computador a tempo"), "{err}");
        assert!(crate::market::requests::MUTATION.try_lock().is_ok(), "MUTATION nunca foi retida");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into()),
            "o código não é queimado pela espera do driver"
        );
    }

    #[tokio::test]
    async fn driver_start_timeout_refuses_without_consuming() {
        // Sem daemon de pé e o start travado: o prazo de preparação estoura,
        // recusa sem consumir — MUTATION nunca entra em cena.
        let f = flow(&[CODE_FIELD]).await;
        let _gate = cua::START_BLOCK.lock().await;
        f.put_code(&f.origin);
        let focus = code_focus();
        let err = type_login_code(
            &f.hub,
            "bot",
            &f.login,
            Backend::Driver,
            &f.attempt,
            &focus,
            driver_timing(Duration::from_millis(200), Duration::from_secs(2)),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("preparar o computador a tempo"), "{err}");
        assert!(crate::market::requests::MUTATION.try_lock().is_ok());
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Ready("123456".into())
        );
    }

    #[tokio::test]
    async fn driver_timeout_after_send_releases_reservation() {
        // type_text saiu mas o driver nunca responde: o prazo crítico estoura,
        // o código já consumido não volta, a mensagem é de resultado incerto e
        // a reserva do daemon é liberada (outra chamada passa na hora).
        let f = flow(&[CODE_FIELD]).await;
        let mut drv = fake_driver(true, false);
        f.hub.screen.driver.seed_daemon_for_test(fake_child(), drv.endpoint.clone()).await;
        f.put_code(&f.origin);
        let focus = code_focus();
        let err = type_login_code(
            &f.hub,
            "bot",
            &f.login,
            Backend::Driver,
            &f.attempt,
            &focus,
            driver_timing(Duration::from_secs(2), Duration::from_millis(400)),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("a tempo"), "{err}");
        assert!(err.to_string().contains("Confira a tela"), "{err}");
        assert_eq!(
            f.hub.login_codes.take("bot", &f.login.id, &f.origin),
            crate::market::login_codes::Take::Missing,
            "o envio já tinha consumido o código"
        );
        assert!(crate::market::requests::MUTATION.try_lock().is_ok(), "MUTATION liberada");
        // O driver viu o type_text sair (e só ele: o Enter nem foi pedido).
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut names = Vec::new();
        while let Ok(m) = drv.frames.try_recv() {
            names.push(m["name"].as_str().unwrap_or_default().to_owned());
        }
        assert_eq!(names, ["type_text"], "{names:?}");
        // A reserva caiu com a função: o mutex do daemon aceita nova chamada.
        let again = tokio::time::timeout(
            Duration::from_secs(2),
            cua::call(&f.hub.screen, "bot", "list_windows", json!({}), &f.attempt),
        )
        .await
        .expect("reserva do daemon não foi liberada")
        .unwrap();
        assert!(again["content"].is_array());
    }
}
