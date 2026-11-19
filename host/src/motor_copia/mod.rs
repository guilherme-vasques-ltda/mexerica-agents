//! Canal de cópia executado como o usuário motor (MX-365, passo 6 do desenho).
//!
//! O Hub lê as origens com as próprias credenciais e envia um fluxo; o serviço,
//! executado como `donnabot-motor` e ativado por socket do systemd, grava abaixo
//! de um destino fixo da raiz de contas do motor. A única operação recebe uma raiz
//! lógica enumerada (conta + caminho relativo validado por componente). Recusa
//! links, especiais, travessia, destinos existentes divergentes e qualquer comando;
//! grava em área temporária e só publica com `rename` sem substituição depois de
//! conferir o hash do manifesto. Transferência incompleta é descartada.
#![allow(dead_code)]
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    ffi::{CString, OsStr},
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt, net::UnixStream},
    },
    path::Path,
};

pub const SOCKET: &str = "/run/donnabot-motor-copia.sock";
pub const MOTOR: &str = "/var/lib/donnabot/motor";
/// Raiz fixa das contas do motor (`motor_roots::accounts()`).
pub const CONTAS: [&str; 2] = ["engine-setup", "accounts"];
const MAX_ARQUIVOS: u64 = 200_000;
/// Pastas + arquivos por transferência (cota de inodes).
const MAX_ENTRADAS: u64 = 250_000;
/// Inatividade máxima por leitura e duração máxima de uma transferência.
const OCIOSO: std::time::Duration = std::time::Duration::from_secs(30);
const DURACAO_MAXIMA: std::time::Duration = std::time::Duration::from_secs(2 * 3600);
/// Registro das raízes F3 já autorizadas, na raiz de contas do motor.
const REGISTRO: &str = ".donnabot-raizes";
/// Stagings anotados antes de criados (`pai relativo<TAB>nome`): só estes são coletados.
const STAGING: &str = ".donnabot-staging";
/// Marca da home de perfil gerida (`engine_accounts::managed::new_profile`).
const MARCA_PERFIL: &str = ".donnabot-profile";
const MAX_REGISTRO: u64 = 1024 * 1024;
const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_LINHA: u64 = 8192;
const MAX_COMPONENTES: usize = 64;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cabecalho {
    pub versao: u32,
    pub conta: String,
    pub relativo: String,
    pub raiz: TipoRaiz,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoRaiz {
    Pasta,
    Arquivo,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entrada {
    Pasta { caminho: String },
    Arquivo { caminho: String, tamanho: u64, executavel: bool },
    Fim { sha256: String },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resposta {
    pub ok: bool,
    pub arquivos: u64,
    pub bytes: u64,
    pub sha256: String,
    pub ja_existia: bool,
    pub erro: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}

/// Hash do manifesto na ordem de envio; mesmo cálculo nos dois lados.
#[derive(Default)]
pub struct Manifesto {
    hash: Sha256,
    pub pastas: u64,
    pub arquivos: u64,
    pub bytes: u64,
}
impl Manifesto {
    fn parte(&mut self, texto: &[u8]) {
        self.hash.update((texto.len() as u64).to_be_bytes());
        self.hash.update(texto);
    }
    pub fn pasta(&mut self, caminho: &str) {
        self.parte(b"pasta");
        self.parte(caminho.as_bytes());
        self.pastas += 1;
    }
    pub fn arquivo(&mut self, caminho: &str, tamanho: u64, executavel: bool, conteudo: &[u8]) {
        self.parte(b"arquivo");
        self.parte(caminho.as_bytes());
        self.parte(&tamanho.to_be_bytes());
        self.parte(&[u8::from(executavel)]);
        self.parte(conteudo);
        self.arquivos += 1;
        self.bytes += tamanho;
    }
    pub fn fim(self) -> String {
        hex(&self.hash.finalize())
    }
}

pub fn componente(texto: &str) -> Result<()> {
    ensure!(
        !texto.is_empty() && texto.len() <= 255 && texto != "." && texto != ".." && !texto.contains(['/', '\0']),
        "componente de caminho inválido"
    );
    Ok(())
}

/// Caminho relativo validado por componente; vazio só quando permitido.
pub fn partes(relativo: &str, vazio: bool) -> Result<Vec<&str>> {
    if relativo.is_empty() {
        ensure!(vazio, "caminho relativo vazio");
        return Ok(Vec::new());
    }
    ensure!(relativo.len() <= 4096, "caminho grande demais");
    let partes: Vec<&str> = relativo.split('/').collect();
    ensure!(partes.len() <= MAX_COMPONENTES, "caminho profundo demais");
    for parte in &partes {
        componente(parte)?;
    }
    Ok(partes)
}

/// Mesmo formato do registro de contas do motor (`ac_` + 64 hexadecimais).
pub fn conta_valida(conta: &str) -> Result<()> {
    ensure!(
        conta.len() == 67 && conta.starts_with("ac_") && conta[3..].bytes().all(|b| b.is_ascii_hexdigit()),
        "conta inválida"
    );
    Ok(())
}

// ---------------------------------------------------------------- servidor (motor)

const DIVERGENTE: &str = "destino já existe com conteúdo diferente";

pub fn c(nome: &str) -> Result<CString> {
    Ok(CString::new(nome)?)
}

pub fn arquivo(fd: libc::c_int) -> Result<File> {
    ensure!(fd >= 0, "{}", std::io::Error::last_os_error());
    // SAFETY: descritor novo devolvido pelo kernel, posse única transferida ao File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Sem nenhuma permissão para grupo e outros (bits 0o077 zerados).
const fn privado(modo: u32) -> bool {
    modo.trailing_zeros() >= 6
}

fn euid() -> u32 {
    // SAFETY: consulta sem efeitos colaterais.
    unsafe { libc::geteuid() }
}

/// Abre (ou cria 0700) um diretório filho sem seguir links e exige dono = euid.
fn subdir(pai: &File, nome: &str, criar: bool) -> Result<File> {
    componente(nome)?;
    let nome_c = c(nome)?;
    if criar {
        // SAFETY: componente validado, diretório pai aberto; mkdirat não segue links.
        let rc = unsafe { libc::mkdirat(pai.as_raw_fd(), nome_c.as_ptr(), 0o700) };
        ensure!(rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST), "mkdir recusado");
    }
    // SAFETY: componente validado relativo ao pai, sem seguir links.
    let dir = arquivo(unsafe {
        libc::openat(
            pai.as_raw_fd(),
            nome_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    let meta = dir.metadata()?;
    ensure!(meta.is_dir() && meta.uid() == euid() && meta.mode() & 0o022 == 0, "diretório do motor inseguro");
    Ok(dir)
}

/// Arquivo regular existente do motor, sem seguir links: dono = euid, um só vínculo, privado.
fn abrir_regular(pai: &File, nome: &str) -> Result<File> {
    componente(nome)?;
    let nome_c = c(nome)?;
    // SAFETY: componente validado relativo ao pai, sem seguir links nem bloquear.
    let existente = arquivo(unsafe {
        libc::openat(
            pai.as_raw_fd(),
            nome_c.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    })?;
    let meta = existente.metadata()?;
    ensure!(
        meta.is_file() && meta.uid() == euid() && meta.nlink() == 1 && privado(meta.mode()),
        "arquivo do motor inseguro"
    );
    Ok(existente)
}

/// Metadados de uma entrada sem seguir links; `None` se ausente.
pub fn estado(pai: &File, nome: &str) -> Result<Option<libc::stat>> {
    let nome = c(nome)?;
    // SAFETY: estrutura local inicializada; não segue o objeto consultado.
    let mut meta: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: descritor e ponteiros válidos.
    if unsafe { libc::fstatat(pai.as_raw_fd(), nome.as_ptr(), &raw mut meta, libc::AT_SYMLINK_NOFOLLOW) } == 0 {
        return Ok(Some(meta));
    }
    ensure!(std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT), "destino inacessível");
    Ok(None)
}

fn existe(pai: &File, nome: &str) -> Result<bool> {
    Ok(estado(pai, nome)?.is_some())
}

/// Nomes de um diretório aberto (pelo próprio descritor, sem resolver caminho).
pub fn nomes(dir: &File) -> Result<Vec<String>> {
    let mut nomes = Vec::new();
    for entrada in std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))? {
        let nome = entrada?.file_name().into_string().map_err(|_| anyhow::anyhow!("nome não UTF-8 no motor"))?;
        nomes.push(nome);
        ensure!(nomes.len() as u64 <= MAX_ARQUIVOS, "diretório grande demais");
    }
    Ok(nomes)
}

/// Torna a árvore durável de baixo para cima (arquivos, subpastas e a própria pasta)
/// e devolve o número de entradas; recusa links e especiais.
fn sincronizar_arvore(dir: &File, profundidade: usize) -> Result<u64> {
    ensure!(profundidade <= MAX_COMPONENTES, "árvore profunda demais");
    let mut total = 0u64;
    for nome in nomes(dir)? {
        let meta = estado(dir, &nome)?.context("entrada desapareceu durante a conferência")?;
        match meta.st_mode & libc::S_IFMT {
            libc::S_IFDIR => total += 1 + sincronizar_arvore(&subdir(dir, &nome, false)?, profundidade + 1)?,
            libc::S_IFREG => {
                abrir_regular(dir, &nome)?.sync_all()?;
                total += 1;
            }
            _ => bail!("link ou arquivo especial na árvore do motor"),
        }
    }
    dir.sync_all()?;
    Ok(total)
}

/// Remove por descritor, sem seguir links, uma entrada do próprio UID (recursivamente).
/// Cada pasta aberta precisa ser o mesmo inode conferido antes de abrir.
pub fn remover_em(pai: &File, nome: &str, profundidade: usize) -> Result<()> {
    componente(nome)?;
    ensure!(profundidade <= MAX_COMPONENTES, "árvore profunda demais");
    let Some(meta) = estado(pai, nome)? else { return Ok(()) };
    ensure!(meta.st_uid == euid(), "entrada de outro dono não é removida: {nome}");
    let nome_c = c(nome)?;
    let flag = if meta.st_mode & libc::S_IFMT == libc::S_IFDIR {
        // SAFETY: componente validado relativo ao pai, sem seguir links.
        let dir = arquivo(unsafe {
            libc::openat(
                pai.as_raw_fd(),
                nome_c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let aberto = dir.metadata()?;
        ensure!(aberto.dev() == meta.st_dev && aberto.ino() == meta.st_ino, "entrada trocada durante a remoção");
        for filho in nomes(&dir)? {
            remover_em(&dir, &filho, profundidade + 1)?;
        }
        libc::AT_REMOVEDIR
    } else {
        0
    };
    // SAFETY: componente validado relativo ao descritor do pai.
    let rc = unsafe { libc::unlinkat(pai.as_raw_fd(), nome_c.as_ptr(), flag) };
    ensure!(rc == 0, "{}", std::io::Error::last_os_error());
    Ok(())
}

/// Nome de staging gerado por este código: `.copia-` + 32 hexadecimais minúsculos.
pub fn nome_de_staging(nome: &str) -> bool {
    nome.strip_prefix(".copia-").is_some_and(|resto| {
        resto.len() == 32 && resto.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Acrescenta uma linha a um arquivo privado de `dir`, de forma durável.
pub fn acrescentar(dir: &File, nome: &str, linha: &str) -> Result<()> {
    let nome_c = c(nome)?;
    // SAFETY: nome fixo no diretório aberto, sem seguir links; acrescenta apenas.
    let mut arquivo_ = arquivo(unsafe {
        libc::openat(
            dir.as_raw_fd(),
            nome_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    })?;
    let meta = arquivo_.metadata()?;
    ensure!(meta.is_file() && meta.uid() == euid() && meta.nlink() == 1 && privado(meta.mode()), "{nome} inseguro");
    arquivo_.write_all(format!("{linha}\n").as_bytes())?;
    arquivo_.sync_all()?;
    dir.sync_all()?;
    Ok(())
}

/// Lê as linhas de um arquivo privado de `dir` (vazio se ausente).
pub fn ler_linhas(dir: &File, nome: &str) -> Result<Vec<String>> {
    if !existe(dir, nome)? {
        return Ok(Vec::new());
    }
    let arquivo_ = abrir_regular(dir, nome)?;
    ensure!(arquivo_.metadata()?.len() <= MAX_REGISTRO, "{nome} grande demais");
    let mut texto = String::new();
    arquivo_.take(MAX_REGISTRO).read_to_string(&mut texto)?;
    Ok(texto.lines().map(str::to_owned).collect())
}

/// Coleta stagings de tentativas mortas (queda, reboot) anotados em `lista` (no diretório
/// confiável `lista_dir`), com caminhos relativos a `base`: só nomes de staging anotados antes de criados, abertos por descritor a partir
/// de `base` e removidos sem seguir links. Pastas com nomes parecidos e não anotadas
/// nunca são tocadas. O chamador garante exclusividade (serviço serial ou trava).
pub fn coletar_orfaos(base: &File, lista_dir: &File, lista: &str) -> Result<()> {
    for linha in ler_linhas(lista_dir, lista)? {
        let (relativo, nome) = linha.split_once('\t').context("anotação de staging inválida")?;
        ensure!(nome_de_staging(nome), "anotação de staging inválida");
        let mut dir = base.try_clone()?;
        let mut alcancado = true;
        for parte in partes(relativo, true)? {
            match estado(&dir, parte)? {
                Some(meta) if meta.st_mode & libc::S_IFMT == libc::S_IFDIR => {
                    let objeto = abrir_em(&dir, parte)?;
                    dir = objeto.arquivo;
                }
                _ => {
                    alcancado = false;
                    break;
                }
            }
        }
        if alcancado && existe(&dir, nome)? {
            println!("staging órfão de tentativa interrompida removido: {relativo}/{nome}");
            remover_em(&dir, nome, 0)?;
            dir.sync_all()?;
        }
    }
    if existe(lista_dir, lista)? {
        let nome_c = c(lista)?;
        // SAFETY: nome fixo no diretório aberto; remove só a lista já processada.
        let rc = unsafe { libc::unlinkat(lista_dir.as_raw_fd(), nome_c.as_ptr(), 0) };
        ensure!(rc == 0, "lista de staging não removida");
        lista_dir.sync_all()?;
    }
    Ok(())
}

fn linha<R: BufRead>(leitor: &mut R) -> Result<String> {
    let mut texto = String::new();
    leitor.take(MAX_LINHA).read_line(&mut texto)?;
    ensure!(texto.ends_with('\n'), "mensagem truncada ou grande demais");
    Ok(texto)
}

/// Resultado conferido de uma transferência.
struct Recebido {
    sha256: String,
    pastas: u64,
    arquivos: u64,
    bytes: u64,
    /// Só no modo de conferência: o destino existente difere do fluxo.
    divergente: bool,
}

const fn modo_arquivo(executavel: bool) -> u32 {
    if executavel { 0o700 } else { 0o600 }
}

fn criar_arquivo(pai: &File, nome: &str, executavel: bool) -> Result<File> {
    componente(nome)?;
    let nome_c = c(nome)?;
    // SAFETY: criação exclusiva sem seguir links, relativa a diretório do motor.
    arquivo(unsafe {
        libc::openat(
            pai.as_raw_fd(),
            nome_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            libc::c_uint::from(modo_arquivo(executavel)),
        )
    })
}

/// O que fazer com cada entrada do fluxo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Modo {
    /// Só calcular o manifesto.
    Hash,
    /// Criar abaixo do destino (staging).
    Gravar,
    /// Comparar com o destino existente: tipos, modos, tamanhos e bytes.
    Conferir,
}

/// Abre o diretório pai de `caminho` dentro de `raiz` (sem criar).
fn pai_de(raiz: &File, anteriores: &[&str]) -> Result<File> {
    let mut atual = raiz.try_clone()?;
    for parte in anteriores {
        atual = subdir(&atual, parte, false)?;
    }
    Ok(atual)
}

/// Lê o fluxo até o fim e grava ou confere abaixo de `raiz` / `destino_arquivo` conforme `modo`.
fn receber<R: BufRead>(
    leitor: &mut R,
    modo: Modo,
    raiz: Option<&File>,
    tipo: TipoRaiz,
    destino_arquivo: Option<(&File, &str)>,
) -> Result<Recebido> {
    let mut manifesto = Manifesto::default();
    let mut divergente = false;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut atual = vec![0u8; 64 * 1024];
    // Cada caminho aparece uma única vez: sem isso, entradas repetidas poderiam igualar
    // a contagem de uma árvore existente com entradas que nunca foram conferidas.
    let mut vistos = std::collections::HashSet::new();
    loop {
        let entrada: Entrada = serde_json::from_str(&linha(leitor)?).context("entrada inválida")?;
        if let Entrada::Pasta { caminho } | Entrada::Arquivo { caminho, .. } = &entrada {
            ensure!(vistos.insert(caminho.clone()), "caminho repetido no fluxo");
        }
        match entrada {
            Entrada::Fim { sha256 } => {
                ensure!(
                    tipo == TipoRaiz::Pasta || manifesto.arquivos == 1,
                    "raiz de arquivo exige exatamente um arquivo"
                );
                let (pastas, arquivos, bytes) = (manifesto.pastas, manifesto.arquivos, manifesto.bytes);
                let calculado = manifesto.fim();
                ensure!(calculado == sha256, "manifesto divergente: transferência descartada");
                return Ok(Recebido { sha256: calculado, pastas, arquivos, bytes, divergente });
            }
            Entrada::Pasta { caminho } => {
                ensure!(tipo == TipoRaiz::Pasta, "pasta em raiz de arquivo");
                ensure!(manifesto.pastas + manifesto.arquivos < MAX_ENTRADAS, "limite de entradas excedido");
                let partes = partes(&caminho, false)?;
                let (ultimo, anteriores) = partes.split_last().context("caminho vazio")?;
                match (modo, raiz) {
                    (Modo::Gravar, Some(raiz)) => {
                        subdir(&pai_de(raiz, anteriores)?, ultimo, true)?;
                    }
                    (Modo::Conferir, Some(raiz)) => {
                        let ok = pai_de(raiz, anteriores)
                            .and_then(|pai| subdir(&pai, ultimo, false))
                            .and_then(|dir| Ok(dir.metadata()?.mode() & 0o7777 == 0o700));
                        divergente |= !matches!(ok, Ok(true));
                    }
                    _ => (),
                }
                manifesto.pasta(&caminho);
            }
            Entrada::Arquivo { caminho, tamanho, executavel } => {
                ensure!(
                    manifesto.arquivos < MAX_ARQUIVOS
                        && manifesto.pastas + manifesto.arquivos < MAX_ENTRADAS
                        && manifesto.bytes.saturating_add(tamanho) <= MAX_BYTES,
                    "limite de cópia excedido"
                );
                let destino: Option<Result<(File, String)>> = match tipo {
                    TipoRaiz::Arquivo => {
                        ensure!(
                            caminho.is_empty() && manifesto.arquivos == 0,
                            "raiz de arquivo aceita um único arquivo"
                        );
                        destino_arquivo.map(|(pai, nome)| Ok((pai.try_clone()?, nome.to_owned())))
                    }
                    TipoRaiz::Pasta => {
                        let partes = partes(&caminho, false)?;
                        let (ultimo, anteriores) = partes.split_last().context("caminho vazio")?;
                        raiz.map(|raiz| Ok((pai_de(raiz, anteriores)?, (*ultimo).to_owned())))
                    }
                };
                let (mut saida, mut existente) = (None, None);
                match (modo, destino) {
                    (Modo::Gravar, Some(destino)) => {
                        let (pai, nome) = destino?;
                        saida = Some(criar_arquivo(&pai, &nome, executavel)?);
                    }
                    (Modo::Conferir, Some(destino)) => {
                        let aberto = destino.and_then(|(pai, nome)| abrir_regular(&pai, &nome));
                        match aberto {
                            Ok(f)
                                if f.metadata()?.len() == tamanho
                                    && f.metadata()?.mode() & 0o7777 == modo_arquivo(executavel) =>
                            {
                                existente = Some(f);
                            }
                            _ => divergente = true,
                        }
                    }
                    _ => (),
                }
                let mut hash = Sha256::new();
                let mut restante = tamanho;
                while restante > 0 {
                    let parte = usize::try_from(restante.min(buffer.len() as u64))?;
                    leitor.read_exact(&mut buffer[..parte])?;
                    hash.update(&buffer[..parte]);
                    if let Some(saida) = saida.as_mut() {
                        saida.write_all(&buffer[..parte])?;
                    }
                    if let Some(f) = existente.as_mut()
                        && (f.read_exact(&mut atual[..parte]).is_err() || atual[..parte] != buffer[..parte])
                    {
                        divergente = true;
                        existente = None;
                    }
                    restante -= parte as u64;
                }
                if let Some(saida) = saida.as_mut() {
                    saida.sync_all()?;
                }
                manifesto.arquivo(&caminho, tamanho, executavel, &hash.finalize());
            }
        }
    }
}

/// Leitor com prazo total (um par lento ou ocioso não prende o serviço sequencial) que
/// revalida a autorização a cada segundo: a transferência cessa quando a janela fecha.
struct ComPrazo<'a> {
    stream: UnixStream,
    fim: std::time::Instant,
    janela: &'a dyn Fn() -> Result<()>,
    conferida: std::time::Instant,
}
impl Read for ComPrazo<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let agora = std::time::Instant::now();
        if agora >= self.fim {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "transferência excedeu a duração máxima"));
        }
        if agora.duration_since(self.conferida) >= std::time::Duration::from_secs(1) {
            (self.janela)().map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("{e:#}")))?;
            self.conferida = agora;
        }
        self.stream.read(buffer)
    }
}

/// Autoriza uma raiz lógica: a mesma raiz já registrada (repetição) ou uma raiz nova
/// que não contém nem está contida em outra registrada; a nova é registrada de forma
/// durável antes de qualquer gravação. Assim o canal só cria raízes F3 inteiras e
/// nunca insere objetos dentro de uma raiz já copiada.
fn autorizar_raiz(contas: &File, raiz: &str, existente: bool) -> Result<()> {
    let registradas = ler_linhas(contas, REGISTRO)?;
    if registradas.iter().any(|r| r == raiz) {
        return Ok(());
    }
    ensure!(!existente, "destino existente não foi criado pelo canal");
    let nova: Vec<&str> = raiz.split('/').collect();
    for registrada in &registradas {
        let outra: Vec<&str> = registrada.split('/').collect();
        let comum = outra.len().min(nova.len());
        ensure!(outra[..comum] != nova[..comum], "raiz sobreposta a outra raiz F3 já copiada");
    }
    ensure!(registradas.len() < 4096, "raízes F3 demais");
    acrescentar(contas, REGISTRO, raiz)
}

fn uuid_minusculo(texto: &str) -> bool {
    texto.len() == 36
        && texto.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_digit() || (b'a'..=b'f').contains(&b) }
        })
}

