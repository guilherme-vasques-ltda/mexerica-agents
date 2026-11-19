//! Bundle curado: nenhuma rede, resolução de tags ou execução de conteúdo externo.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) const MAX_SKILL: usize = 32 * 1024;
const MAX_FILE: usize = 1024 * 1024;
const MAX_PACKAGE: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Origin {
    pub repo: String,
    pub revision: String,
    pub path: String,
    pub source_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileInfo {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Readiness {
    pub enabled: bool,
    pub runtime: String,
    pub reason: String,
    pub unavailable_actions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    pub id: String,
    pub source: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub aliases: Vec<String>,
    pub origin: Origin,
    pub license: String,
    pub availability: String,
    pub reason: String,
    pub requirements: Vec<String>,
    pub losses: Vec<String>,
    pub adaptation: Option<String>,
    pub readiness: Option<Readiness>,
    pub files: Vec<FileInfo>,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Asset {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
    pub mode: String,
    pub content: String,
}

impl Asset {
    pub fn info(&self) -> FileInfo {
        FileInfo { path: self.path.clone(), bytes: self.bytes, sha256: self.sha256.clone(), mode: self.mode.clone() }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Package {
    pub metadata: Metadata,
    pub install_id: String,
    pub assets: Vec<Asset>,
}

pub(super) fn hash(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes).iter().fold(String::with_capacity(64), |mut s, byte| {
        let _ = write!(s, "{byte:02x}");
        s
    })
}

/// JSON UTF-8, chaves ordenadas recursivamente, sem espaços; números do manifesto são inteiros.
pub(super) fn canonical(v: &Value) -> Result<String> {
    Ok(match v {
        Value::Array(items) => format!("[{}]", items.iter().map(canonical).collect::<Result<Vec<_>>>()?.join(",")),
        Value::Object(obj) => {
            let mut keys: Vec<_> = obj.keys().collect();
            keys.sort();
            let pairs = keys
                .iter()
                .map(|key| Ok(format!("{}:{}", serde_json::to_string(key)?, canonical(&obj[*key])?)))
                .collect::<Result<Vec<_>>>()?;
            format!("{{{}}}", pairs.join(","))
        }
        _ => serde_json::to_string(v)?,
    })
}

pub(super) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
}

pub(super) fn safe_path(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    !path.is_empty()
        && path.len() <= 240
        && parts.len() <= 5
        && parts.iter().all(|p| {
            !p.is_empty()
                && !p.starts_with('.')
                && p.len() <= 100
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

fn hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Package {
    pub fn installable(&self) -> bool {
        matches!(self.metadata.availability.as_str(), "text" | "adapted")
    }

    pub fn envelope_digest(&self) -> Result<String> {
        let mut meta = serde_json::to_value(&self.metadata)?;
        if let Some(obj) = meta.as_object_mut() {
            obj.remove("digest");
        }
        Ok(hash(
            canonical(
                &serde_json::json!({"metadata": meta, "installId": self.install_id, "files": self.metadata.files}),
            )?
            .as_bytes(),
        ))
    }

    pub fn validate(&self) -> Result<()> {
        let m = &self.metadata;
        ensure!(safe_id(&m.id) && safe_id(&self.install_id) && m.source == m.id, "Identidade curada inválida");
        ensure!(hex(&m.digest, 64) && self.envelope_digest()? == m.digest, "Digest do envelope inválido");
        ensure!(!m.name.is_empty() && !m.description.is_empty() && !m.reason.is_empty(), "Metadados incompletos");
        ensure!(
            matches!(m.availability.as_str(), "text" | "adapted" | "dependencyUnavailable" | "licenseUnavailable"),
            "Disponibilidade inválida"
        );
        if let Some(r) = &m.readiness {
            ensure!(
                !r.enabled && r.runtime == "unavailable" && !r.reason.is_empty(),
                "Readiness operacional não autorizada"
            );
        }
        if !self.installable() {
            ensure!(self.assets.is_empty() && m.files.is_empty(), "Entrada indisponível deve conter apenas metadados");
            return Ok(());
        }
        ensure!(hex(&m.origin.revision, 40) && hex(&m.origin.source_digest, 64), "Origem sem pin/digest");
        ensure!(
            m.origin.repo.split('/').count() == 2
                && safe_path(m.origin.path.strip_prefix(".agents/").unwrap_or(&m.origin.path)),
            "Origem não curada"
        );
        ensure!(matches!(m.license.as_str(), "MIT" | "Apache-2.0" | "ISC"), "Licença não liberada para este recorte");
        ensure!(
            !self.assets.is_empty() && self.assets.len() <= 100 && self.assets.len() == m.files.len(),
            "Manifesto de arquivos inválido"
        );
        if m.availability == "adapted" {
            ensure!(m.adaptation.is_some() && !m.losses.is_empty(), "Derivado sem revisão/perdas explícitas");
        }
        let mut paths = BTreeSet::new();
        let mut total = 0_usize;
        for (asset, info) in self.assets.iter().zip(&m.files) {
            ensure!(
                safe_path(&asset.path) && paths.insert(asset.path.to_ascii_lowercase()),
                "Path inválido/duplicado no pacote"
            );
            ensure!(asset.mode == "100644" && asset.info() == *info, "Tipo ou manifesto de arquivo inválido");
            let ext = asset.path.rsplit('.').next().unwrap_or("");
            ensure!(matches!(ext, "md" | "txt" | "json") || asset.path == "LICENSE", "Arquivo ativo não permitido");
            total = total.checked_add(asset.bytes).ok_or_else(|| anyhow::anyhow!("Limite de pacote"))?;
            ensure!(
                asset.bytes <= MAX_FILE && total <= MAX_PACKAGE && asset.content.len() == asset.bytes,
                "Limite/tamanho do pacote inválido"
            );
            ensure!(
                !asset.content.contains('\0') && hash(asset.content.as_bytes()) == asset.sha256,
                "Conteúdo/hash de arquivo inválido"
            );
        }
        ensure!(self.assets.iter().any(|a| a.path.starts_with("LICENSE")), "Licença ausente no pacote");
        let md =
            self.assets.iter().find(|a| a.path == "SKILL.md").ok_or_else(|| anyhow::anyhow!("SKILL.md ausente"))?;
        ensure!(md.bytes <= MAX_SKILL, "SKILL.md excede 32 KiB");
        let document = crate::fork::agent_skills::parse(&md.content, &self.install_id)?;
        ensure!(
            document.name == self.install_id && document.name == m.name && document.description == m.description,
            "Identidade/descrição do catálogo deve preservar o SKILL.md original"
        );
        Ok(())
    }
}

pub(super) fn packages() -> Result<Vec<Package>> {
    let mut all: Vec<Package> = serde_json::from_str(include_str!("registry.json"))?;
    all.push(super::criador_de_skills::package()?);
    all.extend(super::conectores::packages()?);
    let mut identities = BTreeSet::new();
    for p in &all {
        p.validate()?;
        for id in std::iter::once(&p.metadata.id).chain(&p.metadata.aliases) {
            ensure!(safe_id(id) && identities.insert(id.clone()), "Identidade/alias duplicado no catálogo");
        }
    }
    Ok(all)
}

pub(super) fn find(id: &str) -> Result<Package> {
    packages()?
        .into_iter()
        .find(|p| p.metadata.id == id || p.metadata.aliases.iter().any(|alias| alias == id))
        .ok_or_else(|| anyhow::anyhow!("Habilidade fora do catálogo curado"))
}

/// Fotografias de envelopes já distribuídos, apenas para ler instalações com recibo.
/// Nunca são candidatas a install/seed; não incluem os corpos das versões anteriores.
pub(super) fn captured(id: &str, digest: &str) -> Result<Option<Package>> {
    if let Some(p) = packages()?.into_iter().find(|p| p.metadata.id == id && p.metadata.digest == digest) {
        return Ok(Some(p));
    }
    let historical: Vec<Package> = serde_json::from_str(include_str!("legacy-envelopes.json"))?;
    for p in historical {
        ensure!(p.assets.is_empty() && p.envelope_digest()? == p.metadata.digest, "Envelope histórico inválido");
        if p.metadata.id == id && p.metadata.digest == digest {
            return Ok(Some(p));
        }
    }
    // O recibo capturado continua disponível mesmo se um catálogo futuro retirar a entrada.
    Ok(None)
}
