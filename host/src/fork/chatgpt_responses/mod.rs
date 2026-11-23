mod admissao;
mod tests;
mod transporte;

use admissao::{Admissao, Vinculo};
use std::time::{Duration, Instant};
use transporte::{CredencialFicticia, Transporte};

const MODELO: &str = "modelo-texto-ficticio";
const LIMITE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Falha {
    Recusado,
    NaoAutorizado,
    Cota,
    Indisponivel,
    Incompleta,
    RespostaInvalida,
    Registro,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Pedido<'a> {
    modelo: &'a str,
    texto: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Nonce {
    sessao: u64,
    gesto: [u8; 32],
}

struct Gesto<'a> {
    nonce: Nonce,
    criado: Instant,
    pedido: Pedido<'a>,
}

struct InscricaoFicticia<'a> {
    vinculo: Vinculo<'a>,
    autorizada: bool,
    credencial: Option<CredencialFicticia>,
}

struct HubDeProva<'a, T> {
    admissao: Admissao<'a>,
    gesto: Option<Gesto<'a>>,
    http: T,
}

impl<'a, T: Transporte> HubDeProva<'a, T> {
    fn consentir(&mut self, inscricao: &InscricaoFicticia<'a>) -> Result<(), Falha> {
        if !inscricao.autorizada || !inscricao.credencial.as_ref().is_some_and(CredencialFicticia::valida) {
            return Err(Falha::Recusado);
        }
        self.gesto = None;
        self.admissao.armar(inscricao.vinculo)
    }

    fn desligar(&mut self) -> Result<(), Falha> {
        self.gesto = None;
        self.admissao.desligar()
    }

    fn confirmar(&mut self, origem: Vinculo<'a>, pedido: Pedido<'a>, agora: Instant) -> Result<Nonce, Falha> {
        let sessao = self.admissao.sessao(origem)?;
        if pedido.modelo != MODELO || pedido.texto.trim().is_empty() || pedido.texto.len() > LIMITE_BYTES {
            return Err(Falha::Recusado);
        }
        let nonce = Nonce { sessao, gesto: crate::remote::crypto::random() };
        self.gesto = Some(Gesto { nonce, criado: agora, pedido });
        Ok(nonce)
    }

    fn enviar(
        &mut self,
        origem: Vinculo<'a>,
        inscricao: &InscricaoFicticia<'_>,
        nonce: Nonce,
        pedido: Pedido<'_>,
        agora: Instant,
    ) -> Result<String, Falha> {
        if nonce.sessao != self.admissao.sessao(origem)? {
            return Err(Falha::Recusado);
        }
        let gesto = self.gesto.take_if(|gesto| gesto.nonce == nonce).ok_or(Falha::Recusado)?;
        if origem != inscricao.vinculo
            || !inscricao.autorizada
            || pedido != gesto.pedido
            || agora.checked_duration_since(gesto.criado).is_none_or(|idade| idade >= Duration::from_secs(30))
        {
            return Err(Falha::Recusado);
        }
        let credencial = inscricao.credencial.as_ref().ok_or(Falha::Recusado)?;
        let requisicao = transporte::requisicao(pedido, credencial)?;
        self.admissao.reservar(nonce.sessao, agora)?;
        let resultado = self.http.enviar(requisicao).and_then(transporte::interpretar);
        if resultado == Err(Falha::Cota) {
            self.admissao.pausar_por_cota()?;
        } else {
            self.admissao.terminar(nonce.sessao)?;
        }
        resultado
    }
}
