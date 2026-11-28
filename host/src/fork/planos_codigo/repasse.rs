//! Repasse local da chave dos planos de código.
//!
//! A chave de um plano nunca entra no processo do OpenCode. O OpenCode fala com este repasse, só em loopback e numa
//! porta efêmera, usando um código de sessão descartável no lugar da chave. É o Hub que troca o código pela chave de
//! verdade e encaminha o pedido ao endereço fixo do plano. Assim o ambiente do OpenCode, o de um MCP filho, o de um
//! comando de shell e o de um plugin só enxergam um código que vale para aquela sessão e que morre com ela.
//!
//! O que o repasse garante: só aceita POST nos caminhos que o fornecedor usa; só encaminha para o endereço fixo do
//! plano, sem seguir redirecionamento; limita o tamanho e o tempo de cada pedido; passa os cabeçalhos do cliente (o
//! User-Agent inclusive) sem alteração, trocando só a autenticação; recusa resposta cujo corpo ou cabeçalho contenha a
//! chave; e nunca registra a chave nem o código.
//!
//! O que não garante: uma ferramenta que roube o código de sessão pode usar o plano enquanto a sessão durar. Ela não
//! leva a chave.
use super::{Autenticacao, Plano, cerca::Cerca};
use crate::LockExt;
use anyhow::{Result, ensure};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::Response,
};
use futures::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use zeroize::Zeroizing;

const LIMITE_PEDIDO: usize = 32 << 20;
const LIMITE_RESPOSTA: usize = 64 << 20;
const PRAZO_DO_CORPO: Duration = Duration::from_secs(120);
const PRAZO_DE_LEITURA: Duration = Duration::from_secs(120);
const PRAZO_TOTAL: Duration = Duration::from_secs(30 * 60);
const PRAZO_DE_CONEXAO: Duration = Duration::from_secs(15);
const MAX_SESSOES: usize = 256;
const MAX_PEDIDOS: usize = 32;

type Hash = [u8; 32];

/// Uma sessão do repasse: a chave de verdade só existe aqui, na memória do Hub.
struct Sessao {
    plano: &'static Plano,
    chave: Zeroizing<String>,
    base_de_prova: Option<String>,
    revogada: watch::Sender<bool>,
}

/// Só para prova: quem só olha a sessão depois de ela ser revogada.
#[cfg(test)]
pub(super) struct Tardia(Arc<Sessao>);

#[cfg(test)]
impl Tardia {
    pub(super) fn marcada(&self) -> bool {
        *self.0.revogada.subscribe().borrow()
    }
}

impl Sessao {
    /// Marca a sessão como revogada de um jeito que quem a enxergar depois também vê (`send_replace` grava o valor
    /// mesmo sem ninguém escutando) e corta os pedidos em andamento.
    fn revogar(&self) {
        self.revogada.send_replace(true);
    }
}

#[derive(Default)]
struct Estado {
    sessoes: HashMap<Hash, Arc<Sessao>>,
    /// Quantas concessões vivas cada plano tem: trocar ou remover a chave de um plano assim é recusado.
    usos: HashMap<&'static str, usize>,
    /// Planos com troca ou remoção em andamento: abrir sessão nova neles é recusado.
    em_troca: HashSet<&'static str>,
}

/// O plano está livre para trocar ou remover a chave agora? A recusa é a confirmação de que há conversa aberta ou
/// abertura em curso.
fn livre_para_troca(plano: &Plano, estado: &Estado) -> Result<()> {
    ensure!(
        !estado.em_troca.contains(plano.id) && estado.usos.get(plano.id).is_none_or(|&usos| usos == 0),
        "plano_em_uso: encerre as conversas deste plano e tente de novo."
    );
    Ok(())
}

/// O repasse de um Hub: um servidor em loopback e as sessões vivas.
pub(crate) struct Repasse {
    porta: u16,
    estado: Mutex<Estado>,
    cliente: reqwest::Client,
    vagas: Arc<Semaphore>,
    #[cfg(test)]
    pub(crate) base_de_prova: Mutex<Option<String>>,
}

