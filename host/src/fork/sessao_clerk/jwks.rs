//! JWKS da instância Clerk fixada no vínculo. A validade de uma busca é a que o HTTP dá
//! (`Cache-Control: max-age` menos `Age`), com teto de 24 h. Sem `max-age` (ausente, repetido ou
//! ilegível), com `no-cache` ou sem cabeçalho, a validade é um teto fixo de 1 h. Um `max-age`
//! explícito já vencido (`0`, ou `Age` igual ou maior) não vale nada. Com `no-store`, o conjunto
//! fica só em memória (nunca vai a disco) com o mesmo teto de 1 h.
//! Passada metade da validade, a renovação corre em segundo plano, sem segurar a verificação; só
//! sem conjunto válido (ou com `kid` desconhecido) a verificação espera a busca. Uma busca por vez,
//! com o GET fora do lock do conjunto; quem chega durante ela (vencido ou `kid` desconhecido) espera o
//! resultado dela e decide de novo com o estado de depois. `kid`
//! desconhecido ou falha anterior: no máximo uma busca a cada 30 s; conjunto vencido depois de uma
//! busca bem-sucedida busca já. Idade e validade são medidas no relógio monotônico, as duas a
//! partir do envio do GET (depois da espera pela busca de outro e da criação do cliente): o tempo
//! do GET, cabeçalhos e corpo, consome a validade. A tentativa é anotada antes do envio e conta
//! como falha até a busca terminar: uma busca cancelada no meio não pula o intervalo. Resposta
//! que termina de chegar já vencida conta como busca falha: não troca nem estende o conjunto, e a
//! próxima espera o intervalo. Só uma busca bem-sucedida renova a validade; cada uma substitui o
//! conjunto inteiro (chave removida sai).
//! Cliente HTTP sem proxy do ambiente, sem redirect e só HTTPS. Corpo e chaves nunca vão a log.
use super::Recusa;
use crate::remote::crypto;
use anyhow::{Result, anyhow, bail, ensure};
use reqwest::header::{AGE, CACHE_CONTROL, HeaderMap};
use ring::signature::RsaPublicKeyComponents;
use serde_json::Value;
use std::time::{Duration, Instant};

pub(crate) const TETO: Duration = Duration::from_secs(24 * 60 * 60);
/// Validade quando o HTTP não dá uma (`no-cache`, `no-store`, sem `max-age`).
pub(crate) const SEM_FRESCOR: Duration = Duration::from_secs(60 * 60);
pub(crate) const INTERVALO: Duration = Duration::from_secs(30);
const PRAZO: Duration = Duration::from_secs(10);
const MAX_CORPO: usize = 64 * 1024;
const MAX_CHAVES: usize = 16;

#[derive(Clone)]
struct Chave {
    kid: String,
    n: Vec<u8>,
    e: Vec<u8>,
}

#[derive(Default)]
struct Cache {
    url: Option<reqwest::Url>,
    chaves: Vec<Chave>,
    buscado_em: Option<Instant>,
    valido_ate: Option<Instant>,
    tentativa_em: Option<Instant>,
    /// A última busca falhou.
    falhou: bool,
}

impl Cache {
    /// Conjunto vencido depois de uma busca bem-sucedida: busca já (uma validade menor que o
    /// intervalo não pode deixar o remoto sem chave com o endpoint saudável). `kid` desconhecido
    /// ou falha anterior: no máximo uma busca por intervalo.
    fn liberada(&self, agora: Instant) -> bool {
        let vencido = self.valido_ate.is_none_or(|ate| agora >= ate);
        (vencido && !self.falhou) || self.tentativa_em.is_none_or(|em| agora.saturating_duration_since(em) >= INTERVALO)
    }

    fn precisa(&self, kid: &str, agora: Instant) -> bool {
        let valida = self.valido_ate.is_some_and(|ate| agora < ate);
        (!valida || !self.chaves.iter().any(|c| c.kid == kid)) && self.liberada(agora)
    }

