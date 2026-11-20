//! Confirmação antes de executar o script de instalação de um fornecedor (GUI-145).
//!
//! Cerca de vinte motores são instalados pelo script do próprio fornecedor (`curl … | bash`,
//! baixado por https e executado sem hash fixado); a trava de integridade do registro de
//! agentes cobre só os binários que o Hub baixa, não esses scripts. Enquanto não houver canal
//! com checksum fixável para cada um, o script não roda sozinho: o terminal de instalação
//! mostra o comando exato e espera a pessoa que está na frente confirmar. A trava fica no
//! ponto de execução, então vale para toda entrada que abre o terminal (app, TUI, celular),
//! sem depender de cada tela.
//!
//! Fail-closed: sem confirmação, nada é executado; resposta inválida, fim de entrada ou
//! interrupção cancelam. O comando confirmado é o mesmo que o cabeçalho do terminal mostra,
//! montado a partir da mesma string de `backends::Install`; nada é baixado antes do "s".

/// A pergunta e o aviso que o terminal mostra antes de qualquer download.
#[cfg(unix)]
const AVISO_UNIX: &str = "Este comando baixa o instalador do fornecedor e o executa no seu computador; a Mexerica não confere o conteúdo antes de rodar.";
#[cfg(unix)]
const PERGUNTA_UNIX: &str = "Rodar agora? [s/N]";
#[cfg(unix)]
const CANCELADO_UNIX: &str = "Cancelado: nada foi executado.";
/// O `cmd` não garante UTF-8, então a mensagem do Windows é ASCII.
#[cfg(windows)]
const PERGUNTA_WINDOWS: &str = "Rodar este comando agora? [S/N]";
#[cfg(windows)]
const CANCELADO_WINDOWS: &str = "Cancelado: nada foi executado.";

/// `comando` (a linha de instalação de `backends::Install`) embrulhada para exigir
/// confirmação no terminal antes de executar.
#[cfg(unix)]
pub fn linha_com_confirmacao(comando: &str) -> String {
    format!(
        "printf '{AVISO_UNIX}\\n{PERGUNTA_UNIX} '\n\
         IFS= read -r mexerica_instalar\n\
         case \"$mexerica_instalar\" in\n\
         \x20   [sS]|[yY]) {comando} ;;\n\
         \x20   *) printf '\\n{CANCELADO_UNIX}\\n'; exit 130 ;;\n\
         esac"
    )
}

/// O mesmo no Windows: `choice` pede S/N e devolve 1 para S e 2 para N; a linha só executa
/// `comando` quando a resposta é S. Qualquer outra saída de `choice` (inclusive erro) cai no
/// `if errorlevel 2`, que cancela. A mensagem é ASCII porque o `cmd` não garante UTF-8.
#[cfg(windows)]
pub fn linha_com_confirmacao(comando: &str) -> String {
    format!("choice /c SN /m \"{PERGUNTA_WINDOWS}\" && if errorlevel 2 (echo {CANCELADO_WINDOWS}) else {comando}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::io::{AsyncWriteExt, BufWriter};

    /// Script de instalação real da lista, para os testes de forma e de comportamento.
    #[cfg(unix)]
    const EXEMPLO: &str = "curl -fsSL https://ampcode.com/install.sh | bash";

    async fn rodar(linha: &str, resposta: &[u8]) -> (Option<i32>, String) {
        let mut child = crate::shell::line(linha).tokio().stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut entrada = BufWriter::new(child.stdin.take().unwrap());
        entrada.write_all(resposta).await.unwrap();
        entrada.flush().await.unwrap();
        drop(entrada);
        let saida = child.wait_with_output().await.unwrap();
        (saida.status.code(), String::from_utf8_lossy(&saida.stdout).into_owned())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pergunta_antes_e_so_executa_com_sim() {
        let linha = linha_com_confirmacao(EXEMPLO);
        assert!(linha.contains(PERGUNTA_UNIX));
        // O comando confirmado é o que aparece: inteiro, uma única vez, sem trapaça no meio.
        assert_eq!(linha.matches(EXEMPLO).count(), 1);
        // Confirmar "n" executa só o cancelamento; nada é baixado.
        let (codigo, saida) = rodar(&linha, b"n\n").await;
        assert_eq!(codigo, Some(130), "cancelar marca que nada rodou, sem virar sucesso da instalação");
        assert!(saida.contains(CANCELADO_UNIX));
        // Confirmar "s" executa o comando embrulhado.
        let (codigo, saida) = rodar(&linha_com_confirmacao("printf 'instalado-pelo-teste'"), b"s\n").await;
        assert_eq!(codigo, Some(0));
        assert!(saida.contains(PERGUNTA_UNIX));
        assert!(saida.contains("instalado-pelo-teste"));
    }

    /// Ctrl-D sem nada digitado (fim de entrada) responde como não: nada executa.
    /// Com "s" digitado o `read` já atribuiu a resposta, então o fim de entrada depois
    /// vale como o "s" da pessoa — o teste cubre só o fim de entrada em linha vazia.
    #[cfg(unix)]
    #[tokio::test]
    async fn fim_de_entrada_cancela() {
        let (codigo, saida) = rodar(&linha_com_confirmacao("printf 'nao-deve-aparecer'"), b"").await;
        assert_eq!(codigo, Some(130));
        assert!(!saida.contains("nao-deve-aparecer"));
    }

    #[cfg(windows)]
    #[test]
    fn wrapper_windows_cancela_sem_s() {
        let linha = linha_com_confirmacao("echo comando-do-fornecedor");
        assert!(linha.contains("choice /c SN"));
        assert!(linha.contains("if errorlevel 2"));
        // Sem S cai no cancelamento; o comando do fornecedor vem só depois do `else`.
        let (_, depois) = linha.split_once(") else ").unwrap();
        assert_eq!(depois, "echo comando-do-fornecedor");
    }
}
