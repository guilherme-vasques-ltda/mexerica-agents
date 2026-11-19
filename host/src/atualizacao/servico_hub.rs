//! Prova, sem executar filhos, de que o serviço do Hub está parado durante a troca.
//!
//! Fontes do kernel: o cgroup da unidade `donnabot-host.service` do gerenciador
//! `systemd --user` (com `KillMode=control-group`, cobre o Hub e todos os filhos)
//! e `/proc`, para um `donnabot-host` iniciado fora da unidade pelo UID do Hub.
//! Ancestrais do próprio wrapper (o coordenador que chamou o sudo) são ignorados.
use anyhow::{Context, Result, bail, ensure};
use std::{collections::BTreeSet, fs, io, path::Path};

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

fn ancestrais() -> Result<BTreeSet<u32>> {
    let mut pids = BTreeSet::new();
    let mut atual = std::process::id();
    for _ in 0..64 {
        let texto = fs::read_to_string(format!("/proc/{atual}/stat"))?;
        // Campo 4 após o nome entre parênteses (que pode conter espaços).
        let resto = texto.rsplit_once(')').context("stat inválido")?.1;
        let pai: u32 = resto.split_whitespace().nth(1).context("ppid ausente")?.parse()?;
        if pai <= 1 {
            break;
        }
        pids.insert(pai);
        atual = pai;
    }
    Ok(pids)
}

fn processo_do_hub(base: &Path, uid_hub: u32) -> Result<bool> {
    let nome = match fs::read_to_string(base.join("comm")) {
        Ok(n) => n,
        Err(e) if ausente(&e) => return Ok(false),
        Err(_) => bail!("não foi possível ler nome de processo"),
    };
    if nome.trim_end() != NOME {
        return Ok(false);
    }
    let status = match fs::read_to_string(base.join("status")) {
        Ok(s) => s,
        Err(e) if ausente(&e) => return Ok(false),
        Err(_) => bail!("não foi possível ler credenciais de processo"),
    };
    let linha = status.lines().find_map(|l| l.strip_prefix("Uid:")).context("credenciais ausentes")?;
    let uids: Vec<u32> = linha.split_whitespace().map(str::parse).collect::<Result<_, _>>()?;
    Ok(uids.contains(&uid_hub))
}

/// Falha fechada se houver qualquer sinal de execução do Hub.
pub fn exigir_parado(uid_hub: u32) -> Result<()> {
    ensure!(uid_hub != 0, "UID do Hub inválido");
    cgroup_vazio(Path::new("/sys/fs/cgroup"), uid_hub)?;
    let ignorar = ancestrais()?;
    let proprio = std::process::id();
    let mut total = 0usize;
    for entrada in fs::read_dir("/proc")? {
        let entrada = entrada.context("falha ao enumerar processos")?;
        let Some(pid) = entrada.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        total += 1;
        ensure!(total <= MAX_PROCESSOS, "limite de processos excedido");
        if pid == proprio || ignorar.contains(&pid) {
            continue;
        }
        ensure!(!processo_do_hub(&entrada.path(), uid_hub)?, "processo do Hub em execução durante a troca (pid {pid})");
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
    fn ancestrais_incluem_o_pai_e_nao_o_proprio() -> Result<()> {
        let pids = ancestrais()?;
        assert!(!pids.contains(&std::process::id()));
        // SAFETY: consulta sem efeitos colaterais.
        let pai = u32::try_from(unsafe { libc::getppid() })?;
        assert!(pai <= 1 || pids.contains(&pai));
        Ok(())
    }
}
