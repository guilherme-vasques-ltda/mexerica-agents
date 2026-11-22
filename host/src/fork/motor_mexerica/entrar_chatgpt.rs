//! Entrar com `ChatGPT` (OAuth PKCE oficial do emissor da conta, com o retorno no ouvinte do app).
//!
//! O app pede a entrada e abre a URL que o Hub monta: o verificador do PKCE, o `state` e o
//! `nonce` nascem aqui, e o navegador volta ao ouvinte local do app (`conectores::oauth_desktop`),
//! que repassa `{state, code}` (ou `{state, erro}`) pelo canal autenticado, com o `client_id`
//! emitido quando é um registro novo. Os endereços do emissor são fixos; o primeiro registro usa
//! o cliente dinâmico e os logins seguintes o `client_id` guardado, com a dica (`login_hint`, o
//! rótulo da conta) só na URL de autorização — nunca em log nem em erro. O ID token é conferido
//! fail-closed (`entrar_chatgpt_jwt`): assinatura RS256 contra o JWKS, emissor exato, público
//! igual ao `client_id` emitido, prazo e `nonce`; o `sub` validado é a identidade da conta. O
//! registro (com os tokens) vai cifrado ao cofre pelo mesmo caminho das outras credenciais; o
//! `ext_agent_host_id` é opaco e fica em kv simples. O ID token nunca fica guardado: conferido e
//! descartado. Nenhum método devolve token, e nenhum erro repete o código, o verificador nem o
//! `state`. A renovação é serializada e
//! grava o refresh novo antes de devolver o access; o «Sair» revoga e apaga os tokens de qualquer
//! jeito. Só este computador, e só com o motor ligado.

use crate::LockExt;
use crate::api::devices::Caller;
use crate::hub::Hub;
use crate::remote::crypto;
use crate::store::Store;
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// Endereço fixo do emissor: em produção nada o lê do ambiente.
pub(super) const BASE: &str = "https://auth.openai.com";
/// Só para provas: troca o endereço por um servidor local de mentira; sem injeção a rede falha
/// fechada, nunca sai à rede real.
#[cfg(test)]
pub(super) static BASE_EM_TESTE: Mutex<Option<String>> = Mutex::new(None);

/// Caminhos fixos do emissor: autorização, troca, revogação e lista de chaves.
const AUTORIZACAO: &str = "/api/accounts/authorize";
const TOKEN: &str = "/api/accounts/oauth/token";
const REVOGACAO: &str = "/api/accounts/oauth/revoke";
/// Cliente do primeiro registro; o emitido (prefixo abaixo) é que fica guardado.
const CLIENTE_DINAMICO: &str = "dynamic_agent_client";
const PREFIXO_CLIENTE: &str = "oaiapp_";
/// Nome do app, só no primeiro registro.
const AGENTE: &str = "Mexerica";
/// Escopo que habilita o uso do plano da conta.
pub(super) const ESCOPO_DIRETO: &str = "chatgpt.tokens.use.direct";
/// Escopo pedido inteiro, sempre o mesmo.
const ESCOPO: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const RECURSO: &str = "https://api.openai.com/v1";

/// Registro cifrado no cofre (nunca em arquivo de configuração, log, erro nem resposta).
pub(super) const SLOT_CONTA: &str = "motor-mexerica/chatgpt/conta";
/// Identificador opaco e estável deste Hub: não é segredo, fica em kv simples.
pub(super) const KV_HOST_ID: &str = "motor-mexerica/chatgpt/host-id";

/// Uma tentativa por vez, dez minutos.
pub(super) const VALIDADE: Duration = Duration::from_secs(10 * 60);
/// Prazo curto de cada pedido ao emissor.
const PRAZO: Duration = Duration::from_secs(15);
/// Teto do corpo de cada resposta do emissor.
const MAX_CORPO: usize = 64 * 1024;
/// Um ano em segundos: teto de qualquer validade declarada pelo emissor.
const ANO_EM_SEGUNDOS: i64 = 31_536_000;
/// Renova o access quando faltam menos de cinco minutos para expirar.
#[cfg(test)]
const MARGEM_RENOVACAO: i64 = 5 * 60;
#[cfg(test)]
pub(super) static TENTATIVAS_RENOVACAO: AtomicU64 = AtomicU64::new(0);
/// Recusa fixa do `state`: nunca ecoa o valor recebido.
const ENTRADA_INVALIDA: &str = "entrada_invalida: comece o login de novo neste computador.";
/// Erros do emissor que invalidam o refresh: apagam os tokens, a sessão precisa entrar de novo.
#[cfg(test)]
const INVALIDANTES: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

