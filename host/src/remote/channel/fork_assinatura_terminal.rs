use crate::remote::{crypto, identity::Identity};

const RECUSA_V1: &[u8; 16] = b"codync/recusa/v1";

pub(super) mod corte {
    use super::Identity;
    const CORTE_V1: &[u8; 15] = b"codync/corte/v1";

    #[derive(Clone, Copy)]
    #[repr(u8)]
    pub(crate) enum Estavel {
        DeviceRevoked = 1,
        WorkspaceRevoked = 2,
        RoleChanged = 3,
    }

    impl Estavel {
        pub(super) fn codigo(self) -> &'static str {
            match self {
                Self::DeviceRevoked => "device_revoked",
                Self::WorkspaceRevoked => "workspace_revoked",
                Self::RoleChanged => "role_changed",
            }
        }
    }

    pub(crate) struct Corte {
        pub(crate) th: [u8; 32],
        pub(crate) estavel: Estavel,
        pub(crate) workspace_epoch: u64,
        pub(crate) aberto_em: u64,
    }

    impl Corte {
        pub(crate) fn bytes(&self) -> Option<[u8; 64]> {
            if self.aberto_em >= self.workspace_epoch {
                return None;
            }
            let mut bytes = [0; 64];
            bytes[..15].copy_from_slice(CORTE_V1);
            bytes[15..47].copy_from_slice(&self.th);
            bytes[47] = self.estavel as u8;
            bytes[48..56].copy_from_slice(&self.workspace_epoch.to_be_bytes());
            bytes[56..].copy_from_slice(&self.aberto_em.to_be_bytes());
            Some(bytes)
        }

        pub(crate) fn assinar(&self, identidade: &Identity) -> Option<[u8; 64]> {
            Some(identidade.sign(&self.bytes()?))
        }

        #[cfg(test)]
        pub(crate) fn verificar(&self, chave: &[u8; 32], bytes: &[u8], sig: &[u8]) -> bool {
            let (Some(esperado), Ok(bytes), Ok(sig)) = (self.bytes(), <&[u8; 64]>::try_from(bytes), sig.try_into())
            else {
                return false;
            };
            bytes == &esperado && super::crypto::verify(chave, bytes, sig).is_ok()
        }
    }
}

pub(super) fn reject_corte(identidade: &Identity, corte: &corte::Corte) -> Option<serde_json::Value> {
    let sig = corte.assinar(identidade)?;
    let mut quadro = super::reject(super::RejectCode::Revoked);
    quadro["stableCode"] = corte.estavel.codigo().into();
    quadro["prova"] = serde_json::json!({
        "v": 1, "tipo": "corte", "workspaceEpoch": corte.workspace_epoch,
        "abertoEm": corte.aberto_em, "sig": crypto::b64(&sig),
    });
    Some(quadro)
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum RecusaEstavel {
    DeviceRevoked = 1,
    DeviceUnpaired = 4,
}

pub(super) struct Recusa {
    pub(super) cid: [u8; 16],
    pub(super) dk: [u8; 32],
    pub(super) ek_d: [u8; 32],
    pub(super) n: [u8; 32],
    pub(super) estavel: RecusaEstavel,
}

impl Recusa {
    pub(super) fn bytes(&self) -> [u8; 129] {
        let mut bytes = [0; 129];
        bytes[..16].copy_from_slice(RECUSA_V1);
        bytes[16..32].copy_from_slice(&self.cid);
        bytes[32..64].copy_from_slice(&self.dk);
        bytes[64..96].copy_from_slice(&self.ek_d);
        bytes[96..128].copy_from_slice(&self.n);
        bytes[128] = self.estavel as u8;
        bytes
    }

    pub(super) fn assinar(&self, identidade: &Identity) -> [u8; 64] {
        identidade.sign(&self.bytes())
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "referência de verificação para os clientes"))]
    pub(super) fn verificar(&self, chave: &[u8; 32], bytes: &[u8], sig: &[u8]) -> bool {
        let (Ok(bytes), Ok(sig)) = (<&[u8; 129]>::try_from(bytes), sig.try_into()) else {
            return false;
        };
        bytes == &self.bytes() && crypto::verify(chave, bytes, sig).is_ok()
    }
}

#[cfg(test)]
mod vetores;
