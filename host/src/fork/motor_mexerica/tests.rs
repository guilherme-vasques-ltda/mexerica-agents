//! Provas da fatia 1: flag desligada não registra nem abre processo; flag
//! ligada com chave de teste responde pelo provedor falso local; a chave não
//! sai em argv/log; o cofre fica preso ao slot do provedor fixado no
//! nascimento e recusa escrita; sem chave o processo falha fechado. Os testes
//! que abrem o motor pulam só fora do CI: com `MEXERICA_MOTOR_TESTS_REQUIRED=1`
//! (definido pelo workflow), pré-requisito ausente reprova em vez de pular.
use super::*;
use std::sync::Arc;

async fn pedir<W: tokio::io::AsyncWrite + Unpin, R: tokio::io::AsyncBufRead + Unpin>(
    write: &mut W,
    respostas: &mut tokio::io::Lines<R>,
    linha: &str,
) -> String {
    write.write_all(linha.as_bytes()).await.unwrap();
    write.write_all(b"\n").await.unwrap();
    respostas.next_line().await.unwrap().unwrap()
}

const SEGREDO: &str = "mexerica-teste-chave-que-nao-pode-vazar";
const OUTRO_SEGREDO: &str = "mexerica-teste-chave-do-segundo-slot";

fn available() -> bool {
    node_path().is_some() && entry_path().is_file()
}

/// Fora do CI (sem a marca de prova obrigatória) um dev sem Node pode pular;
/// no CI pular é falha — a prova exigida não pode passar sem pré-requisito.
pub(super) fn require_motor() -> bool {
    if available() {
        return true;
    }
    assert!(std::env::var_os(REQUIRED_ENV).is_none(), "pré-requisito do motor ausente no CI (node ou entrypoint)");
    eprintln!("sem node/entrypoint: prova do motor pulada fora do CI");
    false
}

#[test]
fn flag_desligada_nao_registra_nem_abre_o_motor() {
    assert!(!enabled_value(None));
    assert!(!enabled_value(Some(std::ffi::OsStr::new("0"))));
    assert!(!enabled_value(Some(std::ffi::OsStr::new("sim"))));
    assert!(enabled_value(Some(std::ffi::OsStr::new("1"))));
    // Sem a flag no ambiente, o id não é um backend conhecido e o lançamento
    // é recusado na fronteira efetiva, não só no registro.
    if !enabled() {
        assert!(crate::agent::backends::harness(ENGINE_ID).is_none());
        assert!(!crate::agent::backends::is_known(ENGINE_ID));
        assert!(launch_candidates().is_err(), "flag desligada tinha de recusar o lançamento");
        #[cfg(unix)]
        {
            let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
            let Err(error) = Engine::spawn(store, "faux", &[]) else {
                panic!("flag desligada permitiu abrir o motor");
            };
            assert!(error.to_string().contains("desligado"));
        }
    }
}

