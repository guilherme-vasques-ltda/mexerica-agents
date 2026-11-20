use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Cursor, ErrorKind, Read, Write as _};
use std::path::{Component, Path, PathBuf};

const MARCADOR: &str = ".installed";
pub(super) const LIMITE_EXTRAIDO: u64 = 512 * 1024 * 1024;
const LIMITE_ENTRADAS: usize = 4096;

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
        diretorio_real(&pasta)?;
    }
    Ok(pasta)
}

fn diretorio_real(pasta: &Path) -> Result<bool> {
    match fs::symlink_metadata(pasta) {
        Ok(meta) => {
            ensure!(meta.is_dir() && pasta.canonicalize()? == pasta, "Pasta de instalação do agente inválida.");
            Ok(true)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn criar_diretorios(pasta: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(pasta)?;
    Ok(())
}

fn preparar_pai(pasta: &Path) -> Result<()> {
    if diretorio_real(pasta)? {
        return Ok(());
    }
    preparar_pai(pasta.parent().context("Pasta de instalação do agente indisponível.")?)?;
    criar_diretorios(pasta)?;
    ensure!(diretorio_real(pasta)?, "Pasta de instalação do agente indisponível.");
    Ok(())
}

fn pasta_privada(pai: &Path, prefixo: &str) -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefixo);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir_in(pai)?)
}

pub(super) fn cache_verificado(destino: &Path, hash: &str, cmd: &Path) -> Result<bool> {
    if !diretorio_real(destino)? {
        return Ok(false);
    }
    let marcador = destino.join(MARCADOR);
    let meta = match fs::symlink_metadata(&marcador) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() || meta.len() != 128 {
        return Ok(false);
    }
    let mut recibo = Vec::with_capacity(128);
    File::open(marcador)?.take(129).read_to_end(&mut recibo)?;
    let Some((compactado, executavel)) = recibo.split_at_checked(64) else { return Ok(false) };
    if !compactado.eq_ignore_ascii_case(hash.as_bytes()) {
        return Ok(false);
    }
    let Some((exe, meta)) = procurar_executavel(destino, cmd)? else { return Ok(false) };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if meta.permissions().mode() & 0o100 == 0 {
            return Ok(false);
        }
    }
    #[cfg(not(unix))]
    let _ = meta;
    // Este recibo detecta mudanças no executável, mas não autentica alguém que também possa reescrevê-lo.
    Ok(hash_executavel(&exe)?.is_some_and(|hash| hash.as_bytes() == executavel))
}

fn hash_executavel(exe: &Path) -> Result<Option<String>> {
    let mut arquivo = File::open(exe)?;
    if arquivo.metadata()?.len() > LIMITE_EXTRAIDO {
        return Ok(None);
    }
    let mut sha = Sha256::new();
    let mut total = 0;
    let mut buffer = [0; 8192];
    loop {
        let n = arquivo.read(&mut buffer)?;
        if n == 0 {
            return Ok(Some(super::digest_hex(&sha.finalize())));
        }
        total += n as u64;
        if total > LIMITE_EXTRAIDO {
            return Ok(None);
        }
        sha.update(&buffer[..n]);
    }
}

pub(super) fn publicar(
    destino: &Path,
    nome: &str,
    bytes: &[u8],
    hash: &str,
    cmd: &Path,
    limite_extraido: u64,
) -> Result<()> {
    ensure!(super::sha256_hex(bytes).eq_ignore_ascii_case(hash), "O SHA-256 do download do agente não confere.");
    let pai = destino.parent().context("Pasta de instalação do agente inválida.")?;
    preparar_pai(pai)?;
    let temporaria = pasta_privada(pai, ".agente-")?;
    let pasta = temporaria.path();
    extrair(nome, bytes, pasta, limite_extraido, LIMITE_ENTRADAS, cmd)?;
    let exe = executavel_regular(pasta, cmd)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissoes = fs::metadata(&exe)?.permissions();
        permissoes.set_mode(permissoes.mode() | 0o100);
        fs::set_permissions(&exe, permissoes)?;
    }
    let hash_exe = hash_executavel(&exe)?.context("O executável do agente excede o limite de tamanho.")?;
    let mut opcoes = fs::OpenOptions::new();
    opcoes.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opcoes.mode(0o600);
    }
    let mut marcador = opcoes.open(pasta.join(MARCADOR))?;
    write!(marcador, "{hash}{hash_exe}")?;
    drop(marcador);
    substituir(pasta, destino, pai)
}

fn substituir(pasta: &Path, destino: &Path, pai: &Path) -> Result<()> {
    ensure!(diretorio_real(pai)?, "Pasta de instalação do agente indisponível.");
    if !diretorio_real(destino)? {
        fs::rename(pasta, destino)?;
        return Ok(());
    }
    let copia = pasta_privada(pai, ".agente-anterior-")?;
    let anterior = copia.path().join("instalacao");
    fs::rename(destino, &anterior)?;
    if let Err(erro) = fs::rename(pasta, destino) {
        if let Err(restaurar) = fs::rename(&anterior, destino) {
            let _ = copia.keep();
            bail!(
                "Falha ao publicar o agente ({erro}) e restaurar a instalação anterior ({restaurar}); a cópia anterior foi preservada."
            );
        }
        return Err(erro).context("Não foi possível publicar a instalação do agente. A anterior foi restaurada.");
    }
    Ok(())
}

