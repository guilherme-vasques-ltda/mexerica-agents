use serde::Serialize;
use serde_json::json;
use std::io;
use std::sync::{Arc, Mutex};

const MODELOS: &[&str] = &["modelo-texto-ficticio"];
const LIMITE_BYTES: usize = 64;
const TOKEN_FICTICIO: &str = "sentinela-ficticia-da-prova";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
    Pessoal(&'static str),
    Compartilhado,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Vinculo {
    principal: &'static str,
    conta: &'static str,
    dispositivo: &'static str,
    space: Space,
    geracao: u64,
}

struct Gesto {
    vinculo: Vinculo,
    nonce: u64,
    modelo: &'static str,
    texto: &'static str,
}

struct CofreDoHub {
    vinculo: Vinculo,
    bearer: &'static str,
}

#[derive(Clone, Copy)]
struct Pedido<'a> {
    modelo: &'a str,
    texto: &'a str,
}

#[derive(Serialize)]
struct Responses<'a> {
    model: &'a str,
    input: [Mensagem<'a>; 1],
    store: bool,
    stream: bool,
}

#[derive(Serialize)]
struct Mensagem<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Falha {
    Recusado,
    NaoAutorizado,
    Cota,
    RespostaInvalida,
}

struct HttpFalso {
    status: u16,
    terminal: EventoTerminal,
    pedidos: Vec<(&'static str, &'static str, serde_json::Value)>,
    recebeu_credencial_do_cofre: bool,
}

#[derive(Clone, Copy)]
enum EventoTerminal {
    Concluido,
    Falhou { codigo: &'static str, _mensagem: &'static str },
}

impl HttpFalso {
    fn responder(
        &mut self,
        metodo: &'static str,
        caminho: &'static str,
        bearer: &str,
        corpo: &Responses<'_>,
    ) -> (u16, EventoTerminal) {
        self.recebeu_credencial_do_cofre = bearer == TOKEN_FICTICIO;
        self.pedidos.push((metodo, caminho, serde_json::to_value(corpo).expect("Serializar pedido sintético")));
        (self.status, self.terminal)
    }
}

struct HubDeProva {
    cofre: CofreDoHub,
    gesto: Option<Gesto>,
    http: HttpFalso,
}

impl HubDeProva {
    fn enviar(&mut self, origem: Vinculo, nonce: u64, pedido: Pedido<'_>) -> Result<(), Falha> {
        let Some(gesto) = &self.gesto else { return Err(Falha::Recusado) };
        if !matches!(origem.space, Space::Pessoal(_))
            || origem != self.cofre.vinculo
            || origem != gesto.vinculo
            || nonce != gesto.nonce
            || pedido.modelo != gesto.modelo
            || pedido.texto != gesto.texto
            || !MODELOS.contains(&pedido.modelo)
            || pedido.texto.trim().is_empty()
            || pedido.texto.len() > LIMITE_BYTES
        {
            return Err(Falha::Recusado);
        }

        self.gesto = None;
        let (status, terminal) = self.http.responder(
            "POST",
            "/v1/responses",
            self.cofre.bearer,
            &Responses {
                model: pedido.modelo,
                input: [Mensagem { role: "user", content: pedido.texto }],
                store: false,
                stream: true,
            },
        );
        let falha = match (status, terminal) {
            (200, EventoTerminal::Concluido) => return Ok(()),
            (401 | 403, _) => Falha::NaoAutorizado,
            (429, _) | (200, EventoTerminal::Falhou { codigo: "subscription_sharing_usage_limit_exceeded", .. }) => {
                Falha::Cota
            }
            _ => Falha::RespostaInvalida,
        };
        tracing::warn!(?falha, "requisição sintética recusada");
        Err(falha)
    }
}

const VINCULO: Vinculo = Vinculo {
    principal: "titular-ficticio",
    conta: "conta-ficticia",
    dispositivo: "dispositivo-ficticio",
    space: Space::Pessoal("space-ficticio"),
    geracao: 1,
};
const NONCE: u64 = 7;
const MODELO: &str = MODELOS[0];
const TEXTO: &str = "Responda com uma saudação.";