/// Tentativa em memória: o `state`, o `nonce` e o verificador nunca saem do Hub; a porta é a do
/// ouvinte do app que abriu o login. `reautorizacao` é `None` no primeiro registro e guarda o
/// `client_id` (com o `sub` esperado e as dicas) nos logins de uma conta já registrada.
pub(super) struct Tentativa {
    pub(super) id: u64,
    pub(super) state: [u8; 32],
    pub(super) nonce: String,
    pub(super) verifier: Zeroizing<String>,
    pub(super) porta: u16,
    geracao: u64,
    reautorizacao: Option<Reautorizacao>,
    pub(super) criada_em: Instant,
}

/// Conta já registrada: o que os logins seguintes reaproveitam e conferem.
#[derive(Clone)]
struct Reautorizacao {
    client_id: String,
    sub: String,
    login_hint: Option<String>,
}

/// Registro da conta: identidade e `client_id` ficam; os tokens só enquanto a sessão existe.
#[derive(Clone, Serialize, Deserialize)]
struct Conta {
    client_id: String,
    sub: String,
    #[serde(default)]
    rotulo: Option<String>,
    /// Escopos concedidos na última troca, separados por espaço.
    #[serde(default)]
    escopos: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Segundos de época da validade do access token.
    #[serde(default)]
    expira_em: Option<i64>,
}

impl Conta {
    fn conectada(&self) -> bool {
        self.access_token.is_some() && self.refresh_token.is_some()
    }

    fn plano_habilitado(&self) -> bool {
        self.conectada() && self.escopos.as_deref().is_some_and(tem_escopo_direto)
    }
}

/// Uma tentativa por vez.
pub(super) static TENTATIVA: Mutex<Option<Tentativa>> = Mutex::new(None);
/// Geração da sessão: o «Sair» a incrementa e uma entrada que começou antes dele não grava depois.
pub(super) static SESSAO: AtomicU64 = AtomicU64::new(0);
/// Identificador da tentativa atual: um segundo login troca e a conclusão do primeiro não grava mais nada.
pub(super) static ID_TENTATIVA: AtomicU64 = AtomicU64::new(0);
/// Série do identificador do Hub: dois primeiros logins nunca geram identificadores diferentes.
static SERIE_HOST_ID: Mutex<()> = Mutex::new(());
/// Renovação serializada: duas renovações simultâneas nunca usam o mesmo refresh.
static RENOVACAO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Falha da troca com código estável para o app; nunca carrega corpo, código nem token.
enum Falha {
    SemResposta,
    Fornecedor(u16),
    RespostaInvalida,
}

/// Resposta da troca do código: só o que o registro precisa.
struct Troca {
    access_token: String,
    refresh_token: String,
    id_token: String,
    expires_in: i64,
    escopos: String,
}

/// Métodos do app. Só este computador, e só com o motor ligado.
pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::fork::engine_setup::require_local(caller)?;
    ensure!(super::enabled(), "motor_desligado: o Motor Mexerica está desligado neste computador.");
    match method {
        "chatgptEntrar" => entrar(hub, &body).await,
        "chatgptEntrarConcluir" => concluir(hub, &body).await,
        "chatgptEstado" => estado(hub),
        "chatgptSair" => sair(hub).await,
        _ => bail!("{method} não é um método de entrada com `ChatGPT`"),
    }
}

