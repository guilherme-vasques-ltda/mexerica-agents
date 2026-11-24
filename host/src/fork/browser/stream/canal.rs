//! Ligação do estado de quadros ao canal protegido do Hub (fatia 2, PR B1).
//! Sem Chromium: a fonte de quadros entra por [`oferecer`]; nos testes ela é falsa.
//! O relay não muda: o quadro vai cifrado dentro dos frames `f` existentes.
#[cfg(test)]
use super::AcceptedInput;
use super::codec::{self, CodecError};
use super::{Authority, Controller, Frame, Input, Stream};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::autoridade::Autoridade,
    hub::Hub,
    store::{Scope, Store},
};
use anyhow::ensure;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

/// Capacidade negociada no `hello`; cliente sem ela nunca recebe quadro.
pub const CAPACIDADE_QUADROS: &str = "browserFramesV1";
/// Maior mensagem binária aceita, conferido antes de decodificar ou copiar o JPEG.
pub const MAX_BINARIO: usize = codec::MAX_WIRE;
const MAX_ID: usize = 256;

/// Métodos internos do stream, interceptados antes do dispatch comum.
pub const METODOS: [&str; 5] =
    ["browserStreamSubscribe", "browserStreamInput", "browserStreamAck", "browserStreamControl", "browserStreamTarget"];

