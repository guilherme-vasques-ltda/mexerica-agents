use crate::fork::motor_exec::fd::{Directory, name, own};
use anyhow::{Result, ensure};
use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// O `:` é impossível num nome de versão do catálogo (`safe_component` recusa): o namespace
/// da quarentena nunca coincide com uma versão válida.
const QUARENTENA: &str = ".agente-limpeza:";
const ARVORE: &str = "versao";

pub(super) fn limpar(instalada: &Path, protecao: Option<&super::Protecao>) {
    let Some(protecao) = protecao else { return };
    let Some(atual) = instalada.file_name().and_then(OsStr::to_str) else { return };
    let Some(id) =
        instalada.parent().and_then(Path::file_name).and_then(OsStr::to_str).and_then(super::super::safe_component)
    else {
        return;
    };
    let Ok(agente) = abrir_agente(instalada) else { return };
    limpar_no_diretorio(&agente, id, atual, protecao);
}

fn abrir_agente(instalada: &Path) -> Result<Directory> {
    let agente = instalada.parent().ok_or_else(|| anyhow::anyhow!("agente ausente"))?;
    let catalogo = agente.parent().ok_or_else(|| anyhow::anyhow!("raiz de agentes ausente"))?;
    let raiz = catalogo.parent().ok_or_else(|| anyhow::anyhow!("raiz de instalação ausente"))?;
    ensure!(catalogo.file_name() == Some(OsStr::new("agents")), "raiz de agentes inválida");
    let raiz = Directory::open(raiz)?;
    verificar(&raiz)?;
    let catalogo = raiz.child(OsStr::new("agents"), None)?;
    verificar(&catalogo)?;
    let agente = catalogo.child(agente.file_name().ok_or_else(|| anyhow::anyhow!("agente ausente"))?, None)?;
    verificar(&agente)?;
    Ok(agente)
}

fn verificar(dir: &Directory) -> Result<()> {
    own(&dir.0)?;
    ensure!(dir.0.metadata()?.mode() & 0o022 == 0, "diretório de limpeza gravável por terceiros");
    Ok(())
}

fn versao_quarentena(nome: &str) -> Option<(&str, Option<&str>)> {
    let sufixo = nome.strip_prefix(QUARENTENA)?;
    let (chave, versao) = match sufixo.split_once('.') {
        Some((chave, versao)) => (chave, Some(super::super::safe_component(versao)?)),
        None => (sufixo, None),
    };
    let uuid = uuid::Uuid::parse_str(chave).ok()?;
    if uuid.to_string() != chave {
        return None;
    }
    Some((chave, versao))
}

fn registrar_falha(id: &str, versao: &str, erro: &anyhow::Error) {
    let categoria =
        erro.downcast_ref::<std::io::Error>().map_or_else(|| "validacao".to_owned(), |io| format!("{:?}", io.kind()));
    tracing::debug!(agente = %id, %versao, %categoria, "limpeza de versão do agente adiada");
}

fn limpar_no_diretorio(agente: &Directory, id: &str, atual: &str, protecao: &super::Protecao) {
    let Ok(nomes) = agente.names() else { return };
    for nome in nomes {
        // Nomes de quarentena não passam em `safe_component` (têm `:`); trate-os primeiro.
        if let Some((_, original)) = nome.to_str().and_then(versao_quarentena) {
            if let Err(erro) = esvaziar_quarentena(agente, &nome, atual, protecao, None) {
                registrar_falha(id, original.unwrap_or("desconhecida"), &erro);
            }
            continue;
        }
        let Some(versao) = nome.to_str().and_then(super::super::safe_component) else { continue };
        if versao.eq_ignore_ascii_case(atual) {
            continue;
        }
        if versao.starts_with(".agente-") {
            continue;
        }
        // Sem recibo de que a versão foi instalada por este protocolo, ela fica.
        if !protecao.liberada(versao) {
            continue;
        }
        let Ok(candidata) = candidata_com_recibo(agente, &nome) else {
            continue;
        };
        if let Err(erro) = remover_versao(agente, versao, &candidata, atual, protecao) {
            registrar_falha(id, versao, &erro);
        }
    }
}

