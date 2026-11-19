//! Tudo o que fala com o binário do wacli: onde ele está, a pasta de cada bot,
//! os comandos (`--store` sempre explícito) e a leitura dos eventos NDJSON.
//!
//! Código de pareamento e QR passam por aqui, mas nunca vão para log.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;

/// Ajuste interno do Hub. O wacli oficial fica em `whatsapp/bin/` (baixado pelo R7,
/// `binario.rs`); esta variável aponta outro binário, só para teste.
const BINARY_ENV: &str = "DONNABOT_WACLI";

/// Caminho do wacli, se existir. Sem binário o número fica "wacli não instalado".
pub(super) fn binary(root: &Path, configured: Option<&Path>) -> Option<PathBuf> {
    let name = if cfg!(windows) { "wacli.exe" } else { "wacli" };
    let path = configured
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os(BINARY_ENV).filter(|v| !v.is_empty()).map(PathBuf::from))
        .unwrap_or_else(|| root.join("bin").join(name));
    path.is_file().then_some(path)
}

/// Só IDs que são um nome de pasta simples; nada de `..` ou separador.
pub(super) fn bot_dir(root: &Path, bot: &str) -> Result<PathBuf> {
    let ok = !bot.is_empty()
        && bot.len() <= 128
        && bot != "bin"
        && bot.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ensure!(ok, "bot_not_found: bot não encontrado.");
    Ok(root.join(bot))
}

/// Cria `whatsapp/<bot>/` só do dono do processo (`0700`). No Windows a pasta fica
/// dentro do perfil do usuário, cuja ACL padrão já exclui outros usuários comuns;
/// o Hub não mexe na ACL.
pub(super) fn ensure_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).context("whatsapp_dir")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(root) = dir.parent() {
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).context("whatsapp_dir")?;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).context("whatsapp_dir")?;
    }
    Ok(())
}

pub(super) fn remove_dir(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error).context("whatsapp_dir_remove"),
        _ => Ok(()),
    }
}