/// Passo 1: exige a porta do ouvinte do app, gera o PKCE, o `state` e o `nonce`, guarda a
/// tentativa (a nova substitui qualquer pendente) e devolve só a URL de autorização. A conta
/// guardada decide registro novo (cliente dinâmico) ou reautorização (cliente emitido, com a
/// dica do rótulo); «Entrar com outra conta» ignora a guardada e registra de novo.
pub(super) async fn entrar(hub: &Arc<Hub>, body: &Value) -> Result<Value> {
    assegurar_space_pessoal(body)?;
    let Ok(Some(porta)) = crate::fork::conectores::oauth_desktop::do_pedido(body) else {
        bail!("porta_invalida: o retorno deste login cai no ouvinte do app; informe uma das portas dele.")
    };
    let consentir = body.get("consentir") == Some(&Value::Bool(true));
    // «Entrar com outra conta»: ignora o registro guardado e faz o registro dinâmico de novo.
    let nova_conta = body.get("novaConta") == Some(&Value::Bool(true));
    // A geração é lida antes do cofre: um «Sair» durante a espera não publica tentativa com a geração nova.
    let geracao = SESSAO.load(Ordering::SeqCst);
    let conta = ler_conta_destravada(hub).await?;
    let reautorizacao = if nova_conta { None } else { conta.as_ref().and_then(reautorizacao_da_conta) };
    let host_id = host_id(&hub.store)?;
    let verifier = crypto::b64(&crypto::random::<32>());
    let nonce = crypto::b64(&crypto::random::<32>());
    let state = crypto::random::<32>();
    {
        // Identificador e tentativa nascem juntos sob a mesma guarda, com a geração reconferida: o
        // «Sair» de dentro da espera não passa e dois logins nunca publicam fora de ordem.
        let mut guard = TENTATIVA.locked();
        let id = ID_TENTATIVA.fetch_add(1, Ordering::SeqCst) + 1;
        if SESSAO.load(Ordering::SeqCst) != geracao {
            bail!(
                "sessao_encerrada: o «Sair» aconteceu enquanto o login era preparado; comece de novo neste computador."
            );
        }
        *guard = Some(Tentativa {
            id,
            state,
            nonce: nonce.clone(),
            verifier: Zeroizing::new(verifier.clone()),
            porta,
            geracao,
            reautorizacao: reautorizacao.clone(),
            criada_em: Instant::now(),
        });
    }
    let dicas = reautorizacao.as_ref().map(|r| (r.client_id.as_str(), r.login_hint.as_deref()));
    Ok(json!({"url": url_autorizacao(porta, &verifier, &crypto::b64(&state), &nonce, &host_id, dicas, consentir)?}))
}

/// A sessão é do dono, só usada pelos bots do Space pessoal dele: o login também só acontece com
/// o Space pessoal aberto (o app manda o Space aberto em todo pedido); sem o campo, recusa fechado.
fn assegurar_space_pessoal(body: &Value) -> Result<()> {
    let Some(space) = body.get("spaceId").and_then(Value::as_str) else {
        tracing::warn!("login com `ChatGPT` recusado: o pedido chegou sem o Space aberto");
        bail!("space_invalido: o login com `ChatGPT` vale só no Space pessoal deste computador.");
    };
    ensure!(
        space == crate::fork::spaces::PERSONAL,
        "space_invalido: o login com `ChatGPT` vale só no Space pessoal deste computador."
    );
    Ok(())
}

/// Conta já registrada para reautorização: só com `client_id` emitido; as dicas são as guardadas.
fn reautorizacao_da_conta(conta: &Conta) -> Option<Reautorizacao> {
    conta.client_id.starts_with(PREFIXO_CLIENTE).then(|| Reautorizacao {
        client_id: conta.client_id.clone(),
        sub: conta.sub.clone(),
        login_hint: conta.rotulo.clone(),
    })
}

/// URL de autorização com os parâmetros codificados; o desafio é o S256 do verificador. O retorno
/// é sempre em `127.0.0.1` com o caminho exato `/auth/callback`; nunca `localhost`.
pub(super) fn url_autorizacao(
    porta: u16,
    verifier: &str,
    state: &str,
    nonce: &str,
    host_id: &str,
    reautorizacao: Option<(&str, Option<&str>)>,
    consentir: bool,
) -> Result<String> {
    let desafio = crypto::b64(&crypto::sha256(&[verifier.as_bytes()]));
    let mut url = reqwest::Url::parse(&format!("{}{AUTORIZACAO}", base_do_login()))
        .map_err(|_| anyhow!("endereço do login do emissor inválido"))?;
    {
        let mut q = url.query_pairs_mut();
        match reautorizacao {
            None => {
                q.append_pair("client_id", CLIENTE_DINAMICO);
                q.append_pair("agent_name_hint", AGENTE);
            }
            Some((client_id, _)) => {
                q.append_pair("client_id", client_id);
            }
        }
        q.append_pair("ext_agent_host_id", host_id);
        q.append_pair("response_type", "code");
        q.append_pair("redirect_uri", &retorno(porta));
        q.append_pair("scope", ESCOPO);
        q.append_pair("resource", RECURSO);
        q.append_pair("state", state);
        q.append_pair("nonce", nonce);
        q.append_pair("code_challenge", &desafio);
        q.append_pair("code_challenge_method", "S256");
        if let Some((_, Some(login_hint))) = reautorizacao {
            q.append_pair("login_hint", login_hint);
        }
        if consentir {
            q.append_pair("prompt", "consent");
        }
    }
    Ok(url.as_str().to_owned())
}

