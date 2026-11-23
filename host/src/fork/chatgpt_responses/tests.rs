use super::admissao::Space;
use super::transporte::Resposta;
use super::*;
use reqwest::{Request, header};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::Path;

const TOKEN: &str = "sentinela-ficticia-da-prova";
const VINCULO: Vinculo<'static> = Vinculo {
    principal: "titular-ficticio",
    emissor: "https://emissor.example.invalid",
    conta: "conta-ficticia",
    cliente: "cliente-ficticio",
    dispositivo: "dispositivo-ficticio",
    space: Space::Pessoal("pessoal-ficticio"),
    conversa: "conversa-ficticia",
    geracao: 1,
    hub_local_do_titular: true,
};
const PEDIDO: Pedido<'static> = Pedido { modelo: MODELO, texto: "Responda com uma saudação." };

struct HttpFalso {
    status: u16,
    sse: bool,
    corpo: String,
    pedidos: Vec<(String, String, Value)>,
    recebeu_credencial: bool,
    falha: Option<Falha>,
}

impl Transporte for HttpFalso {
    fn enviar(&mut self, pedido: Request) -> Result<Resposta, Falha> {
        let bearer = &pedido.headers()[header::AUTHORIZATION];
        self.recebeu_credencial = bearer.is_sensitive() && bearer.as_bytes() == format!("Bearer {TOKEN}").as_bytes();
        let corpo = pedido.body().and_then(reqwest::Body::as_bytes).unwrap();
        self.pedidos.push((
            pedido.method().to_string(),
            pedido.url().to_string(),
            serde_json::from_slice(corpo).unwrap(),
        ));
        if let Some(falha) = self.falha {
            return Err(falha);
        }
        Ok(Resposta {
            status: self.status,
            sse: self.sse,
            partes: self.corpo.as_bytes().chunks(7).map(<[u8]>::to_vec).collect(),
        })
    }
}

fn conta(vinculo: Vinculo<'static>) -> InscricaoFicticia<'static> {
    InscricaoFicticia { vinculo, autorizada: true, credencial: Some(CredencialFicticia(TOKEN)) }
}

fn evento(valor: &Value) -> String {
    format!("event: {}\r\ndata: {valor}\r\n\r\n", valor["type"].as_str().unwrap())
}

fn delta() -> String {
    evento(&json!({"type":"response.output_text.delta", "delta":"Olá"}))
}

fn completo() -> String {
    delta()
        + &evento(&json!({"type":"response.completed", "response":{"status":"completed", "output":[
            {"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"Olá"}]}
        ]}}))
}

#[test]
fn sse_aceita_cr_lf_e_crlf_mesmo_entre_partes() {
    for delimitador in ["\r", "\n", "\r\n"] {
        let corpo = completo().replace("\r\n", delimitador);
        let partes = corpo.as_bytes().chunks(1).map(<[u8]>::to_vec).collect();
        assert_eq!(transporte::interpretar(Resposta { status: 200, sse: true, partes }), Ok("Olá".into()));

        let corpo_incompleto = delta().replace("\r\n", delimitador);
        let partes = corpo_incompleto.as_bytes().chunks(1).map(<[u8]>::to_vec).collect();
        assert_eq!(transporte::interpretar(Resposta { status: 200, sse: true, partes }), Err(Falha::Incompleta));
    }
}

#[test]
fn sse_limita_bytes_consumidos_ate_o_terminal_independente_das_partes() {
    let terminal = completo().replace("\r\n", "\n");
    for extras in [0, 1] {
        let preenchimento = transporte::MAX_CORPO - terminal.len() + extras;
        let prefixo = format!(":{}\n", "x".repeat(preenchimento - 2));
        let cauda = b"event: evento-invalido\ndata: invalido\n\n".to_vec();
        let inteiro = [prefixo.as_bytes(), terminal.as_bytes(), &cauda].concat();
        let esperado = if extras == 0 { Ok("Olá".into()) } else { Err(Falha::RespostaInvalida) };
        for partes in [vec![inteiro], vec![prefixo.into_bytes(), terminal.as_bytes().to_vec(), cauda]] {
            assert_eq!(transporte::interpretar(Resposta { status: 200, sse: true, partes }), esperado);
        }
    }
}

fn hub(path: &Path, criar: bool) -> HubDeProva<'static, HttpFalso> {
    HubDeProva {
        admissao: Admissao::abrir(path, criar).unwrap_or_else(|falha| panic!("abertura da fixture: {falha:?}")),
        gesto: None,
        http: HttpFalso {
            status: 200,
            sse: true,
            corpo: completo(),
            pedidos: vec![],
            recebeu_credencial: false,
            falha: None,
        },
    }
}

fn tentativa(
    hub: &mut HubDeProva<'static, HttpFalso>,
    vinculo: Vinculo<'static>,
    agora: Instant,
) -> Result<String, Falha> {
    let nonce = hub.confirmar(vinculo, PEDIDO, agora)?;
    hub.enviar(vinculo, &conta(vinculo), nonce, PEDIDO, agora)
}

#[test]
fn gesto_do_titular_gera_um_unico_pedido_fixo() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = hub(&dir.path().join("admissao.db"), true);
    let agora = Instant::now();
    assert!(matches!(h.confirmar(VINCULO, PEDIDO, agora), Err(Falha::Recusado)));
    h.consentir(&conta(VINCULO)).unwrap();
    let nonce = h.confirmar(VINCULO, PEDIDO, agora).unwrap_or_else(|_| panic!("gesto recusado"));
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, agora), Ok("Olá".into()));
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, agora), Err(Falha::Recusado));
    assert_eq!(
        h.http.pedidos,
        [(
            "POST".into(),
            "https://api.openai.com/v1/responses".into(),
            json!({
                "model": MODELO, "input": [{"role": "user", "content": PEDIDO.texto}], "store": false, "stream": true
            })
        )]
    );
    assert!(h.http.recebeu_credencial);
}