/// Raízes lógicas F3 de perfil, as únicas que `motor_migration` gera (desenho, passo 6):
/// a home de perfil gerida `<uuid>` (que precisa conter a marca `.donnabot-profile`) e o
/// perfil de CLI externo em `legacy/.claude`, `legacy/.codex` e `legacy/.claude.json`.
/// Devolve se a raiz exige a marca de home gerida.
pub fn raiz_enumerada(relativo: &str, tipo: TipoRaiz) -> Result<bool> {
    match (relativo, tipo) {
        ("legacy/.claude" | "legacy/.codex", TipoRaiz::Pasta) | ("legacy/.claude.json", TipoRaiz::Arquivo) => Ok(false),
        (home, TipoRaiz::Pasta) if uuid_minusculo(home) => Ok(true),
        _ => bail!("raiz fora das raízes F3 enumeradas: {relativo}"),
    }
}

fn exigir_marca(dir: &File) -> Result<()> {
    let marca = abrir_regular(dir, MARCA_PERFIL).context("home de perfil sem a marca .donnabot-profile")?;
    ensure!(marca.metadata()?.len() == 0, "marca de perfil inválida");
    Ok(())
}

fn credencial(stream: &UnixStream) -> Result<u32> {
    // SAFETY: estrutura local inicializada; getsockopt preenche no máximo `tamanho` bytes.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut tamanho = libc::socklen_t::try_from(size_of::<libc::ucred>())?;
    // SAFETY: descritor do socket aceito, buffer e tamanho válidos.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut tamanho,
        )
    };
    ensure!(rc == 0, "credencial do par indisponível");
    Ok(cred.uid)
}

