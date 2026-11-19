//! Interface privilegiada fechada: sem argumentos (atualizar), `--reverter` ou `--aceitar`.
#[path = "../atualizacao/mod.rs"]
mod atualizacao;

fn main() {
    let argumentos: Vec<_> = std::env::args_os().skip(1).take(2).collect();
    #[cfg(target_os = "linux")]
    {
        // Consulta sem privilégio e sem efeitos para detectar o contrato de aceite.
        if argumentos.as_slice() == [std::ffi::OsString::from("--protocolo-verificacao")] {
            println!("mexerica-verificacao-v1");
            return;
        }
        // Forma não permitida: recusa sem tocar em estado algum.
        let Some(operacao) = atualizacao::entrada::interpretar(&argumentos) else { std::process::exit(1) };
        // SUDO_UID é lido antes de descartar o ambiente na normalização.
        let chamador = atualizacao::chamador::capturar();
        let verificacao = if matches!(operacao, atualizacao::entrada::Operacao::Aceitar) && chamador.is_ok() {
            atualizacao::entrada::ler_verificacao()
        } else {
            Ok(None)
        };
        if atualizacao::processo::normalizar().is_err() {
            std::process::exit(1);
        }
        let resultado = chamador.and_then(|chamador| {
            verificacao.and_then(|verificacao| atualizacao::executar::executar(operacao, chamador, verificacao))
        });
        match resultado {
            Ok(()) => return,
            Err(erro) => atualizacao::diagnostico::registrar(&erro),
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = argumentos;
    std::process::exit(1);
}