    /// Antes do envio: a tentativa conta do envio e, até a busca terminar, como falha.
    fn anotar(&mut self, inicio: Instant) {
        self.tentativa_em = Some(inicio);
        self.falhou = true;
    }

    /// A validade conta do envio (`inicio`) e tem de seguir valendo no fim da leitura (`fim`).
    fn concluir(&mut self, busca: Result<(Vec<Chave>, Duration)>, inicio: Instant, fim: Instant) {
        match busca {
            Ok((chaves, frescor)) if inicio + frescor > fim => {
                self.chaves = chaves;
                self.buscado_em = Some(inicio);
                self.valido_ate = Some(inicio + frescor);
                self.falhou = false;
            }
            Ok(_) => {
                self.falhou = true;
                tracing::warn!("JWKS do vínculo chegou vencida; segue o conjunto anterior enquanto valer");
            }
            Err(_) => {
                self.falhou = true;
                tracing::warn!("JWKS do vínculo indisponível; segue o conjunto anterior enquanto valer");
            }
        }
    }
}

/// A chave do `kid`, até quando a JWKS vale e se já passou da hora de renovar em segundo plano.
pub(crate) struct Resolvida {
    pub(crate) chave: RsaPublicKeyComponents<Vec<u8>>,
    pub(crate) ate: Instant,
    pub(crate) renovar: bool,
}

#[cfg(test)]
type Fonte = std::sync::Arc<dyn Fn() -> Result<(Vec<u8>, Duration)> + Send + Sync>;

#[derive(Default)]
pub(crate) struct Jwks {
    /// Só para ler e trocar o conjunto; nunca fica tomado durante o GET.
    cache: std::sync::Mutex<Cache>,
    /// Uma busca por vez; quem chega durante uma busca espera o resultado dela.
    busca: tokio::sync::Mutex<()>,
    /// Provas: corpo da JWKS e validade servidos sem rede.
    #[cfg(test)]
    fonte: std::sync::Mutex<Option<Fonte>>,
    #[cfg(test)]
    buscas: std::sync::atomic::AtomicUsize,
    /// Provas: relógio controlado da busca (só avança quando a prova manda) e a demora de rede
    /// simulada em cada GET.
    #[cfg(test)]
    relogio: std::sync::Mutex<Duration>,
    #[cfg(test)]
    demora: std::sync::Mutex<Duration>,
    /// Provas: segura o GET em voo, depois de anotada a tentativa.
    #[cfg(test)]
    envio: tokio::sync::Mutex<()>,
}