/// Atende uma conexão. O UID do par é autenticado pelo kernel (`SO_PEERCRED`) e a
/// operação só é autorizada dentro da janela pendente aberta pelo wrapper root
/// (`janela`), isto é, entre a publicação de uma atualização e seu aceite.
pub fn atender(stream: &UnixStream, uid_hub: u32, janela: &dyn Fn() -> Result<()>) -> Result<Resposta> {
    atender_na_home(stream, uid_hub, janela, Path::new(MOTOR))
}

/// `atender` com a home do motor explícita (a de produção é fixa; testes usam outra).
fn atender_na_home(
    stream: &UnixStream,
    uid_hub: u32,
    janela: &dyn Fn() -> Result<()>,
    home: &Path,
) -> Result<Resposta> {
    ensure!(credencial(stream)? == uid_hub, "par não é o UID do Hub");
    janela().context("canal do motor fora da janela de migração")?;
    stream.set_read_timeout(Some(OCIOSO))?;
    stream.set_write_timeout(Some(OCIOSO))?;
    let agora = std::time::Instant::now();
    let mut leitor =
        BufReader::new(ComPrazo { stream: stream.try_clone()?, fim: agora + DURACAO_MAXIMA, janela, conferida: agora });
    let home = c(home.to_str().context("home do motor não UTF-8")?)?;
    let motor = arquivo(
        // SAFETY: caminho terminado em NUL, sem seguir links no último componente.
        unsafe { libc::open(home.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) },
    )?;
    ensure!(euid() != 0 && motor.metadata()?.uid() == euid(), "o canal só executa como o dono da home do motor");
    atender_em(&mut leitor, motor, janela)
}