/// Retorno do navegador: caminho exato `/auth/callback`, sempre `127.0.0.1`, nunca `localhost`.
fn retorno(porta: u16) -> String {
    format!("http://127.0.0.1:{porta}/auth/callback")
}

/// Base do emissor: fixa em produção; em teste, a injetada ou a fixa.
fn base_do_login() -> String {
    #[cfg(test)]
    if let Some(base) = BASE_EM_TESTE.locked().clone() {
        return base;
    }
    BASE.to_owned()
}

/// Passo 2: confere o `state` em tempo constante, consome a tentativa antes de qualquer rede (uso
/// único, também no erro), decide o `client_id` da troca e valida o ID token antes de gravar o
/// registro cifrado. Um `state` que não confere não derruba o login em andamento.
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
        tracing::warn!("retorno do login com `ChatGPT` recusado: nenhuma tentativa viva confere com o retorno");
        bail!(ENTRADA_INVALIDA)
    };
    tracing::debug!(porta = tentativa.porta, "retorno do login com `ChatGPT` reconhecido");
    // O emissor disse não: recusa fixa, sem ecoar o motivo recebido. Só «access_denied» é a pessoa
    // desistindo; qualquer outro erro do fornecedor tem código próprio, sem o texto cru na tela.
    if let Some(erro) = body.get("erro").and_then(Value::as_str) {
        let codigo = if erro == "access_denied" { "entrada_negada" } else { "fornecedor_negou" };
        // Recusa registrada sem ecoar o motivo recebido: só o código fixo, sem `state`, `code` nem porta.
        tracing::warn!(codigo, "retorno do login com `ChatGPT` recusado: o emissor não concedeu a entrada");
        return Ok(json!({"ok": false, "erro": codigo}));
    }
    let Some(code) = body.get("code").and_then(Value::as_str).filter(|code| !code.is_empty()) else {
        bail!("entrada_negada: o retorno do login não trouxe o código de acesso.");
    };
    let cliente_do_retorno = body.get("client_id").and_then(Value::as_str);
    let client_id = match &tentativa.reautorizacao {
        // Registro novo: sem `client_id` emitido (ou sem o prefixo dele), o registro está incompleto.
        None => {
            let Some(id) = cliente_do_retorno.filter(|id| id.starts_with(PREFIXO_CLIENTE)) else {
                return Ok(json!({"ok": false, "erro": "registro_incompleto"}));
            };
            id.to_owned()
        }
        // Reautorização: o retorno pode omitir; diferente do guardado, recusa sem trocar nada.
        Some(reautorizacao) => {
            if cliente_do_retorno.is_some_and(|id| id != reautorizacao.client_id) {
                return Ok(json!({"ok": false, "erro": "cliente_trocado"}));
            }
            reautorizacao.client_id.clone()
        }
    };
    match trocar(&tentativa, &client_id, code).await {
        Ok(troca) => gravar(hub, &tentativa, &client_id, troca).await,
        Err(Falha::SemResposta) => Ok(json!({"ok": false, "erro": "sem_resposta"})),
        Err(Falha::Fornecedor(status)) => Ok(json!({"ok": false, "erro": "fornecedor_falhou", "status": status})),
        Err(Falha::RespostaInvalida) => Ok(json!({"ok": false, "erro": "resposta_invalida"})),
    }
}

