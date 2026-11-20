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
//! Fail-closed: nada é executado sem uma linha confirmada — só o "s" da pergunta. Linha
//! vazia, resposta fora da lista e fim de entrada cancelam com saída 130 (não é sucesso da
//! instalação); um fim de entrada depois de texto parcial também cancela: o `read` pode
//! falhar deixando o texto na variável, e o retorno dele é quem decide. Interrupção encerra
//! o terminal sem executar nada. O comando confirmado é o mesmo que o cabeçalho do terminal
//! mostra, montado a partir da mesma string de `backends::Install`; nada é baixado antes do "s".

/// A pergunta e o aviso que o terminal mostra antes de qualquer download.
#[cfg(unix)]
const AVISO_UNIX: &str = "Este comando baixa o instalador do fornecedor e o executa no seu computador; a Mexerica não confere o conteúdo antes de rodar.";
#[cfg(unix)]
const PERGUNTA_UNIX: &str = "Rodar agora? [s/N]";
#[cfg(unix)]
const CANCELADO_UNIX: &str = "Cancelado: nada foi executado.";
/// O `cmd` não garante UTF-8, então as mensagens do Windows são ASCII; o `choice` já
/// acrescenta o "[S,N]?" ao final da pergunta.
#[cfg(any(windows, test))]
const AVISO_WINDOWS: &str = "Este comando baixa o instalador do fornecedor e o executa no computador; a Mexerica nao confere o conteudo antes de rodar.";
#[cfg(any(windows, test))]
const PERGUNTA_WINDOWS: &str = "Rodar este comando agora?";
#[cfg(any(windows, test))]
const CANCELADO_WINDOWS: &str = "Cancelado: nada foi executado.";

/// `comando` (a linha de instalação de `backends::Install`) embrulhada para exigir
/// confirmação no terminal antes de executar.
#[cfg(unix)]
pub fn linha_com_confirmacao(comando: &str) -> String {
    linha_unix(comando)
}

#[cfg(unix)]
fn linha_unix(comando: &str) -> String {
    format!(
        "printf '{AVISO_UNIX}\\n{PERGUNTA_UNIX} '\n\
         if IFS= read -r mexerica_instalar; then\n\
         \x20   case \"$mexerica_instalar\" in\n\
         \x20       [sS]) {comando} ;;\n\
         \x20       *) printf '\\n{CANCELADO_UNIX}\\n'; exit 130 ;;\n\
         \x20   esac\n\
         else\n\
         \x20   printf '\\n{CANCELADO_UNIX}\\n'; exit 130\n\
         fi"
    )
}

/// Windows: `choice` devolve no ERRORLEVEL a resposta, não um código de sucesso
/// (1 = S, 2 = N, 0 = interrupção, 255 = erro), então o `if` roda sempre (`&`, nunca `&&`)
/// e cada desfecho tem seu ramo: só o ERRORLEVEL 1, o "S", executa; N, interrupção e erro
/// cancelam com saída 130. O comando vem solto no fim, sem parênteses em volta: os
/// instaladores Windows chamam o PowerShell com parênteses dentro das aspas, e um bloco
/// aberto seria fechado antes da hora. Invariante para quem acrescentar instaladores no
/// futuro: o comando não pode ter pipe, `&` ou `%` fora de aspas (todos os atuais respeitam).
/// As mensagens são ASCII porque o `cmd` não garante UTF-8.
#[cfg(windows)]
pub fn linha_com_confirmacao(comando: &str) -> String {
    linha_windows(comando)
}