fn hub(status: u16, terminal: EventoTerminal) -> HubDeProva {
    HubDeProva {
        cofre: CofreDoHub { vinculo: VINCULO, bearer: TOKEN_FICTICIO },
        gesto: Some(Gesto { vinculo: VINCULO, nonce: NONCE, modelo: MODELO, texto: TEXTO }),
        http: HttpFalso { status, terminal, pedidos: vec![], recebeu_credencial_do_cofre: false },
    }
}

#[test]
fn gesto_do_titular_gera_um_unico_pedido_fixo() {
    let mut hub = hub(200, EventoTerminal::Concluido);
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Ok(()));
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    assert_eq!(
        hub.http.pedidos,
        [(
            "POST",
            "/v1/responses",
            json!({
                "model": MODELO, "input": [{"role": "user", "content": TEXTO}], "store": false, "stream": true
            })
        )]
    );
    assert!(hub.http.recebeu_credencial_do_cofre);
}

#[test]
fn recusa_outro_vinculo_nonce_texto_modelo_e_tamanho() {
    let mut hub = hub(200, EventoTerminal::Concluido);
    for origem in [
        Vinculo { principal: "outro-titular", ..VINCULO },
        Vinculo { conta: "outra-conta", ..VINCULO },
        Vinculo { dispositivo: "outro-dispositivo", ..VINCULO },
        Vinculo { space: Space::Pessoal("outro-space-ficticio"), ..VINCULO },
        Vinculo { space: Space::Compartilhado, ..VINCULO },
        Vinculo { geracao: 2, ..VINCULO },
    ] {
        assert_eq!(hub.enviar(origem, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    }
    assert_eq!(hub.enviar(VINCULO, NONCE + 1, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: "Alterado." }), Err(Falha::Recusado));
    let gesto = hub.gesto.as_mut().unwrap();
    gesto.modelo = "outro-modelo";
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: "outro-modelo", texto: TEXTO }), Err(Falha::Recusado));
    let longo = "áéíóúáéíóúáéíóúáéíóúáéíóúáéíóúáéíóú";
    assert!(longo.len() > LIMITE_BYTES);
    for texto in [" ", longo] {
        let gesto = hub.gesto.as_mut().unwrap();
        gesto.modelo = MODELO;
        gesto.texto = texto;
        assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto }), Err(Falha::Recusado));
    }
    assert_eq!(hub.http.pedidos.len(), 0);
    assert!(!hub.http.recebeu_credencial_do_cofre);
}

#[test]
fn recusa_cofre_de_outra_conta_ou_gesto_de_geracao_anterior() {
    let mut hub = hub(200, EventoTerminal::Concluido);
    hub.cofre.vinculo.conta = "outra-conta";
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    hub.cofre.vinculo = VINCULO;
    hub.gesto.as_mut().unwrap().vinculo.geracao = 0;
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    assert_eq!(hub.http.pedidos.len(), 0);
    assert!(!hub.http.recebeu_credencial_do_cofre);
}

#[test]
fn cota_e_autorizacao_terminam_sem_retry_fallback_ou_vazamento() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let destino = Arc::clone(&log);
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || {
            struct Captura(Arc<Mutex<Vec<u8>>>);
            impl io::Write for Captura {
                fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                    self.0.lock().unwrap().write(bytes)
                }

                fn flush(&mut self) -> io::Result<()> {
                    Ok(())
                }
            }
            Captura(Arc::clone(&destino))
        })
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        for (status, codigo, falha) in [
            (401, "invalid_user", Falha::NaoAutorizado),
            (403, "user_not_eligible", Falha::NaoAutorizado),
            (429, "usage_limit_exceeded", Falha::Cota),
            (200, "subscription_sharing_usage_limit_exceeded", Falha::Cota),
        ] {
            let terminal = EventoTerminal::Falhou { codigo, _mensagem: TOKEN_FICTICIO };
            let mut hub = hub(status, terminal);
            let resultado = hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO });
            assert_eq!(resultado, Err(falha));
            assert!(!format!("{resultado:?}").contains(TOKEN_FICTICIO));
            assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
            assert_eq!(hub.http.pedidos.len(), 1);
            assert!(!hub.http.pedidos[0].2.to_string().contains(TOKEN_FICTICIO));
            assert!(hub.http.recebeu_credencial_do_cofre);
        }
    });
    let log = log.lock().unwrap();
    let log = std::str::from_utf8(&log).unwrap();
    assert_eq!(log.matches("requisição sintética recusada").count(), 4);
    assert!(!log.contains(TOKEN_FICTICIO));
}
