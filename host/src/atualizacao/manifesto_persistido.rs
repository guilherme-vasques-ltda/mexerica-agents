//! Verificação da release autenticada e persistida para reversão e retomada.
use super::Hex as _;
use super::{
    armazenamento::Diretorio,
    estado::Fase,
    journal::Journal,
    manifesto::{self, Manifesto},
    politica::Politica,
};
use anyhow::{Result, ensure};
use sha2::{Digest as _, Sha256};
use std::fs::File;

impl Diretorio {
    /// Não consulta rede nem aceita conteúdo, nome ou versão do chamador.
    pub fn manifesto_transacao(&self, trava: &File, journal: &Journal, agora: i64) -> Result<Manifesto> {
        let manifesto = self.manifesto_assinado(trava, journal)?;
        match journal.estado.fase {
            Fase::Pendente { .. } | Fase::Expirada => ensure!(
                journal.estado.marca == manifesto.sequencia
                    && manifesto.sequencia_minima_reversivel == journal.estado.piso,
                "sequência ou piso divergente"
            ),
            _ => ensure!(
                journal.estado.marca <= manifesto.sequencia
                    && journal.estado.piso <= manifesto.sequencia_minima_reversivel,
                "marca ou piso inconsistentes"
            ),
        }
        ensure!(manifesto.sequencia_minima_reversivel <= manifesto.sequencia, "piso assinado inválido");
        ensure!(manifesto.inicio <= agora && agora < manifesto.expiracao, "manifesto fora da validade");
        if let Fase::Pendente { prazo, .. } = journal.estado.fase {
            ensure!(prazo <= manifesto.expiracao, "prazo excede validade assinada");
        }
        Ok(manifesto)
    }

    /// Autenticidade e vínculo ao journal, sem exigir validade temporal: usado
    /// para descrever o par ativo e para voltar ao estado anterior, nunca para publicar.
    pub fn manifesto_assinado(&self, trava: &File, journal: &Journal) -> Result<Manifesto> {
        self.validar_trava(trava)?;
        let politica = Politica::ler(self)?;
        let transacao = self.abrir_transacao(&journal.transacao)?;
        let bytes = transacao.ler_bytes("manifesto", manifesto::MAX_MANIFESTO as u64)?;
        let assinatura = transacao.ler_bytes("assinatura", 64)?;
        let digest: String = Sha256::digest(&bytes).hex();
        ensure!(digest == journal.manifesto_sha256, "manifesto persistido adulterado");
        let manifesto = manifesto::verificar(&bytes, &assinatura, &politica.chave()?)?;
        ensure!(
            manifesto.produto == "mexerica"
                && manifesto.canal == politica.canal
                && manifesto.arquitetura == politica.arquitetura,
            "manifesto persistido incompatível"
        );
        ensure!(
            manifesto.sequencia == journal.sequencia_nova
                && manifesto.sequencia_minima_reversivel <= manifesto.sequencia,
            "sequência divergente"
        );
        Ok(manifesto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atualizacao::{
        estado::{Estado, Fase},
        journal::{Etapa, Journal},
        slots::Slot,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::json;
    use std::{
        fs::{self, OpenOptions},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    };

    #[test]
    fn manifesto_persistido_exige_assinatura_sequencia_e_prazo() -> Result<()> {
        let path = std::env::temp_dir().join(format!("mx365-manifesto-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join("lock"))?;
        let estado = Diretorio::abrir_teste(&path)?;
        let trava = estado.travar()?;
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
        let chave = SigningKey::from_bytes(&seed);
        let politica = json!({"endpoint":"https://exemplo.invalid/", "canal":"estavel", "arquitetura":std::env::consts::ARCH,
            "uid_hub":1234,"gid_hub":1234,"chave_manifesto":B64.encode(chave.verifying_key().as_bytes()),
            "tamanho_maximo":1024,"tempo_rede_segundos":10,"janela_minima":10,"janela_maxima":100,
            "cgroup_memoria":1024,"cgroup_processos":2,"cgroup_cpu_quota":1000,"cgroup_cpu_periodo":1000});
        estado.gravar("politica", &politica)?;
        let artefato = |nome| manifesto::Artefato { nome, tamanho: 10, sha256: "a".repeat(64) };
        let m = Manifesto {
            produto: "mexerica".into(),
            canal: "estavel".into(),
            arquitetura: std::env::consts::ARCH.into(),
            versao: "1.0.0".into(),
            sequencia: 8,
            inicio: 100,
            expiracao: 200,
            hub: artefato("donnabot-host".into()),
            ajudante: artefato("donnabot-motor-exec".into()),
            sequencia_minima_reversivel: 4,
            wrapper_minimo: "0.1.0".into(),
        };
        let bytes = serde_json::to_vec(&m)?;
        let mut mensagem = manifesto::DOMINIO.to_vec();
        mensagem.extend_from_slice(&bytes);
        fs::DirBuilder::new().mode(0o700).create(path.join("transacoes"))?;
        let transacao = estado.criar_transacao(&"c".repeat(64))?;
        transacao.gravar_bytes("manifesto", &bytes)?;
        transacao.gravar_bytes("assinatura", &chave.sign(&mensagem).to_bytes())?;
        let mut journal = Journal {
            estado: Estado { marca: 8, piso: 4, fase: Fase::Pendente { anterior: Some(7), prazo: 180 } },
            etapa: Etapa::Pendente,
            transacao: "c".repeat(64),
            sequencia_nova: 8,
            publicado_em: Some(110),
            sequencia_anterior: Some(7),
            slot_anterior: Some(Slot::A),
            slot_novo: Slot::B,
            manifesto_sha256: Sha256::digest(&bytes).hex(),
            snapshot_sha256: "b".repeat(64),
        };
        assert!(estado.manifesto_transacao(&trava, &journal, 150).is_ok());
        assert!(estado.manifesto_transacao(&trava, &journal, 200).is_err());
        journal.estado.piso = 5;
        assert!(estado.manifesto_transacao(&trava, &journal, 150).is_err());
        journal.estado.piso = 4;
        transacao.gravar_bytes("assinatura", &[0; 64])?;
        assert!(estado.manifesto_transacao(&trava, &journal, 150).is_err());
        fs::remove_dir_all(path)?;
        Ok(())
    }
}
