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
use super::{Plano, cerca::Cerca};
use crate::{LockExt, agent::acp::Acp};
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
    collections::HashMap,
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
const PRAZO_DE_CORTESIA: Duration = Duration::from_secs(2);
const PRAZO_DE_ENCERRAMENTO: Duration = Duration::from_secs(8);
/// Quanto uma revogação espera uma abertura em curso prender o processo (a concessão também
/// pode ser largada sem processo nenhum: largar já resolve a espera).
const PRAZO_DA_ABERTURA: Duration = Duration::from_secs(10);

type Hash = [u8; 32];

/// Uma sessão do repasse: a chave de verdade só existe aqui, na memória do Hub.
struct Sessao {
    plano: &'static Plano,
    chave: Zeroizing<String>,
    base_de_prova: Option<String>,
    revogada: watch::Sender<bool>,
    /// Vira true quando a concessão prende um processo (qualquer resultado) ou é largada: quem
    /// revoga e achou a sessão ainda sem processo espera por aqui a abertura se resolver.
    resolvida: watch::Sender<bool>,
    /// O processo do OpenCode que recebeu o código: encerrado junto com a sessão.
    acp: Mutex<Option<Arc<Acp>>>,
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
    /// Marca a sessão como revogada de um jeito que quem a enxergar depois também vê (`send_replace` grava o valor mesmo
    /// sem ninguém escutando) e entrega o processo para ser encerrado.
    fn revogar(&self) -> Option<Arc<Acp>> {
        self.revogada.send_replace(true);
        self.acp.locked().take()
    }
}

#[derive(Default)]
struct Estado {
    sessoes: HashMap<Hash, Arc<Sessao>>,
    geracoes: HashMap<&'static str, u64>,
    /// Processos de um plano que não confirmaram a saída na última revogação: cada revogação
    /// seguinte os reexamina antes de devolver sucesso.
    pendentes: HashMap<&'static str, Vec<Arc<Acp>>>,
}

/// O repasse de um Hub: um servidor em loopback e as sessões vivas.
pub(crate) struct Repasse {
    porta: u16,
    estado: Mutex<Estado>,
    cliente: reqwest::Client,
    vagas: Arc<Semaphore>,
    #[cfg(test)]
    pub(crate) base_de_prova: Mutex<Option<String>>,
    #[cfg(test)]
    /// Prova do estado pendente: ligado, o encerramento roda de verdade mas nenhuma confirmação vale.
    pub(crate) encerramento_de_prova: Mutex<bool>,
}

/// O direito de uma sessão de OpenCode usar o plano. Ao sair de escopo (processo encerrado ou trocado), o código é
/// revogado e os pedidos em andamento são cortados.
pub(crate) struct Concessao {
    repasse: Arc<Repasse>,
    hash: Hash,
    codigo: Zeroizing<String>,
    /// A sessão desta concessão, guardada aqui também: quem revoga remove a sessão do estado e
    /// ainda assim quem abre consegue marcá-la resolvida e saber o plano do processo pendente.
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

    /// Liga o código ao processo que o recebeu: trocar ou remover a chave encerra esse processo. Devolve `false` se a
    /// sessão já foi revogada (a chave mudou entre conceder e abrir): quem chamou deve encerrar o processo que abriu —
    /// e o processo ainda entra em `pendentes` do plano, para a revogação seguinte o encerrar de vez.
    pub(crate) fn prender(&self, acp: &Arc<Acp>) -> bool {
        let mut estado = self.repasse.estado.locked();
        let presa = match estado.sessoes.get(&self.hash).cloned() {
            None => {
                estado.pendentes.entry(self.sessao.plano.id).or_default().push(acp.clone());
                false
            }
            Some(sessao) => {
                *sessao.acp.locked() = Some(acp.clone());
                if *sessao.revogada.borrow() {
                    sessao.acp.locked().take();
                    estado.pendentes.entry(sessao.plano.id).or_default().push(acp.clone());
                    false
                } else {
                    true
                }
            }
        };
        // Qualquer resultado resolve a abertura, e só depois de o processo estar em `pendentes`: quem revoga e esperava
        // então o encontra ao drenar.
        self.sessao.resolvida.send_replace(true);
        presa
    }
}

