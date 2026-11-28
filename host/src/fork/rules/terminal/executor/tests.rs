use super::*;
use crate::fork::sandbox::contrato::Fim;
use std::{fs, sync::mpsc, thread};

const CONSENT: Consent = Consent::OwnerAcceptedExecutionWithoutIsolation;

fn gate() -> Result<Gate> {
    Gate::opaque("bot-ficticio".into(), 1, Decisao::Perguntar, CONSENT)
}

fn prepare<'a>(gate: &'a Gate, cwd: &Path) -> Result<Prepared<'a>> {
    gate.prepare(vec!["/usr/bin/touch".into(), "prova".into()], cwd.into())
}

#[tokio::test]
async fn so_a_aprovacao_do_pedido_exato_inicia_um_processo() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let output = dir.path().join("prova");
    assert!(Gate::opaque("bot-ficticio".into(), 1, Decisao::Perguntar, Consent::Disabled).is_err());
    assert!(Gate::opaque("bot-ficticio".into(), 1, Decisao::Recusar, CONSENT).is_err());
    let gate = gate()?;
    assert!(prepare(&gate, dir.path())?.execute().await.is_err());
    let denied = prepare(&gate, dir.path())?;
    gate.deny(denied.request());
    assert!(gate.approve(denied.request()).is_err());
    assert!(denied.execute().await.is_err());

    let cmd = prepare(&gate, dir.path())?;
    let exact = cmd.request().clone();
    for wrong in [
        Request { id: Uuid::new_v4(), ..exact.clone() },
        Request { bot: "outro-bot".into(), ..exact.clone() },
        Request { turn: 2, ..exact.clone() },
        Request { argv: vec!["/usr/bin/true".into()], ..exact.clone() },
        Request { cwd: dir.path().join("outra-pasta"), ..exact.clone() },
    ] {
        assert!(gate.approve(&wrong).is_err());
    }
    assert!(!output.exists());
    gate.approve(&exact)?;
    assert!(matches!(cmd.execute().await?.fim, Fim::Saiu(0)));
    assert!(output.is_file());
    assert!(gate.approve(&exact).is_err());
    assert!(gate.start(&exact, || -> Result<()> { unreachable!() }).is_err());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn aprovacao_vencida_nao_inicia_processo() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let gate = gate()?;
    let cmd = prepare(&gate, dir.path())?;
    gate.approve(cmd.request())?;
    tokio::time::advance(APPROVAL_TTL).await;
    assert!(cmd.execute().await.is_err());
    assert!(!dir.path().join("prova").exists());
    Ok(())
}

#[tokio::test]
async fn revogacao_concorre_com_inicio_na_mesma_trava() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let gate = gate()?;
    let cmd = prepare(&gate, dir.path())?;
    gate.approve(cmd.request())?;
    let (spawning, locked) = mpsc::channel();
    let (checked, finish) = mpsc::channel();
    thread::scope(|scope| -> Result<()> {
        let gate = &gate;
        let revoke = scope.spawn(move || {
            locked.recv().expect("início sinalizado");
            assert!(matches!(gate.state.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
            checked.send(()).expect("início ainda espera");
            gate.revoke();
        });
        gate.start(cmd.request(), || {
            spawning.send(())?;
            finish.recv()?;
            Ok(())
        })?;
        revoke.join().expect("revogação concluída");
        Ok(())
    })?;
    assert!(cmd.execute().await.is_err());
    assert!(!dir.path().join("prova").exists());

    let gate = Gate::opaque("bot-ficticio".into(), 2, Decisao::Perguntar, CONSENT)?;
    let cmd = prepare(&gate, dir.path())?;
    gate.approve(cmd.request())?;
    thread::scope(|scope| scope.spawn(|| gate.revoke()).join()).expect("revogação concluída");
    assert!(cmd.execute().await.is_err());
    assert!(!dir.path().join("prova").exists());
    Ok(())
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn trocar_o_executavel_e_a_pasta_nao_troca_os_descritores_aprovados() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let gate = gate()?;
    let tool = dir.path().join("ferramenta");
    let cwd = dir.path().join("pasta");
    fs::copy("/usr/bin/touch", &tool)?;
    fs::create_dir(&cwd)?;
    let cmd = gate.prepare(vec![tool.to_string_lossy().into(), "prova".into()], cwd.clone())?;
    gate.approve(cmd.request())?;

    fs::rename(&tool, dir.path().join("ferramenta-original"))?;
    fs::copy("/usr/bin/false", tool)?;
    let original = dir.path().join("pasta-original");
    fs::rename(&cwd, &original)?;
    fs::create_dir(&cwd)?;

    assert!(matches!(cmd.execute().await?.fim, Fim::Saiu(0)));
    assert!(original.join("prova").is_file());
    assert!(!cwd.join("prova").exists());
    Ok(())
}

#[cfg(target_os = "macos")]
pub(in crate::fork) async fn macos_executa_o_pedido_e_recusa_troca_de_executavel_ou_pasta() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let gate = gate()?;
    let cmd = prepare(&gate, dir.path())?;
    gate.approve(cmd.request())?;
    assert!(matches!(cmd.execute().await?.fim, Fim::Saiu(0)));
    assert!(dir.path().join("prova").is_file());

    let tool = dir.path().join("ferramenta");
    let cwd = dir.path().join("pasta");
    fs::copy("/usr/bin/touch", &tool)?;
    fs::create_dir(&cwd)?;
    for swap_executable in [true, false] {
        let cmd = gate.prepare(vec![tool.to_string_lossy().into(), "prova".into()], cwd.clone())?;
        gate.approve(cmd.request())?;
        if swap_executable {
            fs::rename(&tool, dir.path().join("ferramenta-original"))?;
            fs::copy("/usr/bin/touch", &tool)?;
        } else {
            fs::rename(&cwd, dir.path().join("pasta-original"))?;
            fs::create_dir(&cwd)?;
        }
        assert!(cmd.execute().await.is_err());
        assert!(!cwd.join("prova").exists());
    }
    assert!(!dir.path().join("pasta-original/prova").exists());
    Ok(())
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn script_executa_pelo_descritor_aprovado() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir()?;
    let tool = dir.path().join("script");
    fs::write(&tool, "#!/bin/sh\nprintf original > prova\n")?;
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700))?;
    let gate = gate()?;
    let cmd = gate.prepare(vec![tool.to_string_lossy().into_owned()], dir.path().to_path_buf())?;
    gate.approve(cmd.request())?;
    fs::rename(&tool, dir.path().join("script-original"))?;
    fs::write(&tool, "#!/bin/sh\nprintf trocado > prova\n")?;
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700))?;

    assert!(matches!(cmd.execute().await?.fim, Fim::Saiu(0)));
    assert_eq!(fs::read_to_string(dir.path().join("prova"))?, "original");
    Ok(())
}
