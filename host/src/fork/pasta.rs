//! Escolha da pasta de trabalho (spec 14, PR 0). Não é isolamento do shell.

use crate::hub::Hub;
use crate::store::{BotConfig, EntryKind, Lane};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

const LIMITE: usize = 1024;
const SISTEMA: &[&str] = &[
    "/etc",
    "/proc",
    "/sys",
    "/dev",
    "/boot",
    "/root",
    "/var/lib",
    #[cfg(target_os = "macos")]
    "/System",
];

/// Pasta explícita: usada também pelas futuras telas de criação, edição e grupos.
/// `bot_id` só libera o workspace gerenciado daquele bot, nunca o de outro.
pub(crate) fn validar(texto: &str, bot_id: Option<&str>) -> Result<PathBuf> {
    validar_em(texto, bot_id, &crate::service::home(), &crate::service::data_dir())
}

/// Vazio é a intenção existente de alocar o workspace pessoal, não uma pasta explícita.
pub(crate) fn validar_bot(cfg: &BotConfig) -> Result<()> {
    if !cfg.cwd.is_empty() {
        validar(&cfg.cwd, (!cfg.is_group()).then_some(cfg.id.as_str()))?;
    }
    Ok(())
}

/// Só no salvamento; carregar um bot antigo nunca regrava nem substitui sua pasta.
pub(crate) fn normalizar_bot(cfg: &mut BotConfig) -> Result<()> {
    if !cfg.cwd.is_empty() {
        cfg.cwd = validar(&cfg.cwd, (!cfg.is_group()).then_some(cfg.id.as_str()))?
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow::anyhow!("O caminho da pasta precisa ser texto UTF-8 válido."))?;
    }
    Ok(())
}

/// Conserva o reconhecimento de workspaces antigos escritos com o caminho textual
/// (ex.: /var no Mac), além dos novos caminhos canônicos.
pub(crate) fn workspace_proprio(cfg: &BotConfig) -> bool {
    let esperado = crate::agent::workspace::path(&cfg.id);
    Path::new(&cfg.cwd) == esperado || std::fs::canonicalize(esperado).is_ok_and(|real| Path::new(&cfg.cwd) == real)
}

pub(crate) fn aviso(cfg: &BotConfig) -> Option<String> {
    validar_bot(cfg).err().map(|erro| format!("Revise a pasta. {erro} Escolha outra pasta nas configurações do bot."))
}

/// Um aviso no chat por bot/pasta/motivo, inclusive após reiniciar, sem impedir a carga.
pub(crate) fn avisar(hub: &Hub, cfg: &BotConfig) {
    let Some(texto) = aviso(cfg) else { return };
    let hash = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(format!("{}\0{texto}", cfg.cwd).as_bytes()));
    let id = format!("donna-pasta-{}-{hash}", cfg.id);
    if let Err(erro) = hub.add_entry_once(&id, &Lane::main(&cfg.id), EntryKind::Notice, 0, &json!({"text":texto})) {
        tracing::warn!(bot = %cfg.id, %erro, "não foi possível publicar o aviso de pasta");
    }
}

fn validar_em(texto: &str, bot_id: Option<&str>, home: &Path, dados: &Path) -> Result<PathBuf> {
    let caminho = expandir(texto, home)?;
    let caminho = std::fs::canonicalize(caminho).context("A pasta não existe ou não está acessível no Hub.")?;
    let caminho = caminho_principal(caminho);
    conferir_caminho(&caminho)?;
    if !caminho.is_dir() {
        bail!("O caminho escolhido não é uma pasta.");
    }
    validar_destino(caminho, bot_id, home, dados)
}

/// Para a raiz de uma criação: resolve os ancestrais existentes, sem criar nada.
pub(crate) fn validar_criacao(caminho: &Path) -> Result<PathBuf> {
    validar_criacao_em(caminho, &crate::service::home(), &crate::service::data_dir())
}