impl Drop for Concessao {
    fn drop(&mut self) {
        self.sessao.resolvida.send_replace(true);
        self.repasse.revogar_hash(&self.hash);
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
            #[cfg(test)]
            encerramento_de_prova: Mutex::new(false),
        });
        let rotas = Router::new().fallback(tratar).with_state(repasse.clone());
        tokio::spawn(async move {
            if let Err(erro) = axum::serve(escuta, rotas).await {
                tracing::warn!(%erro, "o repasse local dos planos parou");
            }
        });
        Ok(repasse)
    }

    /// Número que sobe a cada troca ou remoção da chave de um plano. Quem vai conceder lê o número ANTES de ler a
    /// chave e o devolve em `conceder`: uma troca no meio recusa a concessão em vez de deixar uma chave velha viva.
    pub(crate) fn geracao(&self, plano: &Plano) -> u64 {
        self.estado.locked().geracoes.get(plano.id).copied().unwrap_or(0)
    }

    pub(crate) fn conceder(
        self: &Arc<Self>,
        plano: &'static Plano,
        chave: Zeroizing<String>,
        geracao: u64,
    ) -> Result<Concessao> {
        let mut codigo = [0u8; 32];
        getrandom::fill(&mut codigo).map_err(|_| anyhow::anyhow!("sem aleatório para o código da sessão"))?;
        let mut texto = String::from("mxs_");
        for byte in codigo {
            let _ = write!(texto, "{byte:02x}");
        }
        let codigo = Zeroizing::new(texto);
        let hash = hash_do_codigo(&codigo);
        let mut estado = self.estado.locked();
        ensure!(
            estado.geracoes.get(plano.id).copied().unwrap_or(0) == geracao,
            "plano_em_troca: a chave do plano mudou enquanto a sessão abria."
        );
        ensure!(
            estado.pendentes.get(plano.id).is_none_or(Vec::is_empty),
            "plano_em_troca: o plano ainda tem processo vivo de uma revogação anterior; tente de novo."
        );
        ensure!(estado.sessoes.len() < MAX_SESSOES, "plano_sessoes: sessões demais abertas.");
        #[cfg(test)]
        let base_de_prova = self.base_de_prova.locked().clone();
        #[cfg(not(test))]
        let base_de_prova = None;
        let (revogada, _) = watch::channel(false);
        let (resolvida, _) = watch::channel(false);
        let sessao = Arc::new(Sessao { plano, chave, base_de_prova, revogada, resolvida, acp: Mutex::new(None) });
        estado.sessoes.insert(hash, sessao.clone());
        drop(estado);
        Ok(Concessao { repasse: self.clone(), hash, codigo, sessao })
    }

    /// A concessão foi solta: o processo já está sendo encerrado por quem a tinha.
    fn revogar_hash(&self, hash: &Hash) {
        let sessao = self.estado.locked().sessoes.remove(hash);
        if let Some(sessao) = sessao {
            sessao.revogar();
        }
    }

    /// Troca ou remoção da chave: todas as sessões do plano morrem, os pedidos em andamento são cortados e os processos
    /// do OpenCode que receberam o código são encerrados (grupo inteiro, saída confirmada) antes de a operação voltar.
    /// Quem não confirma fica pendente do plano e a chamada devolve erro: o código já está revogado de qualquer jeito,
    /// e a chave não está em risco, só o processo. Uma chamada seguinte reexamina os pendentes e só devolve `Ok`
    /// depois de confirmar todos.
    pub(crate) async fn revogar_plano(&self, plano: &Plano) -> Result<()> {
        let (processos, aberturas): (Vec<Arc<Acp>>, Vec<Arc<Sessao>>) = {
            let mut estado = self.estado.locked();
            *estado.geracoes.entry(plano.id).or_insert(0) += 1;
            let mortas: Vec<Hash> = estado
                .sessoes
                .iter()
                .filter(|(_, sessao)| sessao.plano.id == plano.id)
                .map(|(hash, _)| *hash)
                .collect();
            let mut processos = estado.pendentes.remove(plano.id).unwrap_or_default();
            let mut aberturas = Vec::new();
            for hash in mortas {
                let Some(sessao) = estado.sessoes.remove(&hash) else { continue };
                match sessao.revogar() {
                    Some(acp) => processos.push(acp),
                    None => aberturas.push(sessao),
                }
            }
            (processos, aberturas)
        };
        let mut nao_confirmados = self.encerrar_todos(plano, processos).await;
        // Abertura em curso (sessão sem processo ainda): espera quem abre prender o processo —
        // aí ele aparece em `pendentes` — ou largar a concessão, que já resolve a espera.
        for sessao in aberturas {
            let mut resolvida = sessao.resolvida.subscribe();
            if tokio::time::timeout(PRAZO_DA_ABERTURA, resolvida.wait_for(|r| *r)).await.is_err() {
                nao_confirmados += 1;
            }
        }
        let reaparecidos = self.estado.locked().pendentes.remove(plano.id).unwrap_or_default();
        nao_confirmados += self.encerrar_todos(plano, reaparecidos).await;
        ensure!(
            nao_confirmados == 0,
            "plano_processo_vivo: o código do plano foi revogado, mas o processo do OpenCode não confirmou a saída."
        );
        Ok(())
    }

    /// Encerra cada processo em paralelo e devolve quantos não confirmaram a saída: esses voltam
    /// para `pendentes` do plano, para a revogação seguinte reexaminar.
    async fn encerrar_todos(&self, plano: &Plano, processos: Vec<Arc<Acp>>) -> usize {
        if processos.is_empty() {
            return 0;
        }
        let confirmados = futures::future::join_all(
            processos.iter().map(|processo| processo.encerrar_confirmado(PRAZO_DE_CORTESIA, PRAZO_DE_ENCERRAMENTO)),
        )
        .await;
        #[cfg(test)]
        let confirmados =
            if *self.encerramento_de_prova.locked() { vec![false; confirmados.len()] } else { confirmados };
        let sobreviveram: Vec<_> =
            processos.into_iter().zip(confirmados).filter(|(_, saiu)| !*saiu).map(|(processo, _)| processo).collect();
        let total = sobreviveram.len();
        if !sobreviveram.is_empty() {
            self.estado.locked().pendentes.entry(plano.id).or_default().extend(sobreviveram);
        }
        total
    }

    fn sessao(&self, codigo: &str) -> Option<Arc<Sessao>> {
        self.estado.locked().sessoes.get(&hash_do_codigo(codigo)).cloned()
    }

    #[cfg(test)]
    pub(crate) fn sessoes_vivas(&self) -> usize {
        self.estado.locked().sessoes.len()
    }

    #[cfg(test)]
    pub(crate) fn pendentes_do_plano(&self, plano: &Plano) -> usize {
        self.estado.locked().pendentes.get(plano.id).map_or(0, Vec::len)
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
        let mut autorizacao = HeaderValue::from_str(&format!("Bearer {}", sessao.chave.as_str()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        autorizacao.set_sensitive(true);
        cabecalhos.insert(header::AUTHORIZATION, autorizacao);

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