impl Jwks {
    fn cache(&self, url: &reqwest::Url) -> std::sync::MutexGuard<'_, Cache> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.url.as_ref() != Some(url) {
            *cache = Cache { url: Some(url.clone()), ..Cache::default() };
        }
        cache
    }

    /// O relógio da busca: o monotônico; nas provas, o controlado.
    #[cfg(not(test))]
    #[expect(clippy::unused_self, reason = "nas provas o relógio é o controlado, guardado em `self`")]
    fn relogio(&self) -> Instant {
        Instant::now()
    }

    #[cfg(test)]
    fn relogio(&self) -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + *self.relogio.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A busca, com o cliente criado e a tentativa anotada antes do envio: tentativa e validade
    /// contam do envio do GET, que é o instante `agora` do pedido somado ao que já passou no relógio
    /// da busca desde `entrada`.
    async fn buscar_e_concluir(&self, url: &reqwest::Url, agora: Instant, entrada: Instant) {
        let cliente = cliente(true);
        let inicio = agora + self.relogio().saturating_duration_since(entrada);
        self.cache(url).anotar(inicio);
        let busca = match cliente {
            Ok(cliente) => self.buscar_chaves(&cliente, url).await,
            Err(erro) => Err(erro),
        };
        let fim = agora + self.relogio().saturating_duration_since(entrada);
        self.cache(url).concluir(busca, inicio, fim);
    }

    /// A chave `kid`. Sem conjunto válido ou com `kid` desconhecido, espera a busca em curso e,
    /// com o estado de depois da espera, faz a sua se o intervalo deixar.
    pub(crate) async fn chave(&self, url: &reqwest::Url, kid: &str, agora: Instant) -> Result<Resolvida, Recusa> {
        let entrada = self.relogio();
        let depois = |relogio: Instant| agora + relogio.saturating_duration_since(entrada);
        let falta = {
            let cache = self.cache(url);
            cache.valido_ate.is_none_or(|ate| agora >= ate) || !cache.chaves.iter().any(|c| c.kid == kid)
        };
        if falta {
            let _busca = self.busca.lock().await;
            if self.cache(url).precisa(kid, depois(self.relogio()).max(Instant::now())) {
                self.buscar_e_concluir(url, agora, entrada).await;
            }
        }
        let agora = depois(self.relogio()).max(Instant::now());
        let cache = self.cache(url);
        let Some(ate) = cache.valido_ate.filter(|ate| agora < *ate) else { return Err(Recusa::AuthInvalid) };
        let chave = cache.chaves.iter().find(|c| c.kid == kid).ok_or(Recusa::AuthInvalid)?;
        let meia_vida = cache.buscado_em.is_some_and(|em| agora >= em + ate.saturating_duration_since(em) / 2);
        Ok(Resolvida {
            chave: RsaPublicKeyComponents { n: chave.n.clone(), e: chave.e.clone() },
            ate,
            renovar: meia_vida && cache.liberada(agora),
        })
    }

    /// Renovação em segundo plano: só se nenhuma busca estiver em curso; o GET corre sem o lock do
    /// conjunto, e só uma busca bem-sucedida troca o conjunto. Falha não estende nada.
    pub(crate) async fn renovar(&self, url: &reqwest::Url, agora: Instant) {
        let entrada = self.relogio();
        let Ok(_busca) = self.busca.try_lock() else { return };
        if self.cache(url).liberada(agora + self.relogio().saturating_duration_since(entrada)) {
            self.buscar_e_concluir(url, agora, entrada).await;
        }
    }

    async fn buscar_chaves(&self, cliente: &reqwest::Client, url: &reqwest::Url) -> Result<(Vec<Chave>, Duration)> {
        let (corpo, frescor) = self.buscar(cliente, url).await?;
        Ok((interpretar(&corpo)?, frescor))
    }

    async fn buscar(&self, cliente: &reqwest::Client, url: &reqwest::Url) -> Result<(Vec<u8>, Duration)> {
        #[cfg(test)]
        {
            self.buscas.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            drop(self.envio.lock().await);
            let fonte = self.fonte.lock().map_err(|_| anyhow!("fonte"))?.clone();
            if let Some(fonte) = fonte {
                let demora = *self.demora.lock().map_err(|_| anyhow!("demora"))?;
                self.avancar(demora);
                return fonte();
            }
        }
        buscar_com(cliente, url).await
    }

    /// Chave `kid` de um vínculo proposto: busca avulsa, sem cache e sem tocar no conjunto do
    /// vínculo gravado, pelo cliente que só conecta a endereço público. Falha é recusa.
    pub(crate) async fn chave_proposta(
        &self,
        url: &reqwest::Url,
        kid: &str,
    ) -> Result<RsaPublicKeyComponents<Vec<u8>>, Recusa> {
        let corpo = self.buscar_proposta(url).await.map_err(|_| Recusa::AuthInvalid)?;
        let chave = interpretar(&corpo)
            .map_err(|_| Recusa::AuthInvalid)?
            .into_iter()
            .find(|c| c.kid == kid)
            .ok_or(Recusa::AuthInvalid)?;
        Ok(RsaPublicKeyComponents { n: chave.n, e: chave.e })
    }

    async fn buscar_proposta(&self, url: &reqwest::Url) -> Result<Vec<u8>> {
        #[cfg(test)]
        {
            let fonte = self.fonte.lock().map_err(|_| anyhow!("fonte"))?.clone();
            if let Some(fonte) = fonte {
                self.buscas.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return fonte().map(|(corpo, _)| corpo);
            }
        }
        let (corpo, _) = buscar_com(&cliente_publico()?, url).await?;
        Ok(corpo)
    }

    /// Provas: avança o relógio controlado da busca.
    #[cfg(test)]
    pub(crate) fn avancar(&self, quanto: Duration) {
        *self.relogio.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += quanto;
    }

    /// Provas: segura a vez da busca, como uma busca de outro pedido em curso.
    #[cfg(test)]
    pub(crate) async fn segurar_busca(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.busca.lock().await
    }

    /// Provas: segura o GET em voo, como uma rede que ainda não respondeu.
    #[cfg(test)]
    pub(crate) async fn segurar_envio(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.envio.lock().await
    }

    /// Provas: cada GET leva `demora` no relógio controlado, do envio ao fim do corpo.
    #[cfg(test)]
    pub(crate) fn demorar(&self, demora: Duration) {
        *self.demora.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = demora;
    }

    /// Provas: JWKS servida com a validade máxima.
    #[cfg(test)]
    pub(crate) fn servir(&self, fonte: impl Fn() -> Result<Vec<u8>> + Send + Sync + 'static) {
        self.servir_com_frescor(move || fonte().map(|corpo| (corpo, TETO)));
    }

    #[cfg(test)]
    pub(crate) fn servir_com_frescor(&self, fonte: impl Fn() -> Result<(Vec<u8>, Duration)> + Send + Sync + 'static) {
        *self.fonte.lock().expect("fonte") = Some(std::sync::Arc::new(fonte));
    }

    #[cfg(test)]
    pub(crate) fn buscas(&self) -> usize {
        self.buscas.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Provas: o conjunto atual vale só mais `resta`, sem renovação pendente.
    #[cfg(test)]
    pub(crate) fn vence_em(&self, resta: Duration) {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let agora = Instant::now();
        cache.valido_ate = Some(agora + resta);
        cache.buscado_em = Some(agora);
        cache.tentativa_em = None;
        cache.falhou = false;
    }
}