/// `session.db` é acesso total ao número: sai junto com a desconexão.
pub(super) fn remove_session(dir: &Path) {
    for name in ["session.db", "session.db-wal", "session.db-shm"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// Ambiente mínimo: nenhum proxy, token ou config herdados; `HOME` na pasta do bot
/// para o wacli nunca achar o `~/.wacli` do usuário.
pub(super) fn command(binary: &Path, dir: &Path) -> Command {
    let mut command = Command::new(binary);
    command.env_clear();
    for key in ["PATH", "LANG", "LC_ALL", "SYSTEMROOT", "WINDIR", "TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("HOME", dir)
        .env("USERPROFILE", dir)
        .current_dir(dir)
        .arg("--store")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        // Grupo próprio: parar mata o wacli e qualquer filho dele.
        command.process_group(0);
        // SAFETY: só altera a máscara no filho antes do exec, sem estado compartilhado.
        unsafe {
            command.pre_exec(|| {
                libc::umask(0o077);
                Ok(())
            });
        }
    }
    command
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Method {
    Code,
    Qr,
}

/// Pareamento: histórico mínimo (pesquisa §2.1, LGPD e disco).
pub(super) fn auth(binary: &Path, dir: &Path, method: Method, phone: Option<&str>) -> Command {
    let mut command = command(binary, dir);
    command.args(["--events", "auth"]);
    match (method, phone) {
        (Method::Code, Some(phone)) => command.arg("--phone").arg(phone),
        _ => command.args(["--qr-format", "text"]),
    };
    command.args(["--history-days", "1", "--history-max-per-chat", "20"]).stderr(Stdio::piped());
    command
}

/// O processo que fica de pé com o número conectado. Com `hook` (R3), o wacli avisa o
/// Hub em `127.0.0.1` a cada mensagem gravada; o aviso só acorda a leitura do `wacli.db`.
pub(super) fn sync(binary: &Path, dir: &Path, hook: Option<&super::inbox::Hook>) -> Command {
    let mut command = command(binary, dir);
    command
        .args(["--events", "sync", "--follow", "--stale-threshold", "60s", "--max-reconnect", "0"])
        // R4c: rede de segurança do próprio wacli para envios delegados a este `sync`
        // (`cmd/wacli/sync.go:149`, `send_spacing.go`). O piso do Hub (D357) é 8 s; a fila do
        // Hub (`pace.rs`) já espera X + variação, então este piso nunca atrasa um envio na vez.
        .args(["--send-spacing", "8s"])
        .stderr(Stdio::piped());
    if let Some(hook) = hook {
        command.arg("--webhook").arg(&hook.url).arg("--webhook-secret").arg(&hook.secret).args([
            "--webhook-events",
            "message",
            "--webhook-allow-private",
        ]);
    }
    command
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Event {
    Qr(String),
    PairCode(String),
    Connected,
    LoggedOut,
    Stale,
    /// "QR code timed out": o pareamento venceu.
    TimedOut,
    Failed,
    Other,
}

/// Uma linha do stderr com `--events`. Linhas comuns também passam por aqui:
/// o wacli escreve avisos em texto. O conteúdo nunca é guardado nem logado.
pub(super) fn event(line: &str) -> Event {
    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        let lower = line.to_ascii_lowercase();
        return if lower.contains("timed out") {
            Event::TimedOut
        } else if lower.contains("not authenticated") || lower.contains("logged out") {
            Event::LoggedOut
        } else {
            Event::Other
        };
    };
    let code = || value["data"]["code"].as_str().filter(|c| !c.is_empty() && c.len() <= 4096).map(str::to_owned);
    match value["event"].as_str().unwrap_or_default() {
        "qr_code" => code().map_or(Event::Other, Event::Qr),
        "pair_code" => code().map_or(Event::Other, Event::PairCode),
        "connected" => Event::Connected,
        "logged_out" => Event::LoggedOut,
        "stale" => Event::Stale,
        "error" => {
            let text = value["data"].to_string().to_ascii_lowercase();
            if text.contains("timed out") { Event::TimedOut } else { Event::Failed }
        }
        _ => Event::Other,
    }
}

/// A sessão pareada, como o wacli informa.
pub(super) struct Session {
    pub authenticated: bool,
    /// `+` e os dígitos do número conectado, só quando o wacli o confirma.
    pub phone: Option<String>,
}

/// `--read-only auth status --json` (wacli v0.25.1): lê a sessão sem a trava do store, então
/// funciona com o `sync` de pé. Envelope `{"success","data","error"}` (`internal/out/out.go:9-25`);
/// `data` = `authenticated` e, pareado, `linked_jid` (JID do aparelho sem o número do
/// aparelho, `ToNonAD`) e `phone` (a parte antes do `@`): `cmd/wacli/auth.go:301-331`,
/// `cmd/wacli/auth_status_readonly.go:16-55`. Qualquer falha conta como "não conectado".
pub(super) async fn session(binary: &Path, dir: &Path) -> Session {
    let none = Session { authenticated: false, phone: None };
    let mut command = command(binary, dir);
    command.args(["--read-only", "auth", "status", "--json"]).stdout(Stdio::piped());
    let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(15), command.output()).await else {
        return none;
    };
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let data = &value["data"];
    if !output.status.success() || value["success"] != true || data["authenticated"] != true {
        return none;
    }
    Session { authenticated: true, phone: linked_phone(data) }
}

/// Só aceita o telefone quando `phone` é exatamente a parte de usuário de um `linked_jid`
/// de telefone (`…@s.whatsapp.net`) e tem só dígitos. Nada de número digitado ou nome.
fn linked_phone(data: &Value) -> Option<String> {
    let jid = data["linked_jid"].as_str()?;
    let phone = data["phone"].as_str()?;
    let user = jid.strip_suffix("@s.whatsapp.net")?;
    let ok = user == phone && (8..=15).contains(&phone.len()) && phone.chars().all(|c| c.is_ascii_digit());
    ok.then(|| format!("+{phone}"))
}

/// R3b: baixa a mídia de uma mensagem para `output`, sem mexer no store (wacli v0.25.1
/// `--read-only --json media download --chat --id --output`, `cmd/wacli/media.go:236-267`:
/// em só leitura o `--output` é obrigatório, `:246-247`, e o arquivo vem direto do servidor
/// do `WhatsApp`, `cmd/wacli/media_download.go:63-79`). Devolve os bytes que o wacli diz ter
/// gravado (`data.bytes`, `:18-30`). O conteúdo nunca vai para log.
pub(super) async fn download(binary: &Path, dir: &Path, chat: &str, id: &str, output: &Path) -> Result<i64> {
    let mut command = command(binary, dir);
    command
        .args(["--read-only", "--json", "media", "download", "--chat", chat, "--id", id, "--output"])
        .arg(output)
        .stdout(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(180), command.output())
        .await
        .context("whatsapp_media_timeout")?
        .context("whatsapp_media_spawn")?;
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    ensure!(output.status.success() && value["success"] == true, "whatsapp_media_failed");
    value["data"]["bytes"].as_i64().context("whatsapp_media_failed")
}

/// Resultado de um `send text`. Incerto nunca é reenviado (contrato do R4).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Sent {
    /// O wacli confirmou: `{"success":true,"data":{"sent":true,"id":…}}`.
    Yes { id: String },
    /// O processo nem começou: nada saiu.
    NotStarted,
    /// Qualquer outra coisa (erro, saída estranha, tempo esgotado): pode ter saído.
    Uncertain,
}

/// `send text` do wacli v0.25.1 (`cmd/wacli/send.go:56-172`): `--to` com o JID exato (com `@`
/// o wacli não procura por nome, `recipient.go:34-36`), `--message`, sem prévia de link, e sempre
/// `--reply-to` (`send.go:164`): mensagem citada que o wacli não acha dá erro, nunca envio
/// solto (`send.go:302-305,626-638`). Com o
/// `sync --follow` de pé, o wacli delega o envio a ele pela trava do store (`send_ipc.go:165`).
/// Saída `--json`: envelope `{"success","data":{"sent","to","id"}}` (`internal/out/out.go`,
/// `send.go:146-151`, `send_ipc.go:719-724`). Texto e destino nunca vão para log.
pub(super) async fn send_text(binary: &Path, dir: &Path, jid: &str, reply_to: &str, text: &str) -> Sent {
    if reply_to.is_empty() || reply_to.starts_with('-') {
        return Sent::NotStarted;
    }
    let mut command = command(binary, dir);
    command
        .args(["--json", "--timeout", "60s", "send", "text", "--no-preview"])
        .arg(format!("--to={jid}"))
        .arg(format!("--reply-to={reply_to}"))
        .arg(format!("--message={text}"))
        .stdout(Stdio::piped());
    let Ok(child) = command.spawn() else { return Sent::NotStarted };
    let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(75), child.wait_with_output()).await else {
        return Sent::Uncertain;
    };
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let data = &value["data"];
    match data["id"].as_str() {
        Some(id) if output.status.success() && value["success"] == true && data["sent"] == true && !id.is_empty() => {
            Sent::Yes { id: id.to_owned() }
        }
        _ => Sent::Uncertain,
    }
}