/// O direito de uma sessão de OpenCode usar o plano. Vive do início da abertura até o processo sair: ao sair de
/// escopo, o código é revogado (os pedidos em andamento são cortados) e o uso do plano sai da conta.
pub(crate) struct Concessao {
    repasse: Arc<Repasse>,
    hash: Hash,
    codigo: Zeroizing<String>,
    sessao: Arc<Sessao>,
}

impl Concessao {
    /// O código que o OpenCode usa no lugar da chave.
    pub(crate) fn codigo(&self) -> &str {
        &self.codigo
    }

    /// O endereço base que o OpenCode usa no lugar do endereço do fornecedor.
    pub(crate) fn base(&self) -> String {
        format!("http://127.0.0.1:{}/r", self.repasse.porta)
    }
}

impl Drop for Concessao {
    fn drop(&mut self) {
        let mut estado = self.repasse.estado.locked();
        if let Some(sessao) = estado.sessoes.remove(&self.hash) {
            sessao.revogar();
        }
        if let Some(usos) = estado.usos.get_mut(self.sessao.plano.id) {
            *usos = usos.saturating_sub(1);
            if *usos == 0 {
                estado.usos.remove(self.sessao.plano.id);
            }
        }
    }
}

/// A troca (ou remoção) da chave de um plano em andamento: guardada viva durante a gravação, impede abrir sessão
/// nova no plano e, ao sair de escopo, devolve o plano à normalidade.
pub(crate) struct Troca {
    repasse: Arc<Repasse>,
    plano: &'static str,
}

impl Drop for Troca {
    fn drop(&mut self) {
        self.repasse.estado.locked().em_troca.remove(self.plano);
    }
}

fn hash_do_codigo(codigo: &str) -> Hash {
    Sha256::digest(codigo.as_bytes()).into()
}

impl Repasse {
    pub(crate) async fn iniciar() -> Result<Arc<Self>> {
        let escuta = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let porta = escuta.local_addr()?.port();
        let cliente = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(PRAZO_DE_CONEXAO)
            .read_timeout(PRAZO_DE_LEITURA)
            .timeout(PRAZO_TOTAL)
            .build()?;
        let repasse = Arc::new(Self {
            porta,
            estado: Mutex::default(),
            cliente,
            vagas: Arc::new(Semaphore::new(MAX_PEDIDOS)),
            #[cfg(test)]
            base_de_prova: Mutex::default(),
        });
        let rotas = Router::new().fallback(tratar).with_state(repasse.clone());
        tokio::spawn(async move {
            if let Err(erro) = axum::serve(escuta, rotas).await {
                tracing::warn!(%erro, "o repasse local dos planos parou");
            }
        });
        Ok(repasse)
    }