/// Confere o ID token, recusa conta trocada na reautorização e grava o registro cifrado; a
/// resposta ao app não leva token algum.
async fn gravar(hub: &Arc<Hub>, tentativa: &Tentativa, client_id: &str, troca: Troca) -> Result<Value> {
    let identidade = match super::entrar_chatgpt_jwt::validar(
        &base_do_login(),
        &troca.id_token,
        &super::entrar_chatgpt_jwt::Esperado { client_id, nonce: &tentativa.nonce },
    )
    .await
    {
        Ok(identidade) => identidade,
        Err(erro) => return Ok(falha_do_id(&erro)),
    };
    // Reautorização: o `sub` validado tem de ser o da conta guardada, senão recusa sem trocar
    // credenciais (nada foi gravado ainda).
    if let Some(reautorizacao) = &tentativa.reautorizacao
        && !crypto::ct_eq(identidade.sub.as_bytes(), reautorizacao.sub.as_bytes())
    {
        return Ok(json!({"ok": false, "erro": "conta_diferente"}));
    }
    let conta = Conta {
        client_id: client_id.to_owned(),
        sub: identidade.sub,
        rotulo: identidade.email,
        escopos: Some(troca.escopos),
        access_token: Some(troca.access_token),
        refresh_token: Some(troca.refresh_token),
        expira_em: Some(crate::store::now_ms() / 1000 + troca.expires_in),
    };
    let resposta = json!({
        "ok": true,
        "conta": {"rotulo": conta.rotulo.clone(), "planoHabilitado": conta.plano_habilitado()}
    });
    let _serie = RENOVACAO.lock().await;
    // O «Sair» derruba a geração; um segundo login troca o identificador da tentativa. Um dos dois
    // não confere: quem chegou tarde não grava por cima de ninguém.
    if SESSAO.load(Ordering::SeqCst) != tentativa.geracao || ID_TENTATIVA.load(Ordering::SeqCst) != tentativa.id {
        return Ok(json!({"ok": false, "erro": "sessao_encerrada"}));
    }
    // A conta que está sendo substituída tem a sessão encerrada no emissor ANTES de o registro ser
    // sobrescrito: nenhuma concessão fica órfã e ativa. Sem confirmação, o aviso vai à tela (o
    // registro novo é gravado do mesmo jeito).
    let anterior = ler_conta_destravada(hub).await?;
    let anterior_revogado = match &anterior {
        Some(anterior) if anterior.conectada() => match &anterior.refresh_token {
            Some(refresh) => revogar(&anterior.client_id, refresh).await,
            None => true,
        },
        _ => true,
    };
    crate::market::vault::unlock(hub.clone()).await?;
    gravar_conta(&hub.store, &conta)?;
    let mut resposta = resposta;
    if !anterior_revogado {
        resposta["avisoAnteriorSemConfirmacao"] = Value::Bool(true);
    }
    Ok(resposta)
}

/// Erro da validação do ID token em código estável para o app; o da rede mantém o seu.
fn falha_do_id(erro: &anyhow::Error) -> Value {
    for codigo in ["sem_resposta", "fornecedor_falhou", "resposta_invalida"] {
        if erro.to_string().starts_with(codigo) {
            return json!({"ok": false, "erro": codigo});
        }
    }
    json!({"ok": false, "erro": "id_invalido"})
}

/// Troca o código: POST form-encoded, sem segredo de cliente, sem redirecionamento, prazo curto,
/// corpo com teto. Nenhum erro repete corpo, código, verificador nem token.
async fn trocar(tentativa: &Tentativa, client_id: &str, code: &str) -> Result<Troca, Falha> {
    #[cfg(test)]
    let destino = match BASE_EM_TESTE.locked().clone() {
        Some(base) => format!("{base}{TOKEN}"),
        // Sem injeção a troca falha fechada: a prova nunca sai à rede real.
        None => return Err(Falha::SemResposta),
    };
    #[cfg(not(test))]
    let destino = format!("{BASE}{TOKEN}");
    let cliente = crate::fork::conectores::oauth_urls::cliente().map_err(|_| Falha::SemResposta)?;
    let pedido = [
        ("grant_type", "authorization_code".to_owned()),
        ("client_id", client_id.to_owned()),
        ("code", code.to_owned()),
        ("code_verifier", tentativa.verifier.to_string()),
        ("redirect_uri", retorno(tentativa.porta)),
        ("resource", RECURSO.to_owned()),
    ];
    let mut resposta =
        cliente.post(&destino).form(&pedido).timeout(PRAZO).send().await.map_err(|_| Falha::SemResposta)?;
    // 3xx é falha: o `Location` nunca é seguido.
    if !resposta.status().is_success() {
        return Err(Falha::Fornecedor(resposta.status().as_u16()));
    }
    let corpo = ler_corpo(&mut resposta).await.map_err(|falha| match falha {
        FalhaCorpo::SemResposta => Falha::SemResposta,
        FalhaCorpo::AcimaDoTeto => Falha::RespostaInvalida,
    })?;
    let valor: Value = serde_json::from_slice(&corpo).map_err(|_| Falha::RespostaInvalida)?;
    let Some(escopos) = valor["scope"].as_str().map(str::trim).filter(|escopos| !escopos.is_empty()) else {
        return Err(Falha::RespostaInvalida);
    };
    Ok(Troca {
        access_token: campo(&valor, "access_token")?,
        refresh_token: campo(&valor, "refresh_token")?,
        id_token: campo(&valor, "id_token")?,
        // Validade declarada pelo emissor: recusa ausente, não positiva ou maior que um ano (prazo
        // que não faria sentido e estouraria a soma).
        expires_in: valor["expires_in"]
            .as_i64()
            .filter(|segundos| *segundos > 0 && *segundos <= ANO_EM_SEGUNDOS)
            .ok_or(Falha::RespostaInvalida)?,
        escopos: escopos.to_owned(),
    })
}