#[test]
fn recusa_outro_vinculo_cofre_nonce_ou_conteudo() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = hub(&dir.path().join("admissao.db"), true);
    let t = Instant::now();
    h.consentir(&conta(VINCULO)).unwrap();
    for origem in [
        Vinculo { principal: "outro", ..VINCULO },
        Vinculo { emissor: "outro", ..VINCULO },
        Vinculo { conta: "outra", ..VINCULO },
        Vinculo { cliente: "outro", ..VINCULO },
        Vinculo { dispositivo: "outro", ..VINCULO },
        Vinculo { conversa: "outra", ..VINCULO },
        Vinculo { space: Space::Pessoal("outro"), ..VINCULO },
        Vinculo { space: Space::Compartilhado, ..VINCULO },
        Vinculo { geracao: 2, ..VINCULO },
        Vinculo { hub_local_do_titular: false, ..VINCULO },
    ] {
        let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap();
        assert_eq!(h.enviar(origem, &conta(origem), nonce, PEDIDO, t), Err(Falha::Recusado));
        assert_eq!(h.enviar(VINCULO, &conta(origem), nonce, PEDIDO, t), Err(Falha::Recusado));
    }
    for inscricao in [
        InscricaoFicticia { autorizada: false, ..conta(VINCULO) },
        InscricaoFicticia { credencial: None, ..conta(VINCULO) },
        InscricaoFicticia { credencial: Some(CredencialFicticia(" ")), ..conta(VINCULO) },
    ] {
        let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap();
        assert_eq!(h.enviar(VINCULO, &inscricao, nonce, PEDIDO, t), Err(Falha::Recusado));
    }
    let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap();
    let mut invalido = nonce;
    invalido.gesto[0] ^= 1;
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), invalido, PEDIDO, t), Err(Falha::Recusado));
    assert_eq!(h.http.pedidos.len(), 0);
    assert!(h.gesto.as_ref().is_some_and(|gesto| gesto.nonce == nonce));
    assert_eq!(
        h.enviar(VINCULO, &conta(VINCULO), nonce, Pedido { texto: "Alterado", ..PEDIDO }, t),
        Err(Falha::Recusado)
    );
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, t), Err(Falha::Recusado));
    let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap();
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, t + Duration::from_secs(30)), Err(Falha::Recusado));
    let longo = "á".repeat(LIMITE_BYTES);
    for pedido in
        [Pedido { texto: &longo, ..PEDIDO }, Pedido { texto: " ", ..PEDIDO }, Pedido { modelo: "outro", ..PEDIDO }]
    {
        assert!(matches!(h.confirmar(VINCULO, pedido, t), Err(Falha::Recusado)));
    }
    assert_eq!(h.consentir(&conta(Vinculo { space: Space::Compartilhado, ..VINCULO })), Err(Falha::Recusado));
    assert_eq!(h.consentir(&conta(Vinculo { conta: "", ..VINCULO })), Err(Falha::Recusado));
    assert_eq!(h.http.pedidos.len(), 0);
    assert!(!h.http.recebeu_credencial);
}