pub fn negociar(caps: Option<&Value>) -> bool {
    caps.and_then(Value::as_array).is_some_and(|lista| lista.iter().any(|c| c.as_str() == Some(CAPACIDADE_QUADROS)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Escopo {
    pub hub: String,
    pub space: String,
    pub bot: String,
}

fn texto(body: &Value, campo: &str) -> Option<String> {
    let valor = body[campo].as_str()?;
    (!valor.is_empty() && valor.len() <= MAX_ID).then(|| valor.to_owned())
}

pub fn analisar_inscricao(body: &Value) -> Option<(Escopo, String, String)> {
    Some((
        Escopo { hub: texto(body, "hubId")?, space: texto(body, "spaceId")?, bot: texto(body, "botId")? },
        texto(body, "session")?,
        texto(body, "target")?,
    ))
}

pub fn analisar_entrada(body: &Value) -> Option<Input> {
    let numero = |campo: &str| body[campo].as_f64().filter(|v| v.is_finite());
    Some(Input {
        session: texto(body, "session")?,
        target: texto(body, "target")?,
        frame_sequence: body["frameSequence"].as_u64()?,
        target_generation: body["targetGeneration"].as_u64()?,
        sequence: body["sequence"].as_u64()?,
        epoch: body["epoch"].as_u64()?,
        controller: match body["controller"].as_str()? {
            "person" => Controller::Person,
            "agent" => Controller::Agent,
            _ => return None,
        },
        x: numero("x")?,
        y: numero("y")?,
        display_width: numero("displayWidth")?,
        display_height: numero("displayHeight")?,
    })
}

pub fn codificar(frame: &Frame) -> Result<Vec<u8>, CodecError> {
    codec::encode(frame)
}

/// Recusa tamanho e corrupção antes de qualquer cópia do JPEG.
pub fn decodificar(bytes: &[u8]) -> Result<Frame, CodecError> {
    if bytes.len() > MAX_BINARIO {
        return Err(CodecError::TooLarge);
    }
    codec::decode(bytes)
}

pub fn eh_binario(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MXJ1")
}

/// Autoridade real no lugar do stub: a autoridade original do transporte é
/// reconferida a cada quadro e entrada, e o bot precisa pertencer ao Space do
/// escopo. Bot (Ponte), aparelho revogado e outro Space falham fechados.
pub struct AutoridadeCanal {
    hub: Arc<Hub>,
    caller: Caller,
    escopo: Escopo,
    sessao: String,
}

impl AutoridadeCanal {
    pub fn nova(hub: Arc<Hub>, caller: Caller, escopo: Escopo, sessao: String) -> Self {
        Self { hub, caller, escopo, sessao }
    }

    /// Conferência completa usada na inscrição, antes de criar qualquer estado.
    pub fn autorizada(&self) -> bool {
        if self.escopo.hub != self.hub.host_id {
            return false;
        }
        let Caller::Device { scopes, .. } = &self.caller else { return false };
        if !scopes.contains(&Scope::Control) || !scopes.contains(&Scope::Screen) {
            return false;
        }
        self.conferir_escopo()
    }

    fn conferir_escopo(&self) -> bool {
        // Novas operações no canal podem conferir sua autorização corrente.
        let Ok((_, caller)) = crate::fork::autoridade::reconferir(&self.hub, &self.caller) else { return false };
        let Ok(original) = Autoridade::capturar(&self.hub, &caller) else { return false };
        original.with_db(&self.hub, None, |db| self.conferir_bot_no(db)).is_ok()
    }

    fn conferir_bot_no(&self, db: &rusqlite::Connection) -> anyhow::Result<()> {
        let row = Store::bot_on(db, &self.escopo.bot)?.ok_or_else(|| anyhow::anyhow!("browser_scope_unavailable"))?;
        ensure!(
            !row.deleted && !row.config.is_group() && row.config.space_id == self.escopo.space,
            "browser_scope_unavailable"
        );
        Ok(())
    }
}

impl Authority for AutoridadeCanal {
    fn is_owner(&self, session: &str, recipient: &str) -> bool {
        session == self.sessao
            && recipient == self.caller.device_key()
            && self.escopo.hub == self.hub.host_id
            && self.conferir_escopo()
    }
}

struct Registro {
    estado: Arc<Mutex<Stream>>,
    #[cfg(test)]
    autoridade: Arc<AutoridadeCanal>,
    #[cfg(test)]
    epoca_workspace: u64,
}

static REGISTROS: OnceLock<Mutex<HashMap<String, Registro>>> = OnceLock::new();

fn registros() -> &'static Mutex<HashMap<String, Registro>> {
    REGISTROS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Posse de uma sessão de stream; ao cair, a sessão some e o estado desconecta.
pub struct GuardaSessao {
    sessao: String,
    estado: Arc<Mutex<Stream>>,
}

impl Drop for GuardaSessao {
    fn drop(&mut self) {
        self.estado.locked().disconnect();
        let mut mapa = registros().locked();
        if mapa.get(&self.sessao).is_some_and(|r| Arc::ptr_eq(&r.estado, &self.estado)) {
            mapa.remove(&self.sessao);
        }
    }
}

/// Só o primeiro canal que inscreve a sessão é o dono dela; outro é recusado.
pub fn registrar(sessao: &str, estado: Arc<Mutex<Stream>>, autoridade: &Arc<AutoridadeCanal>) -> Option<GuardaSessao> {
    #[cfg(not(test))]
    let _ = autoridade;
    // Captura do stream de prova, sem renovar a autorização já emitida.
    #[cfg(test)]
    let autoridade = Arc::clone(autoridade);
    #[cfg(test)]
    let epoca_workspace = Autoridade::capturar(&autoridade.hub, &autoridade.caller)
        .ok()?
        .with_db(&autoridade.hub, None, |db| {
            autoridade.conferir_bot_no(db)?;
            crate::fork::workspace::epoch_on(db)
        })
        .ok()?;
    let mut mapa = registros().locked();
    if mapa.contains_key(sessao) {
        return None;
    }
    mapa.insert(
        sessao.to_owned(),
        Registro {
            estado: estado.clone(),
            #[cfg(test)]
            autoridade: autoridade.clone(),
            #[cfg(test)]
            epoca_workspace,
        },
    );
    Some(GuardaSessao { sessao: sessao.to_owned(), estado })
}

/// Fonte de quadros (falsa nos testes; o supervisor liga aqui depois).
/// Devolve `false` sem sessão inscrita, sem capacidade ou com quadro velho.
pub fn oferecer(frame: Frame) -> bool {
    let registro = registros().locked().get(&frame.session).map(|r| r.estado.clone());
    match registro {
        Some(estado) => estado.locked().offer(frame),
        None => false,
    }
}

#[cfg(test)]
pub(crate) fn simular_consumidor(sessao: &str) -> bool {
    let estado = registros().locked().get(sessao).map(|r| r.estado.clone());
    let Some(estado) = estado else { return false };
    estado.locked().enable_test_input_consumer();
    true
}

/// Intenção de uso único, presa ao estado e à autoridade do canal original.
#[cfg(test)]
pub struct EntradaPendente {
    estado: Arc<Mutex<Stream>>,
    autoridade: Arc<AutoridadeCanal>,
    epoca_workspace: u64,
    entrada: AcceptedInput,
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub enum EntradaEncerrada {
    Contexto,
    Repetida,
    SemConfirmacao,
}

#[cfg(test)]
impl EntradaPendente {
    /// Consome antes do efeito falso. Nenhuma falha devolve a intenção ou autoriza retry.
    /// A guarda final admite apenas o callback síncrono do fixture, sem rede ou BD.
    pub fn com_efeito<T>(self, efeito: impl FnOnce(&AcceptedInput) -> T) -> Result<T, EntradaEncerrada> {
        use crate::fork::{
            autoridade::{OrigemSaida, com_saida_validada},
            workspace::epoch_on,
        };
        let estado = self.estado.locked();
        if !estado.input_still_current(&self.entrada) {
            return Err(EntradaEncerrada::Contexto);
        }
        let a = &self.autoridade;
        let original = Autoridade::capturar(&a.hub, &a.caller).map_err(|_| EntradaEncerrada::Contexto)?;
        let conferir = |db: &rusqlite::Connection| -> anyhow::Result<bool> {
            a.conferir_bot_no(db)?;
            Ok(epoch_on(db)? == self.epoca_workspace)
        };
        let gravacao = original.with_db(&a.hub, None, |db| {
            if !conferir(db)? {
                return Ok(Err(EntradaEncerrada::Contexto));
            }
            Ok(super::recibo::gravar_no(db, self.entrada.recibo)?)
        });
        gravacao.map_err(|_| EntradaEncerrada::SemConfirmacao)??;
        // Se a autoridade mudar entre commit e admissão, não há efeito. O
        // recibo já comprometido não é removido, nem se tenta outro destino.
        com_saida_validada(
            &a.hub,
            OrigemSaida::Controle(&original, None),
            |db, _, _, _, _| {
                ensure!(conferir(db)?, "browser_scope_unavailable");
                Ok(())
            },
            || efeito(&self.entrada),
        )
        .map_err(|_| EntradaEncerrada::SemConfirmacao)
    }
}

/// Retira a intenção sem executar ou confirmar ação.
#[cfg(test)]
pub fn retirar_entrada(sessao: &str) -> Option<EntradaPendente> {
    let (estado, autoridade, epoca_workspace) =
        registros().locked().get(sessao).map(|r| (r.estado.clone(), r.autoridade.clone(), r.epoca_workspace))?;
    let entrada = estado.locked().next_input(&*autoridade, autoridade.caller.device_key())?;
    Some(EntradaPendente { estado, autoridade, epoca_workspace, entrada })
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::fork::browser::stream::Size;

    pub fn quadro_falso(sequence: u64) -> Frame {
        Frame {
            session: "s".into(),
            target: "a".into(),
            sequence,
            target_generation: 0,
            image: Size { width: 960, height: 540 },
            viewport: Size { width: 1280, height: 720 },
            jpeg: vec![0xff, 0xd8, u8::try_from(sequence).unwrap_or(1), 0xff, 0xd9],
        }
    }

    #[test]
    fn negociacao_e_limite_antes_de_decodificar() {
        assert!(!negociar(None));
        assert!(!negociar(Some(&serde_json::json!(["outra"]))));
        assert!(negociar(Some(&serde_json::json!([CAPACIDADE_QUADROS]))));
        let binario = codificar(&quadro_falso(1)).unwrap();
        assert!(eh_binario(&binario));
        assert_eq!(decodificar(&binario).unwrap().sequence, 1);
        assert_eq!(decodificar(&vec![0u8; MAX_BINARIO + 1]), Err(CodecError::TooLarge));
        let mut corrompido = binario;
        *corrompido.last_mut().unwrap() = 0;
        assert!(decodificar(&corrompido).is_err());
    }

    #[test]
    fn entrada_estrita_recusa_controlador_desconhecido_e_texto() {
        let corpo = serde_json::json!({"session":"s","target":"a","frameSequence":1,"targetGeneration":0,
            "sequence":1,"epoch":0,"controller":"agent","x":10.0,"y":10.0,"displayWidth":100.0,"displayHeight":100.0});
        assert!(analisar_entrada(&corpo).is_some());
        let mut ruim = corpo.clone();
        ruim["controller"] = serde_json::json!("teclado");
        assert!(analisar_entrada(&ruim).is_none());
        ruim["controller"] = serde_json::json!("person");
        ruim["x"] = serde_json::json!("10");
        assert!(analisar_entrada(&ruim).is_none());
    }
}
