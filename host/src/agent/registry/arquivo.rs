use anyhow::{Context, Result, bail, ensure};
use std::fs::{self, File};
use std::io::{self, Cursor, ErrorKind, Read, Write as _};
use std::path::{Component, Path, PathBuf};

const MARCADOR: &str = ".installed";

pub(super) fn caminho_relativo(caminho: &Path) -> Result<PathBuf> {
    let mut relativo = PathBuf::new();
    for parte in caminho.components() {
        if parte == Component::CurDir {
            continue;
        }
        let Component::Normal(nome) = parte else { bail!("Caminho fora da pasta de instalação do agente.") };
        let nome = nome.to_str().context("Nome de arquivo inválido no pacote do agente.")?;
        ensure!(
            !nome.ends_with([' ', '.']) && !nome.chars().any(|c| c.is_control() || "\\:*?\"<>|".contains(c)),
            "Caminho inválido no pacote do agente."
        );
        if cfg!(windows) {
            let antes_do_ponto = nome.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
            ensure!(
                !matches!(antes_do_ponto.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
                    && !antes_do_ponto.strip_prefix("COM").or_else(|| antes_do_ponto.strip_prefix("LPT")).is_some_and(
                        |n| matches!(n, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                    ),
                "Nome reservado no pacote do agente."
            );
        }
        relativo.push(nome);
    }
    Ok(relativo)
}

pub(super) fn pasta_instalacao(raiz: &Path, id: &str, versao: &str) -> Result<PathBuf> {
    let mut pasta = raiz.canonicalize().context("Pasta de dados do agente indisponível.")?;
    for parte in ["agents", id, versao] {
        pasta.push(caminho_relativo(Path::new(parte))?);
        match fs::symlink_metadata(&pasta) {
            Ok(meta) => {
                ensure!(meta.is_dir() && pasta.canonicalize()? == pasta, "Pasta de instalação do agente inválida.");
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(pasta)
}

pub(super) fn cache_verificado(destino: &Path, hash: &str, cmd: &Path) -> Result<bool> {
    let marcador = destino.join(MARCADOR);
    let meta = match fs::symlink_metadata(&marcador) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() || meta.len() != 64 || !fs::read(marcador)?.eq_ignore_ascii_case(hash.as_bytes()) {
        return Ok(false);
    }
    let exe = executavel_regular(destino, cmd)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        ensure!(fs::metadata(exe)?.permissions().mode() & 0o111 != 0, "O arquivo do agente não é executável.");
    }
    #[cfg(not(unix))]
    drop(exe);
    Ok(true)
}

pub(super) fn publicar(destino: &Path, nome: &str, bytes: &[u8], hash: &str, cmd: &Path) -> Result<()> {
    ensure!(super::sha256_hex(bytes).eq_ignore_ascii_case(hash), "O SHA-256 do download do agente não confere.");
    let pai = destino.parent().context("Pasta de instalação do agente inválida.")?;
    fs::create_dir_all(pai)?;
    let temporaria = tempfile::Builder::new().prefix(".agente-").tempdir_in(pai)?;
    let pasta = temporaria.path();
    extrair(nome, bytes, pasta, cmd)?;
    let exe = executavel_regular(pasta, cmd)?;
    #[cfg(unix)]
    super::chmod_x(&exe)?;
    #[cfg(not(unix))]
    drop(exe);
    File::create_new(pasta.join(MARCADOR))?.write_all(hash.as_bytes())?;
    match fs::remove_dir_all(destino) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    fs::rename(pasta, destino)?;
    Ok(())
}

fn executavel_regular(pasta: &Path, cmd: &Path) -> Result<PathBuf> {
    let relativo = caminho_relativo(cmd)?;
    let exe = pasta.join(&relativo);
    let meta = fs::symlink_metadata(&exe).context("O pacote não contém o executável do agente.")?;
    let raiz = pasta.canonicalize()?;
    for parte in relativo.ancestors().skip(1).take_while(|p| !p.as_os_str().is_empty()) {
        let dir = pasta.join(parte);
        ensure!(
            fs::symlink_metadata(&dir)?.is_dir() && dir.canonicalize()?.starts_with(&raiz),
            "O caminho do executável do agente contém um link ou sai da pasta de instalação."
        );
    }
    ensure!(
        meta.is_file() && exe.canonicalize()?.starts_with(&raiz),
        "O executável do agente deve ser um arquivo regular dentro da pasta de instalação, sem links."
    );
    Ok(exe)
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn extrair(nome: &str, bytes: &[u8], pasta: &Path, cmd: &Path) -> Result<()> {
    let nome = nome.to_ascii_lowercase();
    if nome.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("Arquivo ZIP do agente inválido.")?;
        for indice in 0..zip.len() {
            let mut entrada = zip.by_index(indice)?;
            let relativo = caminho_relativo(Path::new::<str>(&entrada.name()?))?;
            let modo = entrada.unix_mode().unwrap_or(0);
            let tipo = modo & 0o170_000;
            ensure!(
                !entrada.is_symlink()
                    && (entrada.is_dir() && matches!(tipo, 0 | 0o040_000)
                        || entrada.is_file() && matches!(tipo, 0 | 0o100_000)),
                "O pacote do agente contém link ou arquivo especial."
            );
            gravar(pasta, &relativo, entrada.is_dir(), modo, cmd, &mut entrada)?;
        }
    } else if nome.ends_with(".tar.gz") || nome.ends_with(".tgz") {
        extrair_tar(flate2::read::MultiGzDecoder::new(bytes), pasta, cmd)?;
    } else if nome.ends_with(".tar.bz2") || nome.ends_with(".tbz2") {
        extrair_tar(bzip2::read::MultiBzDecoder::new(bytes), pasta, cmd)?;
    } else if nome.ends_with(".tar.xz") {
        extrair_tar(lzma_rust2::XzReader::new(bytes, true), pasta, cmd)?;
    } else if nome.ends_with(".tar") {
        extrair_tar(bytes, pasta, cmd)?;
    } else {
        gravar(pasta, &caminho_relativo(cmd)?, false, 0, cmd, &mut Cursor::new(bytes))?;
    }
    Ok(())
}

fn extrair_tar(leitor: impl Read, pasta: &Path, cmd: &Path) -> Result<()> {
    for entrada in tar::Archive::new(leitor).entries()? {
        let mut entrada = entrada?;
        let tipo = entrada.header().entry_type();
        ensure!(tipo.is_dir() || tipo.is_file(), "O pacote do agente contém link ou arquivo especial.");
        let relativo = caminho_relativo(&entrada.path()?)?;
        let modo = entrada.header().mode()?;
        gravar(pasta, &relativo, tipo.is_dir(), modo, cmd, &mut entrada)?;
    }
    Ok(())
}

fn gravar(
    pasta: &Path,
    relativo: &Path,
    diretorio: bool,
    modo: u32,
    cmd: &Path,
    conteudo: &mut impl Read,
) -> Result<()> {
    let destino = pasta.join(relativo);
    if diretorio {
        fs::create_dir_all(destino)?;
    } else {
        ensure!(!relativo.as_os_str().is_empty(), "Falta o nome de um arquivo no pacote do agente.");
        fs::create_dir_all(destino.parent().context("Caminho inválido no pacote do agente.")?)?;
        let mut arquivo = File::create_new(destino)?;
        io::copy(conteudo, &mut arquivo)?;
        #[cfg(unix)]
        if relativo != cmd && modo & 0o111 != 0 {
            use std::os::unix::fs::PermissionsExt as _;
            arquivo.set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(not(unix))]
        let _ = (modo, cmd);
    }
    Ok(())
}

#[cfg(test)]
#[path = "arquivo_testes.rs"]
mod testes;