/// `auth logout`: avisa o `WhatsApp` e tira o aparelho. Falha = o dono tira no celular.
pub(super) async fn logout(binary: &Path, dir: &Path) -> bool {
    let mut command = command(binary, dir);
    command.args(["auth", "logout"]);
    matches!(tokio::time::timeout(Duration::from_secs(20), command.status()).await, Ok(Ok(s)) if s.success())
}

/// `chats cleanup --jid <JID> --confirm` (comando oficial do wacli v0.25.1,
/// `cmd/wacli/chats_cleanup.go`): apaga do `wacli.db` a conversa e, em cascata, as mensagens
/// e chamadas dela. Só a cópia local; nada no `WhatsApp`. Precisa da trava do store, então o
/// `sync` do bot tem de estar parado.
pub(super) async fn cleanup_chat(binary: &Path, dir: &Path, chat: &str) -> bool {
    let mut command = command(binary, dir);
    command.args(["chats", "cleanup", "--jid", chat, "--confirm"]);
    matches!(tokio::time::timeout(Duration::from_secs(20), command.status()).await, Ok(Ok(s)) if s.success())
}

/// Número só com dígitos (o `+` do DDI é opcional); nunca vai para log.
pub(super) fn phone(raw: &str) -> Result<String> {
    let digits: String = raw.chars().filter(|c| !matches!(c, ' ' | '-' | '(' | ')' | '.')).collect();
    let digits = digits.strip_prefix('+').unwrap_or(&digits);
    ensure!(
        (8..=15).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()),
        "invalid_field: número de telefone inválido."
    );
    Ok(format!("+{digits}"))
}

/// Processo de uma execução anterior do Hub que caiu sem parar o wacli. Só mata se
/// a linha de comando ainda for a desta pasta (o PID pode ter sido reaproveitado).
#[cfg(unix)]
pub(super) async fn reap(dir: &Path) {
    let file = dir.join("wacli.pid");
    let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse::<i32>().ok()) else {
        return;
    };
    let _ = std::fs::remove_file(&file);
    if pid <= 1 {
        return;
    }
    let ps = Command::new("ps").args(["-o", "command=", "-p", &pid.to_string()]).output().await;
    let Ok(ps) = ps else { return };
    let line = String::from_utf8_lossy(&ps.stdout);
    if line.contains(&*dir.to_string_lossy()) && line.contains("sync") {
        // SAFETY: só envia sinal ao grupo que o próprio Hub criou para este wacli.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
pub(super) async fn reap(_dir: &Path) {}

pub(super) fn write_pid(dir: &Path, pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = std::fs::write(dir.join("wacli.pid"), pid.to_string());
    }
}