/// Validade dada pelo HTTP: `max-age` menos `Age`, com teto de 24 h; zero quando o `max-age`
/// explícito já venceu (`0`, ou `Age` igual ou maior). Com `no-cache`, `no-store`, `max-age`
/// ausente, repetido ou ilegível, ou `Age` repetido ou ilegível, o teto fixo de 1 h.
pub(crate) fn frescor(cabecalhos: &HeaderMap) -> Duration {
    let mut max_age = None;
    for valor in cabecalhos.get_all(CACHE_CONTROL) {
        let Ok(valor) = valor.to_str() else { return SEM_FRESCOR };
        for diretiva in valor.split(',').map(|d| d.trim().to_ascii_lowercase()) {
            if diretiva == "no-store" || diretiva == "no-cache" || diretiva.starts_with("no-cache=") {
                return SEM_FRESCOR;
            }
            if let Some(segundos) = diretiva.strip_prefix("max-age=") {
                let Ok(segundos) = segundos.trim_matches('"').parse::<u64>() else { return SEM_FRESCOR };
                if max_age.replace(segundos).is_some() {
                    return SEM_FRESCOR;
                }
            }
        }
    }
    let mut idades = cabecalhos.get_all(AGE).iter();
    let idade = match (idades.next(), idades.next()) {
        (None, _) => Some(0),
        (Some(v), None) => v.to_str().ok().and_then(|s| s.trim().parse::<u64>().ok()),
        (Some(_), Some(_)) => None,
    };
    match (max_age, idade) {
        (Some(max_age), Some(idade)) => Duration::from_secs(max_age.saturating_sub(idade)).min(TETO),
        _ => SEM_FRESCOR,
    }
}

/// `https_only` só é falso nas provas com servidor local sem TLS.
pub(crate) fn cliente(https_only: bool) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(https_only)
        .timeout(PRAZO)
        .connect_timeout(PRAZO)
        .build()
        .map_err(|_| anyhow!("cliente da JWKS indisponível"))
}

