//! Prova, sem executar filhos, de que o serviço do Hub está parado durante a troca.
//!
//! Fontes do kernel: o cgroup da unidade `donnabot-host.service` do gerenciador
//! `systemd --user` (com `KillMode=control-group`, cobre o Hub e todos os filhos)
//! e `/proc`, para um `donnabot-host` iniciado fora da unidade pelo UID do Hub.
//! Só o controlador administrativo imediatamente acima do sudo pode ser excetuado.
//! A exceção por argumentos evita operações acidentais, mas não isola um UID do
//! Hub comprometido: o próprio processo pode alterar seus argumentos. Antes de
//! ativar a atualização privilegiada, separar o executável administrativo do
//! servidor e remover essa exceção.
use super::{armazenamento::Diretorio, chamador::Identidade, slots::Slot};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

const UNIDADE: &str = "donnabot-host.service";
const NOME: &str = "donnabot-host";
const MAX_PROCESSOS: usize = 32768;

fn ausente(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ESRCH))
}

/// Recusa se o cgroup da unidade existir com processos. Ausência = parado.
fn cgroup_vazio(raiz: &Path, uid_hub: u32) -> Result<()> {
    let base = raiz.join(format!("user.slice/user-{uid_hub}.slice/user@{uid_hub}.service"));
    for relativo in [format!("app.slice/{UNIDADE}"), UNIDADE.to_owned()] {
        match fs::read_to_string(base.join(&relativo).join("cgroup.events")) {
            Ok(texto) => {
                ensure!(texto.len() <= 4096, "cgroup.events inválido");
                let populado =
                    texto.lines().find_map(|l| l.strip_prefix("populated ")).context("cgroup.events sem populated")?;
                ensure!(populado == "0", "serviço do Hub em execução durante a troca; pare-o antes");
            }
            Err(e) if ausente(&e) => (),
            Err(_) => bail!("não foi possível verificar o cgroup do serviço do Hub"),
        }
    }
    Ok(())
}

pub(super) fn conferir_fs(raiz: &Path, tipo: u32) -> Result<File> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(raiz)?;
    // SAFETY: statfs só contém campos escalares e arrays de inteiros.
    let mut sistema: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        // SAFETY: descritor aberto e ponteiro para estrutura inicializada.
        unsafe { libc::fstatfs(dir.as_raw_fd(), &raw mut sistema) } == 0
            && u32::try_from(sistema.f_type).ok() == Some(tipo),
        "filesystem do kernel não confiável: {}",
        raiz.display()
    );
    Ok(dir)
}

type Inode = (u64, u64);

fn inode(meta: &Metadata) -> Inode {
    (meta.dev(), meta.ino())
}

