//! Grok Build por assinatura: o login por código de pareamento é do próprio programa da xAI.
//! O Hub só repassa o endereço e o código que o programa imprime e confere o resultado com "grok models".
//! O token fica no perfil isolado da conta e nunca é aberto, copiado ou registrado pelo Hub.
use super::{
    process::{Event, environment},
    transport::{self, Challenge},
};
use crate::{
    fork::engine_accounts::{Account, Backend, LaunchReference, Registry, managed, probe},
    hub::{BotStatus, Hub},
    store::Store,
};
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

const RAW_LIMIT: usize = 256 * 1024;
const TAIL: usize = 16 * 1024;

/// Saída do programa como texto: sequências de controle somem e o retorno de carro vira quebra de linha.
#[derive(Default)]
struct Screen {
    text: String,
    raw: usize,
    escape: u8,
    /// Depois de o desafio ir para a tela, a espera pode redesenhar linhas por minutos: o limite de saída deixa de valer
    /// e só a cauda do texto fica guardada, para conferir se aparece outro código ou o motivo do fim.
    settled: bool,
    /// O stream do programa encerrou: a última linha sem quebra é final e entra na conferência.
    eof: bool,
}

impl Screen {
    fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.raw += bytes.len();
        ensure!(self.settled || self.raw <= RAW_LIMIT, "auth_output_limit");
        for &b in bytes {
            match self.escape {
                // ESC: OSC, DCS, SOS, PM e APC são strings até BEL ou ST; CSI até o byte final; ESC ( e afins levam mais um byte.
                1 => {
                    self.escape = match b {
                        b']' | b'P' | b'X' | b'^' | b'_' => 2,
                        b'[' => 4,
                        b'(' | b')' | b'*' | b'+' | b'#' => 5,
                        _ => 0,
                    }
                }
                2 => {
                    if b == 7 {
                        self.escape = 0;
                    } else if b == 27 {
                        self.escape = 3;
                    }
                }
                3 => self.escape = if b == b'\\' { 0 } else { 2 },
                4 => {
                    if (0x40..=0x7e).contains(&b) {
                        self.escape = 0;
                    }
                }
                5 => self.escape = 0,
                _ if b == 27 => self.escape = 1,
                _ if b == b'\r' => self.text.push('\n'),
                _ if b.is_ascii_graphic() || b == b' ' || b == b'\n' || b == b'\t' => self.text.push(char::from(b)),
                _ => {}
            }
        }
        if self.settled {
            if self.text.len() > TAIL {
                self.text.drain(..self.text.len() - TAIL);
            }
            return Ok(());
        }
        ensure!(self.text.len() <= 32 * 1024, "auth_output_limit");
        Ok(())
    }

    /// Linhas completas do texto: enquanto o stream está aberto, a última linha sem quebra fica fora (pode
    /// estar no meio de ser impressa, mesmo depois do desafio); com o stream encerrado, ela é final e entra
    /// na conferência.
    fn complete(&self) -> &str {
        if self.eof { &self.text } else { self.text.rsplit_once('\n').map_or("", |(done, _)| done) }
    }

    /// Endereço e código só saem de linhas completas, para nunca mostrar um código cortado ao meio.
    /// Fail-closed: duas páginas diferentes, dois códigos diferentes (no texto ou na consulta das URLs) ou qualquer
    /// dúvida derrubam o login em vez de mostrar um dos dois.
    fn challenge(&self) -> Result<Option<Challenge>> {
        let done = self.complete();
        let mut urls: Vec<&str> = Vec::new();
        for word in official_urls(done) {
            // URL em site da xAI que não passa na validação derruba o login, em vez de ser ignorada.
            transport::challenge(Backend::Grok, "device_code", word, None)?;
            urls.push(word);
        }
        let Some(&first) = urls.first() else { return Ok(None) };
        // Todas as URLs são da mesma página; a que traz a consulta (se houver) é a que a tela abre.
        let page =
            |raw: &str| reqwest::Url::parse(raw).map(|url| (url.host_str().map(str::to_owned), url.path().to_owned()));
        let reference = page(first).map_err(|_| anyhow!("auth_url_invalid"))?;
        let mut codes = Vec::new();
        let mut shown = first;
        for raw in &urls {
            ensure!(page(raw).ok().as_ref() == Some(&reference), "auth_correlation_invalid");
            let url = reqwest::Url::parse(raw).map_err(|_| anyhow!("auth_url_invalid"))?;
            if let Some((_, code)) = url.query_pairs().next() {
                codes.push(user_code(&code).ok_or_else(|| anyhow!("auth_url_invalid"))?);
                shown = raw;
            }
        }
        codes.extend(text_codes(done));
        let Some(code) = codes.first().cloned() else { return Ok(None) };
        ensure!(codes.iter().all(|other| *other == code), "auth_correlation_invalid");
        Ok(Some(transport::challenge(Backend::Grok, "device_code", shown, Some(&code))?))
    }

    /// Depois do desafio na tela: qualquer outro código impresso (no texto ou no parâmetro de código de uma URL da xAI)
    /// derruba o login. Outras páginas da xAI, como a da conta no texto de sucesso, e outros parâmetros delas não contam.
    fn code_conflict(&self, shown: &str) -> bool {
        let text = self.complete();
        let from_urls = official_urls(text).any(|raw| {
            reqwest::Url::parse(raw).is_ok_and(|url| {
                url.query_pairs().any(|(key, code)| {
                    matches!(&*key, "user_code" | "code") && user_code(&code).is_none_or(|code| code != shown)
                })
            })
        });
        from_urls || text_codes(text).iter().any(|code| code != shown)
    }

    fn expired(&self) -> bool {
        self.text.to_lowercase().contains("expired")
    }
}

