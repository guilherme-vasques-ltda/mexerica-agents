//! Entrar com `OpenRouter` (OAuth PKCE, com o retorno no ouvinte do próprio app).
//!
//! O app pede a entrada e abre a URL que o Hub monta: o verificador do PKCE e o `state` nascem
//! aqui, e o navegador volta ao ouvinte local do app (`conectores::oauth_desktop`), que repassa
//! `{state, code}` (ou `{state, erro}`) pelo canal autenticado. A tentativa vive só em memória,
//! uma por vez, vale dez minutos e se consome no primeiro retorno com o `state` certo — antes de
//! qualquer rede. A troca do código não segue redirecionamento, espera pouco e lê um corpo com
//! teto; só o campo `key` interessa. A chave segue ao cofre pelo mesmo caminho da chave colada
//! (`chaves::salvar`): testada com a listagem de modelos e gravada só se o fornecedor aceitar.
//! A chave nunca volta ao app, a log, a registro nem a mensagem de erro; nenhum erro repete o
//! `state`, o código nem a chave recebidos. Só este computador, e só com o motor ligado.

use super::chaves;
use crate::LockExt;
use crate::api::devices::Caller;
use crate::hub::Hub;
use crate::remote::crypto;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// Endereço fixo do fornecedor: em produção nada o lê do ambiente.
pub(super) const BASE: &str = "https://openrouter.ai";
/// Só para provas: troca o endereço por um servidor local de mentira; sem injeção a troca falha
/// fechada, nunca sai à rede real.
#[cfg(test)]
pub(super) static BASE_EM_TESTE: Mutex<Option<String>> = Mutex::new(None);

/// Uma tentativa por vez, dez minutos.
pub(super) const VALIDADE: Duration = Duration::from_secs(10 * 60);
/// Prazo curto da troca do código.
const PRAZO: Duration = Duration::from_secs(15);
/// Teto do corpo da resposta da troca.
const MAX_CORPO: usize = 64 * 1024;
/// Recusa fixa do `state`: nunca ecoa o valor recebido.
const ENTRADA_INVALIDA: &str = "entrada_invalida: comece o login de novo neste computador.";

/// Tentativa em memória: o `state` e o verificador nunca saem do Hub; a porta é a do ouvinte do
/// app que abriu o login.
pub(super) struct Tentativa {
    pub(super) state: [u8; 32],
    pub(super) verifier: Zeroizing<String>,
    pub(super) porta: u16,
    pub(super) criada_em: Instant,
}

pub(super) static TENTATIVA: Mutex<Option<Tentativa>> = Mutex::new(None);

/// Falha da troca com código estável para o app (os mesmos do «Testar acesso»); nunca carrega
/// corpo, código nem chave.
enum Falha {
    SemResposta,
    Fornecedor(u16),
    RespostaInvalida,
    ChaveInvalida,
}

/// Métodos do app. Só este computador, e só com o motor ligado.
pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::fork::engine_setup::require_local(caller)?;
    ensure!(super::enabled(), "motor_desligado: o Motor Mexerica está desligado neste computador.");
    match method {
        "openrouterEntrar" => entrar(&body),
        "openrouterEntrarConcluir" => concluir(hub, &body).await,
        _ => bail!("{method} não é um método de entrada com OpenRouter"),
    }
}

/// A chave trocada é a do dono, só usada pelos bots do Space pessoal dele: o login também só
/// acontece com o Space pessoal aberto (o app manda o Space aberto em todo pedido).
fn assegurar_space_pessoal(body: &Value) -> Result<()> {
    // Sem o Space aberto não presume nada: recusa fechado em vez de cair no Space pessoal.
    let Some(space) = body.get("spaceId").and_then(Value::as_str) else {
        // A recusa é registrada sem o corpo nem nenhum dado do pedido, como toda negação aqui.
        tracing::warn!("login com OpenRouter recusado: o pedido chegou sem o Space aberto");
        bail!("space_invalido: o login com OpenRouter vale só no Space pessoal deste computador.");
    };
    ensure!(
        space == crate::fork::spaces::PERSONAL,
        "space_invalido: o login com OpenRouter vale só no Space pessoal deste computador."
    );
    Ok(())
}

/// Passo 1: exige a porta do ouvinte do app, gera o PKCE e o `state`, guarda a tentativa (a nova
/// substitui qualquer pendente) e devolve só a URL de autorização.
pub(super) fn entrar(body: &Value) -> Result<Value> {
    assegurar_space_pessoal(body)?;
    let Ok(Some(porta)) = crate::fork::conectores::oauth_desktop::do_pedido(body) else {
        bail!("porta_invalida: o retorno deste login cai no ouvinte do app; informe uma das portas dele.")
    };
    let verifier = crypto::b64(&crypto::random::<32>());
    let state = crypto::random::<32>();
    *TENTATIVA.locked() =
        Some(Tentativa { state, verifier: Zeroizing::new(verifier.clone()), porta, criada_em: Instant::now() });
    Ok(json!({"url": url_autorizacao(porta, &verifier, &crypto::b64(&state))?}))
}

/// URL de autorização com os parâmetros codificados; o desafio é o S256 do verificador.
pub(super) fn url_autorizacao(porta: u16, verifier: &str, state: &str) -> Result<String> {
    let desafio = crypto::b64(&crypto::sha256(&[verifier.as_bytes()]));
    let mut url = reqwest::Url::parse(&format!("{}/auth", base_do_login()))
        .context("endereço do login do fornecedor inválido")?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("callback_url", &format!("http://127.0.0.1:{porta}/oauth/callback"))
            .append_pair("code_challenge", &desafio)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("key_label", "Mexerica");
    }
    Ok(url.as_str().to_owned())
}