fn opcional<T>(r: io::Result<T>) -> Result<Option<T>> {
    match r {
        Ok(valor) => Ok(Some(valor)),
        Err(e) if ausente(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn uids(base: &Path) -> Result<Option<[u32; 4]>> {
    let Some(status) = opcional(fs::read_to_string(base.join("status")))? else { return Ok(None) };
    let linha = status.lines().find_map(|l| l.strip_prefix("Uid:")).context("credenciais ausentes")?;
    let uids: Vec<u32> = linha.split_whitespace().map(str::parse).collect::<Result<_, _>>()?;
    Ok(Some(uids.try_into().map_err(|_| anyhow::anyhow!("credenciais inválidas"))?))
}

fn pai(proc: &Path, pid: u32) -> Result<u32> {
    let texto = fs::read_to_string(proc.join(pid.to_string()).join("stat"))?;
    let resto = texto.rsplit_once(')').context("stat inválido")?.1;
    Ok(resto.split_whitespace().nth(1).context("ppid ausente")?.parse()?)
}

fn acima_do_sudo(proc: &Path, proprio: u32, sudo: Inode) -> Result<Option<u32>> {
    let mut atual = pai(proc, proprio)?;
    for _ in 0..2 {
        if atual <= 1
            || opcional(fs::metadata(proc.join(atual.to_string()).join("exe")))?.as_ref().map(inode) != Some(sudo)
        {
            return Ok(None);
        }
        let superior = pai(proc, atual)?;
        if superior > 1
            && opcional(fs::metadata(proc.join(superior.to_string()).join("exe")))?.as_ref().map(inode) != Some(sudo)
        {
            return Ok(Some(superior));
        }
        atual = superior;
    }
    Ok(None)
}

fn processo_do_hub(base: &Path, uid_hub: u32, slots: &[Inode], candidato: bool) -> Result<bool> {
    let Some(uids) = uids(base)? else { return Ok(false) };
    let real = opcional(fs::metadata(base.join("exe")))?.as_ref().map(inode);
    let gerido = real.is_some_and(|i| slots.contains(&i));
    let legado = if !gerido && uids.contains(&uid_hub) {
        opcional(fs::read_to_string(base.join("comm")))?.is_some_and(|n| n.trim_end() == NOME)
            || opcional(fs::read_link(base.join("exe")))?
                .is_some_and(|p| p.file_name().is_some_and(|n| n == NOME || n == "donnabot-host (deleted)"))
    } else {
        false
    };
    if (gerido || legado) && candidato && real.is_some() && uids == [uid_hub; 4] {
        // Consistência do fluxo administrativo, não prova de autoridade: argv é mutável pelo processo.
        let mut argv = Vec::new();
        File::open(base.join("cmdline"))?.take(65_537).read_to_end(&mut argv)?;
        if argv.len() <= 65_536 && argv.last() == Some(&0) {
            let comando = argv.split(|b| *b == 0).nth(1);
            if comando == Some(b"coordenar-atualizacao") || (gerido && comando == Some(b"instalar-gerido")) {
                return Ok(false);
            }
        }
    }
    Ok(gerido || legado)
}

/// Falha fechada se houver qualquer sinal de execução do Hub.
pub fn exigir_parado(publicacao: &Diretorio, uid_hub: u32, chamador: Identidade) -> Result<()> {
    let mut slots = Vec::new();
    for slot in [Slot::A, Slot::B] {
        if publicacao.existe(slot.nome())?
            && let Some(hub) = super::transacao::abrir_fixo(&publicacao.abrir_slot(slot)?, c"donnabot-host")?
        {
            slots.push(inode(&hub.metadata()?));
        }
    }
    exigir_parado_com(uid_hub, chamador, &slots)
}

pub(super) fn exigir_parado_sem_slots(uid_hub: u32) -> Result<()> {
    exigir_parado_com(uid_hub, Identidade::AdminRoot, &[])
}

fn exigir_parado_com(uid_hub: u32, chamador: Identidade, slots: &[Inode]) -> Result<()> {
    ensure!(uid_hub != 0, "UID do Hub inválido");
    let proc_fd = conferir_fs(Path::new("/proc"), 0x9fa0)?;
    let proc = PathBuf::from(format!("/proc/self/fd/{}", proc_fd.as_raw_fd()));
    let cgroup = conferir_fs(Path::new("/sys/fs/cgroup"), 0x6367_7270)?;
    cgroup_vazio(&PathBuf::from(format!("/proc/self/fd/{}", cgroup.as_raw_fd())), uid_hub)?;
    let proprio = std::process::id();
    let candidato = if chamador == Identidade::Sudo(uid_hub) {
        acima_do_sudo(&proc, proprio, inode(&super::chamador::sudo_confiavel()?))?
    } else {
        None
    };
    let mut total = 0usize;
    for entrada in fs::read_dir(proc)? {
        let entrada = entrada.context("falha ao enumerar processos")?;
        let Some(pid) = entrada.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        total += 1;
        ensure!(total <= MAX_PROCESSOS, "limite de processos excedido");
        if pid == proprio {
            continue;
        }
        ensure!(
            !processo_do_hub(&entrada.path(), uid_hub, slots, candidato == Some(pid))?,
            "processo do Hub em execução durante a troca (pid {pid})"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn cgroup_populado_recusa_e_ausente_ou_vazio_aceita() -> Result<()> {
        let raiz = std::env::temp_dir().join(format!("mx365-cg-{}", uuid::Uuid::new_v4()));
        let unidade = raiz.join("user.slice/user-1234.slice/user@1234.service/app.slice/donnabot-host.service");
        assert!(cgroup_vazio(&raiz, 1234).is_ok());
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&unidade)?;
        fs::write(unidade.join("cgroup.events"), "populated 0\nfrozen 0\n")?;
        assert!(cgroup_vazio(&raiz, 1234).is_ok());
        fs::write(unidade.join("cgroup.events"), "populated 1\nfrozen 0\n")?;
        assert!(cgroup_vazio(&raiz, 1234).is_err());
        fs::write(unidade.join("cgroup.events"), "frozen 0\n")?;
        assert!(cgroup_vazio(&raiz, 1234).is_err());
        fs::remove_dir_all(raiz)?;
        Ok(())
    }

    #[test]
    fn identidade_do_hub_e_excecao_administrativa_estreita() -> Result<()> {
        let base = tempfile::tempdir()?;
        let base = base.path();
        fs::write(base.join("status"), "Uid:\t1234\t1234\t1234\t1234\n")?;
        fs::write(base.join("comm"), "outro-nome\n")?;
        fs::write(base.join("exe"), "binario inerte")?;
        let slots = [inode(&fs::metadata(base.join("exe"))?)];
        for argv in [b"hub\0serve\0".as_slice(), b"hub\0"] {
            fs::write(base.join("cmdline"), argv)?;
            assert!(processo_do_hub(base, 1234, &slots, true)?);
        }
        for argv in [b"hub\0coordenar-atualizacao\0".as_slice(), b"hub\0instalar-gerido\0"] {
            fs::write(base.join("cmdline"), argv)?;
            assert!(!processo_do_hub(base, 1234, &slots, true)?);
            assert!(processo_do_hub(base, 1234, &slots, false)?);
        }
        Ok(())
    }

    #[test]
    fn candidato_esta_logo_acima_dos_processos_sudo_identificados() -> Result<()> {
        use std::os::unix::fs::symlink;

        let proc = tempfile::tempdir()?;
        let proc = proc.path();
        let sudo = proc.join("executavel-administrativo");
        let outro = proc.join("outro-executavel");
        fs::write(&sudo, b"duble")?;
        fs::write(&outro, b"duble distinto")?;
        for (pid, pai, exe) in [(5, 4, &outro), (4, 3, &sudo), (3, 2, &sudo), (2, 1, &outro)] {
            let dir = proc.join(pid.to_string());
            fs::create_dir(&dir)?;
            fs::write(dir.join("stat"), format!("{pid} (nome com ) parêntese) S {pai}\n"))?;
            symlink(exe, dir.join("exe"))?;
        }
        let sudo = inode(&fs::metadata(sudo)?);
        assert_eq!(acima_do_sudo(proc, 5, sudo)?, Some(2));
        assert_eq!(acima_do_sudo(proc, 4, sudo)?, Some(2));
        assert_eq!(acima_do_sudo(proc, 3, sudo)?, None);
        assert!(conferir_fs(proc, 0x9fa0).is_err());
        assert!(conferir_fs(proc, 0x6367_7270).is_err());
        Ok(())
    }
}