fn validar_criacao_em(caminho: &Path, home: &Path, dados: &Path) -> Result<PathBuf> {
    let texto = caminho.to_str().context("O caminho da pasta precisa ser texto UTF-8 válido.")?;
    let caminho = resolver_raiz(&expandir(texto, home)?)?;
    conferir_caminho(&caminho)?;
    if caminho.exists() && !caminho.is_dir() {
        bail!("O caminho escolhido não é uma pasta.");
    }
    validar_destino(caminho, None, home, dados)
}

fn conferir_caminho(caminho: &Path) -> Result<()> {
    validar_texto(caminho.to_str().context("O caminho da pasta precisa ser texto UTF-8 válido.")?)
}

fn expandir(texto: &str, home: &Path) -> Result<PathBuf> {
    validar_texto(texto)?;
    let caminho = if texto == "~" {
        home.to_path_buf()
    } else if let Some(resto) = texto.strip_prefix("~/") {
        home.join(resto)
    } else {
        PathBuf::from(texto)
    };
    if !caminho.is_absolute() {
        bail!("Escolha um caminho absoluto para a pasta no Hub.");
    }
    Ok(caminho)
}

fn validar_texto(texto: &str) -> Result<()> {
    if texto.trim().is_empty() {
        bail!("Informe a pasta de trabalho.");
    }
    if texto.len() > LIMITE || texto.chars().any(char::is_control) {
        bail!("O caminho da pasta deve ter no máximo 1024 bytes e não pode conter caracteres de controle.");
    }
    Ok(())
}

fn validar_destino(caminho: PathBuf, bot_id: Option<&str>, home: &Path, dados: &Path) -> Result<PathBuf> {
    let trabalho = crate::fork::motor_exec::configured().then(crate::fork::motor_roots::work);
    validar_destino_em(caminho, bot_id, home, dados, trabalho.as_deref())
}

fn validar_destino_em(
    caminho: PathBuf,
    bot_id: Option<&str>,
    home: &Path,
    dados: &Path,
    trabalho: Option<&Path>,
) -> Result<PathBuf> {
    let home = resolver_raiz(home)?;
    let dados = resolver_raiz(dados)?;
    for nome in [".ssh", ".codex", ".claude", ".config", ".gnupg"] {
        if relativa_protegida(&caminho, &home.join(nome))?.is_some() {
            bail!("Pastas ocultas da pasta pessoal não podem ser usadas como pasta de trabalho.");
        }
    }
    // D520 B0: aliases das famílias privadas continuam negados mesmo quando
    // o destino canônico está fora da home. Inclui Dev, sandboxes e backups.
    for entrada in std::fs::read_dir(&home).context("Não foi possível verificar as pastas protegidas do Hub.")? {
        let entrada = entrada?;
        if [".mexerica", ".donnabot", ".codync"]
            .iter()
            .any(|prefixo| entrada.file_name().to_string_lossy().to_lowercase().starts_with(*prefixo))
            && relativa_protegida(&caminho, &entrada.path())?.is_some()
            && relativa_protegida(&caminho, &dados)?.is_none()
        {
            bail!("Esta pasta contém dados internos do Hub. Escolha uma pasta de trabalho.");
        }
    }
    let relativa_dados = relativa_protegida(&caminho, &dados)?;
    // A exceção é reconhecida no resultado canônico, nunca no texto de um symlink.
    if relativa_dados.as_ref().is_some_and(|resto| gerenciada(&dados.join(resto), &dados, bot_id)) {
        return Ok(caminho);
    }
    if let Some(trabalho) = trabalho {
        let trabalho = resolver_raiz(trabalho)?;
        if let Some(resto) = relativa_protegida(&caminho, &trabalho)?
            && resto.starts_with("bots")
            && gerenciada(&trabalho.join(resto), &trabalho, bot_id)
        {
            return Ok(caminho);
        }
    }
    if caminho.parent().is_none() {
        bail!("A pasta raiz não pode ser usada como pasta de trabalho.");
    }
    if relativa_protegida(&home, &caminho)?.is_some() {
        bail!("A pasta pessoal inteira ou seus ancestrais não podem ser usados. Escolha uma subpasta de trabalho.");
    }
    if relativa_dados.is_some() || relativa_protegida(&dados, &caminho)?.is_some() {
        bail!("Esta pasta contém dados internos do Hub. Escolha uma pasta de trabalho.");
    }
    if let Some(resto) = relativa_protegida(&caminho, &home)?
        && let Some(Component::Normal(nome)) = resto.components().next()
        && nome.to_string_lossy().starts_with('.')
    {
        bail!("Pastas ocultas da pasta pessoal não podem ser usadas como pasta de trabalho.");
    }
    for sistema in SISTEMA {
        let sistema = resolver_raiz(Path::new(sistema))?;
        if caminho.starts_with(&sistema) || sistema.starts_with(&caminho) {
            bail!("Diretórios do sistema não podem ser usados como pasta de trabalho.");
        }
    }
    for usuarios in ["/home", "/Users"] {
        let usuarios = resolver_raiz(Path::new(usuarios))?;
        if caminho.starts_with(&usuarios) && !caminho.starts_with(&home) {
            bail!("Pastas pessoais de outros usuários não podem ser usadas como pasta de trabalho.");
        }
    }
    Ok(caminho)
}

