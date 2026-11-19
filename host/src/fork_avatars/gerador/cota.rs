//! Custo sob controle do dono: cada geração gasta duas chamadas pagas de imagem.
//! A reserva é atômica na fila do Hub e consumida mesmo em falha, sem retry pago.
use crate::store::Store;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// HIPÓTESE pendente do dono (`thr_5tetnf79su`): gerações por dia UTC. Número não aprovado;
/// se o dono ajustar, vira configuração. Mecanismo (janela + reserva) já é final.
pub(crate) const LIMITE_DIARIO: u32 = 3;
const SLOT: &str = "mexeriquinhas:cota";

#[derive(Serialize, Deserialize)]
struct Cota {
    dia: String,
    usados: u32,
}

fn hoje() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn load(store: &Store) -> Result<Cota> {
    let dia = hoje();
    Ok(store
        .kv_read(SLOT)?
        .and_then(|raw| serde_json::from_str::<Cota>(&raw).ok())
        .filter(|c| c.dia == dia)
        .unwrap_or(Cota { dia, usados: 0 }))
}

/// Chamada já sob a trava do gerador: lê, confere e reserva uma tentativa na mesma gravação.
pub fn reservar(store: &Store) -> Result<()> {
    let mut cota = load(store)?;
    if cota.usados >= LIMITE_DIARIO {
        return Err(super::falha("COTA_ATINGIDA"));
    }
    cota.usados += 1;
    store.kv_set(SLOT, &serde_json::to_string(&cota)?)
}

#[cfg(test)]
pub(crate) fn usados(store: &Store) -> u32 {
    load(store).map_or(0, |c| c.usados)
}
