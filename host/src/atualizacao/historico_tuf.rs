//! Histórico antirrollback do subconjunto TUF; exige trava de atualização.
use super::{
    armazenamento::Diretorio,
    tuf::{self, ResultadoCadeia, VersaoConfiavel},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs::File};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Historico {
    pub raiz: u64,
    pub raiz_base64: String,
    pub versoes: BTreeMap<String, VersaoConfiavel>,
}

impl Historico {
    fn validar(&self) -> Result<()> {
        ensure!(
            self.raiz > 0
                && self.versoes.len() <= 1024
                && !self.raiz_base64.is_empty()
                && self.raiz_base64.len() <= 700_000,
            "histórico TUF inválido"
        );
        for (nome, versao) in &self.versoes {
            ensure!(
                versao.versao > 0
                    && versao.sha256.len() == 64
                    && versao.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && nome.len() <= 128
                    && (nome == "timestamp"
                        || nome == "snapshot"
                        || nome.strip_prefix("meta:").is_some_and(|n| !n.is_empty()
                            && n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')))),
                "histórico TUF inválido"
            );
        }
        Ok(())
    }

    fn conferir_avanco(&self, raiz: u64, novas: &BTreeMap<String, VersaoConfiavel>) -> Result<()> {
        self.validar()?;
        ensure!(raiz >= self.raiz, "rollback da raiz TUF");
        for (nome, antiga) in &self.versoes {
            ensure!(
                novas.get(nome).is_some_and(|nova| nova.versao > antiga.versao
                    || (nova.versao == antiga.versao && nova.sha256 == antiga.sha256)),
                "rollback ou remoção de metadados TUF"
            );
        }
        Historico { raiz, raiz_base64: self.raiz_base64.clone(), versoes: novas.clone() }.validar()
    }
}

impl Diretorio {
    /// O arquivo inicial é criado na etapa administrativa; sua ausência é erro.
    pub fn carregar_historico_tuf(&self, trava: &File) -> Result<Historico> {
        self.validar_trava(trava)?;
        let historico: Historico = self.ler("historico-tuf")?;
        historico.validar()?;
        Ok(historico)
    }

    /// A raiz inicial tem de ser autenticada por canal independente pelo administrador.
    pub fn raiz_tuf(&self, trava: &File, agora: i64) -> Result<tuf::Raiz> {
        let historico = self.carregar_historico_tuf(trava)?;
        let bytes = B64.decode(&historico.raiz_base64).context("raiz codificada inválida")?;
        let raiz = tuf::carregar_raiz_pinada(&bytes, agora)?;
        ensure!(raiz.version == historico.raiz, "versão da raiz não corresponde ao histórico");
        Ok(raiz)
    }

    /// Rotação autenticada (TUF 5.3.4–5.3.11): exige a versão seguinte assinada pelo
    /// quórum da raiz confiável e pelo da nova. Se as chaves de timestamp ou snapshot
    /// mudarem, descarta as versões confiáveis desses dois papéis (recuperação de
    /// fast-forward); as demais versões continuam impedindo rollback.
    /// Persiste raiz e histórico em uma única substituição atômica.
    pub fn atualizar_raiz_tuf(&self, trava: &File, bytes: &[u8]) -> Result<u64> {
        let mut historico = self.carregar_historico_tuf(trava)?;
        let atual = B64.decode(&historico.raiz_base64).context("raiz codificada inválida")?;
        let anterior = tuf::carregar_raiz_confiavel(&atual)?;
        ensure!(anterior.version == historico.raiz, "versão da raiz não corresponde ao histórico");
        let nova = tuf::rotacionar_raiz(&anterior, bytes)?;
        let mut trocou_online = false;
        for papel in ["timestamp", "snapshot"] {
            trocou_online |= anterior.chaves_do_papel(papel)? != nova.chaves_do_papel(papel)?;
        }
        if trocou_online {
            historico.versoes.retain(|nome, _| nome != "timestamp" && nome != "snapshot");
        }
        historico.raiz = nova.version;
        historico.raiz_base64 = B64.encode(bytes);
        historico.validar()?;
        self.gravar("historico-tuf", &historico)?;
        Ok(nova.version)
    }

    /// Somente a etapa administrativa, após conferir o fingerprint por canal
    /// independente: substitui a raiz confiável (ex.: quórum antigo comprometido)
    /// e zera o histórico, que pode ter sido inflado com as chaves antigas.
    pub fn repinar_raiz_tuf(&self, trava: &File, bytes: &[u8], agora: i64) -> Result<u64> {
        let historico = self.carregar_historico_tuf(trava)?;
        let nova = tuf::carregar_raiz_pinada(bytes, agora)?;
        let repinado = Historico { raiz: nova.version, raiz_base64: B64.encode(bytes), versoes: BTreeMap::new() };
        ensure!(nova.version >= historico.raiz, "repinagem não pode retroceder a versão da raiz");
        repinado.validar()?;
        self.gravar("historico-tuf", &repinado)?;
        Ok(nova.version)
    }

    /// Persistir antes de qualquer publicação, sob a mesma trava exclusiva.
    pub fn registrar_historico_tuf(&self, trava: &File, raiz: u64, resultado: &ResultadoCadeia) -> Result<()> {
        let anterior = self.carregar_historico_tuf(trava)?;
        anterior.conferir_avanco(raiz, &resultado.versoes)?;
        ensure!(raiz == anterior.raiz, "raiz não corresponde ao histórico");
        self.gravar(
            "historico-tuf",
            &Historico { raiz, raiz_base64: anterior.raiz_base64, versoes: resultado.versoes.clone() },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(versao: u64, digito: char) -> VersaoConfiavel {
        VersaoConfiavel { versao, sha256: digito.to_string().repeat(64) }
    }
    #[test]
    fn versoes_e_papeis_nao_retrocedem() {
        let anterior = Historico {
            raiz: 3,
            raiz_base64: "e30=".into(),
            versoes: BTreeMap::from([("timestamp".into(), v(4, 'a')), ("meta:targets.json".into(), v(7, 'b'))]),
        };
        let novas = BTreeMap::from([("timestamp".into(), v(5, 'c')), ("meta:targets.json".into(), v(7, 'b'))]);
        assert!(anterior.conferir_avanco(3, &novas).is_ok());
        assert!(anterior.conferir_avanco(2, &novas).is_err());
        assert!(anterior.conferir_avanco(3, &BTreeMap::from([("timestamp".into(), v(5, 'c'))])).is_err());
        assert!(
            anterior
                .conferir_avanco(
                    3,
                    &BTreeMap::from([("timestamp".into(), v(3, 'a')), ("meta:targets.json".into(), v(7, 'b'))])
                )
                .is_err()
        );
        assert!(
            anterior
                .conferir_avanco(
                    3,
                    &BTreeMap::from([("timestamp".into(), v(4, 'f')), ("meta:targets.json".into(), v(7, 'b'))])
                )
                .is_err()
        );
    }

    #[test]
    fn rotacao_persistida_descarta_versoes_online_so_quando_as_chaves_mudam() -> Result<()> {
        use crate::atualizacao::{Hex as _, tuf::canonico};
        use ed25519_dalek::{Signer as _, SigningKey};
        use serde_json::{Value, json};
        use std::{
            fs,
            os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        };
        let chave = || {
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).map_err(|_| anyhow::anyhow!("aleatoriedade indisponível"))?;
            anyhow::Ok(SigningKey::from_bytes(&seed))
        };
        let publica = |k: &SigningKey| json!({"keytype":"ed25519","scheme":"ed25519","keyval":{"public":k.verifying_key().as_bytes().hex()}});
        let assinar = |valor: &Value, chaves: &[(&SigningKey, &str)]| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            canonico(valor, &mut bytes)?;
            let sigs: Vec<Value> =
                chaves.iter().map(|(k, id)| json!({"keyid": id, "sig": k.sign(&bytes).to_bytes().hex()})).collect();
            Ok(serde_json::to_vec(&json!({"signed": valor, "signatures": sigs}))?)
        };
        let (r1, r2, alvo, snap, ts1, ts2) = (chave()?, chave()?, chave()?, chave()?, chave()?, chave()?);
        let raiz = |versao: u64, raiz: &SigningKey, ts: &SigningKey, expira: &str| {
            json!({"_type":"root","spec_version":"1.0.0","version":versao,
            "expires":expira,"keys":{"r":publica(raiz),"t":publica(&alvo),"s":publica(&snap),"ts":publica(ts)},
            "roles":{"root":{"keyids":["r"],"threshold":1},"targets":{"keyids":["t"],"threshold":1},
            "snapshot":{"keyids":["s"],"threshold":1},"timestamp":{"keyids":["ts"],"threshold":1}}})
        };
        let path = std::env::temp_dir().join(format!("mx365-rotacao-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(path.join("lock"))?;
        let dir = Diretorio::abrir_teste(&path)?;
        let trava = dir.travar()?;
        // Raiz 1 já vencida: a rotação parte dela mesmo assim.
        let v1 = assinar(&raiz(1, &r1, &ts1, "2020-01-01T00:00:00Z"), &[(&r1, "r")])?;
        dir.gravar(
            "historico-tuf",
            &Historico {
                raiz: 1,
                raiz_base64: B64.encode(&v1),
                versoes: BTreeMap::from([
                    ("timestamp".into(), v(9, 'a')),
                    ("snapshot".into(), v(9, 'b')),
                    ("meta:targets.json".into(), v(9, 'c')),
                ]),
            },
        )?;
        // Só o quórum novo, sem o antigo: recusada.
        let so_nova = assinar(&raiz(2, &r2, &ts1, "2099-01-01T00:00:00Z"), &[(&r2, "r")])?;
        assert!(dir.atualizar_raiz_tuf(&trava, &so_nova).is_err());
        // Versão pulada: recusada.
        let pulada = assinar(&raiz(3, &r1, &ts1, "2099-01-01T00:00:00Z"), &[(&r1, "r")])?;
        assert!(dir.atualizar_raiz_tuf(&trava, &pulada).is_err());
        // Troca da raiz com as mesmas chaves online: preserva as versões.
        // Os dois quóruns assinam sob o mesmo keyid "r": cada raiz só aceita a própria chave.
        let v2 = assinar(&raiz(2, &r2, &ts1, "2099-01-01T00:00:00Z"), &[(&r1, "r"), (&r2, "r")])?;
        assert_eq!(dir.atualizar_raiz_tuf(&trava, &v2)?, 2);
        assert!(dir.carregar_historico_tuf(&trava)?.versoes.contains_key("timestamp"));
        // Rotação da chave de timestamp: descarta timestamp/snapshot, mantém o resto.
        let v3 = assinar(&raiz(3, &r2, &ts2, "2099-01-01T00:00:00Z"), &[(&r2, "r")])?;
        assert_eq!(dir.atualizar_raiz_tuf(&trava, &v3)?, 3);
        let historico = dir.carregar_historico_tuf(&trava)?;
        assert!(!historico.versoes.contains_key("timestamp") && !historico.versoes.contains_key("snapshot"));
        assert!(historico.versoes.contains_key("meta:targets.json"));
        assert!(dir.raiz_tuf(&trava, 1_800_000_000).is_ok());
        fs::remove_dir_all(path)?;
        Ok(())
    }
}
