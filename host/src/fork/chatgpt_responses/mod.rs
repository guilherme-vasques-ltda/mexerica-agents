mod tests;

use serde::Serialize;
use tests::CredencialFicticia;

const MODELOS: &[&str] = &["modelo-texto-ficticio"];
const LIMITE_BYTES: usize = 64;

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

struct InscricaoFicticia {
    vinculo: Vinculo,
    credencial: CredencialFicticia,
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

#[derive(Clone, Copy)]
enum EventoTerminal {
    Concluido,
    Falhou { codigo: &'static str, _mensagem: &'static str },
}

trait Transporte {
    fn enviar(
        &mut self,
        metodo: &'static str,
        caminho: &'static str,
        credencial: &CredencialFicticia,
        corpo: &Responses<'_>,
    ) -> (u16, EventoTerminal);
}

struct HubDeProva<T> {
    inscricao: InscricaoFicticia,
    gesto: Option<Gesto>,
    http: T,
}

impl<T: Transporte> HubDeProva<T> {
    fn enviar(&mut self, origem: Vinculo, nonce: u64, pedido: Pedido<'_>) -> Result<(), Falha> {
        let Some(gesto) = &self.gesto else {
            return Err(Falha::Recusado);
        };
        if !matches!(origem.space, Space::Pessoal(_))
            || origem != self.inscricao.vinculo
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
        let (status, terminal) = self.http.enviar(
            "POST",
            "/v1/responses",
            &self.inscricao.credencial,
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
