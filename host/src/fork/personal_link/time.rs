//! Fonte de intervalo D430 (complemento v2 §3). `TimeSource` é fechado e nasce `Unqualified`.
//! `Qualified` só sai do produtor privado que compara fatos do runtime a um perfil EMBUTIDO,
//! versionado, com digest e evidência real revisada. Nenhuma config/env/CLI/flag o fabrica.
//! Sem perfil embutido (estado deste corte), a raiz fica FECHADA em produção.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Janela da quota e da quarentena: `L(idade) > 86.400.000 ms`.
pub(crate) const JANELA_MS: u64 = 86_400_000;
pub(crate) const DESAFIO_MS: u64 = 30_000;

/// `L(d)=max(0,floor((d−2)×1000/1001))`; `None` só em overflow (fecha).
pub(crate) fn minimo(d: u64) -> Option<u64> {
    match d.checked_sub(2) {
        None => Some(0),
        Some(x) => x.checked_mul(1000).map(|x| x / 1001),
    }
}

/// `U(d)=ceil((d+2)×1000/999)`; `None` em overflow (fecha).
pub(crate) fn maximo(d: u64) -> Option<u64> {
    d.checked_add(2)?.checked_mul(1000)?.checked_add(998).map(|x| x / 999)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motivo {
    SemPerfilEmbutido,
    FatosIndisponiveis,
    FatosDivergentes,
}

/// Fatos lidos do próprio runtime; nunca recebidos de cliente/config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fatos {
    clocksource: String,
    timens: String,
    kernel: String,
    virtualizacao: String,
    freq: i64,
    tick: i64,
    status: i32,
}

/// Perfil revisado. Evidência real precisa constar no código antes de entrar na lista.
pub(crate) struct Perfil {
    id: &'static str,
    digest: &'static str,
    clocksource: &'static str,
    timens: &'static str,
    kernel: &'static str,
    virtualizacao: &'static str,
    freq_max_abs: i64,
    tick: i64,
    status: i32,
    /// Limite de variação de (BOOTTIME − MONOTONIC) aceito por amostragem; 0 = qualquer suspensão fecha.
    suspensao_ms: u64,
}

/// VAZIO: nenhum perfil físico foi qualificado. Não inventar entrada positiva.
const PERFIS: &[Perfil] = &[];

#[derive(Debug)]
pub(crate) enum TimeSource {
    Unqualified(Motivo),
    Qualified { perfil_id: &'static str, perfil_digest: &'static str, fatos: Fatos, suspensao_ms: u64 },
}
impl Default for TimeSource {
    fn default() -> Self {
        Self::Unqualified(Motivo::SemPerfilEmbutido)
    }
}

impl Perfil {
    fn aceita(&self, f: &Fatos) -> bool {
        f.clocksource == self.clocksource
            && f.timens == self.timens
            && f.kernel == self.kernel
            && f.virtualizacao == self.virtualizacao
            && f.freq.checked_abs().is_some_and(|x| x <= self.freq_max_abs)
            && f.tick == self.tick
            && f.status == self.status
    }
}