/// Campo de texto obrigatório da resposta da troca.
fn campo(valor: &Value, nome: &str) -> Result<String, Falha> {
    valor[nome]
        .as_str()
        .map(str::trim)
        .filter(|valor| !valor.is_empty())
        .map(str::to_owned)
        .ok_or(Falha::RespostaInvalida)
}

/// Falha da leitura do corpo da resposta.
enum FalhaCorpo {
    SemResposta,
    AcimaDoTeto,
}

/// Lê o corpo da resposta com o teto; sem redirecionamento e sem corpo maior que o esperado.
async fn ler_corpo(resposta: &mut reqwest::Response) -> Result<Vec<u8>, FalhaCorpo> {
    let mut corpo = Vec::new();
    while let Some(parte) = resposta.chunk().await.map_err(|_| FalhaCorpo::SemResposta)? {
        if corpo.len() + parte.len() > MAX_CORPO {
            return Err(FalhaCorpo::AcimaDoTeto);
        }
        corpo.extend_from_slice(&parte);
    }
    Ok(corpo)
}

/// Estado passivo para a tela: não desbloqueia o cofre nem renova credenciais.
pub(super) fn estado(hub: &Arc<Hub>) -> Result<Value> {
    let conta = match ler_conta(&hub.store) {
        Ok(conta) => conta,
        Err(erro) if crate::market::vault::indisponivel(&erro) => None,
        Err(erro) => return Err(erro),
    };
    Ok(resposta_da_conta(conta.as_ref()))
}

/// Devolve o access token válido, renovando se faltar menos de cinco minutos; `None` sem sessão
/// conectada (registro ausente, tokens apagados ou refresh invalidado nesta chamada). Erro é rede,
/// 5xx ou `invalid_client`: nada foi apagado. Para o motor usar a conta nas chamadas de uso.
#[cfg(test)]
pub(crate) async fn token_fresco(hub: &Arc<Hub>) -> Result<Option<Zeroizing<String>>> {
    let Some(conta) = ler_conta_destravada(hub).await? else { return Ok(None) };
    let agora = crate::store::now_ms() / 1000;
    if conta.expira_em.is_some_and(|expira_em| expira_em > agora + MARGEM_RENOVACAO) {
        return Ok(conta.access_token.map(Zeroizing::new));
    }
    renovar(hub).await
}