fn candidata_com_recibo(agente: &Directory, nome: &OsStr) -> Result<Directory> {
    use super::super::arquivo::{MARCADOR, RECIBO, conferir_recibo};

    let candidata = agente.child(nome, None)?;
    verificar(&candidata)?;
    ensure!(candidata.0.metadata()?.dev() == agente.0.metadata()?.dev(), "versão em outro volume");
    let marcador = candidata.file(OsStr::new(MARCADOR), libc::O_RDONLY, 0)?;
    let recibo = candidata.file(OsStr::new(RECIBO), libc::O_RDONLY, 0)?;
    own(&marcador)?;
    own(&recibo)?;
    ensure!(conferir_recibo(marcador, recibo)?, "recibo de publicação inválido");
    Ok(candidata)
}

fn identidade(dir: &Directory) -> Result<(u64, u64)> {
    let meta = dir.0.metadata()?;
    Ok((meta.dev(), meta.ino()))
}

fn nome_prova(chave: &str) -> String {
    format!("{QUARENTENA}prova-{chave}")
}

fn conteudo_prova(arvore: &Directory, versao: &str) -> Result<String> {
    let (dev, ino) = identidade(arvore)?;
    Ok(format!("{versao}\n{dev}\n{ino}\n"))
}

fn remover_versao(
    agente: &Directory,
    versao: &str,
    candidata: &Directory,
    atual: &str,
    protecao: &super::Protecao,
) -> Result<()> {
    let chave = uuid::Uuid::new_v4().to_string();
    let nome = OsStr::new(versao);
    let curto = format!("{QUARENTENA}{chave}");
    let completo = format!("{curto}.{versao}");
    let mut nome_quarentena = OsStr::new(&completo);
    let quarentena = match agente.quarantine(nome, nome_quarentena) {
        Err(erro)
            if erro.downcast_ref::<std::io::Error>().and_then(std::io::Error::raw_os_error)
                == Some(libc::ENAMETOOLONG) =>
        {
            nome_quarentena = OsStr::new(&curto);
            agente.quarantine(nome, nome_quarentena)?
        }
        resultado => resultado?,
    };
    let movida = quarentena.child(OsStr::new(ARVORE), None).and_then(|dir| {
        verificar(&dir)?;
        ensure!(identidade(&dir)? == identidade(candidata)?, "árvore movida diverge da prova");
        Ok(dir)
    });
    let arvore = match movida {
        Ok(arvore) => arvore,
        Err(erro) => {
            if quarentena.repor(agente, nome).is_ok()
                && identidade(&quarentena).is_ok_and(|esperada| {
                    agente.quarentena_privada(nome_quarentena).and_then(|dir| identidade(&dir)).ok() == Some(esperada)
                })
            {
                let _ = agente.unlink(nome_quarentena, true);
            }
            return Err(erro);
        }
    };
    let mut prova =
        arvore.file(OsStr::new(&nome_prova(&chave)), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
    prova.write_all(conteudo_prova(&arvore, versao)?.as_bytes())?;
    prova.sync_all()?;
    arvore.0.sync_all()?;
    quarentena.0.sync_all()?;
    agente.0.sync_all()?;
    esvaziar_quarentena(agente, nome_quarentena, atual, protecao, Some(candidata))
}

fn esvaziar_quarentena(
    agente: &Directory,
    nome: &OsStr,
    atual: &str,
    protecao: &super::Protecao,
    candidata: Option<&Directory>,
) -> Result<()> {
    let Some((chave, original)) = nome.to_str().and_then(versao_quarentena) else {
        anyhow::bail!("nome de quarentena inválido");
    };
    let quarentena = agente.quarentena_privada(nome)?;
    let dev = agente.0.metadata()?.dev();
    ensure!(quarentena.0.metadata()?.dev() == dev, "quarentena em outro volume");
    let nome_arvore = OsStr::new(ARVORE);
    let arvore = quarentena.child(nome_arvore, None)?;
    verificar(&arvore)?;
    ensure!(arvore.0.metadata()?.dev() == dev, "versão em outro volume");
    if let Some(candidata) = candidata {
        ensure!(identidade(&arvore)? == identidade(candidata)?, "árvore em quarentena mudou");
    }
    let arquivo_prova = nome_prova(chave);
    let arquivo_prova = OsStr::new(&arquivo_prova);
    let prova = arvore.file(arquivo_prova, libc::O_RDONLY, 0)?;
    own(&prova)?;
    let mut conteudo = String::new();
    prova.take(4096).read_to_string(&mut conteudo)?;
    let versao = conteudo.split_once('\n').and_then(|(versao, _)| super::super::safe_component(versao));
    let Some(versao) = versao else { anyhow::bail!("prova de limpeza inválida") };
    ensure!(original.is_none_or(|original| original == versao), "versão em quarentena diverge da prova");
    ensure!(conteudo == conteudo_prova(&arvore, versao)?, "árvore em quarentena diverge da prova");
    ensure!(!versao.eq_ignore_ascii_case(atual) && !protecao.em_uso(versao), "versão em quarentena protegida");
    for entrada in arvore.names()? {
        if entrada != arquivo_prova {
            remover_entrada(&arvore, &entrada, dev)?;
        }
    }
    ensure!(identidade(&quarentena.child(nome_arvore, None)?)? == identidade(&arvore)?, "árvore em quarentena mudou");
    arvore.unlink(arquivo_prova, false)?;
    quarentena.unlink(nome_arvore, true)?;
    ensure!(identidade(&agente.quarentena_privada(nome)?)? == identidade(&quarentena)?, "quarentena mudou");
    agente.unlink(nome, true)
}

fn remover_entrada(pai: &Directory, nome: &OsStr, dev: u64) -> Result<()> {
    match pai.unlink(nome, false) {
        Ok(()) => return Ok(()),
        Err(erro) if erro.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == ErrorKind::NotFound) => {
            return Ok(());
        }
        Err(_) => (),
    }
    let dir = pai.child(nome, None)?;
    verificar(&dir)?;
    ensure!(dir.0.metadata()?.dev() == dev, "limpeza não atravessa volumes");
    for entrada in dir.names()? {
        remover_entrada(&dir, &entrada, dev)?;
    }
    pai.unlink(nome, true)
}