/// Base do login: fixa em produção; em teste, a injetada ou a fixa.
fn base_do_login() -> String {
    #[cfg(test)]
    if let Some(base) = BASE_EM_TESTE.locked().clone() {
        return base;
    }
    BASE.to_owned()
}

/// Passo 2: confere o `state` em tempo constante, consome a tentativa antes de qualquer rede
/// (uso único, também no erro) e troca o código pela chave, que segue ao cofre pelo caminho da
/// chave colada. Um `state` que não confere não derruba o login em andamento.
pub(super) async fn concluir(hub: &Arc<Hub>, body: &Value) -> Result<Value> {
    assegurar_space_pessoal(body)?;
    let Some(Ok(state)) = body.get("state").and_then(Value::as_str).map(crypto::unb64_n::<32>) else {
        bail!(ENTRADA_INVALIDA)
    };
    let tentativa = {
        let mut guard = TENTATIVA.locked();
        let confere =
            guard.as_ref().is_some_and(|t| t.criada_em.elapsed() < VALIDADE && crypto::ct_eq(&t.state, &state));
        if confere { guard.take() } else { None }
    };
    let Some(tentativa) = tentativa else {
        // A recusa é registrada sem o `state`, o código nem a porta: uma fila delas denuncia retorno forjado.
        tracing::warn!("retorno do login com OpenRouter recusado: nenhuma tentativa viva confere com o retorno");
        bail!(ENTRADA_INVALIDA)
    };
    tracing::debug!(porta = tentativa.porta, "retorno do login com OpenRouter reconhecido");
    // O fornecedor disse não: recusa fixa, sem ecoar o motivo recebido.
    if body.get("erro").and_then(Value::as_str).is_some() {
        return Ok(json!({"ok": false, "erro": "entrada_negada"}));
    }
    let Some(code) = body.get("code").and_then(Value::as_str).filter(|code| !code.is_empty()) else {
        bail!("entrada_negada: o retorno do login não trouxe o código de acesso.");
    };
    let acesso = match trocar(&tentativa.verifier, code).await {
        Ok(candidata) => {
            crate::market::vault::unlock(hub.clone()).await?;
            let (fornecedor, extra) = alvo_da_gravacao();
            chaves::salvar(hub, fornecedor, candidata, extra).await?
        }
        Err(Falha::SemResposta) => json!({"ok": false, "erro": "sem_resposta"}),
        Err(Falha::Fornecedor(status)) => json!({"ok": false, "erro": "fornecedor_falhou", "status": status}),
        Err(Falha::RespostaInvalida) => json!({"ok": false, "erro": "resposta_invalida"}),
        Err(Falha::ChaveInvalida) => json!({"ok": false, "erro": "chave_invalida"}),
    };
    Ok(acesso)
}

/// Troca o código pela chave: POST único, sem redirecionamento, prazo curto, corpo com teto; lê
/// só o campo `key` e o valida como a chave colada. Nenhum erro repete corpo, código nem chave.
async fn trocar(verifier: &Zeroizing<String>, code: &str) -> Result<Zeroizing<String>, Falha> {
    #[cfg(test)]
    let destino = match BASE_EM_TESTE.locked().clone() {
        Some(base) => format!("{base}/api/v1/auth/keys"),
        // Sem injeção a troca falha fechada: a prova nunca sai à rede real.
        None => return Err(Falha::SemResposta),
    };
    #[cfg(not(test))]
    let destino = format!("{BASE}/api/v1/auth/keys");
    let cliente = crate::fork::conectores::oauth_urls::cliente().map_err(|_| Falha::SemResposta)?;
    let pedido = json!({"code": code, "code_verifier": verifier.to_string(), "code_challenge_method": "S256"});
    let mut resposta = cliente
        .post(&destino)
        .header("accept", "application/json")
        .json(&pedido)
        .timeout(PRAZO)
        .send()
        .await
        .map_err(|_| Falha::SemResposta)?;
    // 3xx é falha: o `Location` nunca é seguido.
    if !resposta.status().is_success() {
        return Err(Falha::Fornecedor(resposta.status().as_u16()));
    }
    let mut corpo = Vec::new();
    while let Some(parte) = resposta.chunk().await.map_err(|_| Falha::SemResposta)? {
        if corpo.len() + parte.len() > MAX_CORPO {
            return Err(Falha::RespostaInvalida);
        }
        corpo.extend_from_slice(&parte);
    }
    let valor: Value = serde_json::from_slice(&corpo).map_err(|_| Falha::RespostaInvalida)?;
    let Some(chave) = valor["key"].as_str().map(str::trim).filter(|chave| !chave.is_empty()) else {
        return Err(Falha::RespostaInvalida);
    };
    chaves::chave(Some(chave)).map_err(|_| Falha::ChaveInvalida)
}

/// Fornecedor e ambiente da gravação final: `openrouter` em produção; só a prova troca pelo falso
/// do motor, para percorrer o mesmo caminho sem rede.
fn alvo_da_gravacao() -> (&'static str, &'static [(&'static str, &'static str)]) {
    #[cfg(test)]
    if GRAVACAO_FAUX.load(std::sync::atomic::Ordering::Relaxed) {
        return ("faux", &[(super::FAUX_ENV, "1")]);
    }
    ("openrouter", &[])
}

/// Só para provas: a gravação final usa o falso do motor (listagem sem rede).
#[cfg(test)]
pub(super) static GRAVACAO_FAUX: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
