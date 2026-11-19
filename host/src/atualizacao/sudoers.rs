//! Conteúdo restrito para futura etapa administrativa, ainda sem instalação.
use anyhow::{Result, ensure};

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
            "Cmnd_Alias DONNABOT_ATUALIZAR = /opt/donnabot/sbin/atualizar-privilegiado \"\", /opt/donnabot/sbin/atualizar-privilegiado --reverter, /opt/donnabot/sbin/atualizar-privilegiado --aceitar\n",
            "Defaults!DONNABOT_ATUALIZAR env_reset\n",
            "Defaults!DONNABOT_ATUALIZAR !setenv\n",
            "{login} ALL=(root) NOPASSWD: NOEXEC: DONNABOT_ATUALIZAR\n"
        ),
        login = login
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apenas_login_literal_e_tres_comandos_fixos() {
        let texto = fragmento("hublogin", 1000).unwrap();
        assert!(texto.contains("atualizar-privilegiado \"\", /opt/donnabot/sbin/atualizar-privilegiado --reverter"));
        assert!(texto.contains("hublogin ALL=(root) NOPASSWD: NOEXEC: DONNABOT_ATUALIZAR\n"));
        for login in ["", "123", "hub all", "hub\nroot", "hub,root", "hub%wheel", "hub:root", "hub/root"] {
            assert!(fragmento(login, 1000).is_err());
        }
        assert!(fragmento("hublogin", 0).is_err());
    }
}
