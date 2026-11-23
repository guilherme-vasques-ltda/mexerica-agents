//! JWKS da instância Clerk fixada no vínculo. A validade de uma busca é a que o HTTP dá
//! (`Cache-Control: max-age` menos `Age`), com teto de 24 h. Sem `max-age` útil (ausente, `0`,
//! ilegível, vencido pelo `Age`), com `no-cache` ou sem cabeçalho, a validade é um teto fixo de
//! 1 h. Com `no-store`, o conjunto fica só em memória (nunca vai a disco) com o mesmo teto de 1 h.
//! Passada metade da validade, a renovação corre em segundo plano, sem segurar a verificação; só
//! sem conjunto válido (ou com `kid` desconhecido) a verificação espera a busca. Uma busca por vez,
//! com o GET fora do lock do conjunto; quem chega durante ela (vencido ou `kid` desconhecido) espera o
//! resultado dela e decide de novo com o estado de depois. `kid`
//! desconhecido ou falha anterior: no máximo uma busca a cada 30 s; conjunto vencido depois de uma
//! busca bem-sucedida busca já. Idade e validade são medidas no relógio monotônico. Só uma busca
//! bem-sucedida renova a validade; cada uma substitui o conjunto inteiro (chave removida sai).
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

    fn concluir(&mut self, busca: Result<(Vec<Chave>, Duration)>, agora: Instant) {
        if let Ok((chaves, frescor)) = busca {
            self.chaves = chaves;
            self.buscado_em = Some(agora);
            self.valido_ate = Some(agora + frescor);
            self.falhou = false;
        } else {
            self.falhou = true;
            tracing::warn!("JWKS do vínculo indisponível; segue o conjunto anterior enquanto valer");
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
}

impl Jwks {
    fn cache(&self, url: &reqwest::Url) -> std::sync::MutexGuard<'_, Cache> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.url.as_ref() != Some(url) {
            *cache = Cache { url: Some(url.clone()), ..Cache::default() };
        }
        cache
    }

    /// A chave `kid`. Sem conjunto válido ou com `kid` desconhecido, espera a busca em curso e,
    /// com o estado de depois da espera, faz a sua se o intervalo deixar.
    pub(crate) async fn chave(&self, url: &reqwest::Url, kid: &str, agora: Instant) -> Result<Resolvida, Recusa> {
        let falta = {
            let cache = self.cache(url);
            cache.valido_ate.is_none_or(|ate| agora >= ate) || !cache.chaves.iter().any(|c| c.kid == kid)
        };
        if falta {
            let _busca = self.busca.lock().await;
            let minha = {
                let mut cache = self.cache(url);
                let minha = cache.precisa(kid, agora.max(Instant::now()));
                if minha {
                    cache.tentativa_em = Some(agora);
                }
                minha
            };
            if minha {
                let busca = self.buscar_chaves(url).await;
                let mut cache = self.cache(url);
                cache.concluir(busca, agora);
            }
        }
        let agora = agora.max(Instant::now());
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
        let Ok(_busca) = self.busca.try_lock() else { return };
        {
            let mut cache = self.cache(url);
            if !cache.liberada(agora) {
                return;
            }
            cache.tentativa_em = Some(agora);
        }
        let busca = self.buscar_chaves(url).await;
        self.cache(url).concluir(busca, agora);
    }

    async fn buscar_chaves(&self, url: &reqwest::Url) -> Result<(Vec<Chave>, Duration)> {
        let (corpo, frescor) = self.buscar(url).await?;
        Ok((interpretar(&corpo)?, frescor))
    }

    async fn buscar(&self, url: &reqwest::Url) -> Result<(Vec<u8>, Duration)> {
        #[cfg(test)]
        {
            self.buscas.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let fonte = self.fonte.lock().map_err(|_| anyhow!("fonte"))?.clone();
            if let Some(fonte) = fonte {
                return fonte();
            }
        }
        buscar_com(&cliente(true)?, url).await
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

/// Validade dada pelo HTTP: `max-age` menos `Age`, com teto de 24 h. Sem um valor positivo
/// (`no-cache`, `no-store`, `max-age` ausente, `0`, repetido ou ilegível, `Age` repetido, ilegível
/// ou maior),
/// o teto fixo de 1 h.
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
        (Some(max_age), Some(idade)) if max_age > idade => Duration::from_secs(max_age - idade).min(TETO),
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