/// Renovação serializada: releitura sob a guarda (outra renovação já gravou tokens novos, ou o
/// «Sair» apagou a sessão), uma troca só, e o refresh novo fica gravado ANTES do access voltar.
/// Erros que invalidam o refresh apagam os tokens e marcam «precisa entrar de novo»; `invalid_client`
/// e rede/5xx são erro explícito sem apagar nada.
#[cfg(test)]
async fn renovar(hub: &Arc<Hub>) -> Result<Option<Zeroizing<String>>> {
    TENTATIVAS_RENOVACAO.fetch_add(1, Ordering::SeqCst);
    let _serie = RENOVACAO.lock().await;
    let conta = ler_conta_destravada(hub).await?;
    let Some(conta) = conta else { return Ok(None) };
    let agora = crate::store::now_ms() / 1000;
    if conta.expira_em.is_some_and(|expira_em| expira_em > agora + MARGEM_RENOVACAO) {
        return Ok(conta.access_token.map(Zeroizing::new));
    }
    let Some(refresh_token) = conta.refresh_token.clone() else { return Ok(None) };
    #[cfg(test)]
    let destino = match BASE_EM_TESTE.locked().clone() {
        Some(base) => format!("{base}{TOKEN}"),
        // Sem injeção a renovação falha fechada: a prova nunca sai à rede real.
        None => bail!("renovacao_falhou: o emissor não respondeu."),
    };
    #[cfg(not(test))]
    let destino = format!("{BASE}{TOKEN}");
    let cliente = crate::fork::conectores::oauth_urls::cliente()?;
    let pedido = [
        ("grant_type", "refresh_token".to_owned()),
        ("client_id", conta.client_id.clone()),
        ("refresh_token", refresh_token),
        ("resource", RECURSO.to_owned()),
    ];
    let mut resposta = cliente
        .post(&destino)
        .form(&pedido)
        .timeout(PRAZO)
        .send()
        .await
        .map_err(|_| anyhow!("renovacao_falhou: o emissor não respondeu."))?;
    if !resposta.status().is_success() {
        if resposta.status().as_u16() != 400 {
            bail!("renovacao_falhou: o emissor não renovou a sessão.");
        }
        let corpo =
            ler_corpo(&mut resposta).await.map_err(|_| anyhow!("renovacao_falhou: o emissor não respondeu."))?;
        let erro: Value =
            serde_json::from_slice(&corpo).map_err(|_| anyhow!("renovacao_falhou: o emissor não respondeu."))?;
        match erro["error"].as_str() {
            Some(codigo) if INVALIDANTES.contains(&codigo) => {
                apagar_tokens(&hub.store, &conta)?;
                return Ok(None);
            }
            Some("invalid_client") => bail!("renovacao_falhou: o emissor recusou o client_id (invalid_client)."),
            _ => bail!("renovacao_falhou: o emissor recusou a renovação."),
        }
    }
    let corpo = ler_corpo(&mut resposta).await.map_err(|_| anyhow!("renovacao_falhou: o emissor não respondeu."))?;
    let valor: Value =
        serde_json::from_slice(&corpo).map_err(|_| anyhow!("resposta_invalida: a renovação não é legível."))?;
    let nova = Conta {
        access_token: Some(
            campo(&valor, "access_token").map_err(|_| anyhow!("resposta_invalida: a renovação não é legível."))?,
        ),
        refresh_token: Some(
            campo(&valor, "refresh_token").map_err(|_| anyhow!("resposta_invalida: a renovação não é legível."))?,
        ),
        expira_em: Some(
            crate::store::now_ms() / 1000
                + valor["expires_in"]
                    .as_i64()
                    .filter(|segundos| *segundos > 0 && *segundos <= ANO_EM_SEGUNDOS)
                    .ok_or_else(|| anyhow!("resposta_invalida: a renovação não declara a validade."))?,
        ),
        // Sem `scope` na resposta, o plano fica desabilitado: só uma nova entrada reconfirma os escopos.
        escopos: valor["scope"].as_str().map(str::to_owned),
        ..conta
    };
    // O refresh novo fica guardado antes do access voltar a quem chamou; sem ele o antigo morreu na rotação.
    let mut gravada = gravar_conta(&hub.store, &nova);
    for _ in 0..2 {
        if gravada.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        gravada = gravar_conta(&hub.store, &nova);
    }
    gravada
        .map_err(|_| anyhow!("renovacao_incerta: a sessão foi renovada mas não pôde ser guardada; entre de novo."))?;
    Ok(nova.access_token.map(Zeroizing::new))
}

/// Apaga os tokens da sessão (conserva `client_id`, `sub` e rótulo para o próximo login).
fn apagar_tokens(store: &Store, conta: &Conta) -> Result<()> {
    let sem_tokens = Conta {
        client_id: conta.client_id.clone(),
        sub: conta.sub.clone(),
        rotulo: conta.rotulo.clone(),
        escopos: None,
        access_token: None,
        refresh_token: None,
        expira_em: None,
    };
    gravar_conta(store, &sem_tokens)
}

/// Sair: revoga o refresh no emissor e apaga os tokens locais de qualquer jeito, conservando
/// `client_id`, `sub` e rótulo para o próximo login. Sem confirmação da revogação, a tela é avisada
/// (`revogado: false`) e os tokens somem do mesmo jeito.
pub(super) async fn sair(hub: &Arc<Hub>) -> Result<Value> {
    let _serie = RENOVACAO.lock().await;
    SESSAO.fetch_add(1, Ordering::SeqCst);
    *TENTATIVA.locked() = None;
    let conta = ler_conta_destravada(hub).await?;
    let Some(conta) = conta else { return Ok(json!({"ok": true, "revogado": true})) };
    let Some(refresh_token) = conta.refresh_token.clone() else {
        return Ok(json!({"ok": true, "revogado": true}));
    };
    let revogado = revogar(&conta.client_id, &refresh_token).await;
    apagar_tokens(&hub.store, &conta)?;
    Ok(json!({"ok": true, "revogado": revogado}))
}