/// Destino já existente: só aceita a mesma árvore efetiva (tipos, modos, tamanhos,
/// bytes e nenhuma entrada a mais); qualquer divergência é recusada. Torna o destino
/// durável antes de confirmar, pois pode vir de tentativa interrompida.
fn conferir_existente<R: BufRead>(
    leitor: &mut R,
    pai: &File,
    final_: &str,
    tipo: TipoRaiz,
    exige_marca: bool,
    janela: &dyn Fn() -> Result<()>,
) -> Result<Recebido> {
    match tipo {
        TipoRaiz::Pasta => {
            let destino = subdir(pai, final_, false).and_then(|d| {
                ensure!(d.metadata()?.mode() & 0o7777 == 0o700, "{DIVERGENTE}");
                Ok(d)
            });
            let Ok(destino) = destino else {
                receber(leitor, Modo::Hash, None, tipo, None)?;
                bail!(DIVERGENTE);
            };
            let recebido = receber(leitor, Modo::Conferir, Some(&destino), tipo, None)?;
            ensure!(!recebido.divergente, DIVERGENTE);
            let total = sincronizar_arvore(&destino, 0).context(DIVERGENTE)?;
            ensure!(total == recebido.pastas + recebido.arquivos, DIVERGENTE);
            if exige_marca {
                exigir_marca(&destino)?;
            }
            janela().context("autorização encerrada antes da confirmação")?;
            Ok(recebido)
        }
        TipoRaiz::Arquivo => {
            let recebido = receber(leitor, Modo::Conferir, None, tipo, Some((pai, final_)))?;
            ensure!(!recebido.divergente, DIVERGENTE);
            abrir_regular(pai, final_)?.sync_all()?;
            janela().context("autorização encerrada antes da confirmação")?;
            Ok(recebido)
        }
    }
}

