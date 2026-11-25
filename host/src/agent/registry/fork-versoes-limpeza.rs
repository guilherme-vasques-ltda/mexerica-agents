use crate::fork::motor_exec::fd::{Directory, name, own};
use anyhow::{Result, ensure};
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// O `:` é impossível num nome de versão do catálogo (`safe_component` recusa): o namespace
/// da quarentena nunca coincide com uma versão válida.
const QUARENTENA: &str = ".agente-limpeza:";

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

fn versao_quarentena(nome: &str) -> Option<&str> {
    let sufixo = nome.strip_prefix(QUARENTENA)?;
    let (chave, versao) = sufixo.split_once('.').unwrap_or((sufixo, ""));
    let uuid = uuid::Uuid::parse_str(chave).ok()?;
    if uuid.to_string() != chave {
        return None;
    }
    Some(super::super::safe_component(versao).unwrap_or("desconhecida"))
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
        if let Some(original) = nome.to_str().and_then(versao_quarentena) {
            if let Err(erro) = agente.quarentena_privada(&nome).and_then(|dir| esvaziar_quarentena(agente, &nome, &dir))
            {
                registrar_falha(id, original, &erro);
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
        if agente.child(&nome, None).is_err() {
            continue;
        }
        if let Err(erro) = remover_versao(agente, versao) {
            registrar_falha(id, versao, &erro);
        }
    }
}

fn remover_versao(agente: &Directory, versao: &str) -> Result<()> {
    let chave = format!("{QUARENTENA}{}", uuid::Uuid::new_v4());
    let nome = OsStr::new(versao);
    let completo = format!("{chave}.{versao}");
    let mut nome_quarentena = OsStr::new(&completo);
    let quarentena = match agente.quarantine(nome, nome_quarentena) {
        Err(erro)
            if erro.downcast_ref::<std::io::Error>().and_then(std::io::Error::raw_os_error)
                == Some(libc::ENAMETOOLONG) =>
        {
            nome_quarentena = OsStr::new(&chave);
            agente.quarantine(nome, nome_quarentena)?
        }
        resultado => resultado?,
    };
    esvaziar_quarentena(agente, nome_quarentena, &quarentena)
}

fn esvaziar_quarentena(agente: &Directory, nome: &OsStr, quarentena: &Directory) -> Result<()> {
    let dev = agente.0.metadata()?.dev();
    ensure!(quarentena.0.metadata()?.dev() == dev, "quarentena em outro volume");
    remover_entrada(quarentena, OsStr::new("versao"), dev)?;
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

    fn unlink(&self, entry: &OsStr, directory: bool) -> Result<()> {
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

    #[test]
    fn troca_ancestral_por_link_nao_remove_sentinela() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let agente = raiz.join("agents/agente");
        let atual = agente.join("2");
        fs::create_dir_all(&atual).unwrap();
        fs::create_dir_all(agente.join("1")).unwrap();
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
        fs::create_dir_all(&antiga).unwrap();
        fs::set_permissions(&raiz, fs::Permissions::from_mode(0o770)).unwrap();
        limpar(&raiz.join("agents/agente/2"), Some(&registradas(&["1"])));
        assert!(antiga.is_dir());
    }

    #[test]
    fn proxima_limpeza_retoma_quarentena_parcial_sem_seguir_links() {
        let tmp = tempfile::tempdir().unwrap();
        let raiz = tmp.path().canonicalize().unwrap();
        let agente = raiz.join("agents/agente");
        let atual = agente.join("2");
        let pendente = agente.join("1/restante");
        fs::create_dir_all(&atual).unwrap();
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
        limpar(&atual, Some(&registradas(&["1"])));
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