#[test]
fn listagem_respeita_flag_nos_dois_estados() {
    const MARCADOR: &str = "DONNABOT_MEX_LISTA_TEST";
    if let Some(value) = std::env::var_os(MARCADOR) {
        let presente = crate::agent::backends::list().iter().any(|item| item["id"] == ENGINE_ID);
        assert_eq!(presente, value == "1");
        return;
    }
    for value in ["0", "1"] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fork::motor_mexerica::tests::listagem_respeita_flag_nos_dois_estados"])
            .env(MARCADOR, value)
            .env(FLAG, value)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

/// O leitor é o mesmo do motor e dos servidores MCP: o teto de 8 MiB vale para os dois.
#[tokio::test]
async fn leitor_aceita_linha_no_teto_e_recusa_acima_dele() {
    use mcp_host::{MAX_LINE, read_line};
    let mut buf = Vec::new();

    let mut no_teto = vec![b'a'; MAX_LINE];
    no_teto.extend_from_slice(b"\nproxima\n");
    let mut leitor = BufReader::new(&no_teto[..]);
    assert_eq!(read_line(&mut leitor, &mut buf).await.unwrap(), MAX_LINE + 1, "8 MiB de conteúdo mais a quebra passam");
    assert_eq!(read_line(&mut leitor, &mut buf).await.unwrap(), 8, "a linha seguinte segue intacta");
    assert_eq!(buf, b"proxima\n");

    let mut acima = vec![b'a'; MAX_LINE + 1];
    acima.extend_from_slice(b"\n");
    let mut leitor = BufReader::new(&acima[..]);
    let error = read_line(&mut leitor, &mut buf).await.unwrap_err();
    assert!(error.to_string().contains("acima de"), "{error:#}");
    assert!(buf.len() <= MAX_LINE + 1, "o buffer não cresce além do teto: {}", buf.len());

    let sem_quebra = vec![b'a'; MAX_LINE + 4096];
    let mut leitor = BufReader::new(&sem_quebra[..]);
    assert!(read_line(&mut leitor, &mut buf).await.is_err(), "linha gigante sem quebra também é recusada");

    let mut leitor = BufReader::new(&b""[..]);
    assert_eq!(read_line(&mut leitor, &mut buf).await.unwrap(), 0, "fim da saída segue sendo 0");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn sem_identidade_separada_recusa_spawn() {
    const MARCADOR: &str = "DONNABOT_MEX_SEM_ISOLAMENTO_TEST";
    if std::env::var_os(MARCADOR).is_some() {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
        let Err(error) = Engine::spawn(store, "faux", &[]) else {
            panic!("abertura sem isolamento foi aceita");
        };
        assert!(error.to_string().contains("identidade separada"), "{error:#}");
    } else if !crate::fork::motor_exec::configured() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fork::motor_mexerica::tests::sem_identidade_separada_recusa_spawn"])
            .env(MARCADOR, "1")
            .env(FLAG, "1")
            .status()
            .unwrap();
        assert!(status.success());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn conversa_responde_pelo_provedor_falso() {
    let Some(_case) = crate::fork::motor_exec::test_support::case(
        "fork::motor_mexerica::tests::conversa_responde_pelo_provedor_falso",
    ) else {
        return;
    };
    if !require_motor() {
        return;
    }
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    let mut engine = Engine::spawn(store, "faux", &[(FAUX_ENV, "1")]).unwrap();
    let turn = engine.prompt("faux", "faux-1", "ola mexerica").await.unwrap();
    assert!(turn.text.contains("eco: ola mexerica"), "esperava eco, veio: {}", turn.text);
    engine.finish().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn chave_nunca_em_argv_log_ou_saida() {
    let Some(_case) =
        crate::fork::motor_exec::test_support::case("fork::motor_mexerica::tests::chave_nunca_em_argv_log_ou_saida")
    else {
        return;
    };
    if !require_motor() {
        return;
    }
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    // O slot do provedor fixado no spawn: `credential.read` entrega a chave ao
    // processo pelo fd 3; ela não pode aparecer na saída nem no erro.
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();

    let mut engine = Engine::spawn(store, "faux", &[(FAUX_ENV, "1")]).unwrap();
    // O turno confirma que o helper já fez exec do Node antes de ler cmdline.
    let turn = engine.prompt("faux", "faux-1", "oi").await.unwrap();
    // Falha de leitura do cmdline reprova: string vazia esconderia vazamento.
    let cmdline = engine
        .pid()
        .map(|pid| std::fs::read_to_string(format!("/proc/{pid}/cmdline")).expect("cmdline do motor ilegível"))
        .unwrap_or_default();
    assert!(!cmdline.is_empty(), "não consegui ler o argv do motor");
    assert!(!cmdline.contains(SEGREDO), "a chave vazou no argv do motor");
    let (stderr, _) = engine.finish().await.unwrap();
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(!stderr.contains(SEGREDO), "a chave vazou no stderr do motor");
    let eventos = serde_json::to_string(&turn.events).unwrap();
    assert!(!eventos.contains(SEGREDO), "a chave vazou nos eventos RPC");
}

#[cfg(unix)]
#[tokio::test]
async fn cofre_preso_ao_slot_do_provedor_e_sem_escrita() {
    // Dois slots povoados: o processo nasce preso a "google"; nem ler o outro
    // slot nem escrever/apagar são permitidos, inclusive em pedido malformado.
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    crate::market::vault::write(&store, &credential_slot("google"), SEGREDO).unwrap();
    crate::market::vault::write(&store, &credential_slot("openai"), OUTRO_SEGREDO).unwrap();

    let (hub_side, mut child_side) = tokio::net::UnixStream::pair().unwrap();
    let server = tokio::spawn(serve_vault(store_vault(store.clone()), hub_side, "google".to_owned()));
    let (read, mut write) = child_side.split();
    let mut respostas = BufReader::new(read).lines();

    // Leitura do slot autorizado devolve a chave.
    let res =
        pedir(&mut write, &mut respostas, r#"{"id":1,"method":"credential.read","params":{"providerId":"google"}}"#)
            .await;
    assert!(res.contains(SEGREDO), "leitura autorizada falhou: {res}");

    // Leitura de outro slot é recusada e não devolve a segunda chave.
    let res =
        pedir(&mut write, &mut respostas, r#"{"id":2,"method":"credential.read","params":{"providerId":"openai"}}"#)
            .await;
    assert!(res.contains("error"), "slot alheio tinha de ser recusado: {res}");
    assert!(!res.contains(OUTRO_SEGREDO), "a segunda chave vazou: {res}");

    // Escrita é desligada nesta fatia: nada substitui a credencial.
    let res = pedir(
        &mut write, &mut respostas, r#"{"id":3,"method":"credential.modify","params":{"providerId":"google","credential":{"type":"api_key","key":"x"}}}"#,
    )
    .await;
    assert!(res.contains("error"), "escrita tinha de ser recusada: {res}");

    // Pedido malformado sem credential.key NÃO apaga a entrada.
    let res =
        pedir(&mut write, &mut respostas, r#"{"id":4,"method":"credential.modify","params":{"providerId":"google"}}"#)
            .await;
    assert!(res.contains("error"), "pedido malformado tinha de ser recusado: {res}");
    assert_eq!(crate::market::vault::read(&store, &credential_slot("google")).unwrap().as_deref(), Some(SEGREDO));
    assert_eq!(crate::market::vault::read(&store, &credential_slot("openai")).unwrap().as_deref(), Some(OUTRO_SEGREDO));
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn sem_chave_o_turno_falha_fechado() {
    let Some(_case) =
        crate::fork::motor_exec::test_support::case("fork::motor_mexerica::tests::sem_chave_o_turno_falha_fechado")
    else {
        return;
    };
    if !require_motor() {
        return;
    }
    // Cofre sem a entrada do provedor: credential.read responde null e a
    // conversa NÃO completa — nunca cai em chave do ambiente.
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    let mut engine = Engine::spawn(store, "google", &[]).unwrap();
    let resultado = engine.prompt("google", "gemini-2.5-flash", "oi").await;
    assert!(resultado.is_err(), "sem chave o turno tinha de falhar");
    engine.finish().await.unwrap();
}

// Executada com o ajudante real em ambiente descartável; a fixture Node verifica
// leitura de sentinela do Hub, descritor herdado e HOME do UID separado.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requer ajudante real e dois UIDs em ambiente descartável"]
async fn identidade_real_nega_sentinela_e_isola_descritores() {
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    let mut engine = Engine::spawn(store, "faux", &[]).unwrap();
    let report = engine.next_line().await.unwrap();
    assert_eq!(report["sentinela_negada"], true, "{report}");
    assert_eq!(report["descritor_fechado"], true, "{report}");
    assert_eq!(report["home_privado"], true, "{report}");
    assert_eq!(report["home_gravavel"], true, "{report}");
    assert_eq!(report["ambiente_minimo"], true, "{report}");
    assert_eq!(report["cwd_privado"], true, "{report}");
    engine.finish().await.unwrap();
}