fn comparar(perfis: &[Perfil], fatos: Option<Fatos>) -> TimeSource {
    if perfis.is_empty() {
        return TimeSource::Unqualified(Motivo::SemPerfilEmbutido);
    }
    let Some(fatos) = fatos else { return TimeSource::Unqualified(Motivo::FatosIndisponiveis) };
    match perfis.iter().find(|p| p.aceita(&fatos)) {
        Some(p) => {
            TimeSource::Qualified { perfil_id: p.id, perfil_digest: p.digest, fatos, suspensao_ms: p.suspensao_ms }
        }
        None => TimeSource::Unqualified(Motivo::FatosDivergentes),
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Fatos;
    use anyhow::{Result, ensure};

    fn texto(path: &str) -> Result<String> {
        let s = std::fs::read_to_string(path)?;
        ensure!(s.len() <= 4096, "fato de relógio grande demais");
        Ok(s.trim().to_owned())
    }

    pub(super) fn ajuste() -> Result<(i64, i64, i32)> {
        // SAFETY: timex é POD; zerado com modes=0 torna adjtimex uma leitura sem efeito.
        let mut t: libc::timex = unsafe { std::mem::zeroed() };
        // SAFETY: ponteiro válido para timex alinhado e vivo durante a chamada; não é retido.
        let estado = unsafe { libc::adjtimex(&raw mut t) };
        ensure!(estado >= 0, "adjtimex indisponível");
        Ok((t.freq, t.tick, t.status))
    }

    pub(super) fn fatos() -> Result<Fatos> {
        let virtualizacao = match texto("/sys/hypervisor/type") {
            Ok(v) => v,
            Err(_) if std::fs::read_to_string("/proc/cpuinfo")?.split_whitespace().any(|w| w == "hypervisor") => {
                "cpuid-hypervisor".into()
            }
            Err(_) => "none".into(),
        };
        let (freq, tick, status) = ajuste()?;
        Ok(Fatos {
            clocksource: texto("/sys/devices/system/clocksource/clocksource0/current_clocksource")?,
            timens: texto("/proc/self/timens_offsets")?,
            kernel: texto("/proc/sys/kernel/osrelease")?,
            virtualizacao,
            freq,
            tick,
            status,
        })
    }

    pub(super) fn boot_id() -> Result<String> {
        let id = texto("/proc/sys/kernel/random/boot_id")?;
        ensure!(uuid::Uuid::parse_str(&id).is_ok_and(|u| u.to_string() == id), "boot_id inválido");
        Ok(id)
    }

    fn ms(clock: libc::clockid_t) -> Result<u64> {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: timespec válido e vivo; clock_gettime só escreve nele.
        let estado = unsafe { libc::clock_gettime(clock, &raw mut ts) };
        ensure!(estado == 0, "clock_gettime indisponível");
        let s = u64::try_from(ts.tv_sec)?;
        let n = u64::try_from(ts.tv_nsec)?;
        s.checked_mul(1000).and_then(|x| x.checked_add(n / 1_000_000)).ok_or_else(|| anyhow::anyhow!("overflow"))
    }

    pub(super) fn amostra() -> Result<(u64, u64)> {
        Ok((ms(libc::CLOCK_BOOTTIME)?, ms(libc::CLOCK_MONOTONIC)?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Amostra {
    pub boottime_ms: u64,
    monotonic_ms: u64,
}

/// Relógio D430 de um runtime. Descontinuidade fecha até um novo runtime (nova quarentena).
pub(crate) struct Relogio {
    fonte: TimeSource,
    boot_id: String,
    inicio: Mutex<Option<Amostra>>,
    fechado: AtomicBool,
    #[cfg(test)]
    teste: Option<std::sync::Arc<Mutex<Amostra>>>,
}

impl Relogio {
    /// Único produtor de produção. Sem perfil embutido aplicável: `Unqualified`.
    pub(crate) fn produzir() -> Self {
        #[cfg(target_os = "linux")]
        let (fonte, boot_id) = (comparar(PERFIS, linux::fatos().ok()), linux::boot_id().unwrap_or_default());
        #[cfg(not(target_os = "linux"))]
        let (fonte, boot_id) = (TimeSource::Unqualified(Motivo::FatosIndisponiveis), String::new());
        Self {
            fonte,
            boot_id,
            inicio: Mutex::new(None),
            fechado: AtomicBool::new(false),
            #[cfg(test)]
            teste: None,
        }
    }

    /// Descrição pública da fonte para o histórico do runtime (sem segredo).
    pub(crate) fn descricao(&self) -> String {
        match &self.fonte {
            TimeSource::Unqualified(motivo) => format!("unqualified:{motivo:?}"),
            TimeSource::Qualified { perfil_id, perfil_digest, .. } => format!("qualified:{perfil_id}:{perfil_digest}"),
        }
    }

    pub(crate) fn qualificada(&self) -> bool {
        matches!(self.fonte, TimeSource::Qualified { .. }) && !self.fechado.load(Ordering::Acquire)
    }

    pub(crate) fn boot_id(&self) -> &str {
        &self.boot_id
    }

    fn suspensao_ms(&self) -> Result<u64> {
        match &self.fonte {
            TimeSource::Qualified { suspensao_ms, fatos, .. } => {
                #[cfg(target_os = "linux")]
                {
                    #[cfg(test)]
                    let atual =
                        if self.teste.is_some() { Ok((fatos.freq, fatos.tick, fatos.status)) } else { linux::ajuste() };
                    #[cfg(not(test))]
                    let atual = linux::ajuste();
                    // A taxa admitida vale ao longo do intervalo: ajuste novo divergente fecha.
                    let (freq, tick, status) = atual?;
                    ensure!(
                        freq == fatos.freq && tick == fatos.tick && status == fatos.status,
                        "not_allowed: ajuste do relógio mudou"
                    );
                }
                #[cfg(not(target_os = "linux"))]
                let _ = fatos;
                Ok(*suspensao_ms)
            }
            TimeSource::Unqualified(motivo) => bail!("not_allowed: fonte de tempo não qualificada ({motivo:?})"),
        }
    }

    /// Amostra do sistema (produção). A fonte de teste só existe em `cfg(test)`, em `amostra`.
    fn ler() -> Result<Amostra> {
        #[cfg(target_os = "linux")]
        {
            let (boottime_ms, monotonic_ms) = linux::amostra()?;
            Ok(Amostra { boottime_ms, monotonic_ms })
        }
        #[cfg(not(target_os = "linux"))]
        bail!("not_allowed: fonte de tempo D430 requer Linux")
    }

    /// Amostra qualificada, contínua e sem regressão; qualquer falha fecha o runtime.
    pub(crate) fn amostra(&self) -> Result<Amostra> {
        ensure!(!self.fechado.load(Ordering::Acquire), "not_allowed: continuidade de tempo encerrada");
        let resultado = (|| -> Result<Amostra> {
            let tolerancia = self.suspensao_ms()?;
            #[cfg(test)]
            let agora = match &self.teste {
                Some(teste) => *teste.lock().map_err(|_| anyhow::anyhow!("relógio de teste"))?,
                None => Self::ler()?,
            };
            #[cfg(not(test))]
            let agora = Self::ler()?;
            let inicio = *self.inicio.lock().map_err(|_| anyhow::anyhow!("relógio"))?;
            if let Some(inicio) = inicio {
                ensure!(
                    agora.boottime_ms >= inicio.boottime_ms && agora.monotonic_ms >= inicio.monotonic_ms,
                    "not_allowed: relógio regrediu"
                );
                let antes = inicio.boottime_ms.checked_sub(inicio.monotonic_ms);
                let depois = agora.boottime_ms.checked_sub(agora.monotonic_ms);
                let (Some(antes), Some(depois)) = (antes, depois) else { bail!("not_allowed: relógio inconsistente") };
                ensure!(depois.abs_diff(antes) <= tolerancia, "not_allowed: suspensão sem limite qualificado");
            }
            Ok(agora)
        })();
        if resultado.is_err() {
            self.fechado.store(true, Ordering::Release);
        }
        resultado
    }

    /// Fixa o início do runtime com amostra posterior ao commit observado.
    pub(crate) fn iniciar(&self) -> Result<()> {
        ensure!(
            self.inicio.lock().map_err(|_| anyhow::anyhow!("relógio"))?.is_none(),
            "not_allowed: runtime já iniciado"
        );
        let agora = self.amostra()?;
        *self.inicio.lock().map_err(|_| anyhow::anyhow!("relógio"))? = Some(agora);
        Ok(())
    }

    fn idade(desde_ms: u64, agora: Amostra) -> Result<u64> {
        agora.boottime_ms.checked_sub(desde_ms).context("not_allowed: relógio regrediu")
    }

    /// P1 (a): NOVAS admissões só depois de `L(idade do runtime) > 24 h`, em todo runtime.
    pub(crate) fn quarentena_terminada(&self, agora: Amostra) -> Result<bool> {
        let inicio = (*self.inicio.lock().map_err(|_| anyhow::anyhow!("relógio"))?)
            .context("not_allowed: runtime sem início observado")?;
        let l = minimo(Self::idade(inicio.boottime_ms, agora)?).context("not_allowed: overflow de tempo")?;
        Ok(l > JANELA_MS)
    }

    /// Prazos expiram quando `U(idade)` alcança o limite.
    pub(crate) fn vencido(nascido_ms: u64, ttl_ms: u64, agora: Amostra) -> Result<bool> {
        let u = maximo(Self::idade(nascido_ms, agora)?).context("not_allowed: overflow de tempo")?;
        Ok(u >= ttl_ms)
    }

    /// Reserva continua na janela enquanto `L(idade) <= 24 h` (borda conservadora).
    pub(crate) fn dentro_da_janela(ancora_ms: u64, agora: Amostra) -> bool {
        agora.boottime_ms.checked_sub(ancora_ms).and_then(minimo).is_none_or(|l| l <= JANELA_MS)
    }

    pub(crate) fn prazo(&self, runtime: &str, ttl_ms: u64) -> Result<(Value, Amostra)> {
        let agora = self.amostra()?;
        Ok((super::wire::prazo(&self.boot_id, runtime, agora.boottime_ms, ttl_ms), agora))
    }
}

#[cfg(test)]
impl Relogio {
    /// Prova de LÓGICA apenas; inalcançável no binário de produção.
    pub(crate) fn de_teste(qualificado: bool, inicial: u64) -> (Self, std::sync::Arc<Mutex<Amostra>>) {
        let amostra = std::sync::Arc::new(Mutex::new(Amostra { boottime_ms: inicial, monotonic_ms: inicial }));
        let fonte = if qualificado {
            TimeSource::Qualified {
                perfil_id: "test-only",
                perfil_digest: "test-only",
                fatos: Fatos {
                    clocksource: "test".into(),
                    timens: "test".into(),
                    kernel: "test".into(),
                    virtualizacao: "test".into(),
                    freq: 0,
                    tick: 10_000,
                    status: 0,
                },
                suspensao_ms: 2,
            }
        } else {
            TimeSource::default()
        };
        let relogio = Self {
            fonte,
            boot_id: "c5386ca1-bad0-4cfb-b19b-88badf199264".into(),
            inicio: Mutex::new(None),
            fechado: AtomicBool::new(false),
            teste: Some(amostra.clone()),
        };
        (relogio, amostra)
    }

    pub(crate) fn avancar(amostra: &Mutex<Amostra>, ms: u64, suspenso_ms: u64) {
        let mut a = amostra.lock().expect("relógio de teste");
        a.boottime_ms += ms + suspenso_ms;
        a.monotonic_ms += ms;
    }

    /// Produção: lista embutida vazia → sempre `Unqualified`.
    pub(crate) fn comparar_embutidos(fatos: Option<Fatos>) -> TimeSource {
        comparar(PERFIS, fatos)
    }

    /// Lógica do comparador com um perfil FICTÍCIO de teste (nunca embutido em produção).
    pub(crate) fn comparar_ficticio(fatos: Option<Fatos>) -> TimeSource {
        let perfil = Perfil {
            id: "test-only",
            digest: "test-only",
            clocksource: "tsc",
            timens: "monotonic 0 0 boottime 0 0",
            kernel: "test",
            virtualizacao: "none",
            freq_max_abs: 0,
            tick: 10_000,
            status: 0,
            suspensao_ms: 2,
        };
        comparar(std::slice::from_ref(&perfil), fatos)
    }

    pub(crate) fn fatos_ficticios(freq: i64, tick: i64) -> Fatos {
        Fatos {
            clocksource: "tsc".into(),
            timens: "monotonic 0 0 boottime 0 0".into(),
            kernel: "test".into(),
            virtualizacao: "none".into(),
            freq,
            tick,
            status: 0,
        }
    }
}