/// Palavras que são endereço https em site da xAI (os dois sites de login), sem pontuação no fim.
fn official_urls(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter(|word| word.starts_with("https://"))
        .map(|word| word.trim_end_matches(['.', ',', ';', ')', '>', '"', '\'']))
        .filter(|word| {
            reqwest::Url::parse(word).is_ok_and(|url| matches!(url.host_str(), Some("auth.x.ai" | "accounts.x.ai")))
        })
}

/// Códigos de pareamento impressos como texto: depois de um rótulo com "code:" ou numa linha inteira ABCD-EFGH.
fn text_codes(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.contains("http"))
        .filter_map(|line| {
            let labelled = if line.to_lowercase().contains("code") { line.rsplit_once(':') } else { None };
            labelled.and_then(|(_, rest)| user_code(rest)).or_else(|| paired_code(line.trim()))
        })
        .collect()
}

/// O servidor só emite maiúsculas, dígitos e hífen no código de pareamento.
fn user_code(raw: &str) -> Option<String> {
    let code = raw.trim().trim_matches(['[', ']', '(', ')', '\'', '"', '.']);
    let valid = (4..=24).contains(&code.len())
        && code.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        && code.bytes().any(|b| b.is_ascii_alphanumeric());
    valid.then(|| code.to_owned())
}