fn executavel_regular(pasta: &Path, cmd: &Path) -> Result<PathBuf> {
    procurar_executavel(pasta, cmd)?
        .map(|(exe, _)| exe)
        .context("O executável do agente deve ser um arquivo regular dentro da pasta de instalação, sem links.")
}

fn procurar_executavel(pasta: &Path, cmd: &Path) -> Result<Option<(PathBuf, fs::Metadata)>> {
    let relativo = caminho_relativo(cmd)?;
    let mut caminho = pasta.to_path_buf();
    let mut partes = relativo.components().peekable();
    while let Some(parte) = partes.next() {
        caminho.push(parte);
        let meta = match fs::symlink_metadata(&caminho) {
            Ok(meta) => meta,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if partes.peek().is_none() {
            return if meta.is_file() && caminho.canonicalize()?.starts_with(pasta) {
                Ok(Some((caminho, meta)))
            } else {
                Ok(None)
            };
        }
        if !meta.is_dir() || caminho.canonicalize()? != caminho {
            return Ok(None);
        }
    }
    Ok(None)
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn extrair(nome: &str, bytes: &[u8], pasta: &Path, limite: u64, entradas: usize, cmd: &Path) -> Result<()> {
    let nome = nome.to_ascii_lowercase();
    if nome.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("Arquivo ZIP do agente inválido.")?;
        ensure!(zip.len() <= entradas, "O pacote do agente excede o limite de entradas.");
        let mut restantes = limite;
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
            ensure!(entrada.size() <= restantes, "O pacote do agente excede o limite descomprimido.");
            let diretorio = entrada.is_dir();
            let mut leitor = LeitorLimitado { leitor: &mut entrada, restantes };
            gravar(pasta, &relativo, diretorio, if relativo == cmd { 0 } else { modo }, &mut leitor)?;
            restantes = leitor.restantes;
        }
    } else if nome.ends_with(".tar.gz") || nome.ends_with(".tgz") {
        extrair_tar(flate2::read::MultiGzDecoder::new(bytes), pasta, limite, entradas, cmd)?;
    } else if nome.ends_with(".tar.bz2") || nome.ends_with(".tbz2") {
        extrair_tar(bzip2::read::MultiBzDecoder::new(bytes), pasta, limite, entradas, cmd)?;
    } else if nome.ends_with(".tar.xz") {
        extrair_tar(lzma_rust2::XzReader::new_mem_limit(bytes, true, 128 * 1024), pasta, limite, entradas, cmd)?;
    } else if nome.ends_with(".tar") {
        extrair_tar(bytes, pasta, limite, entradas, cmd)?;
    } else {
        ensure!(bytes.len() as u64 <= limite, "O pacote do agente excede o limite descomprimido.");
        gravar(pasta, &caminho_relativo(cmd)?, false, 0, &mut Cursor::new(bytes))?;
    }
    Ok(())
}

struct LeitorLimitado<R> {
    leitor: R,
    restantes: u64,
}

impl<R: Read> Read for LeitorLimitado<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let maximo = buf.len().min(usize::try_from(self.restantes.saturating_add(1)).unwrap_or(usize::MAX));
        let n = self.leitor.read(&mut buf[..maximo])?;
        self.restantes = self
            .restantes
            .checked_sub(n as u64)
            .ok_or_else(|| io::Error::other("O pacote do agente excede o limite descomprimido."))?;
        Ok(n)
    }
}

fn extrair_tar(leitor: impl Read, pasta: &Path, limite: u64, entradas: usize, cmd: &Path) -> Result<()> {
    let mut tar = tar::Archive::new(LeitorLimitado { leitor, restantes: limite });
    for (indice, entrada) in tar.entries()?.enumerate() {
        ensure!(indice < entradas, "O pacote do agente excede o limite de entradas.");
        let mut entrada = entrada?;
        let tipo = entrada.header().entry_type();
        if tipo.is_pax_global_extensions() {
            continue;
        }
        ensure!(tipo.is_dir() || tipo.is_file(), "O pacote do agente contém link ou arquivo especial.");
        let relativo = caminho_relativo(&entrada.path()?)?;
        let modo = entrada.header().mode()?;
        gravar(pasta, &relativo, tipo.is_dir(), if relativo == cmd { 0 } else { modo }, &mut entrada)?;
    }
    io::copy(&mut tar.into_inner(), &mut io::sink())?;
    Ok(())
}

fn gravar(pasta: &Path, relativo: &Path, diretorio: bool, modo: u32, conteudo: &mut impl Read) -> Result<()> {
    let destino = pasta.join(relativo);
    if diretorio {
        criar_diretorios(&destino)?;
        io::copy(conteudo, &mut io::sink())?;
    } else {
        ensure!(!relativo.as_os_str().is_empty(), "Falta o nome de um arquivo no pacote do agente.");
        criar_diretorios(destino.parent().context("Caminho inválido no pacote do agente.")?)?;
        let mut opcoes = fs::OpenOptions::new();
        opcoes.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opcoes.mode(0o600 | (modo & 0o111));
        }
        #[cfg(not(unix))]
        let _ = modo;
        let mut arquivo = opcoes.open(destino)?;
        io::copy(conteudo, &mut arquivo)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "arquivo_testes.rs"]
mod testes;
