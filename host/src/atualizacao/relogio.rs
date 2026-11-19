//! Compara relógio de parede com monotônico persistido; não autentica a hora.
use super::armazenamento::Diretorio;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Write};

const MIN_PAREDE_MS: u64 = 1_577_836_800_000; // 2020-01-01 UTC
const TOLERANCIA_MS: u64 = 5_000;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Amostra {
    parede_ms: u64,
    monotono_ms: u64,
    boot: String,
}

fn milissegundos(id: libc::clockid_t) -> Result<u64> {
    // SAFETY: ponteiro válido para clock_gettime preencher.
    let mut t: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: clock fixo e estrutura válida.
    ensure!(unsafe { libc::clock_gettime(id, &raw mut t) } == 0, "relógio indisponível");
    ensure!(t.tv_sec >= 0 && (0..1_000_000_000).contains(&t.tv_nsec), "relógio inválido");
    u64::try_from(t.tv_sec)?
        .checked_mul(1000)
        .and_then(|v| v.checked_add(t.tv_nsec.cast_unsigned() / 1_000_000))
        .ok_or_else(|| anyhow::anyhow!("relógio fora do intervalo"))
}

fn coletar() -> Result<Amostra> {
    let inicio = milissegundos(libc::CLOCK_BOOTTIME)?;
    let parede_ms = milissegundos(libc::CLOCK_REALTIME)?;
    let monotono_ms = milissegundos(libc::CLOCK_BOOTTIME)?;
    ensure!(monotono_ms >= inicio && monotono_ms - inicio <= 1000, "amostragem do relógio inconsistente");
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim().to_owned();
    let id = uuid::Uuid::parse_str(&boot)?;
    ensure!(id.to_string() == boot, "identidade de boot inválida");
    let atual = Amostra { parede_ms, monotono_ms, boot };
    validar_amostra(&atual)?;
    Ok(atual)
}
fn validar_amostra(amostra: &Amostra) -> Result<()> {
    ensure!(
        amostra.parede_ms >= MIN_PAREDE_MS && uuid::Uuid::parse_str(&amostra.boot)?.to_string() == amostra.boot,
        "relógio inválido"
    );
    Ok(())
}
fn comparar(anterior: &Amostra, atual: &Amostra) -> Result<()> {
    validar_amostra(anterior)?;
    validar_amostra(atual)?;
    let parede = atual
        .parede_ms
        .checked_sub(anterior.parede_ms)
        .ok_or_else(|| anyhow::anyhow!("relógio de parede retrocedeu"))?;
    if anterior.boot == atual.boot {
        let monotono = atual
            .monotono_ms
            .checked_sub(anterior.monotono_ms)
            .ok_or_else(|| anyhow::anyhow!("relógio monotônico retrocedeu"))?;
        let tolerancia = TOLERANCIA_MS
            .checked_add(monotono / 1000)
            .ok_or_else(|| anyhow::anyhow!("intervalo de relógio inválido"))?;
        ensure!(parede.abs_diff(monotono) <= tolerancia, "desvio suspeito entre relógios");
    }
    // Depois de reinício não há medição monotônica do tempo desligado.
    Ok(())
}

impl Diretorio {
    /// Uso administrativo uma única vez; não sobrescreve marca anterior.
    pub fn inicializar_relogio(&self, trava: &File) -> Result<()> {
        self.validar_trava(trava)?;
        let amostra = coletar()?;
        let mut file = self.criar_regular("relogio")?;
        file.write_all(&serde_json::to_vec(&amostra)?)?;
        file.sync_all()?;
        self.descritor().sync_all()?;
        Ok(())
    }
    /// Exige marca root existente, compara e a avança antes de retornar a hora.
    pub fn hora_conferida(&self, trava: &File) -> Result<i64> {
        self.validar_trava(trava)?;
        let anterior: Amostra = self.ler("relogio")?;
        let atual = coletar()?;
        comparar(&anterior, &atual)?;
        self.gravar("relogio", &atual)?;
        Ok(i64::try_from(atual.parede_ms / 1000)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const BOOT: &str = "00000000-0000-4000-8000-000000000001";
    const OUTRO: &str = "00000000-0000-4000-8000-000000000002";
    #[test]
    fn rejeita_regresso_e_saltos_no_mesmo_boot() {
        let base = Amostra { parede_ms: MIN_PAREDE_MS + 100_000, monotono_ms: 50_000, boot: BOOT.into() };
        let amostra = |parede_ms, monotono_ms, boot: &str| Amostra { parede_ms, monotono_ms, boot: boot.into() };
        assert!(comparar(&base, &amostra(base.parede_ms + 10_000, 60_000, BOOT)).is_ok());
        assert!(comparar(&base, &amostra(base.parede_ms - 1, 60_000, BOOT)).is_err());
        assert!(comparar(&base, &amostra(base.parede_ms + 100_000, 60_000, BOOT)).is_err());
        assert!(comparar(&base, &amostra(base.parede_ms + 10_000, 40_000, BOOT)).is_err());
        assert!(comparar(&base, &amostra(base.parede_ms - 1, 100, OUTRO)).is_err());
        assert!(comparar(&base, &amostra(base.parede_ms + 10_000, 100, OUTRO)).is_ok());
    }
}