/// Código sem rótulo: só uma linha inteira no formato ABCD-EFGH.
fn paired_code(line: &str) -> Option<String> {
    let (left, right) = line.split_once('-')?;
    let block = |part: &str| {
        (4..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    };
    (block(left) && block(right)).then(|| line.to_owned())
}

/// O programa de login no PTY. Só encerra o grupo se o filho ainda não foi colhido: depois de colhido o PID pode ser de outro.
struct LoginPty {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    reaped: bool,
}
impl Drop for LoginPty {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        super::process::kill_group(self.child.process_id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Registra só o código do motivo (nunca a saída do programa) de uma recusa na leitura do login.
fn refused<T>(result: Result<T>) -> Result<T> {
    if let Err(error) = &result {
        tracing::warn!(reason = %error, "login do Grok recusado");
    }
    result
}

fn refuse(consistent: bool) -> Result<()> {
    refused(if consistent { Ok(()) } else { Err(anyhow!("auth_correlation_invalid")) })
}

pub(super) async fn login(
    executable: &Path,
    directory: &Path,
    events: mpsc::Sender<Event>,
) -> Result<probe::Observation> {
    use portable_pty::{PtySize, native_pty_system};
    let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 100, pixel_width: 0, pixel_height: 0 })?;
    let mut cmd = crate::fork::motor_exec::pty(executable, directory)?;
    cmd.args(["login", "--device-auth"]);
    cmd.env_clear();
    for (key, value) in environment(directory, Backend::Grok) {
        cmd.env(key, value);
    }
    cmd.cwd(crate::fork::motor_exec::hub_cwd(directory));
    #[cfg(unix)]
    cmd.umask(Some(0o077));
    let mut reader = pair.master.try_clone_reader()?;
    let child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    // O destrutor encerra o grupo do programa no cancelamento, no prazo e em qualquer erro.
    let mut pty = LoginPty { child, _master: pair.master, reaped: false };
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(16);
    std::thread::spawn(move || {
        let mut bytes = [0_u8; 2048];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.blocking_send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut screen = Screen::default();
    let mut offered: Option<String> = None;
    let mut output_closed = false;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let status = loop {
        tokio::select! {
            bytes = rx.recv(), if !output_closed => match bytes {
                Some(bytes) => {
                    screen.push(&bytes)?;
                    // Continua conferindo a saída: outro código depois do que foi mostrado derruba o login.
                    if let Some(shown) = &offered {
                        refuse(!screen.code_conflict(shown))?;
                    } else if let Some(challenge) = refused(screen.challenge())? {
                        offered = challenge.code.clone();
                        events.send(Event::Challenge(challenge)).await?;
                        screen.settled = true;
                    }
                }
                None => output_closed = true,
            },
            _ = tick.tick() => if let Some(status) = pty.child.try_wait()? { break status },
        }
    };
    pty.reaped = true;
    drop(pty);
    // Última saída do programa, para distinguir código vencido de recusa. O dreno ainda lê com o stream
    // "aberto": pedaço cortado não é conflito. Só com o dreno vazio a última linha sem quebra é final e
    // entra na conferência, uma única vez, depois do laço.
    while let Ok(Some(bytes)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
        screen.push(&bytes)?;
        if let Some(shown) = &offered {
            refuse(!screen.code_conflict(shown))?;
        }
    }
    screen.eof = true;
    if let Some(shown) = &offered {
        refuse(!screen.code_conflict(shown))?;
    }
    if !status.success() {
        bail!(if screen.expired() { "setup_expired" } else { "auth_failed" });
    }
    ensure!(offered.is_some(), "auth_correlation_invalid");
    events.send(Event::Verifying).await?;
    Ok(probe::inspect(Backend::Grok, executable, directory).await)
}

/// Pasta de perfil (home da conta) que guarda a credencial do programa; só de conta Grok gerenciada.
pub(super) fn profile_home(account: &Account) -> Option<PathBuf> {
    let (Backend::Grok, Some(_), LaunchReference::CliProfile { directory, .. }) =
        (account.backend, &account.managed, &account.launch)
    else {
        return None;
    };
    directory.parent().map(Path::to_path_buf)
}

/// A pasta antiga fica para trás quando um novo login cria outra: a credencial dela também sai.
pub(super) fn previous_home(account: &Account, next: &Path) -> Option<PathBuf> {
    profile_home(account).filter(|home| next.parent() != Some(home.as_path()))
}

/// Confere que a pasta tem o formato exato criado pelo Hub (raiz/accounts/id/uuid), sem links em nenhum trecho do caminho.
fn checked_home(store: &Store, id: &str, home: &Path) -> Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(home) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("profile_invalid: perfil indisponível."),
    }
    let directory = home.join(Backend::Grok.profile_directory());
    let relative = directory
        .strip_prefix(store.engine_setup_root.join("accounts").join(id))
        .map_err(|_| anyhow!("profile_invalid: referência inválida."))?;
    let mut parts = relative.components();
    let generation = parts.next().and_then(|part| part.as_os_str().to_str());
    ensure!(
        generation.is_some_and(|name| uuid::Uuid::parse_str(name).is_ok())
            && parts.next().is_some_and(|part| part.as_os_str() == Backend::Grok.profile_directory())
            && parts.next().is_none(),
        "profile_invalid: referência inválida."
    );
    // Raiz, accounts, id e a pasta da geração precisam ser diretórios reais, mesmo sem .grok dentro.
    managed::validate_profile(store, id, home)?;
    if std::fs::symlink_metadata(&directory).is_ok() {
        managed::validate_profile(store, id, &directory)?;
    }
    Ok(Some(directory))
}

/// Desconectar: apaga tudo o que o programa guardou na pasta da conta (login, sessões, cache), mas deixa o marcador do
/// Hub e uma pasta de perfil vazia no lugar, para a conta poder entrar de novo. Falha é erro, nunca silêncio.
pub(super) fn wipe(store: &Store, id: &str, home: &Path) -> Result<()> {
    let Some(directory) = checked_home(store, id, home)? else { return Ok(()) };
    let failed = || {
        anyhow!(
            "credential_not_erased: não foi possível apagar o login local. Remova a conta ou conecte de novo para tentar outra vez."
        )
    };
    for entry in std::fs::read_dir(home).map_err(|_| failed())? {
        let entry = entry.map_err(|_| failed())?;
        if entry.file_name() == ".donnabot-profile" {
            continue;
        }
        let kind = entry.file_type().map_err(|_| failed())?;
        if kind.is_dir() {
            std::fs::remove_dir_all(entry.path()).map_err(|_| failed())?;
        } else {
            std::fs::remove_file(entry.path()).map_err(|_| failed())?;
        }
    }
    managed::private_dir(&directory).map_err(|_| failed())
}

/// Perfis órfãos da conta: pasta com o formato exato do Hub, no lugar da conta, que o registro não aponta
/// mais — sobra de um apagamento que falhou depois de a conta já apontar para outro. Sai no fim de um login
/// bem-sucedido, na desconexão e na remoção; falha em uma pasta vira aviso e não impede as outras.
pub(super) fn sweep_orphans(store: &Store, id: &str, keep: Option<&Path>) {
    let root = store.engine_setup_root.join("accounts").join(id);
    let Ok(entries) = std::fs::read_dir(&root) else { return };
    for entry in entries.flatten() {
        let home = entry.path();
        if keep.is_some_and(|keep| home == keep) {
            continue;
        }
        // Pasta que não existe ou sem o formato exato do Hub não é nossa: fica.
        if let Ok(Some(_)) = checked_home(store, id, &home)
            && let Err(error) = managed::remove_profile_home(&home)
        {
            tracing::warn!(home = %home.display(), %error, "perfil órfão do Grok não removido");
        }
    }
}

/// Remover a conta ou aposentar o perfil de um login anterior: a pasta inteira sai.
pub(super) fn erase(store: &Store, id: &str, home: &Path) -> Result<()> {
    if checked_home(store, id, home)?.is_none() {
        return Ok(());
    }
    managed::remove_profile_home(home)
        .map_err(|_| anyhow!("credential_not_erased: não foi possível apagar o login local. Remova a conta para apagar o que sobrar dele."))
}

/// Antes de desconectar ou remover uma conta Grok, fora da trava de contas (a espera pode durar alguns segundos) e só
/// depois de a solicitação passar na validação: confere revisão e operação em andamento; desconectar encerra o trabalho
/// em curso dos bots da conta; remover exige a conta sem bots e já apaga o login local. A mutação confere tudo de novo
/// e recusa se algo mudou no meio.
pub(super) async fn before_mutation(hub: &Arc<Hub>, method: &str, body: &Value) -> Result<()> {
    let request = super::parse::<super::Mutation>(body.clone())?;
    let id = request.engine_account_id.as_str();
    let home = {
        let mut registry = Registry::load(&hub.store)?;
        if registry.account(id).is_ok_and(|account| account.backend != Backend::Grok) {
            return Ok(());
        }
        let home = profile_home(registry.account_mut(id, request.expected_revision)?);
        ensure!(!super::in_setup(&hub.store, id), "setup_in_progress: cancele o setup primeiro.");
        home
    };
    if method == "donnaEngineAccountRemove" {
        ensure!(managed::bot_ids(&hub.store, id)?.is_empty(), "account_in_use: remova os vínculos dos bots primeiro.");
        managed::idle(hub, id)?;
        if let Some(home) = home {
            wipe(&hub.store, id, &home)?;
        }
        return Ok(());
    }
    for bot in managed::bot_ids(&hub.store, id)? {
        if matches!(hub.runtime(&bot).status, BotStatus::Working | BotStatus::NeedsInput | BotStatus::Waiting) {
            let _ = hub.send_cmd(&bot, crate::agent::bot::Cmd::Stop);
        }
    }
    for _ in 0..50 {
        if managed::idle(hub, id).is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("account_busy: aguarde o trabalho parar.")
}

#[cfg(test)]
mod tests;