impl Directory {
    fn quarentena_privada(&self, nome: &OsStr) -> Result<Self> {
        let dir = self.child(nome, None)?;
        own(&dir.0)?;
        ensure!(dir.0.metadata()?.mode().trailing_zeros() >= 6, "quarentena não é privada");
        Ok(dir)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn repor(&self, pai: &Directory, destino: &OsStr) -> Result<()> {
        let destino = name(destino)?;
        #[cfg(target_os = "linux")]
        // SAFETY: descritores vivos e nomes de um componente; a flag impede substituição.
        let rc = unsafe {
            libc::renameat2(
                self.0.as_raw_fd(),
                c"versao".as_ptr(),
                pai.0.as_raw_fd(),
                destino.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        // SAFETY: descritores vivos e nomes de um componente; a flag impede substituição.
        let rc = unsafe {
            libc::renameatx_np(
                self.0.as_raw_fd(),
                c"versao".as_ptr(),
                pai.0.as_raw_fd(),
                destino.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn repor(&self, _pai: &Directory, _destino: &OsStr) -> Result<()> {
        Err(std::io::Error::from(ErrorKind::Unsupported).into())
    }

    fn quarantine(&self, entry: &OsStr, quarantine: &OsStr) -> Result<Self> {
        let entry = name(entry)?;
        let quarantine = name(quarantine)?;
        // SAFETY: ambos os nomes são componentes únicos e o descritor está aberto.
        if unsafe { libc::mkdirat(self.0.as_raw_fd(), quarantine.as_ptr(), 0o700) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let dir = self.quarentena_privada(OsStr::from_bytes(quarantine.as_bytes()))?;
        // SAFETY: move somente a entrada, sem seguir links, para a quarentena recém-criada.
        if unsafe { libc::renameat(self.0.as_raw_fd(), entry.as_ptr(), dir.0.as_raw_fd(), c"versao".as_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(dir)
    }

    pub(super) fn unlink(&self, entry: &OsStr, directory: bool) -> Result<()> {
        let entry = name(entry)?;
        let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: unlinkat não segue links e opera no descritor aberto e em um único componente.
        if unsafe { libc::unlinkat(self.0.as_raw_fd(), entry.as_ptr(), flags) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn registradas(versoes: &[&str]) -> super::super::Protecao {
        super::super::Protecao { em_uso: Vec::new(), registradas: versoes.iter().map(|v| (*v).to_owned()).collect() }
    }

    fn publicar(pasta: &Path) {
        let bytes = b"fixture";
        let hash = super::super::super::sha256_hex(bytes);
        super::super::super::arquivo::publicar(pasta, "adapter", bytes, &hash, Path::new("adapter"), 1024).unwrap();
    }

    #[test]
    fn troca_ancestral_por_link_nao_remove_sentinela() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let agente = raiz.join("agents/agente");
        let atual = agente.join("2");
        fs::create_dir_all(&atual).unwrap();
        publicar(&agente.join("1"));
        let fora = raiz.join("fora");
        fs::create_dir_all(fora.join("1")).unwrap();
        fs::write(fora.join("1/sentinela"), "fora").unwrap();
        symlink(fora.join("1"), agente.join("1/link")).unwrap();
        let dir = abrir_agente(&atual).unwrap();
        fs::rename(&agente, raiz.join("movido")).unwrap();
        symlink(&fora, &agente).unwrap();
        limpar_no_diretorio(&dir, "agente", "2", &registradas(&["1"]));
        assert_eq!(fs::read_to_string(fora.join("1/sentinela")).unwrap(), "fora");
        assert!(!raiz.join("movido/1").exists());
        assert!(raiz.join("movido/2").is_dir());
    }

    #[test]
    fn preserva_versao_em_raiz_gravavel_por_terceiros() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let antiga = raiz.join("agents/agente/1");
        publicar(&antiga);
        fs::set_permissions(&raiz, fs::Permissions::from_mode(0o770)).unwrap();
        limpar(&raiz.join("agents/agente/2"), Some(&registradas(&["1"])));
        assert!(antiga.is_dir());
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn prova_aberta_nao_autoriza_outra_arvore_com_recibo_no_mesmo_nome() {
        let tmp = tempfile::tempdir().unwrap();
        let agente = tmp.path().canonicalize().unwrap().join("agents/agente");
        let antiga = agente.join("1");
        publicar(&antiga);
        let dir = abrir_agente(&agente.join("2")).unwrap();
        let candidata = candidata_com_recibo(&dir, OsStr::new("1")).unwrap();
        fs::rename(&antiga, agente.join("retida")).unwrap();
        publicar(&antiga);
        fs::write(antiga.join("sentinela"), b"preservada").unwrap();

        let erro = remover_versao(&dir, "1", &candidata, "2", &registradas(&["1"])).unwrap_err();
        assert!(erro.to_string().contains("diverge da prova"));
        assert_eq!(fs::read(antiga.join("sentinela")).unwrap(), b"preservada");
        assert!(agente.join("retida/adapter").is_file());
    }

    #[test]
    fn registro_por_nome_nao_autoriza_recibo_via_link() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let antiga = raiz.join("agents/agente/1");
        publicar(&antiga);
        for nome in [super::super::super::arquivo::MARCADOR, super::super::super::arquivo::RECIBO] {
            let arquivo = antiga.join(nome);
            let fora = raiz.join(nome);
            fs::rename(&arquivo, &fora).unwrap();
            symlink(&fora, &arquivo).unwrap();
            limpar(&antiga.with_file_name("2"), Some(&registradas(&["1"])));
            assert!(antiga.join("adapter").is_file());
            fs::rename(fora, arquivo).unwrap();
        }
    }

    #[test]
    fn retomada_preserva_quarentena_sem_prova_ou_com_arvore_divergente() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let agente = raiz.join("agents/agente");
        let chave = uuid::Uuid::new_v4().to_string();
        let quarentena = agente.join(format!("{QUARENTENA}{chave}.1"));
        let arvore = quarentena.join(ARVORE);
        let atual = agente.join("2");
        fs::create_dir_all(&quarentena).unwrap();
        fs::set_permissions(&quarentena, fs::Permissions::from_mode(0o700)).unwrap();
        publicar(&arvore);
        limpar(&atual, Some(&registradas(&["1"])));
        assert!(arvore.join("adapter").is_file());

        let dir = Directory::open(&arvore).unwrap();
        let prova = conteudo_prova(&dir, "1").unwrap();
        fs::rename(&arvore, raiz.join("retida")).unwrap();
        publicar(&arvore);
        fs::write(arvore.join(nome_prova(&chave)), prova).unwrap();
        limpar(&atual, Some(&registradas(&["1"])));
        assert!(arvore.join("adapter").is_file());
    }

    #[test]
    fn proxima_limpeza_retoma_quarentena_parcial_sem_seguir_links() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let agente = raiz.join("agents/agente");
        let atual = agente.join("2");
        let pendente = agente.join("1/restante");
        fs::create_dir_all(&atual).unwrap();
        publicar(&agente.join("1"));
        fs::create_dir_all(&pendente).unwrap();
        fs::set_permissions(&pendente, fs::Permissions::from_mode(0o770)).unwrap();
        limpar(&atual, Some(&registradas(&["1"])));
        assert!(!agente.join("1").exists());
        let quarentena = fs::read_dir(&agente)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(QUARENTENA))
            .unwrap();
        assert!(quarentena.join("versao/restante").is_dir());
        let fora = raiz.join("fora");
        fs::write(&fora, "sentinela").unwrap();
        symlink(&fora, agente.join(format!("{QUARENTENA}{}", uuid::Uuid::new_v4()))).unwrap();
        fs::set_permissions(quarentena.join("versao/restante"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut em_uso = registradas(&[]);
        em_uso.em_uso.push("1".into());
        limpar(&atual, Some(&em_uso));
        assert!(quarentena.join("versao/restante").is_dir());
        limpar(&agente.join("1"), Some(&registradas(&[])));
        assert!(quarentena.join("versao/restante").is_dir());
        limpar(&atual, Some(&registradas(&[])));
        assert!(!quarentena.exists());
        assert!(atual.is_dir());
        assert_eq!(fs::read_to_string(&fora).unwrap(), "sentinela");
    }

    #[test]
    fn falha_ao_retomar_quarentena_registra_agente_versao_e_categoria_sem_caminho() {
        use crate::LockExt;
        use std::io::{self, Write};
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Saida(Arc<Mutex<Vec<u8>>>);
        impl Write for Saida {
            fn write(&mut self, dados: &[u8]) -> io::Result<usize> {
                self.0.locked().write(dados)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap().join("caminho-privado-sentinela");
        let agente = raiz.join("agents/adaptador");
        let atual = agente.join("2.0");
        let pendente = agente.join("1.4/restante");
        fs::create_dir_all(&atual).unwrap();
        publicar(&agente.join("1.4"));
        fs::create_dir_all(&pendente).unwrap();
        fs::set_permissions(&pendente, fs::Permissions::from_mode(0o770)).unwrap();
        limpar(&atual, Some(&registradas(&["1.4"])));
        assert!(!agente.join("1.4").exists());

        let saida = Saida::default();
        let writer = saida.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || limpar(&atual, Some(&registradas(&["1.4"]))));
        let log = String::from_utf8(saida.0.locked().clone()).unwrap();
        for campo in ["agente=adaptador", "versao=1.4", "categoria=validacao"] {
            assert!(log.contains(campo), "{log}");
        }
        for privado in ["caminho-privado-sentinela", ".agente-limpeza-", "gravável por terceiros", "restante"] {
            assert!(!log.contains(privado), "{log}");
        }
    }
}
