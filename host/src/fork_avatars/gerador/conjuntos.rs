//! Conjuntos gerados: arquivos do usuário fora do repo, referenciados por `assetId`.
//! Diretório `<raiz>/<uuid>/` com `conjunto.json` (manifesto, só ao salvar) e
//! `{64,256}/<quadro>.webp` — os 18 quadros do contrato, sem PNGs intermediários servidos.
use super::{QUADROS, TAMANHOS, falha, id_gerado};
use crate::store::Store;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Manifesto versionado produzido pelo Hub (contrato D2): formato, conjunto fixo
/// de quadros/tamanhos, bytes e sha256 de cada arquivo, versão dos prompts e a
/// referência estável de fallback. Só existe depois de Salvar; trabalho sem
/// manifesto não é conjunto. `nome` é só exibição — nunca vira caminho.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifesto {
    pub formato: u32,
    pub asset_id: String,
    pub nome: String,
    pub criado_em: i64,
    pub prompts: String,
    pub tamanhos: Vec<u32>,
    pub quadros: Vec<String>,
    pub fallback_conjunto_id: String,
    pub arquivos: Vec<Arquivo>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arquivo {
    pub tamanho: u32,
    pub quadro: String,
    pub bytes: u64,
    pub sha256: String,
}

fn raiz(store: &Store) -> PathBuf {
    store.gerados_root.clone()
}

/// Pasta de trabalho do job; vira conjunto quando o manifesto é gravado ao salvar.
pub fn pasta_job(store: &Store, id: &uuid::Uuid) -> PathBuf {
    raiz(store).join(id.to_string())
}

/// Modo privado do diretório e dos arquivos (achado A-01 da revisão #600).
#[cfg(unix)]
pub fn privado(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}
#[cfg(not(unix))]
pub fn privado(_path: &Path) -> Result<()> {
    Ok(())
}

/// Verifica o modo de cada arquivo gerado; 0600 garantido (A-01).
#[cfg(unix)]
fn conferir_modo(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for size in TAMANHOS {
        for quadro in QUADROS {
            let meta = std::fs::metadata(dir.join(size.to_string()).join(format!("{quadro}.webp")))?;
            if meta.permissions().mode() & 0o777 != 0o600 {
                return Err(falha("MODO_ARQUIVO"));
            }
        }
    }
    Ok(())
}
#[cfg(not(unix))]
fn conferir_modo(_dir: &Path) -> Result<()> {
    Ok(())
}

fn manifesto_path(dir: &Path) -> PathBuf {
    dir.join("conjunto.json")
}

fn ler_manifesto(dir: &Path) -> Option<Manifesto> {
    serde_json::from_str(&std::fs::read_to_string(manifesto_path(dir)).ok()?).ok()
}

