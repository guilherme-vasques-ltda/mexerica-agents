//! Atualização privilegiada: verificação, transação por fase, reversão e retomada.
#![allow(dead_code)]

#[cfg(target_os = "linux")]
pub(super) mod armazenamento;
#[cfg(target_os = "linux")]
mod baseline;
#[cfg(target_os = "linux")]
mod certidao;
#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(target_os = "linux")]
pub(super) mod chamador;
#[cfg(target_os = "linux")]
pub(super) mod diagnostico;
pub(super) mod entrada;
pub(super) mod estado;
#[cfg(target_os = "linux")]
pub(super) mod executar;
#[cfg(target_os = "linux")]
mod historico_tuf;
#[cfg(target_os = "linux")]
pub(super) mod instalacao;
#[cfg(target_os = "linux")]
pub(super) mod journal;
mod manifesto;
#[cfg(target_os = "linux")]
mod manifesto_persistido;
#[cfg(target_os = "linux")]
pub(super) mod modo_motor;
#[cfg(target_os = "linux")]
pub(super) mod politica;
#[cfg(target_os = "linux")]
pub(super) mod processo;
#[cfg(target_os = "linux")]
mod processos;
#[cfg(target_os = "linux")]
pub(super) mod recuperacao;
mod rede;
#[cfg(target_os = "linux")]
mod release;
#[cfg(target_os = "linux")]
mod relogio;
#[cfg(target_os = "linux")]
mod restaurar_config;
#[cfg(target_os = "linux")]
pub(super) mod servico_hub;
#[cfg(target_os = "linux")]
pub(super) mod slots;
#[cfg(target_os = "linux")]
mod snapshot;
mod status;
mod sudoers;
#[cfg(target_os = "linux")]
pub(super) mod transacao;
mod tuf;
#[cfg(target_os = "linux")]
pub(super) mod verificacao;

/// Hexadecimal minúsculo para hashes e nomes aleatórios.
pub(crate) trait Hex {
    fn hex(&self) -> String;
}
impl<T: AsRef<[u8]> + ?Sized> Hex for T {
    fn hex(&self) -> String {
        use std::fmt::Write as _;
        self.as_ref().iter().fold(String::with_capacity(self.as_ref().len() * 2), |mut texto, byte| {
            let _ = write!(texto, "{byte:02x}");
            texto
        })
    }
}

#[cfg(test)]
mod tests;
