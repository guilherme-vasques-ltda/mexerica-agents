//! Conteúdo restrito para futura etapa administrativa, ainda sem instalação.
use anyhow::{Context, Result, ensure};

pub(super) const WRAPPER: &str = "/opt/donnabot/sbin/atualizar-privilegiado";
pub(super) const FORMAS_WRAPPER: [&[&str]; 3] = [&[], &["--reverter"], &["--aceitar"]];

fn comandos() -> [String; 3] {
    FORMAS_WRAPPER.map(|args| format!("{WRAPPER} {}", args.first().copied().unwrap_or("\"\"")))
}

pub fn fragmento(login: &str, uid: u32) -> Result<String> {
    let mut letras = login.bytes();
    let primeiro = letras.next();
    ensure!(
        uid != 0
            && login.len() <= 32
            && primeiro.is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
            && letras.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'),
        "login local incompatível com sudoers"
    );
    Ok(format!(
        concat!(
            "Cmnd_Alias DONNABOT_ATUALIZAR = {comandos}\n",
            "Defaults!DONNABOT_ATUALIZAR env_reset\n",
            "Defaults!DONNABOT_ATUALIZAR !setenv\n",
            "Defaults!DONNABOT_ATUALIZAR !env_keep\n",
            "{login} ALL=(root) NOPASSWD: NOEXEC: DONNABOT_ATUALIZAR\n"
        ),
        login = login,
        comandos = comandos().join(", "),
    ))
}

pub(super) fn conferir_lista(lista: &str, login: &str) -> Result<()> {
    let cabecalho = format!("User {login} may run the following commands on ");
    let mut linhas = lista.lines();
    ensure!(
        linhas.by_ref().any(|linha| linha.strip_prefix(&cabecalho).is_some_and(|fim| fim.ends_with(':'))),
        "sudo -l sem lista de comandos reconhecida"
    );
    let comandos = comandos();
    let mut vistos = [false; 3];
    let mut continuacao = false;
    for linha in linhas {
        let limpa = linha.trim();
        if limpa.is_empty() {
            ensure!(!continuacao, "sudo -l lista continuação incompleta");
            continue;
        }
        let (corpo, mut tags) = if continuacao {
            ensure!(linha.len() > linha.trim_start().len(), "sudo -l lista continuação sem recuo");
            (limpa, [true; 2])
        } else {
            (limpa.strip_prefix("(root)").context("sudo -l lista outra regra")?.trim_start(), [false; 2])
        };
        let (corpo, segue) = corpo.strip_suffix(',').map_or((corpo, false), |resto| (resto, true));
        for item in corpo.split(',').map(str::trim) {
            let mut comando = item;
            while let Some((i, resto)) = ["NOPASSWD:", "NOEXEC:"]
                .iter()
                .enumerate()
                .find_map(|(i, tag)| comando.strip_prefix(tag).map(|resto| (i, resto)))
            {
                ensure!(!continuacao, "sudo -l lista tag na continuação");
                tags[i] = true;
                comando = resto.trim_start();
            }
            ensure!(tags == [true; 2], "sudo -l lista comando sem restrição");
            let i =
                comandos.iter().position(|permitido| permitido == comando).context("sudo -l lista outro comando")?;
            ensure!(!vistos[i], "sudo -l lista regra repetida");
            vistos[i] = true;
        }
        continuacao = segue;
    }
    ensure!(!continuacao, "sudo -l lista continuação incompleta");
    ensure!(vistos == [true; 3], "sudo -l não lista todas as formas restritas do wrapper");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apenas_login_literal_e_tres_comandos_fixos() {
        let texto = fragmento("hublogin", 1000).unwrap();
        assert!(texto.contains("atualizar-privilegiado \"\", /opt/donnabot/sbin/atualizar-privilegiado --reverter"));
        assert!(texto.contains("hublogin ALL=(root) NOPASSWD: NOEXEC: DONNABOT_ATUALIZAR\n"));
        assert!(texto.contains("Defaults!DONNABOT_ATUALIZAR !env_keep\n"));
        for login in ["", "123", "hub all", "hub\nroot", "hub,root", "hub%wheel", "hub:root", "hub/root"] {
            assert!(fragmento(login, 1000).is_err());
        }
        assert!(fragmento("hublogin", 0).is_err());
    }

    #[test]
    fn lista_aceita_so_as_tres_formas_restritas() -> Result<()> {
        let cabecalho = "Matching Defaults entries for hubteste on example.invalid:\n    env_reset\n\n\
            User hubteste may run the following commands on example.invalid:\n";
        let regras = comandos().map(|cmd| format!("    (root) NOPASSWD: NOEXEC: {cmd}"));
        let lista = format!("{cabecalho}{}\n", regras.join("\n"));
        conferir_lista(&lista, "hubteste")?;
        conferir_lista(&format!("{cabecalho}(root) NOEXEC: NOPASSWD: {}\n", comandos().join(", ")), "hubteste")?;
        for extra in
            ["(ALL) ALL", "(ALL:ALL) ALL", "(ALL : ALL) NOPASSWD: ALL", "(root) NOPASSWD: ALL", "(ALL) NOPASSWD: ALL"]
        {
            assert!(conferir_lista(&format!("{lista}    {extra}\n"), "hubteste").is_err());
        }
        for invalida in [
            lista.replace("NOEXEC:", "EXEC:"),
            lista.replace("--aceitar", "*"),
            lista.replace(" \"\"", ""),
            lista.replace(&regras[0], ""),
            format!("{lista}{}\n", regras[0]),
        ] {
            assert!(conferir_lista(&invalida, "hubteste").is_err());
        }
        Ok(())
    }

    #[test]
    fn lista_aceita_mais_de_uma_linha_so_na_mesma_regra() -> Result<()> {
        let cabecalho = "User hubteste may run the following commands on example.invalid:\n";
        let [sem_args, reverter, aceitar] = comandos();
        let lista = |corpo| format!("{cabecalho}{corpo}\n");
        let inicio = format!("    (root) NOPASSWD: NOEXEC: {sem_args},");
        conferir_lista(&lista(format!("{inicio}\n        {reverter},\n        {aceitar}")), "hubteste")?;
        conferir_lista(&lista(format!("{inicio}\n\t{reverter}, {aceitar}")), "hubteste")?;
        for invalida in [
            format!("{inicio}\n{reverter}, {aceitar}"),
            format!("{inicio}\n\n        {reverter}, {aceitar}"),
            format!("{inicio}\n    (root) NOPASSWD: NOEXEC: {reverter}, {aceitar}"),
            format!("{inicio}\n    (root : root) NOPASSWD: NOEXEC: {reverter}, {aceitar}"),
            format!("{inicio}\n        EXEC: {reverter}, {aceitar}"),
            format!("{inicio}\n        NOPASSWD: NOEXEC: {reverter}, {aceitar}"),
            format!("{inicio}\n        {reverter}, {aceitar}, ALL"),
            format!("{inicio}\n        , {reverter}, {aceitar}"),
            format!("{inicio}\n        {reverter}, {aceitar},"),
            format!("{inicio}\n        {reverter}"),
            format!("{}\n        {reverter}, {aceitar}", inicio.trim_end_matches(',')),
            format!("    (root) NOPASSWD: NOEXEC: ,\n        {sem_args}, {reverter}, {aceitar}"),
            format!("    (root) NOPASSWD: {sem_args},\n        NOEXEC: {reverter}, {aceitar}"),
        ] {
            assert!(conferir_lista(&lista(invalida), "hubteste").is_err());
        }
        Ok(())
    }
}