/// Cliente da JWKS proposta: os limites de [`cliente`] e só endereços públicos na resolução (o
/// próprio cliente conecta ao endereço conferido).
fn cliente_publico() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .timeout(PRAZO)
        .connect_timeout(PRAZO)
        .dns_resolver(SoPublico)
        .build()
        .map_err(|_| anyhow!("cliente da JWKS indisponível"))
}

struct SoPublico;

impl reqwest::dns::Resolve for SoPublico {
    fn resolve(&self, nome: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = nome.as_str().to_owned();
        Box::pin(async move {
            let enderecos: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if enderecos.is_empty() || !enderecos.iter().all(|a| publico(a.ip())) {
                return Err("destino da JWKS fora da rede pública".into());
            }
            Ok(Box::new(enderecos.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Endereço roteável na internet: nada de loopback, rede privada, link-local, CGNAT, multicast,
/// documentação ou não especificado, nem IPv4 embutido ou traduzido num IPv6 (mapeado, 6to4,
/// Teredo, NAT64).
pub(crate) fn publico(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped().or_else(|| v6.to_ipv4()) {
                return publico(IpAddr::V4(v4));
            }
            let [primeiro, segundo, ..] = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (primeiro & 0xfe00) == 0xfc00
                || (primeiro & 0xffc0) == 0xfe80
                || (primeiro & 0xffc0) == 0xfec0
                || primeiro == 0x2001 && (segundo == 0x0db8 || segundo == 0)
                || primeiro == 0x2002
                || primeiro == 0x0064 && segundo == 0xff9b)
        }
    }
}

/// Só `200`; redirect, outro status ou corpo acima do teto falham. Devolve o corpo e a validade.
pub(crate) async fn buscar_com(cliente: &reqwest::Client, url: &reqwest::Url) -> Result<(Vec<u8>, Duration)> {
    let mut resposta = cliente.get(url.clone()).send().await.map_err(|_| anyhow!("JWKS sem resposta"))?;
    ensure!(resposta.status() == reqwest::StatusCode::OK, "JWKS recusada");
    let validade = frescor(resposta.headers());
    let mut corpo = Vec::new();
    while let Some(parte) = resposta.chunk().await.map_err(|_| anyhow!("JWKS sem resposta"))? {
        ensure!(corpo.len() + parte.len() <= MAX_CORPO, "JWKS maior que o teto");
        corpo.extend_from_slice(&parte);
    }
    Ok((corpo, validade))
}

/// Só RSA de assinatura (`use` e `alg`, se presentes, têm de ser `sig` e `RS256`). Documento com
/// mais de 16 entradas falha inteiro; um `kid` repetido em qualquer entrada sai inteiro. Corpo sem
/// `keys` falha e mantém o conjunto anterior.
fn interpretar(corpo: &[u8]) -> Result<Vec<Chave>> {
    let jwks: Value = serde_json::from_slice(corpo).map_err(|_| anyhow!("JWKS ilegível"))?;
    let Some(lista) = jwks["keys"].as_array() else { bail!("JWKS sem keys") };
    ensure!(lista.len() <= MAX_CHAVES, "JWKS com chaves demais");
    let kids: Vec<&str> = lista.iter().filter_map(|k| k["kid"].as_str()).collect();
    let repetido = |kid: &str| kids.iter().filter(|k| **k == kid).count() > 1;
    let mut chaves: Vec<Chave> = vec![];
    for k in lista {
        let aceita =
            k["kty"] == "RSA" && k.get("use").is_none_or(|u| u == "sig") && k.get("alg").is_none_or(|a| a == "RS256");
        let Some(kid) = k["kid"].as_str().filter(|kid| aceita && !kid.is_empty() && kid.len() <= 128 && !repetido(kid))
        else {
            continue;
        };
        let (Some(Ok(n)), Some(Ok(e))) = (k["n"].as_str().map(crypto::unb64), k["e"].as_str().map(crypto::unb64))
        else {
            continue;
        };
        chaves.push(Chave { kid: kid.to_owned(), n, e });
    }
    Ok(chaves)
}