/// Além do texto canônico, reconhece aliases do sistema pelo diretório real.
/// A pasta pessoal só é recusada inteira ou no primeiro nível oculto: seus
/// descendentes de trabalho continuam permitidos, inclusive por um alias.
fn relativa_protegida(caminho: &Path, protegida: &Path) -> Result<Option<PathBuf>> {
    if let Ok(resto) = caminho.strip_prefix(protegida) {
        return Ok(Some(resto.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fn identidade(pasta: &Path) -> Result<Option<(u64, u64)>> {
            match std::fs::metadata(pasta) {
                Ok(meta) => Ok(Some((meta.dev(), meta.ino()))),
                Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(erro) => Err(erro).context("Não foi possível verificar as pastas protegidas do Hub."),
            }
        }
        if let Some(identidade_protegida) = identidade(protegida)? {
            for ancestral in caminho.ancestors() {
                if identidade(ancestral)? == Some(identidade_protegida) {
                    return Ok(Some(caminho.strip_prefix(ancestral)?.to_path_buf()));
                }
            }
        }
    }
    Ok(None)
}

fn caminho_principal(real: PathBuf) -> PathBuf {
    // canonicalize não traduz o firmlink do volume Data para o caminho principal.
    #[cfg(target_os = "macos")]
    if let Ok(resto) = real.strip_prefix("/System/Volumes/Data") {
        return Path::new("/").join(resto);
    }
    real
}

fn gerenciada(caminho: &Path, dados: &Path, bot_id: Option<&str>) -> bool {
    let Ok(resto) = caminho.strip_prefix(dados) else { return false };
    let mut partes = resto.components();
    let (Some(Component::Normal(tipo)), Some(Component::Normal(id)), Some(Component::Normal(pasta))) =
        (partes.next(), partes.next(), partes.next())
    else {
        return false;
    };
    (tipo == "grupos" && pasta == "pasta")
        || (tipo == "bots" && pasta == "workspace" && bot_id.is_some_and(|bot| id == bot))
}

/// Dados configurados e diretórios de sistema podem ainda não existir. Resolve seus
/// ancestrais existentes também (ex.: /var → /private/var no Mac), sem criar nada.
fn resolver_raiz(caminho: &Path) -> Result<PathBuf> {
    if !caminho.is_absolute() {
        return resolver_raiz(
            &std::env::current_dir().context("Não foi possível resolver a pasta do Hub.")?.join(caminho),
        );
    }
    match std::fs::canonicalize(caminho) {
        Ok(real) => Ok(caminho_principal(real)),
        Err(erro) if erro.kind() == std::io::ErrorKind::NotFound && !caminho.is_symlink() => {
            let pai = caminho.parent().context("Não foi possível resolver a pasta protegida do Hub.")?;
            let nome = caminho.file_name().context("Não foi possível resolver o nome da pasta protegida.")?;
            Ok(resolver_raiz(pai)?.join(nome))
        }
        Err(erro) => Err(erro).context("Não foi possível verificar as pastas protegidas do Hub."),
    }
}

#[cfg(test)]
mod tests;