#[test]
fn erros_e_ferramentas_terminam_sem_retry_fallback_ou_vazamento() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let destino = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || {
            struct Captura(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
            impl std::io::Write for Captura {
                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().write(bytes)
                }

                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            Captura(destino.clone())
        })
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let erro = evento(&json!({"type":"response.failed","response":{"error":{
            "code":"subscription_sharing_usage_limit_exceeded","message":TOKEN
        }}}));
        let ferramenta = evento(&json!({"type":"response.output_item.added","item":{"type":"function_call"}}));
        for (status, corpo, falha) in [
            (302, String::new(), Falha::RespostaInvalida),
            (401, json!({"detail":TOKEN}).to_string(), Falha::NaoAutorizado),
            (403, json!({"detail":TOKEN}).to_string(), Falha::NaoAutorizado),
            (429, json!({"detail":TOKEN}).to_string(), Falha::Cota),
            (429, json!({"error":{"code":"subscription_sharing_usage_limit_exceeded"}}).to_string(), Falha::Cota),
            (200, delta() + &erro, Falha::Cota),
            (
                200,
                delta()
                    + &evento(&json!({"type":"error","error":{"code":"subscription_sharing_usage_limit_exceeded"}})),
                Falha::Cota,
            ),
            (200, delta(), Falha::Incompleta),
            (200, evento(&json!({"type":"response.incomplete"})), Falha::Incompleta),
            (200, ferramenta + &completo(), Falha::RespostaInvalida),
            (
                200,
                evento(&json!({"type":"response.function_call_arguments.delta", "delta": TOKEN})) + &completo(),
                Falha::RespostaInvalida,
            ),
            (
                200,
                delta()
                    + &evento(
                        &json!({"type":"response.completed", "response":{"status":"completed", "output":[{"type":"function_call"}]}}),
                    ),
                Falha::RespostaInvalida,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("admissao.db");
            let mut h = hub(&path, true);
            let t = Instant::now();
            h.http.status = status;
            h.http.corpo = corpo;
            h.consentir(&conta(VINCULO)).unwrap();
            let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap_or_else(|_| panic!("gesto recusado"));
            let resultado = h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, t);
            assert_eq!(resultado, Err(falha));
            assert!(!format!("{resultado:?}").contains(TOKEN));
            assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, t), Err(Falha::Recusado));
            assert_eq!(h.http.pedidos.len(), 1);
            assert!(h.http.recebeu_credencial);
            if falha == Falha::Cota {
                drop(h);
                let mut reaberto = hub(&path, false);
                let outra = Vinculo { conta: "outra", ..VINCULO };
                assert_eq!(reaberto.consentir(&conta(outra)), Err(Falha::Cota));
                assert_eq!(reaberto.http.pedidos.len(), 0);
            }
        }
    });
    let log = log.lock().unwrap();
    let log = std::str::from_utf8(&log).unwrap();
    assert_eq!(log.len(), 0);
    for dado in [TOKEN, "subscription_sharing_usage_limit_exceeded", PEDIDO.texto, VINCULO.conta] {
        assert!(!log.contains(dado));
    }
}

#[test]
fn saldo_unico_apos_troca_reconexao_reinicio_e_virada_da_janela() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admissao.db");
    let mut h = hub(&path, true);
    let t = Instant::now();
    h.consentir(&conta(VINCULO)).unwrap();
    assert!(tentativa(&mut h, VINCULO, t).is_ok());
    let antigo = h.confirmar(VINCULO, PEDIDO, t).unwrap_or_else(|_| panic!("gesto recusado"));
    h.desligar().unwrap();
    let b = Vinculo { conta: "conta-b", geracao: 2, ..VINCULO };
    h.consentir(&conta(b)).unwrap();
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), antigo, PEDIDO, t), Err(Falha::Recusado));
    assert!(tentativa(&mut h, b, t).is_ok());
    h.consentir(&conta(VINCULO)).unwrap();
    assert!(tentativa(&mut h, VINCULO, t).is_ok());
    assert_eq!(tentativa(&mut h, VINCULO, t), Err(Falha::Cota));
    assert_eq!(h.http.pedidos.len(), 3);
    drop(h);
    let mut h = hub(&path, false);
    let reinicio = t + Duration::from_secs(60 * 60);
    assert_eq!(tentativa(&mut h, VINCULO, reinicio), Err(Falha::Recusado));
    h.consentir(&conta(VINCULO)).unwrap();
    assert_eq!(tentativa(&mut h, VINCULO, reinicio), Err(Falha::Cota));
    assert_eq!(tentativa(&mut h, VINCULO, reinicio + Duration::from_secs(24 * 60 * 60 - 1)), Err(Falha::Cota));
    assert!(tentativa(&mut h, VINCULO, reinicio + Duration::from_secs(24 * 60 * 60)).is_ok());
    assert_eq!(tentativa(&mut h, VINCULO, reinicio), Err(Falha::Registro));
    assert_eq!(h.http.pedidos.len(), 1);
}

