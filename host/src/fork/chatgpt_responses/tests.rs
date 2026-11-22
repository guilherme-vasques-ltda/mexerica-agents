use super::*;
use serde_json::json;
use std::io;
use std::sync::{Arc, Mutex};

const TOKEN_FICTICIO: &str = "sentinela-ficticia-da-prova";

pub(super) struct CredencialFicticia(&'static str);

struct HttpFalso {
    status: u16,
    terminal: EventoTerminal,
    pedidos: Vec<(&'static str, &'static str, serde_json::Value)>,
    recebeu_credencial_do_cofre: bool,
}

impl Transporte for HttpFalso {
    fn enviar(
        &mut self,
        metodo: &'static str,
        caminho: &'static str,
        credencial: &CredencialFicticia,
        corpo: &Responses<'_>,
    ) -> (u16, EventoTerminal) {
        self.recebeu_credencial_do_cofre = credencial.0 == TOKEN_FICTICIO;
        self.pedidos.push((metodo, caminho, serde_json::to_value(corpo).expect("Serializar pedido sintético")));
        (self.status, self.terminal)
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

fn hub(status: u16, terminal: EventoTerminal) -> HubDeProva<HttpFalso> {
    HubDeProva {
        inscricao: InscricaoFicticia { vinculo: VINCULO, credencial: CredencialFicticia(TOKEN_FICTICIO) },
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
    hub.inscricao.vinculo.conta = "outra-conta";
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    hub.inscricao.vinculo = VINCULO;
    hub.gesto.as_mut().unwrap().vinculo.geracao = 0;
    assert_eq!(hub.enviar(VINCULO, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    assert_eq!(hub.http.pedidos.len(), 0);
    assert!(!hub.http.recebeu_credencial_do_cofre);
}

#[test]
fn recusa_space_compartilhado_mesmo_quando_todos_os_vinculos_coincidem() {
    let mut hub = hub(200, EventoTerminal::Concluido);
    let compartilhado = Vinculo { space: Space::Compartilhado, ..VINCULO };
    hub.inscricao.vinculo = compartilhado;
    hub.gesto.as_mut().unwrap().vinculo = compartilhado;
    assert_eq!(hub.enviar(compartilhado, NONCE, Pedido { modelo: MODELO, texto: TEXTO }), Err(Falha::Recusado));
    assert!(hub.http.pedidos.is_empty());
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
    assert_eq!(log.matches("falha=NaoAutorizado").count(), 2);
    assert_eq!(log.matches("falha=Cota").count(), 2);
    assert!(!log.contains(TOKEN_FICTICIO));
    for dado in ["invalid_user", "user_not_eligible", "usage_limit_exceeded", TEXTO, VINCULO.conta] {
        assert!(!log.contains(dado));
    }
}
