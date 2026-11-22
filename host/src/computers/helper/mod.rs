//! Executor de referência (lado do computador do dono) do protocolo de computadores do dono; ver docs/guides/executor-de-planilha.md.
//!
//! É um processo separado do Hub, com identidade e estado próprios, que fala o protocolo executor que o Hub já tem
//! (cadastro por código, hello/poll/start/result/audit/bye). Faz UMA coisa: criar planilha nova num local que o dono
//! liberou aqui. Não é o ajudante nativo do Mac com Touch ID (que continua previsto) nem tem shell, programa ou leitura.
mod policy;
mod state;
mod wire;

use crate::remote::crypto;
use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::Subcommand;
use hmac::{Hmac, KeyInit as _, Mac as _};
use serde_json::{Value, json};
use sha2::Sha256;
use state::{Authorized, Grant, Home, HubLink};
use std::io::BufRead as _;
use std::path::PathBuf;
use std::time::Duration;
use wire::{HubError, Session};

#[derive(Subcommand)]
pub enum Action {
    /// Cadastra este computador no Hub com o código de 16 caracteres mostrado no Hub (vale 10 minutos, uma vez).
    Enroll {
        #[arg(long)]
        hub: String,
        /// Sem esta opção o código é pedido na entrada padrão (não fica no histórico nem nos argumentos do processo).
        #[arg(long)]
        code: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Libera, neste computador e por decisão sua, um local para a Mexerica criar planilhas novas.
    Authorize {
        #[arg(long)]
        location: String,
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Tira a liberação de um local.
    Unauthorize {
        #[arg(long)]
        location: String,
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Mostra o vínculo, os locais liberados e o diário.
    Status {
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Fica conectado ao Hub e atende pedidos de planilha; Ctrl-C desconecta.
    Run {
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Revoga este computador no Hub e apaga a identidade e as liberações locais.
    Revoke {
        #[arg(long)]
        local_only: bool,
        #[arg(long)]
        home: Option<PathBuf>,
    },
}

pub(crate) fn state_dir(action: &Action) -> Result<PathBuf> {
    let custom = match action {
        Action::Enroll { home, .. }
        | Action::Authorize { home, .. }
        | Action::Unauthorize { home, .. }
        | Action::Status { home }
        | Action::Run { home }
        | Action::Revoke { home, .. } => home.clone(),
    };
    match custom {
        Some(p) => Ok(p),
        None => Ok(dirs::data_dir()
            .ok_or_else(|| anyhow!("sem pasta de dados; use --home"))?
            .join("Mexerica")
            .join("executor")),
    }
}

fn home(path: Option<PathBuf>) -> Result<Home> {
    let dir = match path {
        Some(p) => p,
        None => {
            dirs::data_dir().ok_or_else(|| anyhow!("sem pasta de dados; use --home"))?.join("Mexerica").join("executor")
        }
    };
    Home::open(&dir)
}

/// Só o macOS grava: o Windows não tem ainda gravação confinada por handle, então o executor recusa lá.
/// Em build de desenvolvimento, a plataforma pode ser declarada à mão
/// (`MEXERICA_EXECUTOR_TEST_PLATFORM`) para provar o protocolo numa máquina Linux descartável; isso é simulação.
fn platform() -> Result<&'static str> {
    #[cfg(debug_assertions)]
    let os = std::env::var("MEXERICA_EXECUTOR_TEST_PLATFORM").ok();
    #[cfg(not(debug_assertions))]
    let os: Option<String> = None;
    match os.as_deref().unwrap_or(std::env::consts::OS) {
        "macos" => Ok("macos"),
        other => bail!("este executor só funciona no macOS ({other} não é suportado ainda)"),
    }
}

pub async fn main(action: Action) -> Result<()> {
    match action {
        Action::Enroll { hub, code, name, home: h } => {
            let code = if let Some(code) = code {
                code
            } else {
                println!("Digite o código de cadastro mostrado no Hub e Enter:");
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                line
            };
            enroll(&home(h)?, &hub, &code, &name).await
        }
        Action::Authorize { location, home: h } => authorize(&home(h)?, &location),
        Action::Unauthorize { location, home: h } => {
            ensure!(policy::LOCATIONS.contains(&location.as_str()), "local desconhecido");
            let home = home(h)?;
            let _lock = home.lock()?;
            let mut auth = home.authorized()?;
            home.bump_auth_epoch()?;
            auth.desktop = None;
            home.save_authorized(&auth)?;
            println!("Liberação de {location} removida. Pedidos para esse local serão recusados.");
            Ok(())
        }
        Action::Status { home: h } => status(&home(h)?),
        Action::Run { home: h } => run(&home(h)?).await,
        Action::Revoke { local_only, home: h } => revoke(&home(h)?, local_only).await,
    }
}

async fn enroll(home: &Home, hub: &str, code: &str, name: &str) -> Result<()> {
    let _enrolling = home.enroll_lock()?;
    state::ensure_plain_url(hub)?;
    let name = name.trim();
    ensure!(!name.is_empty() && name.chars().count() <= 100, "nome inválido");
    // A chave é escolhida e guardada sob o trinco, antes de qualquer rede: um cadastro não pode trocar a chave do outro.
    let (key, enroll_epoch) = {
        let _lock = home.lock()?;
        ensure!(!home.has_link_file(), "este computador já tem vínculo; revogue antes de cadastrar de novo");
        let epoch = home.bump_binding_epoch()?;
        home.bump_auth_epoch()?;
        home.reset_binding()?;
        (home.key(true)?, epoch)
    };
    let norm: String =
        code.chars().filter(|c| *c != '-' && !c.is_whitespace()).map(|c| c.to_ascii_uppercase()).collect();
    ensure!(norm.len() == 16 && norm.is_ascii(), "o código tem 16 caracteres (4 grupos de 4)");
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(format!("{}/computers/enroll-hello", hub.trim_end_matches('/')))
        .timeout(Duration::from_secs(20))
        .json(&json!({"a": &norm[..4]}))
        .send()
        .await
        .context("não foi possível falar com o Hub")?;
    ensure!(
        response.status().as_u16() != 429,
        "rate_limited: o Hub limita tentativas (3 por minuto); espere um minuto"
    );
    ensure!(response.status().is_success(), "o Hub recusou o código (HTTP {}): expirado ou errado", response.status());
    let hello: Value = response.json().await?;
    let sign_pub =
        crypto::unb64_n::<32>(hello["signPub"].as_str().unwrap_or_default()).context("resposta do Hub inválida")?;
    let cid = hello["cid"].as_str().unwrap_or_default();
    ensure!(cid == crypto::computer_id(&sign_pub), "o Hub informou uma identidade incoerente; cadastro recusado");
    let mut mac =
        Hmac::<Sha256>::new_from_slice(norm.as_bytes()[4..].as_ref()).map_err(|_| anyhow!("código inválido"))?;
    mac.update(&sign_pub);
    mac.update(cid.as_bytes());
    mac.verify_slice(&crypto::unb64(hello["mac"].as_str().unwrap_or_default())?)
        .map_err(|_| anyhow!("o Hub não provou conhecer o código; pode não ser o seu Hub. Cadastro recusado"))?;
    {
        let _lock = home.lock()?;
        ensure!(
            !home.has_link_file(),
            "este computador foi cadastrado enquanto você informava o código; nada foi pareado"
        );
        ensure!(home.binding_epoch()? == enroll_epoch, "o estado local mudou durante o cadastro; nada foi pareado");
    }
    let mut session = Session::connect(hub, &key, &sign_pub, true).await?;
    let ok = session
        .call(
            "pair",
            json!({"role":"executor","code":norm,"name":name,"platform":platform()?,"arch":std::env::consts::ARCH}),
            Duration::from_secs(20),
        )
        .await?;
    session.expect_paired_close().await?;
    {
        let _lock = home.lock()?;
        ensure!(!home.has_link_file(), "este computador foi cadastrado durante o cadastro; nada foi salvo");
        ensure!(home.binding_epoch()? == enroll_epoch, "o estado local mudou durante o cadastro; nada foi salvo");
        let computer_id = ok["computerId"].as_str().unwrap_or_default();
        ensure!(
            computer_id == crypto::computer_id(&key.verifying_key().to_bytes()),
            "o Hub cadastrou uma chave diferente desta; nada foi salvo"
        );
        home.bump_binding_epoch()?;
        home.bump_auth_epoch()?;
        home.save_link(&HubLink {
            url: hub.trim_end_matches('/').into(),
            sign_pub: crypto::b64(&sign_pub),
            label: name.into(),
            computer_id: computer_id.into(),
            binding_id: crypto::b64(&crypto::random::<16>()),
        })?;
    }
    println!(
        "Cadastrado como {name}. Nenhum local foi liberado ainda: rode authorize para escolher onde a Mexerica pode criar planilhas."
    );
    Ok(())
}

fn authorize(home: &Home, location: &str) -> Result<()> {
    ensure!(policy::LOCATIONS.contains(&location), "local desconhecido; use: {}", policy::LOCATIONS.join(", "));
    let dir = policy::location_dir(location).map_err(|r| anyhow!(r.message))?;
    let root = dir.canonicalize().with_context(|| format!("a pasta {} não existe", dir.display()))?;
    let home_dir = policy::home_dir().ok_or_else(|| anyhow!("pasta pessoal desconhecida"))?;
    ensure!(
        !policy::critical(&root, &home_dir),
        "{} é uma área crítica do sistema ou da conta; não pode ser liberada",
        root.display()
    );
    let (dev, ino) = policy::dir_identity(&root).ok_or_else(|| anyhow!("não consegui identificar a pasta"))?;
    let (initial_link, initial_auth_epoch) = {
        let _lock = home.lock()?;
        (home.link().ok(), home.auth_epoch()?)
    };
    println!(
        "Você está liberando a Mexerica a CRIAR planilhas NOVAS em:\n  {}\nElas nunca sobrescrevem, apagam, movem nem leem arquivos seus. Dentro dessa pasta, pedidos explícitos do assistente não perguntam de novo.",
        root.display()
    );
    // A confirmação é um ato do dono: entrada redirecionada por outro programa não vale (build de depuração, usado pelos
    // testes automatizados, dispensa isto).
    #[cfg(not(debug_assertions))]
    ensure!(
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
        "a liberação precisa ser confirmada por você, digitando no terminal; entrada redirecionada não vale"
    );
    println!("Para confirmar, digite AUTORIZAR e Enter (qualquer outra coisa cancela):");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    ensure!(line.trim() == "AUTORIZAR", "cancelado; nada foi liberado");
    // Mesmo trinco da retirada, da revogação e da criação: uma liberação lenta não pode terminar depois delas.
    let _lock = home.lock()?;
    auth_state_unchanged(
        home,
        initial_link.as_ref(),
        &initial_auth_epoch,
        "o vínculo ou a liberação mudou enquanto você confirmava; nada foi liberado",
    )?;
    let mut auth = home.authorized()?;
    // A identidade guardada é a da pasta mostrada na confirmação: se ela foi trocada enquanto você lia, cancela.
    ensure!(policy::same_dir(&root, dev, ino), "a pasta mudou enquanto você confirmava; nada foi liberado");
    home.bump_auth_epoch()?;
    auth.desktop = Some(Grant { path: root.to_string_lossy().into_owned(), at: crate::store::now_ms(), dev, ino });
    home.save_authorized(&auth)?;
    println!("Liberado: {location}. Para retirar: unauthorize --location {location}.");
    Ok(())
}

fn status(home: &Home) -> Result<()> {
    match home.link() {
        Ok(l) => println!("Vínculo: {} ({}) no Hub {}", l.label, l.computer_id, l.url),
        Err(e) => println!("Sem vínculo: {e}"),
    }
    match home.authorized()?.desktop {
        Some(g) => println!("Liberado: desktop = {}", g.path),
        None => println!("Nada liberado."),
    }
    let rows = home.audit_rows()?;
    println!("Diário: {} registros ({} já confirmados pelo Hub)", rows.len(), home.audit_acked());
    for r in rows.iter().rev().take(5).rev() {
        println!(
            "  #{} {} {}",
            r["seq"],
            r["event"].as_str().unwrap_or_default(),
            r["jobId"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

async fn revoke(home: &Home, local_only: bool) -> Result<()> {
    // Um cadastro em andamento pode estar a ponto de salvar o vínculo: revogar agora apagaria a chave que ele usa.
    let _enrolling = home.enroll_lock()?;
    // O vínculo que esta revogação alcança é o de agora: se mudar enquanto a rede responde, o novo não é apagado.
    // Vínculo e chave são lidos juntos, sob o trinco: um recadastro no meio não mistura o novo com o antigo.
    let (link, key, initial_binding_epoch) = {
        let _lock = home.lock()?;
        (home.link().ok(), home.key(false).ok(), home.binding_epoch()?)
    };
    if !local_only {
        let link = link.as_ref().context("este computador ainda não foi cadastrado no Hub")?;
        let key = key.as_ref().context("identidade do executor ausente; cadastre o computador antes")?;
        let sign_pub = crypto::unb64_n::<32>(&link.sign_pub)?;
        let mut session = Session::connect(&link.url, key, &sign_pub, false)
            .await
            .context("não consegui avisar o Hub; use --local-only para apagar só aqui")?;
        // O Hub apaga a credencial ao processar o pedido e fecha o canal antes de a resposta chegar: erro aqui não
        // significa que a revogação falhou, só que a confirmação não veio.
        match session.call("executorBye", json!({"reason":"revoke"}), Duration::from_secs(20)).await {
            Ok(_) => println!("Hub avisado: esta credencial foi revogada lá."),
            Err(e) => {
                println!("O Hub não confirmou ({e:#}); confira na lista de computadores do Hub que este foi removido.");
            }
        }
        session.close().await;
    }
    // Mesmo trinco da criação, só durante a limpeza (não durante a rede): se um pedido está gravando, a revogação
    // espera; depois dela nada novo é criado.
    let _lock = home.lock()?;
    binding_state_unchanged(
        home,
        link.as_ref(),
        &initial_binding_epoch,
        "o vínculo mudou durante a revogação; nada foi apagado",
    )?;
    home.bump_binding_epoch()?;
    home.bump_auth_epoch()?;
    home.wipe_identity()?;
    println!("Identidade e liberações locais apagadas.");
    Ok(())
}

fn capabilities(home: &Home) -> Value {
    let auth = home.authorized().unwrap_or_else(|_| Authorized::default());
    let mut locations = Vec::new();
    if auth.desktop.is_some() {
        locations.push("desktop");
    }
    json!({"planilha": !locations.is_empty(), "locais": locations})
}
fn presence() -> Value {
    json!({"appConnected":true,"locked":false,"idleSeconds":0,"stopped":false})
}

async fn run(home: &Home) -> Result<()> {
    let _only_one = home.run_lock()?;
    let link = home.link()?;
    let key = home.key(false)?;
    let sign_pub = crypto::unb64_n::<32>(&link.sign_pub)?;
    let boot = crypto::b64(&crypto::random::<12>());
    let mut backoff = 1u64;
    println!(
        "Executor {} conectando ao Hub {}. Locais liberados: {}",
        link.label,
        link.url,
        capabilities(home)["locais"]
    );
    loop {
        let started = std::time::Instant::now();
        let one = async {
            same_binding(home, &link)?;
            let mut session = Session::connect(&link.url, &key, &sign_pub, false).await?;
            session_loop(&mut session, home, &boot, &link).await
        };
        tokio::select! {
            result = one => match result {
                Err(e) if revoked(&e) => bail!("o Hub revogou este computador; esta credencial não vale mais. Cadastre de novo se quiser voltar"),
                Err(e) => { eprintln!("conexão caiu: {e:#}; tentando de novo em {backoff}s"); }
                Ok(()) => {}
            },
            _ = tokio::signal::ctrl_c() => {
                println!("Encerrando; avisando o Hub.");
                if let Ok(mut s) = Session::connect(&link.url, &key, &sign_pub, false).await {
                    let _ = s.call("executorBye", json!({"reason":"quit"}), Duration::from_secs(5)).await;
                    s.close().await;
                }
                return Ok(());
            }
        }
        // Uma sessão que durou se desfez por uma oscilação: a próxima tentativa volta a ser rápida.
        if started.elapsed() > Duration::from_secs(60) {
            backoff = 1;
        }
        // Vínculo local removido ou trocado: este processo acabou (e solta o trinco de `run` para o novo).
        same_binding(home, &link)?;
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

/// Este processo conserva a chave e o Hub do vínculo em que nasceu: se o vínculo mudou ou sumiu, ele para.
fn same_binding(home: &Home, link: &HubLink) -> Result<()> {
    let now = home.link().context("o vínculo com o Hub mudou ou foi removido; este processo para")?;
    ensure!(
        now.computer_id == link.computer_id
            && now.url == link.url
            && now.sign_pub == link.sign_pub
            && now.binding_id == link.binding_id,
        "o vínculo com o Hub mudou; este processo para"
    );
    let key = home.key(false).context("a identidade do vínculo mudou; este processo para")?;
    ensure!(
        crypto::computer_id(&key.verifying_key().to_bytes()) == link.computer_id,
        "a chave local não corresponde ao vínculo; este processo para"
    );
    Ok(())
}

fn auth_state_unchanged(home: &Home, link: Option<&HubLink>, epoch: &str, message: &str) -> Result<()> {
    ensure!(home.link().ok().as_ref() == link && home.auth_epoch()? == epoch, "{message}");
    Ok(())
}

fn binding_state_unchanged(home: &Home, link: Option<&HubLink>, epoch: &str, message: &str) -> Result<()> {
    ensure!(home.link().ok().as_ref() == link && home.binding_epoch()? == epoch, "{message}");
    Ok(())
}

/// O Hub fecha com 4003 e depois recusa o dispositivo: sem tentar de novo, a credencial acabou.
fn revoked(e: &anyhow::Error) -> bool {
    let text = format!("{e:#}");
    text.contains("removed:") || text.contains("Pair it again") || text.contains("4003")
}

fn journal_for_hello(home: &Home) -> Vec<Value> {
    let mut rows: Vec<Value> =
        home.outbox().into_iter().map(|(_, r)| json!({"jobId":r["jobId"],"state":"done"})).collect();
    rows.extend(home.pending_running().into_iter().map(|(id, _)| json!({"jobId":id,"state":"unknown"})));
    rows.truncate(50);
    rows
}

fn audit_refusal(home: &Home, link: &HubLink, id: &str, detail: &Value) -> Result<u64> {
    let _lock = home.lock()?;
    same_binding(home, link)?;
    home.audit(id, "refused", detail)
}
fn abort_job(home: &Home, link: &HubLink, id: &str, why: &str) -> Result<()> {
    let _lock = home.lock()?;
    same_binding(home, link)?;
    home.audit(id, "aborted", &json!({"why":why}))?;
    home.clear_running(id);
    Ok(())
}
fn clear_job(home: &Home, link: &HubLink, id: &str) -> Result<()> {
    let _lock = home.lock()?;
    same_binding(home, link)?;
    home.clear_outbox(id);
    home.clear_running(id);
    Ok(())
}

async fn session_loop(s: &mut Session, home: &Home, boot: &str, link: &HubLink) -> Result<()> {
    let hello_body = {
        let _lock = home.lock()?;
        same_binding(home, link)?;
        json!({"proto":1,"bootId":boot,"version":env!("CARGO_PKG_VERSION"),"platform":platform()?,"arch":std::env::consts::ARCH,
            "capabilities":capabilities(home),"presence":presence(),"journal":journal_for_hello(home),"routines":[]})
    };
    let hello = s.call("executorHello", hello_body, Duration::from_secs(20)).await?;
    for item in hello["reconcile"].as_array().into_iter().flatten() {
        let id = item["jobId"].as_str().unwrap_or_default();
        if item["action"] == "resend" {
            match resend(s, home, link, id).await {
                Err(e) if e.downcast_ref::<HubError>().is_some_and(HubError::terminal) => {
                    println!("o Hub recusou o resultado de {id}: {e:#}; descartado aqui");
                    clear_job(home, link, id)?;
                }
                other => other?,
            }
        } else {
            clear_job(home, link, id)?;
        }
    }
    sync_audit(s, home, link).await;
    println!("Conectado. Aguardando pedidos.");
    loop {
        let poll_body = {
            let _lock = home.lock()?;
            same_binding(home, link)?;
            json!({"bootId":boot,"waitMs":25_000,"presence":presence(),"capabilities":capabilities(home)})
        };
        let poll = s.call("executorPoll", poll_body, Duration::from_secs(40)).await?;
        for job in poll["jobs"].as_array().into_iter().flatten() {
            handle(s, home, job, link).await?;
            sync_audit(s, home, link).await;
        }
        sync_audit(s, home, link).await;
    }
}

async fn resend(s: &mut Session, home: &Home, link: &HubLink, id: &str) -> Result<()> {
    enum Source {
        Saved(Value),
        Recover(String, String),
    }
    let source = {
        let _lock = home.lock()?;
        same_binding(home, link)?;
        if let Some((_, result)) = home.outbox().into_iter().find(|(name, _)| name == id) {
            Some(Source::Saved(result))
        } else {
            home.pending_running()
                .into_iter()
                .find(|(name, _)| name == id)
                .map(|(job, lease)| Source::Recover(job, lease))
        }
    };
    let Some(source) = source else { return Ok(()) };
    let from_running = matches!(&source, Source::Recover(_, _));
    let result = match source {
        Source::Saved(result) => result,
        Source::Recover(job, lease) => recovered_result(home, link, &job, &lease)?,
    };
    if from_running {
        // O resultado reconstruído vira resultado guardado ANTES de ir ao Hub: se o envio falhar, o reenvio manda este
        // mesmo resultado, sem recalcular uma prova que o disco já pode não confirmar.
        let _lock = home.lock()?;
        same_binding(home, link)?;
        home.save_outbox(id, &result)?;
        home.clear_running(id);
    }
    s.call("executorResult", result, Duration::from_secs(20)).await?;
    {
        let _lock = home.lock()?;
        same_binding(home, link)?;
        home.clear_outbox(id);
    }
    Ok(())
}

/// O executor caiu com o pedido em andamento. Só o que está PROVADO vira resultado:
/// - fase "planned" (nada no disco havia sido tocado): falha segura `not_written`, repetir é liberado;
/// - fase "writing": só o arquivo íntegro cuja identidade (dispositivo e inode) a tentativa registrou ao criá-lo vira recibo `done`;
/// - qualquer outra coisa (arquivo sumiu, alvo que já existia, dúvida) continua `unknown` e o Hub barra a repetição.
fn recovered_result(home: &Home, link: &HubLink, job: &str, lease: &str) -> Result<Value> {
    let _lock = home.lock()?;
    same_binding(home, link)?;
    let found = (|| {
        let record = home.running_record(job)?;
        if record["phase"] == "planned" {
            return Some(None);
        }
        if record["phase"] != "writing" || record["preexisting"] != false {
            return None;
        }
        let created = record["created"].as_array()?;
        let created = (created.first()?.as_u64()?, created.get(1)?.as_u64()?);
        let plan = policy::plan(record["kind"].as_str()?, &record["spec"]).ok()?;
        let folder_id = if plan.folder.is_some() {
            let folder = record["folderId"].as_array()?;
            Some((folder.first()?.as_u64()?, folder.get(1)?.as_u64()?))
        } else {
            None
        };
        let grant = home.authorized().ok()?.desktop?;
        let root = policy::authorized_root(&plan.location, Some(&grant.path)).ok()?;
        if grant.dev == 0 || grant.ino == 0 || !policy::same_dir(&root, grant.dev, grant.ino) {
            return None;
        }
        policy::recover(&plan, &root, Some((grant.dev, grant.ino)), folder_id, created).map(|r| Some(r.0))
    })();
    Ok(match found {
        Some(Some(receipt)) => {
            let seq = home.audit(job, "recovered", &json!({"path":receipt["path"],"sha256":receipt["sha256"]}))?;
            json!({"jobId":job,"leaseToken":lease,"outcome":"done","decidedBy":"policy","auditSeq":seq,"receipt":receipt})
        }
        Some(None) => {
            let seq = home.audit(job, "not_written", &json!({"why":"o executor caiu antes de tocar no disco"}))?;
            json!({"jobId":job,"leaseToken":lease,"outcome":"failed","code":"not_written","decidedBy":"policy","auditSeq":seq,
                "message":"o computador reiniciou antes de gravar; nenhum arquivo foi criado"})
        }
        None => {
            let seq = home.audit(job, "unknown", &json!({"why":"o executor caiu e o disco não permite concluir"}))?;
            json!({"jobId":job,"leaseToken":lease,"outcome":"unknown","code":"executor_restarted","decidedBy":"policy","auditSeq":seq})
        }
    })
}

/// Só em build de depuração: derruba o executor num ponto exato para provar a conferência pós-queda nos testes.
fn crash_point(name: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("MEXERICA_EXECUTOR_TEST_CRASH").as_deref() == Ok(name) {
        std::process::exit(86);
    }
    #[cfg(not(debug_assertions))]
    let _ = name;
}

async fn sync_audit(s: &mut Session, home: &Home, link: &HubLink) {
    let batch = {
        let Ok(_lock) = home.lock() else { return };
        if same_binding(home, link).is_err() {
            return;
        }
        let Ok(rows) = home.audit_rows() else { return };
        let acked = home.audit_acked();
        rows.into_iter().filter(|r| r["seq"].as_u64().unwrap_or(0) > acked).take(50).collect::<Vec<_>>()
    };
    if batch.is_empty() {
        return;
    }
    match s.call("executorAudit", json!({"records":batch}), Duration::from_secs(20)).await {
        Ok(ok) => {
            if let Some(seq) = ok["ackSeq"].as_u64() {
                let Ok(_lock) = home.lock() else { return };
                if same_binding(home, link).is_err() {
                    return;
                }
                let _ = home.set_audit_acked(seq);
            }
        }
        Err(e) => eprintln!("diário não confirmado pelo Hub: {e:#}"),
    }
}

async fn finish(s: &mut Session, home: &Home, link: &HubLink, id: &str, result: Value) -> Result<()> {
    {
        let _lock = home.lock()?;
        same_binding(home, link)?;
        home.save_outbox(id, &result)?;
        home.clear_running(id);
    }
    crash_point("after_outbox");
    match s.call("executorResult", result, Duration::from_secs(20)).await {
        Ok(_) => {
            let _lock = home.lock()?;
            same_binding(home, link)?;
            home.clear_outbox(id);
        }
        Err(e) if e.downcast_ref::<HubError>().is_some_and(HubError::terminal) => {
            println!("o Hub recusou o resultado de {id}: {e:#}; descartado aqui");
            let _lock = home.lock()?;
            same_binding(home, link)?;
            home.clear_outbox(id);
        }
        Err(e) => return Err(e.context("o resultado fica guardado e é reenviado ao reconectar")),
    }
    Ok(())
}

async fn handle(s: &mut Session, home: &Home, job: &Value, link: &HubLink) -> Result<()> {
    let id = job["jobId"].as_str().unwrap_or_default().to_owned();
    let lease = job["leaseToken"].as_str().unwrap_or_default().to_owned();
    let step = |state: &str| json!({"jobId":id,"leaseToken":lease,"state":state});
    if let Err(e) = s.call("executorUpdate", step("accepted"), Duration::from_secs(20)).await {
        println!("pedido {id} ignorado: {e:#}");
        return Ok(());
    }
    let refuse = |code: &str, message: String, seq: u64| json!({"jobId":id,"leaseToken":lease,"outcome":"refused","code":code,"message":message,"decidedBy":"none","auditSeq":seq});
    let plan = match policy::plan(job["kind"].as_str().unwrap_or_default(), &job["spec"]) {
        Ok(p) => p,
        Err(r) => {
            println!("pedido {id} RECUSADO ({}): {}", r.code, r.message);
            let seq = audit_refusal(home, link, &id, &json!({"code":r.code}))?;
            return finish(s, home, link, &id, refuse(r.code, r.message, seq)).await;
        }
    };
    let grant = home.authorized()?.desktop;
    let granted = grant.as_ref().map(|g| g.path.clone());
    let root = match policy::authorized_root(&plan.location, granted.as_deref()) {
        Ok(r) => r,
        Err(r) => {
            println!("pedido {id} RECUSADO ({}): {}", r.code, r.message);
            let seq = audit_refusal(home, link, &id, &json!({"code":r.code,"location":plan.location}))?;
            return finish(s, home, link, &id, refuse(r.code, r.message, seq)).await;
        }
    };
    if let Some(g) = &grant
        && !policy::same_dir(&root, g.dev, g.ino)
    {
        let r = policy::Refusal {
            code: "location_changed",
            message: "a pasta liberada não é a mesma que você confirmou; libere de novo neste computador".into(),
        };
        println!("pedido {id} RECUSADO ({}): {}", r.code, r.message);
        let seq = audit_refusal(home, link, &id, &json!({"code":r.code}))?;
        return finish(s, home, link, &id, refuse(r.code, r.message, seq)).await;
    }
    println!(
        "pedido {id}: criar planilha {:?} ({} abas) em {}{} — local liberado por você; nada será sobrescrito",
        plan.filename,
        plan.sheets.len(),
        root.display(),
        plan.folder.as_ref().map_or(String::new(), |f| format!("/{f}"))
    );
    // A tentativa é guardada ANTES de pedir o start: se a resposta do Hub se perder, o próximo hello ainda reconcilia.
    let preexisting = !policy::target_absent(&plan, &root);
    {
        let _lock = home.lock()?;
        same_binding(home, link)?;
        home.mark_running(&id, &lease, job["kind"].as_str().unwrap_or_default(), &job["spec"], preexisting)?;
        home.audit(
            &id,
            "intent",
            &json!({"location":plan.location,"folder":plan.folder,"filename":plan.filename,"sha256":plan.sha256}),
        )?;
    }
    match s.call("executorUpdate", step("start"), Duration::from_secs(20)).await {
        Ok(ok) if ok["go"] == true => {}
        Ok(_) => {
            println!("pedido {id} não iniciado: o Hub não liberou o start");
            abort_job(home, link, &id, "start sem liberação")?;
            return Ok(());
        }
        Err(e) if e.downcast_ref::<HubError>().is_some_and(HubError::terminal) => {
            println!("pedido {id} não iniciado: {e:#}");
            abort_job(home, link, &id, "start negado pelo Hub")?;
            return Ok(());
        }
        Err(e) => return Err(e),
    }
    // A retirada da liberação e a criação do arquivo se excluem pelo mesmo trinco: depois que `unauthorize` confirma, nada é criado.
    let lock = home.lock()?;
    // Este processo só atende o Hub do vínculo em que nasceu: um vínculo novo não herda pedidos do antigo.
    same_binding(home, link)?;
    if home.authorized()?.desktop != grant {
        let seq = home.audit(&id, "failed", &json!({"code":"location_not_authorized"}))?;
        let revoked = json!({"jobId":id,"leaseToken":lease,"outcome":"failed","code":"location_not_authorized","decidedBy":"policy","auditSeq":seq,
            "message":"a liberação deste local foi retirada ou mudou antes da gravação; nada foi gravado"});
        drop(lock);
        return finish(s, home, link, &id, revoked).await;
    }
    crash_point("before_write");
    home.mark_writing(&id)?;
    crash_point("before_create");
    let root_id = grant.as_ref().map(|g| (g.dev, g.ino));
    let state_dir = home.0.clone();
    let job_id = id.clone();
    let work = tokio::task::spawn_blocking(move || {
        let _lock = lock;
        policy::execute(&plan, &root, root_id, &|(dev, ino), folder| {
            Home(state_dir.clone()).mark_created(&job_id, dev, ino, folder).map_err(std::io::Error::other)
        })
    })
    .await?;
    crash_point("after_write");
    let result = match work {
        Ok(receipt) => {
            let seq = {
                let _lock = home.lock()?;
                same_binding(home, link)?;
                home.audit(&id, "created", &json!({"path":receipt.0["path"],"sha256":receipt.0["sha256"]}))?
            };
            println!("pedido {id}: criado e conferido em {}", receipt.0["path"].as_str().unwrap_or_default());
            json!({"jobId":id,"leaseToken":lease,"outcome":"done","decidedBy":"policy","auditSeq":seq,"receipt":receipt.0})
        }
        Err(f) => {
            // Depois de criar o arquivo, uma falha deixa algo no disco: o efeito é incerto, não "falhou sem efeito".
            let uncertain = matches!(f.code, "verify_failed" | "write_incomplete");
            let outcome = if uncertain { "unknown" } else { "failed" };
            let seq = {
                let _lock = home.lock()?;
                same_binding(home, link)?;
                home.audit(&id, outcome, &json!({"code":f.code,"detail":f.detail}))?
            };
            println!("pedido {id}: {outcome} ({}): {}", f.code, f.message);
            json!({"jobId":id,"leaseToken":lease,"outcome":outcome,"code":f.code,"message":f.message,"detail":f.detail,"decidedBy":"policy","auditSeq":seq})
        }
    };
    finish(s, home, link, &id, result).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;

    fn test_home() -> (PathBuf, Home) {
        let base = std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir());
        let dir = base.join(format!("mexerica-helper-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        (dir, home)
    }
    fn link(home: &Home, binding_id: &str) -> HubLink {
        let key = home.key(true).unwrap();
        let sign_pub = key.verifying_key().to_bytes();
        HubLink {
            url: "https://hub.example.invalid".into(),
            sign_pub: crypto::b64(&sign_pub),
            label: "Computador".into(),
            computer_id: crypto::computer_id(&sign_pub),
            binding_id: binding_id.into(),
        }
    }

    #[test]
    fn a_stale_process_cannot_act_on_a_link_with_another_binding_id() {
        let (dir, home) = test_home();
        let original = link(&home, "original");
        home.save_link(&original).unwrap();
        assert!(same_binding(&home, &original).is_ok());
        let mut reenrolled = original.clone();
        reenrolled.binding_id = "reenrolled".into();
        home.save_link(&reenrolled).unwrap();
        assert!(same_binding(&home, &original).is_err());
        assert!(same_binding(&home, &reenrolled).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_different_local_key_cannot_reuse_a_link_from_another_identity() {
        let (dir, home) = test_home();
        let original = link(&home, "binding");
        home.save_link(&original).unwrap();
        assert!(same_binding(&home, &original).is_ok());
        std::fs::write(home.0.join("identity.key"), crypto::b64(&[42u8; 32])).unwrap();
        let refused = same_binding(&home, &original).unwrap_err();
        assert!(refused.to_string().contains("a chave local não corresponde ao vínculo"), "{refused:#}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn auth_epoch_rotation_leaves_the_binding_epoch_used_by_revoke_alone() {
        let (dir, home) = test_home();
        let binding = home.binding_epoch().unwrap();
        let bound = link(&home, "binding");
        home.save_link(&bound).unwrap();
        let auth = home.auth_epoch().unwrap();
        home.bump_auth_epoch().unwrap();
        assert_ne!(home.auth_epoch().unwrap(), auth);
        assert_eq!(home.binding_epoch().unwrap(), binding);
        assert!(auth_state_unchanged(&home, Some(&bound), &auth, "mudou").is_err());
        assert!(binding_state_unchanged(&home, Some(&bound), &binding, "mudou").is_ok());
        home.bump_auth_epoch().unwrap();
        assert!(binding_state_unchanged(&home, Some(&bound), &binding, "mudou").is_ok());
        let after_revoke = home.bump_binding_epoch().unwrap();
        assert_ne!(after_revoke, binding);
        home.bump_auth_epoch().unwrap();
        assert!(binding_state_unchanged(&home, Some(&bound), &after_revoke, "mudou").is_ok());
        assert!(binding_state_unchanged(&home, Some(&bound), &binding, "mudou").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn revoke_refuses_while_an_enroll_is_in_progress_without_erasing_the_identity() {
        let (dir, home) = test_home();
        let bound = link(&home, "binding");
        home.save_link(&bound).unwrap();
        let binding = home.binding_epoch().unwrap();
        let auth = home.auth_epoch().unwrap();
        let enrolling = home.enroll_lock().unwrap();
        let refused = revoke(&home, true).await.unwrap_err();
        assert!(refused.to_string().contains("já existe um cadastro em andamento"), "{refused:#}");
        assert_eq!(home.link().unwrap(), bound);
        assert_eq!(home.binding_epoch().unwrap(), binding);
        assert_eq!(home.auth_epoch().unwrap(), auth);
        assert!(home.0.join("identity.key").is_file());
        assert!(home.0.join("hub.json").is_file());
        drop(enrolling);
        main(Action::Revoke { local_only: true, home: Some(dir.clone()) }).await.unwrap();
        assert!(home.link().is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn unauthorize_rotates_only_the_auth_epoch_and_invalidates_a_state_captured_before_it() {
        let (dir, home) = test_home();
        home.save_authorized(&Authorized {
            desktop: Some(Grant { path: "caminho-de-teste".into(), at: crate::store::now_ms(), dev: 1, ino: 2 }),
        })
        .unwrap();
        let before = home.auth_epoch().unwrap();
        let binding = home.binding_epoch().unwrap();
        main(Action::Unauthorize { location: "desktop".into(), home: Some(dir.clone()) }).await.unwrap();
        assert_eq!(home.authorized().unwrap().desktop, None);
        assert_ne!(home.auth_epoch().unwrap(), before);
        assert_eq!(home.binding_epoch().unwrap(), binding);
        assert!(auth_state_unchanged(&home, None, &before, "mudou").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