/// O conjunto existe e está registrado (manifesto presente)? Pasta sem manifesto = trabalho.
pub fn existe(store: &Store, asset_id: &str) -> bool {
    id_gerado(asset_id) && ler_manifesto(&raiz(store).join(asset_id)).is_some()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write as _;
    let digest = sha2::Sha256::digest(data);
    digest.iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Escolha persistida aponta para conjunto registrado; sem apagar referência em leitura.
pub fn conferir_vinculo(store: &Store, cfg: &crate::store::BotConfig) -> Result<()> {
    if let Some(asset_id) = crate::fork_avatars::asset_id_gerado(cfg) {
        anyhow::ensure!(existe(store, asset_id), "conjunto_gerado_inexistente");
    }
    Ok(())
}

/// Conjuntos registrados do usuário, para a galeria do app.
pub fn listar(store: &Store) -> Result<Value> {
    let mut conjuntos = Vec::new();
    let raiz = raiz(store);
    if raiz.is_dir() {
        for entrada in std::fs::read_dir(&raiz)? {
            let dir = entrada?.path();
            let Some(nome) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
            if !dir.is_dir() || !id_gerado(&nome) {
                continue;
            }
            if let Some(m) = ler_manifesto(&dir) {
                conjuntos.push(json!({"id": m.asset_id, "nome": m.nome, "criadoEm": m.criado_em}));
            }
        }
    }
    conjuntos.sort_by_key(|c| c["criadoEm"].as_i64().unwrap_or_default());
    Ok(json!(conjuntos))
}

const MAX_QUADRO: u64 = 512 * 1024;

/// Um quadro servido por ID registrado; ID conhecido não é autorização — o caller é Local.
/// Allowlist fechada de assetId/tamanho/quadro + realpath dentro da raiz.
pub fn quadro(store: &Store, asset_id: &str, tamanho: u32, quadro: &str) -> Result<Vec<u8>> {
    if !id_gerado(asset_id) || !TAMANHOS.contains(&tamanho) || !QUADROS.contains(&quadro) {
        return Err(falha("NAO_ENCONTRADO"));
    }
    let raiz = raiz(store).canonicalize()?;
    let arquivo = raiz.join(asset_id).join(tamanho.to_string()).join(format!("{quadro}.webp"));
    let real = arquivo.canonicalize().map_err(|_| falha("NAO_ENCONTRADO"))?;
    if !real.starts_with(&raiz) || ler_manifesto(&raiz.join(asset_id)).is_none() {
        return Err(falha("NAO_ENCONTRADO"));
    }
    let data = std::fs::read(&real)?;
    if data.len() as u64 > MAX_QUADRO {
        return Err(falha("NAO_ENCONTRADO"));
    }
    Ok(data)
}

/// Salva o trabalho pronto num assetId NOVO (D1: id de job não é id de asset).
/// Confere os 18 arquivos e o modo, renomeia a pasta e grava o manifesto
/// versionado com bytes+sha256; sem manifesto, o diretório novo é órfão limpável.
pub fn salvar(store: &Store, job_id: &uuid::Uuid, nome: &str, fallback: &str) -> Result<Value> {
    let dir = pasta_job(store, job_id);
    if !dir.is_dir() || !crate::fork_avatars::mexeriquinhas::conjunto_catalogo(fallback) {
        return Err(falha("CONJUNTO_INDISPONIVEL"));
    }
    let asset_id = uuid::Uuid::new_v4();
    let mut arquivos = Vec::with_capacity(18);
    for size in TAMANHOS {
        for quadro in QUADROS {
            let arquivo = dir.join(size.to_string()).join(format!("{quadro}.webp"));
            let data = std::fs::read(&arquivo).map_err(|_| falha("CONJUNTO_INDISPONIVEL"))?;
            arquivos.push(Arquivo {
                tamanho: size,
                quadro: quadro.to_owned(),
                bytes: data.len() as u64,
                sha256: sha256_hex(&data),
            });
        }
    }
    conferir_modo(&dir)?;
    let destino = raiz(store).join(asset_id.to_string());
    std::fs::rename(&dir, &destino)?;
    let manifesto = Manifesto {
        formato: 1,
        asset_id: asset_id.to_string(),
        nome: nome.to_owned(),
        criado_em: crate::store::now_ms(),
        prompts: super::PROMPTS_VERSAO.to_owned(),
        tamanhos: TAMANHOS.to_vec(),
        quadros: QUADROS.iter().map(ToString::to_string).collect(),
        fallback_conjunto_id: fallback.to_owned(),
        arquivos,
    };
    let tmp = destino.join("conjunto.json.tmp");
    std::fs::write(&tmp, serde_json::to_string(&manifesto)?)?;
    privado(&tmp)?;
    std::fs::rename(&tmp, manifesto_path(&destino))?;
    Ok(json!({"id": manifesto.asset_id, "nome": manifesto.nome, "criadoEm": manifesto.criado_em}))
}

/// Remove um conjunto registrado. Recusa quando algum bot ainda o referencia.
pub fn apagar(store: &Store, asset_id: &str) -> Result<()> {
    if !id_gerado(asset_id) {
        return Err(falha("NAO_ENCONTRADO"));
    }
    for row in store.bots()? {
        if crate::fork_avatars::asset_id_gerado(&row.config) == Some(asset_id) {
            return Err(falha("CONJUNTO_EM_USO"));
        }
    }
    let dir = raiz(store).join(asset_id);
    if ler_manifesto(&dir).is_none() {
        return Err(falha("NAO_ENCONTRADO"));
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// Trabalho não salvo não sobrevive ao recomeço: pastas sem manifesto são descartadas.
pub fn limpar_orfaos(store: &Store) {
    let raiz = raiz(store);
    if !raiz.is_dir() {
        return;
    }
    if let Ok(entradas) = std::fs::read_dir(&raiz) {
        for entrada in entradas.flatten() {
            let dir = entrada.path();
            if dir.is_dir()
                && dir.file_name().and_then(|n| n.to_str()).is_some_and(id_gerado)
                && ler_manifesto(&dir).is_none()
            {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
}