/// Destino novo: grava em staging, torna a árvore durável e publica sem substituir.
#[allow(clippy::too_many_arguments)]
fn publicar_nova<R: BufRead>(
    leitor: &mut R,
    contas: &File,
    pai_relativo: &str,
    pai: &File,
    final_: &str,
    tipo: TipoRaiz,
    exige_marca: bool,
    janela: &dyn Fn() -> Result<()>,
) -> Result<Recebido> {
    let mut aleatorio = [0u8; 16];
    getrandom::fill(&mut aleatorio).map_err(|_| anyhow::anyhow!("aleatoriedade indisponível"))?;
    let temporario = format!(".copia-{}", hex(&aleatorio));
    // Anotado de forma durável antes de existir: só stagings anotados são coletados.
    acrescentar(contas, STAGING, &format!("{pai_relativo}\t{temporario}"))?;
    let resultado = (|| -> Result<Recebido> {
        let recebido = match tipo {
            TipoRaiz::Pasta => {
                let raiz = subdir(pai, &temporario, true)?;
                let recebido = receber(leitor, Modo::Gravar, Some(&raiz), tipo, None)?;
                ensure!(
                    sincronizar_arvore(&raiz, 0)? == recebido.pastas + recebido.arquivos,
                    "árvore gravada incompleta"
                );
                if exige_marca {
                    exigir_marca(&raiz)?;
                }
                recebido
            }
            TipoRaiz::Arquivo => receber(leitor, Modo::Gravar, None, tipo, Some((pai, &temporario)))?,
        };
        janela().context("autorização encerrada antes da publicação")?;
        let (origem, destino) = (c(&temporario)?, c(final_)?);
        ensure!(
            // SAFETY: nomes validados no mesmo diretório do motor; RENAME_NOREPLACE nunca substitui.
            unsafe {
                libc::renameat2(
                    pai.as_raw_fd(),
                    origem.as_ptr(),
                    pai.as_raw_fd(),
                    destino.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } == 0,
            "publicação da cópia recusada"
        );
        // A conferência acima e o rename não são atômicos: a janela pode fechar entre os
        // dois. Conferida de novo logo depois; fechada, a publicação é desfeita (volta ao
        // nome de staging anotado, que a limpeza abaixo remove). Assim só permanece publicada
        // a cópia cuja janela ainda estava aberta numa conferência posterior ao rename.
        if let Err(erro) = janela() {
            ensure!(
                // SAFETY: os mesmos nomes validados; RENAME_NOREPLACE nunca substitui o staging.
                unsafe {
                    libc::renameat2(
                        pai.as_raw_fd(),
                        destino.as_ptr(),
                        pai.as_raw_fd(),
                        origem.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                } == 0,
                "janela encerrada durante a publicação e a cópia publicada não pôde ser desfeita: {erro:#}"
            );
            return Err(erro.context("autorização encerrada durante a publicação; cópia desfeita"));
        }
        Ok(recebido)
    })();
    if resultado.is_err() {
        limpar_e_sincronizar(pai, &temporario)?;
    }
    resultado
}

fn limpar_e_sincronizar(pai: &File, temporario: &str) -> Result<()> {
    let limpeza = remover_em(pai, temporario, 0).context("limpando staging após falha");
    let sincronizacao = pai.sync_all().context("sincronizando diretório após desfazer publicação");
    limpeza?;
    sincronizacao
}

/// Processa um fluxo já autenticado, gravando abaixo da home do motor `motor`.
fn atender_em<R: BufRead>(leitor: &mut R, motor: File, janela: &dyn Fn() -> Result<()>) -> Result<Resposta> {
    let cabecalho: Cabecalho = serde_json::from_str(&linha(leitor)?).context("cabeçalho inválido")?;
    ensure!(cabecalho.versao == 1, "versão de protocolo desconhecida");
    conta_valida(&cabecalho.conta)?;
    let exige_marca = raiz_enumerada(&cabecalho.relativo, cabecalho.raiz)?;
    let relativo = partes(&cabecalho.relativo, cabecalho.raiz == TipoRaiz::Pasta)?;
    let mut caminho: Vec<&str> = CONTAS.to_vec();
    caminho.push(&cabecalho.conta);
    caminho.extend(relativo);
    let (final_, pais) = caminho.split_last().context("destino vazio")?;
    // Raiz de contas (fixa) primeiro; a autorização precede a criação de qualquer
    // ancestral da raiz pedida.
    let mut ancestrais = vec![motor.try_clone()?];
    let mut pai = motor;
    for parte in CONTAS {
        pai = subdir(&pai, parte, true)?;
        ancestrais.push(pai.try_clone()?);
    }
    let contas = pai.try_clone()?;
    // Serviço serial: nenhum staging anotado está em uso nesta chamada.
    coletar_orfaos(&contas, &contas, STAGING)?;
    let existente = {
        let mut atual = Some(contas.try_clone()?);
        for parte in &pais[CONTAS.len()..] {
            atual = match atual {
                Some(dir) if existe(&dir, parte)? => Some(subdir(&dir, parte, false)?),
                _ => None,
            };
        }
        match atual {
            Some(dir) => existe(&dir, final_)?,
            None => false,
        }
    };
    autorizar_raiz(&contas, &caminho[CONTAS.len()..].join("/"), existente)?;
    for parte in &pais[CONTAS.len()..] {
        pai = subdir(&pai, parte, true)?;
        ancestrais.push(pai.try_clone()?);
    }
    let pai_relativo = pais[CONTAS.len()..].join("/");
    let (recebido, ja_existia) = if existente {
        (conferir_existente(leitor, &pai, final_, cabecalho.raiz, exige_marca, janela)?, true)
    } else {
        let tipo = cabecalho.raiz;
        (publicar_nova(leitor, &contas, &pai_relativo, &pai, final_, tipo, exige_marca, janela)?, false)
    };
    // Ligações duráveis de baixo para cima, inclusive de ancestrais recém-criados.
    for dir in ancestrais.iter().rev() {
        dir.sync_all()?;
    }
    Ok(Resposta {
        ok: true,
        arquivos: recebido.arquivos,
        bytes: recebido.bytes,
        sha256: recebido.sha256,
        ja_existia,
        erro: None,
    })
}

// ---------------------------------------------------------------- cliente (Hub)

/// Origem aberta sem seguir links e fixada por descritor.
pub struct Objeto {
    pub arquivo: File,
    pub meta: std::fs::Metadata,
}
impl Objeto {
    pub fn pasta(&self) -> bool {
        self.meta.is_dir()
    }
}

fn conferir_objeto(arquivo: File) -> Result<Objeto> {
    let meta = arquivo.metadata()?;
    if meta.is_file() {
        ensure!(meta.nlink() == 1, "arquivo com vínculo múltiplo (hard link) recusado na origem");
    } else {
        ensure!(meta.is_dir(), "link ou arquivo especial na origem");
    }
    Ok(Objeto { arquivo, meta })
}

/// Abre um caminho absoluto componente a componente a partir de `/`, por descritores e
/// sem seguir links em nenhum nível (ancestral trocado por link é recusado). O objeto
/// final é pasta ou arquivo regular com um único vínculo.
pub fn abrir_raiz(caminho: &Path) -> Result<Objeto> {
    use std::path::Component;
    ensure!(caminho.is_absolute(), "caminho de origem não absoluto");
    let mut nomes_ = Vec::new();
    for parte in caminho.components() {
        match parte {
            Component::RootDir => (),
            Component::Normal(nome) => nomes_.push(nome.to_str().context("caminho de origem não UTF-8")?),
            _ => bail!("caminho de origem não canônico"),
        }
    }
    let (ultimo, anteriores) = nomes_.split_last().context("caminho de origem vazio")?;
    // SAFETY: caminho fixo terminado em NUL.
    let mut dir = arquivo(unsafe { libc::open(c"/".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) })?;
    for nome in anteriores {
        let objeto = abrir_em(&dir, nome).with_context(|| format!("ancestral inseguro em {}", caminho.display()))?;
        ensure!(objeto.pasta(), "ancestral da origem não é pasta: {}", caminho.display());
        dir = objeto.arquivo;
    }
    abrir_em(&dir, ultimo).with_context(|| format!("abrindo origem {}", caminho.display()))
}

/// Abre um filho de `pai` sem seguir links. Confere o tipo antes de abrir (especiais
/// nunca são abertos) e exige que o objeto aberto seja o mesmo inode conferido.
pub fn abrir_em(pai: &File, nome: &str) -> Result<Objeto> {
    componente(nome)?;
    let antes = estado(pai, nome)?.context("entrada desapareceu durante a cópia")?;
    let tipo = antes.st_mode & libc::S_IFMT;
    ensure!(tipo == libc::S_IFDIR || tipo == libc::S_IFREG, "link ou arquivo especial na origem: {nome}");
    let nome_c = c(nome)?;
    let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    if tipo == libc::S_IFDIR {
        flags |= libc::O_DIRECTORY;
    }
    // SAFETY: componente validado relativo ao descritor do pai, sem seguir links.
    let objeto = conferir_objeto(arquivo(unsafe { libc::openat(pai.as_raw_fd(), nome_c.as_ptr(), flags) })?)?;
    ensure!(
        objeto.meta.dev() == antes.st_dev && objeto.meta.ino() == antes.st_ino,
        "origem trocada durante a cópia: {nome}"
    );
    Ok(objeto)
}

/// Nomes de uma pasta aberta, em ordem (a mesma nos dois lados do manifesto).
pub fn nomes_ordenados(dir: &File) -> Result<Vec<String>> {
    let mut lista = nomes(dir)?;
    lista.sort();
    Ok(lista)
}

/// Percorre a origem como o Hub por descritores, sem seguir links; recusa especiais e hard links.
fn enviar_pasta(saida: &mut impl Write, dir: &File, relativo: &str, manifesto: &mut Manifesto) -> Result<()> {
    for nome in nomes_ordenados(dir)? {
        let filho = if relativo.is_empty() { nome.clone() } else { format!("{relativo}/{nome}") };
        let objeto = abrir_em(dir, &nome)?;
        if objeto.pasta() {
            serde_json::to_writer(&mut *saida, &Entrada::Pasta { caminho: filho.clone() })?;
            saida.write_all(b"\n")?;
            manifesto.pasta(&filho);
            enviar_pasta(saida, &objeto.arquivo, &filho, manifesto)?;
        } else {
            enviar_arquivo(saida, objeto, &filho, manifesto)?;
        }
    }
    Ok(())
}

fn enviar_arquivo(saida: &mut impl Write, objeto: Objeto, caminho: &str, manifesto: &mut Manifesto) -> Result<()> {
    let Objeto { mut arquivo, meta } = objeto;
    let tamanho = meta.len();
    let executavel = meta.mode() & 0o100 != 0;
    serde_json::to_writer(&mut *saida, &Entrada::Arquivo { caminho: caminho.to_owned(), tamanho, executavel })?;
    saida.write_all(b"\n")?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut restante = tamanho;
    while restante > 0 {
        let parte = usize::try_from(restante.min(buffer.len() as u64))?;
        arquivo.read_exact(&mut buffer[..parte]).context("origem mudou durante a cópia")?;
        hash.update(&buffer[..parte]);
        saida.write_all(&buffer[..parte])?;
        restante -= parte as u64;
    }
    ensure!(arquivo.metadata()?.len() == tamanho, "origem mudou durante a cópia");
    manifesto.arquivo(caminho, tamanho, executavel, &hash.finalize());
    Ok(())
}

/// Escreve cabeçalho, entradas e o fim com o hash do manifesto; devolve (hash, arquivos, bytes).
pub fn escrever_fluxo(
    saida: &mut impl Write,
    conta: &str,
    relativo: &str,
    origem: &Path,
) -> Result<(String, u64, u64)> {
    conta_valida(conta)?;
    let objeto = abrir_raiz(origem)?;
    let raiz = if objeto.pasta() { TipoRaiz::Pasta } else { TipoRaiz::Arquivo };
    partes(relativo, raiz == TipoRaiz::Pasta)?;
    raiz_enumerada(relativo, raiz)?;
    let cabecalho = Cabecalho { versao: 1, conta: conta.to_owned(), relativo: relativo.to_owned(), raiz };
    serde_json::to_writer(&mut *saida, &cabecalho)?;
    saida.write_all(b"\n")?;
    let mut manifesto = Manifesto::default();
    match raiz {
        TipoRaiz::Pasta => enviar_pasta(saida, &objeto.arquivo, "", &mut manifesto)?,
        TipoRaiz::Arquivo => enviar_arquivo(saida, objeto, "", &mut manifesto)?,
    }
    let (arquivos, bytes) = (manifesto.arquivos, manifesto.bytes);
    let hash = manifesto.fim();
    serde_json::to_writer(&mut *saida, &Entrada::Fim { sha256: hash.clone() })?;
    saida.write_all(b"\n")?;
    saida.flush()?;
    Ok((hash, arquivos, bytes))
}

/// Envia uma raiz de perfil ao canal do motor e confere o manifesto devolvido.
pub fn enviar(socket: &Path, conta: &str, relativo: &str, origem: &Path) -> Result<Resposta> {
    let stream = UnixStream::connect(socket).context("canal do motor indisponível")?;
    let mut saida = BufWriter::new(stream.try_clone()?);
    let escrita = escrever_fluxo(&mut saida, conta, relativo, origem);
    drop(saida);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut leitor = BufReader::new(stream);
    let resposta: Option<Resposta> = linha(&mut leitor).ok().and_then(|l| serde_json::from_str(&l).ok());
    // Uma recusa do motor explica uma escrita interrompida; vale mais que o erro de E/S.
    if let Some(Resposta { ok: false, erro, .. }) = &resposta {
        bail!("canal do motor recusou: {}", erro.clone().unwrap_or_default());
    }
    let (hash, arquivos, bytes) = escrita?;
    let resposta = resposta.context("resposta do motor inválida")?;
    ensure!(resposta.ok, "canal do motor recusou: {}", resposta.erro.unwrap_or_default());
    ensure!(resposta.sha256 == hash, "manifesto devolvido pelo motor diverge do enviado");
    Ok(Resposta { arquivos, bytes, ..resposta })
}

/// Laço do serviço: uma conexão por vez, resposta em linha JSON.
pub fn servir(ouvinte: &std::os::unix::net::UnixListener, uid_hub: u32, janela: &dyn Fn() -> Result<()>) -> Result<()> {
    for conexao in ouvinte.incoming() {
        let stream = conexao?;
        let resposta = match atender(&stream, uid_hub, janela) {
            Ok(resposta) => resposta,
            Err(erro) => Resposta { erro: Some(format!("{erro:#}")), ..Resposta::default() },
        };
        // Resposta numa única escrita, terminada por quebra de linha.
        let mut bytes = serde_json::to_vec(&resposta).unwrap_or_default();
        bytes.push(b'\n');
        let mut saida = stream;
        let _ = saida.write_all(&bytes);
        if !resposta.ok {
            // Recusa antes do fim do fluxo: drena (com limite) para o cliente receber o
            // motivo em vez de ECONNRESET por dados não lidos.
            // Prazo total de 5 s e no máximo 64 MiB: um par lento não prende o laço serial.
            let _ = saida.shutdown(std::net::Shutdown::Write);
            let fim = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut buffer = vec![0u8; 64 * 1024];
            let mut drenado = 0usize;
            while drenado < 64 * 1024 * 1024 {
                let Some(resta) = fim.checked_duration_since(std::time::Instant::now()) else { break };
                if resta.is_zero() || saida.set_read_timeout(Some(resta)).is_err() {
                    break;
                }
                match (&saida).read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => drenado += n,
                }
            }
        }
    }
    Ok(())
}

pub fn ler_hub_uid(motor_conf: &Path) -> Result<u32> {
    let texto = std::fs::read_to_string(motor_conf)?;
    let linha = texto.lines().find_map(|l| l.strip_prefix("hub_uid=")).context("hub_uid ausente em motor.conf")?;
    let uid: u32 = linha.parse().context("hub_uid inválido")?;
    ensure!(uid != 0, "hub_uid inválido");
    Ok(uid)
}

pub fn os(texto: &str) -> &OsStr {
    OsStr::from_bytes(texto.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    const CONTA: &str = "ac_dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const PERFIL: &str = "legacy/.claude";
    const HOME: &str = "0b9f1c2e-3a4d-4e5f-8a6b-7c8d9e0f1a2b";

    fn temporario(nome: &str) -> Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("mx365-copia-{nome}-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        Ok(dir)
    }

    fn origem() -> Result<std::path::PathBuf> {
        let dir = temporario("origem")?;
        std::fs::create_dir(dir.join("sub"))?;
        std::fs::write(dir.join("settings.json"), b"{\"a\":1}")?;
        std::fs::write(dir.join("sub/script.sh"), b"#!/bin/sh\necho oi\n")?;
        std::fs::set_permissions(dir.join("sub/script.sh"), std::fs::Permissions::from_mode(0o755))?;
        Ok(dir)
    }

    fn fluxo(fonte: &Path, relativo: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        escrever_fluxo(&mut bytes, CONTA, relativo, fonte)?;
        Ok(bytes)
    }

    fn atender_bytes(bytes: &[u8], home: &Path) -> Result<Resposta> {
        atender_em(&mut BufReader::new(bytes), File::open(home)?, &|| -> Result<()> { Ok(()) })
    }

    fn destino(home: &Path, relativo: &str) -> std::path::PathBuf {
        home.join(format!("engine-setup/accounts/{CONTA}/{relativo}"))
    }

    #[test]
    fn copia_grava_como_dono_e_repeticao_so_aceita_conteudo_identico() -> Result<()> {
        let (fonte, home) = (origem()?, temporario("motor")?);
        let bytes = fluxo(&fonte, PERFIL)?;
        let resposta = atender_bytes(&bytes, &home)?;
        assert!(resposta.ok && !resposta.ja_existia && resposta.arquivos == 2);
        let alvo = destino(&home, PERFIL);
        assert_eq!(std::fs::read(alvo.join("settings.json"))?, b"{\"a\":1}");
        assert_eq!(std::fs::metadata(alvo.join("settings.json"))?.mode() & 0o7777, 0o600);
        assert_eq!(std::fs::metadata(alvo.join("sub/script.sh"))?.mode() & 0o7777, 0o700);
        assert_eq!(std::fs::metadata(&alvo)?.mode() & 0o7777, 0o700);
        assert!(atender_bytes(&bytes, &home)?.ja_existia);
        std::fs::write(fonte.join("settings.json"), b"{\"a\":2}")?;
        assert!(atender_bytes(&fluxo(&fonte, PERFIL)?, &home).is_err());
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn repeticao_confere_o_destino_real_e_nao_uma_marca() -> Result<()> {
        let (fonte, home) = (origem()?, temporario("motor")?);
        let bytes = fluxo(&fonte, PERFIL)?;
        atender_bytes(&bytes, &home)?;
        let alvo = destino(&home, PERFIL);
        let arquivo_ = alvo.join("settings.json");
        let modo = |caminho: &Path, m: u32| std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(m));
        std::fs::write(&arquivo_, b"{\"a\":9}")?;
        assert!(atender_bytes(&bytes, &home).is_err(), "conteúdo alterado no destino foi aceito");
        std::fs::write(&arquivo_, b"{\"a\":1}")?;
        assert!(atender_bytes(&bytes, &home)?.ja_existia);
        std::fs::remove_file(&arquivo_)?;
        assert!(atender_bytes(&bytes, &home).is_err(), "arquivo removido no destino foi aceito");
        std::fs::write(&arquivo_, b"{\"a\":1}")?;
        modo(&arquivo_, 0o600)?;
        std::fs::write(alvo.join("extra"), b"x")?;
        assert!(atender_bytes(&bytes, &home).is_err(), "entrada extra no destino foi aceita");
        std::fs::remove_file(alvo.join("extra"))?;
        modo(&alvo.join("sub/script.sh"), 0o600)?;
        assert!(atender_bytes(&bytes, &home).is_err(), "modo alterado no destino foi aceito");
        modo(&alvo.join("sub/script.sh"), 0o700)?;
        std::fs::remove_file(&arquivo_)?;
        std::os::unix::fs::symlink("/etc/hostname", &arquivo_)?;
        assert!(atender_bytes(&bytes, &home).is_err(), "link no destino foi aceito");
        std::fs::remove_file(&arquivo_)?;
        std::fs::write(&arquivo_, b"{\"a\":1}")?;
        modo(&arquivo_, 0o600)?;
        assert!(atender_bytes(&bytes, &home)?.ja_existia);
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    /// Fluxo forjado a partir de entradas JSON e conteúdos, com o manifesto correspondente.
    fn forjar(cabecalho: &str, entradas: &[(&str, &[u8])]) -> Vec<u8> {
        let mut manifesto = Manifesto::default();
        let mut bytes = format!("{cabecalho}\n").into_bytes();
        for (json, conteudo) in entradas {
            bytes.extend_from_slice(json.as_bytes());
            bytes.push(b'\n');
            match serde_json::from_str::<Entrada>(json).unwrap_or(Entrada::Fim { sha256: String::new() }) {
                Entrada::Pasta { caminho } => manifesto.pasta(&caminho),
                Entrada::Arquivo { caminho, tamanho, executavel } => {
                    bytes.extend_from_slice(conteudo);
                    manifesto.arquivo(&caminho, tamanho, executavel, &Sha256::digest(conteudo));
                }
                Entrada::Fim { .. } => (),
            }
        }
        bytes.extend_from_slice(format!("{{\"tipo\":\"fim\",\"sha256\":\"{}\"}}\n", manifesto.fim()).as_bytes());
        bytes
    }

    fn cabecalho(relativo: &str, raiz: &str) -> String {
        format!("{{\"versao\":1,\"conta\":\"{CONTA}\",\"relativo\":\"{relativo}\",\"raiz\":\"{raiz}\"}}")
    }

    #[test]
    fn caminho_repetido_e_raiz_de_arquivo_vazia_sao_recusados() -> Result<()> {
        let home = temporario("motor")?;
        let pasta = cabecalho(PERFIL, "pasta");
        let a = r#"{"tipo":"arquivo","caminho":"a","tamanho":1,"executavel":false}"#;
        let extra = r#"{"tipo":"arquivo","caminho":"extra","tamanho":1,"executavel":false}"#;
        assert!(!atender_bytes(&forjar(&pasta, &[(a, b"1"), (extra, b"2")]), &home)?.ja_existia);
        // `a` duas vezes igualaria a contagem de duas entradas reais sem conferir `extra`.
        assert!(atender_bytes(&forjar(&pasta, &[(a, b"1"), (a, b"1")]), &home).is_err(), "caminho repetido aceito");
        let p = r#"{"tipo":"pasta","caminho":"p"}"#;
        assert!(atender_bytes(&forjar(&pasta, &[(p, b""), (p, b"")]), &home).is_err());
        let arquivo_ = cabecalho("legacy/.claude.json", "arquivo");
        assert!(atender_bytes(&forjar(&arquivo_, &[]), &home).is_err(), "raiz de arquivo nova sem arquivo aceita");
        let unico = r#"{"tipo":"arquivo","caminho":"","tamanho":2,"executavel":false}"#;
        assert!(!atender_bytes(&forjar(&arquivo_, &[(unico, b"{}")]), &home)?.ja_existia);
        assert!(atender_bytes(&forjar(&arquivo_, &[(unico, b"{}")]), &home)?.ja_existia);
        assert!(atender_bytes(&forjar(&arquivo_, &[]), &home).is_err(), "raiz de arquivo existente sem arquivo aceita");
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn so_raizes_f3_enumeradas_inteiras_e_nao_sobrepostas() -> Result<()> {
        let (fonte, home) = (origem()?, temporario("motor")?);
        // Fora das raízes enumeradas: recusado antes de criar qualquer pasta.
        for relativo in ["outra", "legacy", "legacy/.claude/sub", "", "../x", "0B9F1C2E-3A4D-4E5F-8A6B-7C8D9E0F1A2B"] {
            let forjado = forjar(&cabecalho(relativo, "pasta"), &[]);
            assert!(atender_bytes(&forjado, &home).is_err(), "{relativo} aceito");
        }
        assert!(!home.join(format!("engine-setup/accounts/{CONTA}")).exists());
        // Home gerida exige a marca de perfil; sem ela nada é publicado.
        assert!(atender_bytes(&fluxo(&fonte, HOME)?, &home).is_err(), "home sem marca aceita");
        assert!(!destino(&home, HOME).exists());
        std::fs::write(fonte.join(MARCA_PERFIL), b"")?;
        assert!(!atender_bytes(&fluxo(&fonte, HOME)?, &home)?.ja_existia);
        std::fs::remove_file(fonte.join(MARCA_PERFIL))?;
        atender_bytes(&fluxo(&fonte, PERFIL)?, &home)?;
        assert!(!atender_bytes(&fluxo(&fonte, "legacy/.codex")?, &home)?.ja_existia);
        let registro = std::fs::read_to_string(home.join("engine-setup/accounts").join(REGISTRO))?;
        assert_eq!(registro, format!("{CONTA}/{HOME}\n{CONTA}/{PERFIL}\n{CONTA}/legacy/.codex\n"));
        // Raiz enumerada existente que o canal não criou é recusada.
        let alheia = "11111111-2222-4333-8444-555555555555";
        std::fs::DirBuilder::new().mode(0o700).create(destino(&home, alheia))?;
        std::fs::write(fonte.join(MARCA_PERFIL), b"")?;
        assert!(atender_bytes(&fluxo(&fonte, alheia)?, &home).is_err());
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn autorizacao_encerrada_durante_a_transferencia_nao_publica() -> Result<()> {
        let (fonte, home) = (origem()?, temporario("motor")?);
        let fechada = || -> Result<()> { bail!("janela fechada") };
        let bytes = fluxo(&fonte, PERFIL)?;
        assert!(atender_em(&mut BufReader::new(bytes.as_slice()), File::open(&home)?, &fechada).is_err());
        assert!(!destino(&home, PERFIL).exists(), "publicado após a autorização acabar");
        let restos: Vec<_> = std::fs::read_dir(destino(&home, "legacy"))?.collect();
        assert!(restos.is_empty(), "staging não removido");
        // Repetição também exige a autorização viva no fim.
        atender_bytes(&bytes, &home)?;
        assert!(atender_em(&mut BufReader::new(bytes.as_slice()), File::open(&home)?, &fechada).is_err());
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    /// Conexão real (par de sockets, `SO_PEERCRED` do próprio processo) pelo `atender`:
    /// a janela fecha durante a leitura e o leitor com prazo (`ComPrazo`) interrompe a
    /// transferência antes da conferência final, sem publicar nem deixar staging.
    #[test]
    fn janela_fechada_durante_a_leitura_interrompe_a_conexao_real() -> Result<()> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        // SAFETY: consulta sem efeitos colaterais.
        if unsafe { libc::geteuid() } == 0 {
            return Ok(()); // o canal recusa root por desenho
        }
        let (fonte, home) = (origem()?, temporario("motor")?);
        let bytes = fluxo(&fonte, PERFIL)?;
        let uid = euid();
        // Aberta do início ao fim: a mesma conexão real publica.
        let (servidor, mut cliente) = UnixStream::pair()?;
        let envio = bytes.clone();
        let escritor = std::thread::spawn(move || -> std::io::Result<()> {
            cliente.write_all(&envio)?;
            cliente.shutdown(std::net::Shutdown::Write)
        });
        assert!(!atender_na_home(&servidor, uid, &|| Ok(()), &home)?.ja_existia);
        escritor.join().map_err(|_| anyhow::anyhow!("escritor falhou"))??;
        std::fs::remove_dir_all(destino(&home, PERFIL))?;
        std::fs::remove_file(home.join("engine-setup/accounts").join(REGISTRO))?;
        // Aberta na conexão, fechada no meio do fluxo: a leitura seguinte, passado 1 s,
        // revalida e falha (erro da janela, não o da conferência antes de publicar).
        let fechada = Arc::new(AtomicBool::new(false));
        let (servidor, mut cliente) = UnixStream::pair()?;
        let (marca, envio) = (Arc::clone(&fechada), bytes.clone());
        let escritor = std::thread::spawn(move || -> std::io::Result<()> {
            let meio = envio.len() / 2;
            cliente.write_all(&envio[..meio])?;
            marca.store(true, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(1300));
            // Resto em duas partes: a segunda leitura já ocorre depois do prazo de 1 s.
            let tres_quartos = meio + (envio.len() - meio) / 2;
            let _ = cliente.write_all(&envio[meio..tres_quartos]);
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = cliente.write_all(&envio[tres_quartos..]);
            let _ = cliente.shutdown(std::net::Shutdown::Write);
            Ok(())
        });
        let janela = || -> Result<()> {
            ensure!(!fechada.load(Ordering::SeqCst), "janela fechada");
            Ok(())
        };
        let erro = atender_na_home(&servidor, uid, &janela, &home).err().map(|e| format!("{e:#}")).unwrap_or_default();
        drop(servidor);
        escritor.join().map_err(|_| anyhow::anyhow!("escritor falhou"))??;
        assert!(erro.contains("janela fechada"), "{erro}");
        assert!(!erro.contains("antes da publicação"), "parou só na conferência final: {erro}");
        assert!(!destino(&home, PERFIL).exists(), "publicado com a janela fechada");
        let restos: Vec<_> = std::fs::read_dir(destino(&home, "legacy"))?.collect();
        assert!(restos.is_empty(), "staging não removido");
        // Fechada antes da conexão: recusa sem ler nada.
        let (servidor, _cliente) = UnixStream::pair()?;
        assert!(atender_na_home(&servidor, uid, &janela, &home).is_err());
        // Outro UID no SO_PEERCRED: recusado.
        let (servidor, _cliente) = UnixStream::pair()?;
        assert!(atender_na_home(&servidor, uid + 1, &|| Ok(()), &home).is_err());
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn falha_de_sincronizacao_no_desfazimento_e_propagada() -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = temporario("sync")?;
        let caminho = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_PATH).open(&dir)?;
        let erro = limpar_e_sincronizar(&caminho, "ausente").err().context("fsync inválido aceito")?;
        assert!(format!("{erro:#}").contains("sincronizando diretório"));
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// SEG P3 residual: a janela fecha exatamente entre a última conferência e o
    /// `renameat2` (simulado: fechada assim que o destino existe). A conferência posterior
    /// ao rename detecta, desfaz a publicação e não deixa staging.
    #[test]
    fn janela_fechada_entre_conferencia_e_rename_desfaz_a_publicacao() -> Result<()> {
        // SAFETY: consulta sem efeitos colaterais.
        if unsafe { libc::geteuid() } == 0 {
            return Ok(()); // o canal recusa root por desenho
        }
        let (fonte, home) = (origem()?, temporario("motor")?);
        let bytes = fluxo(&fonte, PERFIL)?;
        let (servidor, mut cliente) = UnixStream::pair()?;
        let escritor = std::thread::spawn(move || -> std::io::Result<()> {
            cliente.write_all(&bytes)?;
            cliente.shutdown(std::net::Shutdown::Write)
        });
        let publicado = destino(&home, PERFIL);
        let alvo = publicado.clone();
        let janela = move || -> Result<()> {
            ensure!(std::fs::symlink_metadata(&alvo).is_err(), "janela fechada depois da conferência");
            Ok(())
        };
        let erro =
            atender_na_home(&servidor, euid(), &janela, &home).err().map(|e| format!("{e:#}")).unwrap_or_default();
        drop(servidor);
        escritor.join().map_err(|_| anyhow::anyhow!("escritor falhou"))??;
        assert!(erro.contains("durante a publicação; cópia desfeita"), "{erro}");
        assert!(!publicado.exists(), "cópia ficou publicada com a janela fechada");
        let restos: Vec<_> = std::fs::read_dir(destino(&home, "legacy"))?.collect();
        assert!(restos.is_empty(), "staging não removido: {restos:?}");
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn so_staging_anotado_e_coletado_por_descritor() -> Result<()> {
        let (fonte, home) = (origem()?, temporario("motor")?);
        let contas = home.join("engine-setup/accounts");
        let pai = contas.join(CONTA).join("legacy");
        std::fs::DirBuilder::new().mode(0o700).recursive(true).create(&pai)?;
        let orfao = ".copia-0123456789abcdef0123456789abcdef";
        let parecido = ".copia-0123456789abcdef0123456789abcdee";
        for nome in [orfao, parecido, ".copia-documentos"] {
            std::fs::DirBuilder::new().mode(0o700).recursive(true).create(pai.join(nome).join("sub"))?;
            std::fs::write(pai.join(nome).join("sub/x"), b"dados")?;
        }
        std::fs::write(contas.join(STAGING), format!("{CONTA}/legacy\t{orfao}\n"))?;
        std::fs::set_permissions(contas.join(STAGING), std::fs::Permissions::from_mode(0o600))?;
        atender_bytes(&fluxo(&fonte, PERFIL)?, &home)?;
        assert!(!pai.join(orfao).exists(), "staging anotado não coletado");
        assert!(pai.join(parecido).join("sub/x").exists() && pai.join(".copia-documentos/sub/x").exists());
        // Anotação forjada fora do formato é recusada sem remover nada.
        std::fs::write(contas.join(STAGING), format!("{CONTA}/legacy\t.copia-documentos\n"))?;
        assert!(atender_bytes(&fluxo(&fonte, PERFIL)?, &home).is_err());
        assert!(pai.join(".copia-documentos/sub/x").exists());
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn manifesto_adulterado_e_descartado_sem_restos() -> Result<()> {
        let (fonte, home) = (origem()?, temporario("motor")?);
        let texto = String::from_utf8_lossy(&fluxo(&fonte, PERFIL)?).into_owned();
        let adulterado = texto.replacen("\"sha256\":\"", "\"sha256\":\"0", 1).into_bytes();
        assert!(atender_bytes(&adulterado, &home).is_err());
        let restos: Vec<_> = std::fs::read_dir(destino(&home, "legacy"))?.collect();
        assert!(restos.is_empty(), "transferência incompleta deixou restos");
        std::fs::remove_dir_all(fonte)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn travessia_links_hardlinks_e_ancestral_trocado_sao_recusados() -> Result<()> {
        let home = temporario("motor")?;
        assert!(atender_bytes(cabecalho("../fora", "pasta").as_bytes(), &home).is_err());
        let fluxo_mau = format!("{}\n{{\"tipo\":\"pasta\",\"caminho\":\"../../x\"}}\n", cabecalho(PERFIL, "pasta"));
        assert!(atender_bytes(fluxo_mau.as_bytes(), &home).is_err());
        assert!(conta_valida("../x").is_err() && conta_valida("ac_1").is_err() && conta_valida(CONTA).is_ok());
        let fonte = origem()?;
        std::os::unix::fs::symlink("/etc/passwd", fonte.join("link"))?;
        assert!(escrever_fluxo(&mut Vec::new(), CONTA, PERFIL, &fonte).is_err());
        std::fs::remove_file(fonte.join("link"))?;
        std::fs::hard_link(fonte.join("settings.json"), fonte.join("vinculo"))?;
        assert!(escrever_fluxo(&mut Vec::new(), CONTA, PERFIL, &fonte).is_err(), "hard link na origem aceito");
        std::fs::remove_file(fonte.join("vinculo"))?;
        // Ancestral da origem trocado por link: a abertura por descritores recusa.
        let base = temporario("ancestral")?;
        std::fs::rename(&fonte, base.join("real"))?;
        std::os::unix::fs::symlink(base.join("real"), base.join("trocado"))?;
        assert!(abrir_raiz(&base.join("real/sub")).is_ok());
        assert!(abrir_raiz(&base.join("trocado/sub")).is_err(), "ancestral link seguido");
        assert!(escrever_fluxo(&mut Vec::new(), CONTA, PERFIL, &base.join("trocado")).is_err());
        assert!(!home.join("fora").exists());
        std::fs::remove_dir_all(base)?;
        std::fs::remove_dir_all(home)?;
        Ok(())
    }
}