/// O construtor puro, compilado também nos testes do Linux para a forma poder ser
/// conferida onde o `cmd` não roda; o comportamento do `cmd` em si só é verificável no
/// Windows.
#[cfg(any(windows, test))]
fn linha_windows(comando: &str) -> String {
    format!(
        "echo {AVISO_WINDOWS} \
         & choice /c SN /m \"{PERGUNTA_WINDOWS}\" \
         & if errorlevel 2 (echo {CANCELADO_WINDOWS} & exit 130) \
         else if not errorlevel 1 (echo {CANCELADO_WINDOWS} & exit 130) \
         else {comando}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::process::Stdio;
    #[cfg(unix)]
    use tokio::io::{AsyncWriteExt, BufWriter};

    /// Script de instalação real da lista, para os testes de forma e de comportamento.
    #[cfg(unix)]
    const EXEMPLO: &str = "curl -fsSL https://ampcode.com/install.sh | bash";

    #[cfg(unix)]
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

    /// Fim de entrada em linha vazia cancela: nada executa.
    #[cfg(unix)]
    #[tokio::test]
    async fn fim_de_entrada_cancela() {
        let (codigo, saida) = rodar(&linha_com_confirmacao("printf 'nao-deve-aparecer'"), b"").await;
        assert_eq!(codigo, Some(130));
        assert!(saida.contains(CANCELADO_UNIX));
        assert!(!saida.contains("nao-deve-aparecer"));
    }

    /// Fim de entrada depois de texto parcial NÃO autoriza: o `read` falha deixando o texto
    /// na variável, e é o retorno dele quem decide, não o conteúdo.
    #[cfg(unix)]
    #[tokio::test]
    async fn fim_de_entrada_parcial_cancela() {
        let (codigo, saida) = rodar(&linha_com_confirmacao("printf 'nao-deve-aparecer'"), b"s").await;
        assert_eq!(codigo, Some(130), "s sem Enter não é linha confirmada");
        assert!(saida.contains(CANCELADO_UNIX));
        assert!(!saida.contains("nao-deve-aparecer"));
    }

    /// Linha vazia e resposta fora da lista cancelam do mesmo jeito.
    #[cfg(unix)]
    #[tokio::test]
    async fn linha_vazia_e_resposta_fora_da_lista_cancelam() {
        for resposta in ["\n", "x\n", "y\n", "sim e depois algo\n"] {
            let (codigo, saida) =
                rodar(&linha_com_confirmacao("printf 'nao-deve-aparecer'"), resposta.as_bytes()).await;
            assert_eq!(codigo, Some(130), "resposta {resposta:?}");
            assert!(saida.contains(CANCELADO_UNIX));
            assert!(!saida.contains("nao-deve-aparecer"), "resposta {resposta:?}");
        }
    }

    /// No Windows os quatro desfechos têm ramo próprio: só o ERRORLEVEL 1 (o "S") executa;
    /// N (2), interrupção (0) e erro (255) cancelam com saída 130. Roda também no Linux
    /// porque o construtor é string pura — o comportamento do `cmd` em si só é verificável
    /// no Windows.
    #[test]
    fn wrapper_windows_executa_somente_com_s() {
        let linha = linha_windows("echo comando-do-fornecedor");
        assert!(linha.contains("choice /c SN"), "{linha}");
        // O aviso vem antes da pergunta; o `choice` é que acrescenta o "[S,N]?".
        assert!(linha.contains(&format!("echo {AVISO_WINDOWS} & choice /c SN")), "{linha}");
        // `&` e nunca `&&`: a resposta de `choice` é o ERRORLEVEL (1 = S, 2 = N), não um código
        // de sucesso; com `&&` o "S" (1) nunca executaria e a interrupção (0) executaria.
        assert!(linha.contains("\" & if "), "{linha}");
        assert!(!linha.contains("&&"), "{linha}");
        // O comando vem solto no fim, depois do último `else`, e só uma vez: nenhum ramo de
        // cancelamento o contém, e sem parênteses em volta os parênteses do PowerShell não
        // fecham bloco nenhum.
        assert_eq!(linha.matches("comando-do-fornecedor").count(), 1);
        let (_, comando) = linha.rsplit_once(" else ").unwrap();
        assert_eq!(comando, "echo comando-do-fornecedor");
        assert!(linha.contains("if errorlevel 2 (echo Cancelado: nada foi executado. & exit 130)"), "{linha}");
        assert!(linha.contains("else if not errorlevel 1 (echo Cancelado: nada foi executado. & exit 130)"), "{linha}");
    }
}