    /// Abre a sessão de um plano a partir de um retrato do que o OpenCode vai usar: `retrato` roda DENTRO da trava e
    /// devolve `(revisão, chave)`. A conferência da troca, a leitura da revisão e da chave e a contagem do uso
    /// acontecem numa só seção crítica, então uma troca ou remoção não pode gravar no meio: a sessão nunca fica com a
    /// revisão nova e a chave velha.
    pub(crate) fn conceder<F>(self: &Arc<Self>, plano: &'static Plano, retrato: F) -> Result<(Concessao, u64)>
    where
        F: FnOnce() -> Result<(u64, Zeroizing<String>)>,
    {
        let mut estado = self.estado.locked();
        ensure!(
            !estado.em_troca.contains(plano.id),
            "plano_em_troca: o plano está em troca de chave ou sendo encerrado; tente de novo."
        );
        ensure!(estado.sessoes.len() < MAX_SESSOES, "plano_sessoes: sessões demais abertas.");
        let (revisao, chave) = retrato()?;
        let mut codigo = [0u8; 32];
        getrandom::fill(&mut codigo).map_err(|_| anyhow::anyhow!("sem aleatório para o código da sessão"))?;
        let mut texto = String::from("mxs_");
        for byte in codigo {
            let _ = write!(texto, "{byte:02x}");
        }
        let codigo = Zeroizing::new(texto);
        let hash = hash_do_codigo(&codigo);
        #[cfg(test)]
        let base_de_prova = self.base_de_prova.locked().clone();
        #[cfg(not(test))]
        let base_de_prova = None;
        let (revogada, _) = watch::channel(false);
        let sessao = Arc::new(Sessao { plano, chave, base_de_prova, revogada });
        estado.sessoes.insert(hash, sessao.clone());
        *estado.usos.entry(plano.id).or_insert(0) += 1;
        drop(estado);
        Ok((Concessao { repasse: self.clone(), hash, codigo, sessao }, revisao))
    }

    /// Se a troca ou a remoção da chave passaria agora, sem reservar nada: leitura só, para recusar antes de testar a
    /// chave nova na rede. A gravação confere de novo em `reservar_troca`.
    pub(crate) fn pode_trocar(&self, plano: &Plano) -> Result<()> {
        livre_para_troca(plano, &self.estado.locked())
    }

    /// Troca ou remoção da chave de um plano: recusada enquanto o plano tiver qualquer sessão viva (a recusa é a
    /// confirmação de que há gente usando). Sem sessões, reserva a troca e devolve a guarda que a mantém reservada
    /// até o fim da gravação: nenhuma sessão nova abre no plano no meio.
    pub(crate) fn reservar_troca(self: &Arc<Self>, plano: &Plano) -> Result<Troca> {
        let mut estado = self.estado.locked();
        livre_para_troca(plano, &estado)?;
        estado.em_troca.insert(plano.id);
        Ok(Troca { repasse: self.clone(), plano: plano.id })
    }

    fn sessao(&self, codigo: &str) -> Option<Arc<Sessao>> {
        self.estado.locked().sessoes.get(&hash_do_codigo(codigo)).cloned()
    }

    #[cfg(test)]
    pub(crate) fn sessoes_vivas(&self) -> usize {
        self.estado.locked().sessoes.len()
    }

    #[cfg(test)]
    pub(crate) fn usos_do_plano(&self, plano: &Plano) -> usize {
        self.estado.locked().usos.get(plano.id).copied().unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn vagas_livres(&self) -> usize {
        self.vagas.available_permits()
    }

    #[cfg(test)]
    pub(super) fn sessao_tardia(&self, codigo: &str) -> Option<Tardia> {
        self.sessao(codigo).map(Tardia)
    }
}

async fn tratar(State(repasse): State<Arc<Repasse>>, pedido: Request) -> Response {
    match repasse.encaminhar(pedido).await {
        Ok(resposta) => resposta,
        Err(status) => {
            let mut vazia = Response::new(Body::empty());
            *vazia.status_mut() = status;
            vazia
        }
    }
}

/// O código da sessão, onde o SDK de cada formato o manda: `Authorization`, `x-api-key` ou `x-goog-api-key`.
fn codigo_do_pedido(cabecalhos: &HeaderMap) -> Option<String> {
    let texto = |nome: &str| cabecalhos.get(nome).and_then(|valor| valor.to_str().ok()).map(str::trim);
    texto("authorization")
        .map(|valor| valor.strip_prefix("Bearer ").or_else(|| valor.strip_prefix("bearer ")).unwrap_or(valor))
        .or_else(|| texto("x-api-key"))
        .or_else(|| texto("x-goog-api-key"))
        .filter(|codigo| !codigo.is_empty() && codigo.len() <= 128)
        .map(str::to_owned)
}

/// O destino do caminho pedido, pela tabela de rotas do plano; nada fora dela passa.
pub(super) fn destino(plano: &Plano, caminho: &str, consulta: Option<&str>) -> Option<String> {
    let resto = caminho.strip_prefix("/r/")?;
    if consulta.is_some_and(|consulta| consulta != "alt=sse") {
        return None;
    }
    let url = plano.rotas.iter().find_map(|(padrao, url)| match padrao.split_once('*') {
        None => (resto == *padrao).then(|| (*url).to_owned()),
        Some((inicio, fim)) => {
            let meio = resto.strip_prefix(inicio)?.strip_suffix(fim)?;
            let valido = !meio.is_empty()
                && meio.len() <= 128
                && meio.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
            valido.then(|| url.replace('*', meio))
        }
    })?;
    Some(if consulta.is_some() { format!("{url}?alt=sse") } else { url })
}

fn repassa_o_cabecalho_do_pedido(nome: &HeaderName) -> bool {
    !matches!(
        nome.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
            | "authorization"
            | "x-api-key"
            | "x-goog-api-key"
            | "api-key"
            | "cookie"
            | "accept-encoding"
    )
}

fn devolve_o_cabecalho_da_resposta(nome: &HeaderName) -> bool {
    !matches!(
        nome.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "set-cookie"
            | "location"
    )
}

impl Repasse {
    async fn encaminhar(&self, pedido: Request) -> Result<Response, StatusCode> {
        let (partes, corpo) = pedido.into_parts();
        if partes.method != Method::POST {
            return Err(StatusCode::NOT_FOUND);
        }
        // Só o próprio computador, pelo endereço da sessão: uma página da web (Origin) ou um nome apontado para o
        // loopback (Host) não chega ao código.
        let autoridade = format!("127.0.0.1:{}", self.porta);
        let host_ok = partes.headers.get(header::HOST).and_then(|host| host.to_str().ok()) == Some(autoridade.as_str());
        if partes.headers.contains_key(header::ORIGIN)
            || !host_ok
            || partes.uri.authority().is_some_and(|dada| dada.as_str() != autoridade)
        {
            return Err(StatusCode::FORBIDDEN);
        }
        let codigo = codigo_do_pedido(&partes.headers).ok_or(StatusCode::NOT_FOUND)?;
        let sessao = self.sessao(&codigo).ok_or(StatusCode::NOT_FOUND)?;
        let mut url = destino(sessao.plano, partes.uri.path(), partes.uri.query()).ok_or(StatusCode::NOT_FOUND)?;
        if let Some(base) = &sessao.base_de_prova
            && let Some(caminho) = url.strip_prefix("https://").and_then(|resto| resto.split_once('/')).map(|(_, c)| c)
        {
            url = format!("{base}/{caminho}");
        }
        let vaga = self.vagas.clone().try_acquire_owned().map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
        let mut revogada = sessao.revogada.subscribe();
        if *revogada.borrow() {
            return Err(StatusCode::NOT_FOUND);
        }
        let corpo: Bytes = tokio::select! {
            biased;
            _ = revogada.wait_for(|revogada| *revogada) => return Err(StatusCode::NOT_FOUND),
            lido = tokio::time::timeout(PRAZO_DO_CORPO, axum::body::to_bytes(corpo, LIMITE_PEDIDO)) => lido
                .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
                .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?,
        };
        // Último ponto de admissão: depois dele o pedido segue ao fornecedor (e já não dá para desfazê-lo).
        if *revogada.borrow() {
            return Err(StatusCode::NOT_FOUND);
        }

        let mut cabecalhos = HeaderMap::new();
        for (nome, valor) in &partes.headers {
            if repassa_o_cabecalho_do_pedido(nome) {
                cabecalhos.append(nome.clone(), valor.clone());
            }
        }
        cabecalhos.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        let (nome_da_chave, valor_da_chave) = match sessao.plano.autenticacao {
            Autenticacao::Bearer => (header::AUTHORIZATION, format!("Bearer {}", sessao.chave.as_str())),
            Autenticacao::XApiKey => (HeaderName::from_static("x-api-key"), sessao.chave.as_str().to_owned()),
            Autenticacao::XGoogApiKey => (HeaderName::from_static("x-goog-api-key"), sessao.chave.as_str().to_owned()),
        };
        let mut autorizacao = HeaderValue::from_str(&valor_da_chave).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        autorizacao.set_sensitive(true);
        cabecalhos.insert(nome_da_chave, autorizacao);

        let envio = self.cliente.post(url).headers(cabecalhos).body(corpo).send();
        let resposta = tokio::select! {
            biased;
            _ = revogada.wait_for(|revogada| *revogada) => return Err(StatusCode::NOT_FOUND),
            resposta = envio => resposta.map_err(|erro| {
                if erro.is_timeout() { StatusCode::GATEWAY_TIMEOUT } else { StatusCode::BAD_GATEWAY }
            })?,
        };
        let status = resposta.status();
        if status.is_redirection() {
            return Err(StatusCode::BAD_GATEWAY);
        }
        if resposta
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|codificacao| !codificacao.as_bytes().eq_ignore_ascii_case(b"identity"))
        {
            return Err(StatusCode::BAD_GATEWAY);
        }
        let cerca = Cerca::nova(&sessao.chave);
        let mut saida = Response::builder().status(status);
        for (nome, valor) in resposta.headers() {
            if !devolve_o_cabecalho_da_resposta(nome) {
                continue;
            }
            if cerca.contem(nome.as_str().as_bytes()) || cerca.contem(valor.as_bytes()) {
                return Err(StatusCode::BAD_GATEWAY);
            }
            saida = saida.header(nome, valor);
        }
        let fluxo = filtrar(Box::pin(resposta.bytes_stream()), cerca, revogada, vaga);
        saida.body(Body::from_stream(fluxo)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }
}

type Fluxo = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

struct Filtro {
    fluxo: Fluxo,
    pendente: Vec<u8>,
    total: usize,
    cerca: Cerca,
    revogada: watch::Receiver<bool>,
    fim: bool,
    /// A vaga do teto de pedidos fica com o fluxo até ele terminar ou ser largado, não só até os cabeçalhos.
    _vaga: OwnedSemaphorePermit,
}

/// A resposta do fornecedor, em pedaços, sem nunca deixar passar a chave (nem a chave disfarçada, ver `cerca.rs`); corta
/// o fluxo se a sessão for revogada. O fim da janela que ainda pode ser o começo da chave fica retido até o próximo
/// pedaço; o resto segue na hora (o streaming não atrasa).
fn filtrar(
    fluxo: Fluxo,
    cerca: Cerca,
    revogada: watch::Receiver<bool>,
    vaga: OwnedSemaphorePermit,
) -> impl Stream<Item = std::io::Result<Bytes>> {
    let estado = Filtro { fluxo, pendente: Vec::new(), total: 0, cerca, revogada, fim: false, _vaga: vaga };
    futures::stream::unfold(estado, |mut e| async move {
        if e.fim {
            return None;
        }
        let proximo = tokio::select! {
            biased;
            _ = e.revogada.wait_for(|revogada| *revogada) => None,
            proximo = e.fluxo.next() => Some(proximo),
        };
        let Some(proximo) = proximo else {
            e.fim = true;
            return Some((Err(std::io::Error::other("sessão revogada")), e));
        };
        match proximo {
            None => {
                e.fim = true;
                let resto = std::mem::take(&mut e.pendente);
                (!resto.is_empty()).then(|| (Ok(Bytes::from(resto)), e))
            }
            Some(Err(_)) => {
                e.fim = true;
                Some((Err(std::io::Error::other("fornecedor interrompido")), e))
            }
            Some(Ok(pedaco)) => {
                e.total += pedaco.len();
                let mut janela = std::mem::take(&mut e.pendente);
                janela.extend_from_slice(&pedaco);
                if e.total > LIMITE_RESPOSTA || e.cerca.contem(&janela) {
                    e.fim = true;
                    return Some((Err(std::io::Error::other("resposta recusada")), e));
                }
                let segura = janela.len() - e.cerca.retido(&janela);
                e.pendente = janela.split_off(segura);
                Some((Ok(Bytes::from(janela)), e))
            }
        }
    })
}