#[test]
fn escritor_concorrente_e_falha_de_gravacao_nao_armam_nem_enviam() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admissao.db");
    let mut h = hub(&path, true);
    assert!(matches!(Admissao::abrir(&path, false), Err(Falha::Registro)));
    let externo = Connection::open(&path).unwrap();
    externo
        .execute_batch("CREATE TRIGGER falha BEFORE UPDATE ON admissao BEGIN SELECT RAISE(ABORT, 'fixture'); END;")
        .unwrap();
    assert_eq!(h.consentir(&conta(VINCULO)), Err(Falha::Registro));
    let t = Instant::now();
    assert_eq!(tentativa(&mut h, VINCULO, t), Err(Falha::Recusado));
    externo.execute_batch("DROP TRIGGER falha").unwrap();
    drop(h);
    let mut h = hub(&path, false);
    h.consentir(&conta(VINCULO)).unwrap();
    let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap_or_else(|_| panic!("gesto recusado"));
    externo
        .execute_batch("CREATE TRIGGER falha BEFORE UPDATE ON admissao BEGIN SELECT RAISE(ABORT, 'fixture'); END;")
        .unwrap();
    assert_eq!(h.desligar(), Err(Falha::Registro));
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, t), Err(Falha::Recusado));
    assert_eq!(h.http.pedidos.len(), 0);
    drop(h);
    let mut h = hub(&path, false);
    assert_eq!(tentativa(&mut h, VINCULO, t), Err(Falha::Recusado));
}

#[test]
fn reserva_confirmada_sem_terminal_nao_rearma_apos_reinicio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admissao.db");
    let mut h = hub(&path, true);
    h.consentir(&conta(VINCULO)).unwrap();
    let t = Instant::now();
    let nonce = h.confirmar(VINCULO, PEDIDO, t).unwrap_or_else(|_| panic!("gesto recusado"));
    h.admissao.reservar(nonce.sessao, t).unwrap();
    drop(h);
    let mut h = hub(&path, false);
    assert_eq!(h.enviar(VINCULO, &conta(VINCULO), nonce, PEDIDO, t), Err(Falha::Recusado));
    assert_eq!(h.consentir(&conta(VINCULO)), Err(Falha::Registro));
    assert_eq!(h.http.pedidos.len(), 0);
}

#[test]
fn falha_da_reserva_e_divergencia_do_registro_invalidam_a_sessao() {
    for registrar_falha in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("admissao.db");
        let mut h = hub(&path, true);
        h.consentir(&conta(VINCULO)).unwrap();
        let t = Instant::now();
        let externo = Connection::open(&path).unwrap();
        externo
            .execute_batch(if registrar_falha {
                "CREATE TRIGGER falha BEFORE UPDATE ON admissao BEGIN SELECT RAISE(ABORT, 'fixture'); END;"
            } else {
                "UPDATE admissao SET estado = json_set(estado, '$.epoca', 2, '$.consentimento', NULL)"
            })
            .unwrap();
        assert_eq!(tentativa(&mut h, VINCULO, t), Err(Falha::Registro));
        assert_eq!(tentativa(&mut h, VINCULO, t), Err(Falha::Recusado));
        assert_eq!(h.consentir(&conta(VINCULO)), Err(Falha::Registro));
        assert_eq!(h.http.pedidos.len(), 0);
    }
}

#[test]
fn disputa_pela_ultima_reserva_e_falha_de_transporte_nao_reembolsam() {
    let dir = tempfile::tempdir().unwrap();
    let h = std::sync::Mutex::new(hub(&dir.path().join("admissao.db"), true));
    let t = Instant::now();
    {
        let mut h = h.lock().unwrap();
        h.consentir(&conta(VINCULO)).unwrap();
        h.http.falha = Some(Falha::Indisponivel);
        assert_eq!(tentativa(&mut h, VINCULO, t), Err(Falha::Indisponivel));
        h.http.falha = None;
        assert!(tentativa(&mut h, VINCULO, t).is_ok());
    }
    let barreira = std::sync::Barrier::new(2);
    std::thread::scope(|escopo| {
        let rodar = || {
            barreira.wait();
            tentativa(&mut h.lock().unwrap(), VINCULO, t)
        };
        let a = escopo.spawn(rodar);
        let b = escopo.spawn(rodar);
        let resultados = [a.join().unwrap(), b.join().unwrap()];
        assert_eq!(resultados.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(resultados.contains(&Err(Falha::Cota)));
    });
    assert_eq!(h.lock().unwrap().http.pedidos.len(), 3);
}