/// Revoga o refresh no emissor: POST form-encoded, sem segredo de cliente. Só uma resposta de
/// sucesso confirma; rede, 5xx ou redirecionamento não confirmam.
async fn revogar(client_id: &str, refresh_token: &str) -> bool {
    #[cfg(test)]
    let destino = match BASE_EM_TESTE.locked().clone() {
        Some(base) => format!("{base}{REVOGACAO}"),
        // Sem injeção a revogação não é confirmada; os tokens locais somem do mesmo jeito.
        None => return false,
    };
    #[cfg(not(test))]
    let destino = format!("{BASE}{REVOGACAO}");
    let Ok(cliente) = crate::fork::conectores::oauth_urls::cliente() else { return false };
    let pedido = [
        ("token", refresh_token.to_owned()),
        ("token_type_hint", "refresh_token".to_owned()),
        ("client_id", client_id.to_owned()),
    ];
    // Falha passageira (rede ou 5xx) tem uma segunda chance; 4xx é recusa definitiva.
    for _ in 0..2 {
        let Ok(resposta) = cliente.post(&destino).form(&pedido).timeout(PRAZO).send().await else {
            tokio::time::sleep(Duration::from_millis(300)).await;
            continue;
        };
        if resposta.status().is_success() {
            return true;
        }
        if resposta.status().as_u16() < 500 {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

/// Identificador opaco e estável deste Hub: um `urn:uuid:` v4 aleatório, gerado uma vez e
/// guardado em kv; nunca derivado de hardware nem de dado pessoal.
fn host_id(store: &Store) -> Result<String> {
    let _serie = SERIE_HOST_ID.locked();
    if let Some(id) = store.kv_read(KV_HOST_ID)? {
        return Ok(id);
    }
    let mut bytes = crypto::random::<16>();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    let id = format!("urn:uuid:{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]);
    store.kv_set(KV_HOST_ID, &id)?;
    Ok(id)
}

/// Lê o registro cifrado da conta; o erro nunca repete o conteúdo do registro.
fn ler_conta(store: &Store) -> Result<Option<Conta>> {
    let Some(raw) = store.kv_read(SLOT_CONTA)? else { return Ok(None) };
    let texto = crate::market::vault::open(store, SLOT_CONTA, &raw)?;
    serde_json::from_str(&texto).map(Some).map_err(|_| anyhow!("registro_invalido: entre de novo."))
}

/// Lê o registro e, se o cofre estiver trancado, destrava antes de recusar.
async fn ler_conta_destravada(hub: &Arc<Hub>) -> Result<Option<Conta>> {
    match ler_conta(&hub.store) {
        Ok(conta) => Ok(conta),
        Err(erro) if crate::market::vault::indisponivel(&erro) => {
            crate::market::vault::unlock(hub.clone()).await?;
            ler_conta(&hub.store)
        }
        Err(erro) => Err(erro),
    }
}

/// Grava o registro cifrado no cofre, pelo mesmo caminho das outras credenciais.
fn gravar_conta(store: &Store, conta: &Conta) -> Result<()> {
    crate::market::vault::write(store, SLOT_CONTA, &serde_json::to_string(conta)?)
}

/// Estado para a tela, sem token algum.
fn resposta_da_conta(conta: Option<&Conta>) -> Value {
    let Some(conta) = conta else {
        // Sem registro nenhum é o primeiro acesso: pede entrada, não «entrada de novo».
        return json!({"conectado": false, "rotulo": null, "planoHabilitado": false, "precisaEntrar": false});
    };
    json!({
        "conectado": conta.conectada(),
        "rotulo": conta.rotulo.clone(),
        "planoHabilitado": conta.plano_habilitado(),
        "precisaEntrar": !conta.conectada(),
    })
}

/// O escopo concedido habilita o uso do plano da conta?
fn tem_escopo_direto(escopos: &str) -> bool {
    escopos.split_whitespace().any(|escopo| escopo == ESCOPO_DIRETO)
}
