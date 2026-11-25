//! Provas da rota com transporte falso: nenhum caso toca a rede, o Keychain do
//! sistema ou um provedor real. O foco é recusar antes do envio e nunca
//! transformar dúvida em sucesso.
#![allow(clippy::unwrap_used)]
use super::{cofre, limite, transporte::FalhaTransporte, *};
use crate::{LockExt, store::Store};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const CHAVE: &str = "sk-ant-api-fixture-da-rota-nao-e-chave-real";
const MODELO: &str = "claude-haiku-5-5";
/// Instante fixo (2025-11-25 UTC); o período do teto sai dele.
const AGORA: i64 = 1_764_000_000_000;

struct CofreBom;

impl Cofre for CofreBom {
    fn abrir(&self, _: &Vinculo<'_>) -> Result<Segredo, RecusaCofre> {
        Segredo::novo(CHAVE.to_owned())
    }
}

struct CofreRuim(RecusaCofre);

impl Cofre for CofreRuim {
    fn abrir(&self, _: &Vinculo<'_>) -> Result<Segredo, RecusaCofre> {
        Err(self.0)
    }
}

/// Baixa o teto no meio do turno, entre a reserva e a conferência perto do
/// envio: a chamada não pode passar.
struct CofreQueBaixaTeto<'a>(&'a Store);

impl Cofre for CofreQueBaixaTeto<'_> {
    fn abrir(&self, vinculo: &Vinculo<'_>) -> Result<Segredo, RecusaCofre> {
        limite::configurar(self.0, &format!("bot:{}", vinculo.bot), 1, AGORA).map_err(|_| RecusaCofre::Indisponivel)?;
        Segredo::novo(CHAVE.to_owned())
    }
}

struct Fake {
    chamadas: AtomicUsize,
    resposta: Mutex<Option<Result<CorpoBruto, FalhaTransporte>>>,
    corpos: Mutex<Vec<String>>,
}

impl Fake {
    fn com(corpo: String) -> Self {
        Self {
            chamadas: AtomicUsize::new(0),
            resposta: Mutex::new(Some(Ok(CorpoBruto::novo(corpo.into_bytes())))),
            corpos: Mutex::new(Vec::new()),
        }
    }

    fn falhando(falha: FalhaTransporte) -> Self {
        Self { chamadas: AtomicUsize::new(0), resposta: Mutex::new(Some(Err(falha))), corpos: Mutex::new(Vec::new()) }
    }

    fn chamadas(&self) -> usize {
        self.chamadas.load(Ordering::SeqCst)
    }

    fn corpo(&self) -> String {
        self.corpos.lock().unwrap().first().cloned().unwrap_or_default()
    }
}

impl Transporte for Fake {
    #[allow(clippy::unused_async_trait_impl)]
    async fn enviar(&self, pedido: &Pedido<'_>, chave: &Segredo) -> Result<CorpoBruto, FalhaTransporte> {
        self.chamadas.fetch_add(1, Ordering::SeqCst);
        self.corpos.lock().unwrap().push(pedido.corpo().to_string());
        assert_eq!(chave.expor(), CHAVE, "o transporte recebe a chave do cofre, e só ela");
        self.resposta.lock().unwrap().take().unwrap_or(Err(FalhaTransporte::Indisponivel))
    }
}

fn evento(valor: &serde_json::Value) -> String {
    let nome = valor["type"].as_str().unwrap().to_owned();
    format!("event: {nome}\ndata: {}\n\n", serde_json::to_string(valor).unwrap())
}

fn stream(modelo: &str, texto: &str, entrada: u64, saida: u64) -> String {
    [
        evento(&serde_json::json!({"type":"message_start","message":{"model":modelo,
            "usage":{"input_tokens":entrada,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})),
        evento(&serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
        evento(&serde_json::json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":texto}})),
        evento(&serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
            "usage":{"output_tokens":saida}})),
        evento(&serde_json::json!({"type":"message_stop"})),
    ]
    .concat()
}

fn loja() -> Store {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    migrate(&store.db.locked()).unwrap();
    store
}

fn com_teto(store: &Store, limite: i64) {
    limite::configurar(store, "bot:bot-fixture", limite, AGORA).unwrap();
}

fn vinculo() -> Vinculo<'static> {
    Vinculo { bot: "bot-fixture", concessao: "concessao-fixture" }
}

fn pedido() -> Pedido<'static> {
    Pedido { modelo: MODELO, sistema: "sistema fictício", usuario: "pedido fictício" }
}

fn estimativa() -> i64 {
    estimativa_micros(MODELO).unwrap()
}

async fn rodar<T: Transporte>(store: &Store, fake: &T, cofre: &dyn Cofre) -> Result<Resposta, Falha> {
    executar(store, fake, cofre, &vinculo(), &pedido(), AGORA).await
}

#[tokio::test]
async fn sem_teto_nao_envia() {
    let store = loja();
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::SemTeto);
    assert_eq!(fake.chamadas(), 0);
}

#[tokio::test]
async fn teto_estourado_bloqueia_antes_do_envio() {
    let store = loja();
    com_teto(&store, estimativa() - 1);
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::TetoEsgotado);
    assert_eq!(fake.chamadas(), 0);
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), estimativa() - 1);
}

#[tokio::test]
async fn modelo_sem_preco_nao_envia() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    let sem_preco = Pedido { modelo: "claude-sem-linha-de-preco", sistema: "s", usuario: "u" };
    let falha = executar(&store, &fake, &CofreBom, &vinculo(), &sem_preco, AGORA).await.unwrap_err();
    assert_eq!(falha, Falha::SemPreco);
    assert_eq!(fake.chamadas(), 0);
}

#[tokio::test]
async fn sem_cofre_nao_envia_e_devolve_a_margem() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    let falha = rodar(&store, &fake, &CofreRuim(RecusaCofre::Ausente)).await.unwrap_err();
    assert_eq!(falha, Falha::Cofre(RecusaCofre::Ausente));
    assert_eq!(fake.chamadas(), 0);
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), estimativa());
}

#[tokio::test]
async fn cofre_trancado_nao_envia() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    let falha = rodar(&store, &fake, &CofreRuim(RecusaCofre::Indisponivel)).await.unwrap_err();
    assert_eq!(falha, Falha::Cofre(RecusaCofre::Indisponivel));
    assert_eq!(fake.chamadas(), 0);
}

#[tokio::test]
async fn teto_baixado_depois_da_reserva_nao_envia() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    let falha = executar(&store, &fake, &CofreQueBaixaTeto(&store), &vinculo(), &pedido(), AGORA).await.unwrap_err();
    assert_eq!(falha, Falha::TetoEsgotado);
    assert_eq!(fake.chamadas(), 0);
}

#[tokio::test]
async fn pedido_grande_ou_vazio_recusado() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    let grande = "x".repeat(MAX_ENTRADA + 1);
    let pedido_grande = Pedido { modelo: MODELO, sistema: "", usuario: &grande };
    assert_eq!(
        executar(&store, &fake, &CofreBom, &vinculo(), &pedido_grande, AGORA).await.unwrap_err(),
        Falha::PedidoInvalido
    );
    let vazio = Pedido { modelo: MODELO, sistema: "", usuario: "   " };
    assert_eq!(executar(&store, &fake, &CofreBom, &vinculo(), &vazio, AGORA).await.unwrap_err(), Falha::PedidoInvalido);
    assert_eq!(fake.chamadas(), 0);
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), estimativa());
}

#[tokio::test]
async fn vinculo_invalido_nao_le_cofre_nem_envia() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "texto", 2, 3));
    let invalido = Vinculo { bot: "bot:outro", concessao: "c" };
    let falha = executar(&store, &fake, &CofreBom, &invalido, &pedido(), AGORA).await.unwrap_err();
    assert_eq!(falha, Falha::VinculoInvalido);
    assert_eq!(fake.chamadas(), 0);
}

#[tokio::test]
async fn erro_do_provedor_faz_uma_chamada_e_fecha_o_periodo() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::falhando(FalhaTransporte::Provedor { status: 529 });
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::Provedor { status: Some(529) });
    assert_eq!(fake.chamadas(), 1, "sem fallback nem retentativa");
    let novo = Fake::com(stream(MODELO, "texto", 2, 3));
    assert_eq!(rodar(&store, &novo, &CofreBom).await.unwrap_err(), Falha::UsoIncerto);
    assert_eq!(novo.chamadas(), 0);
}

#[tokio::test]
async fn corte_de_transporte_deixa_uso_incerto_sem_falso_sucesso() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::falhando(FalhaTransporte::Indisponivel);
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::Transporte);
    assert_eq!(fake.chamadas(), 1);
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), 0, "a reserva cheia fica contada");
}

#[tokio::test]
async fn sucesso_entrega_texto_e_liquida_pelo_uso_medido() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, "Resposta fictícia", 1_200, 300));
    let resposta = rodar(&store, &fake, &CofreBom).await.unwrap();
    assert_eq!(resposta.texto, "Resposta fictícia");
    assert_eq!(resposta.modelo, MODELO);
    assert_eq!(resposta.uso, Uso { entrada: 1_200, saida: 300, cache_lido: 0, cache_escrito: 0 });
    assert!(!format!("{resposta:?}").contains("Resposta fictícia"), "o texto não entra em Debug");
    let real = uso_micros(MODELO, resposta.uso).unwrap();
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), estimativa() - real);
    let corpo = fake.corpo();
    assert!(corpo.contains("\"stream\":true"));
    assert!(corpo.contains(MODELO));
    assert!(!corpo.contains(CHAVE));
}

#[tokio::test]
async fn ferramenta_no_stream_recusada_e_fecha_o_periodo() {
    let store = loja();
    com_teto(&store, estimativa());
    let corpo = [
        evento(&serde_json::json!({"type":"message_start","message":{"model":MODELO,
            "usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})),
        evento(&serde_json::json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"t","name":"shell","input":{}}})),
        evento(&serde_json::json!({"type":"message_stop"})),
    ]
    .concat();
    let fake = Fake::com(corpo);
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::ConteudoNaoTexto);
    assert_eq!(fake.chamadas(), 1);
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), 0);
}

#[tokio::test]
async fn credencial_repetida_na_resposta_recusada() {
    let store = loja();
    com_teto(&store, estimativa());
    let fake = Fake::com(stream(MODELO, CHAVE, 2, 3));
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::CredencialRepetida);
}

#[tokio::test]
async fn parada_anormal_recusada() {
    let store = loja();
    com_teto(&store, estimativa());
    let corpo = [
        evento(&serde_json::json!({"type":"message_start","message":{"model":MODELO,
            "usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})),
        evento(&serde_json::json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"cortado"}})),
        evento(&serde_json::json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"},
            "usage":{"output_tokens":9}})),
        evento(&serde_json::json!({"type":"message_stop"})),
    ]
    .concat();
    let fake = Fake::com(corpo);
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::ParadaAnormal);
}

#[tokio::test]
async fn uso_ausente_nao_vira_zero() {
    let store = loja();
    com_teto(&store, estimativa());
    let corpo = [
        evento(&serde_json::json!({"type":"message_start","message":{"model":MODELO,
            "usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})),
        evento(&serde_json::json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"texto"}})),
        evento(&serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}})),
        evento(&serde_json::json!({"type":"message_stop"})),
    ]
    .concat();
    let fake = Fake::com(corpo);
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::UsoNaoConfirmado);
    assert_eq!(limite::disponivel(&store, "bot:bot-fixture", AGORA).unwrap(), 0);
}

#[tokio::test]
async fn evento_desconhecido_recusado() {
    let store = loja();
    com_teto(&store, estimativa());
    let corpo = [
        evento(&serde_json::json!({"type":"message_start","message":{"model":MODELO,
            "usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})),
        evento(&serde_json::json!({"type":"invencao"})),
    ]
    .concat();
    let fake = Fake::com(corpo);
    assert_eq!(rodar(&store, &fake, &CofreBom).await.unwrap_err(), Falha::RespostaInvalida);
}

#[test]
fn reserva_concorrente_nao_passa_da_ultima_margem() {
    let store = loja();
    let est = estimativa();
    com_teto(&store, est.saturating_mul(2).saturating_sub(1));
    let primeira = limite::reservar(&store, "bot:bot-fixture", est, AGORA).unwrap();
    assert_eq!(primeira.micros, est);
    assert_eq!(limite::reservar(&store, "bot:bot-fixture", est, AGORA).unwrap_err(), Falha::TetoEsgotado);
}

#[test]
fn incerta_fecha_admissao_e_resolver_reabre() {
    let store = loja();
    let est = estimativa();
    com_teto(&store, est);
    let reserva = limite::reservar(&store, "bot:bot-fixture", est, AGORA).unwrap();
    limite::incerta(&store, &reserva, AGORA).unwrap();
    assert_eq!(limite::reservar(&store, "bot:bot-fixture", est, AGORA).unwrap_err(), Falha::UsoIncerto);
    limite::resolver_incerta(&store, &reserva.id, 0, AGORA).unwrap();
    assert!(limite::reservar(&store, "bot:bot-fixture", est, AGORA).is_ok());
}

#[test]
fn reserva_sobrevive_ao_reinicio() {
    let pasta = tempfile::tempdir().unwrap();
    let caminho = pasta.path().join("test.db");
    {
        let store = Store::open(&caminho).unwrap();
        migrate(&store.db.locked()).unwrap();
        com_teto(&store, estimativa());
        let reserva = limite::reservar(&store, "bot:bot-fixture", estimativa(), AGORA).unwrap();
        limite::incerta(&store, &reserva, AGORA).unwrap();
    }
    let store = Store::open(&caminho).unwrap();
    assert_eq!(limite::reservar(&store, "bot:bot-fixture", estimativa(), AGORA).unwrap_err(), Falha::UsoIncerto);
}

#[test]
fn cofre_do_hub_so_abre_o_registro_cifrado_do_vinculo() {
    let store = loja();
    let vinculo = vinculo();
    let outro = Vinculo { bot: "bot-fixture", concessao: "outra-concessao" };
    assert_eq!(cofre::CofreDoHub::novo(&store).abrir(&vinculo).unwrap_err(), RecusaCofre::Ausente);
    cofre::gravar(&store, &vinculo, CHAVE).unwrap();
    assert_eq!(cofre::CofreDoHub::novo(&store).abrir(&vinculo).unwrap().expor(), CHAVE);
    assert_eq!(cofre::CofreDoHub::novo(&store).abrir(&outro).unwrap_err(), RecusaCofre::Ausente);
    cofre::trancar(&store);
    assert_eq!(cofre::CofreDoHub::novo(&store).abrir(&vinculo).unwrap_err(), RecusaCofre::Indisponivel);
}

#[test]
fn registro_em_texto_claro_recusado_sem_migrar() {
    let store = loja();
    let vinculo = vinculo();
    let slot = cofre::aad_do_vinculo(&vinculo).unwrap();
    store.kv_set(&slot, CHAVE).unwrap();
    assert_eq!(cofre::CofreDoHub::novo(&store).abrir(&vinculo).unwrap_err(), RecusaCofre::Recusado);
    assert_eq!(store.kv_read(&slot).unwrap().as_deref(), Some(CHAVE), "nada foi migrado nem apagado");
}

#[test]
fn registro_ligado_a_outro_vinculo_recusado() {
    let store = loja();
    let original = vinculo();
    let slot = cofre::aad_do_vinculo(&original).unwrap();
    let cifrado = crate::market::vault::seal(&store, &slot, CHAVE).unwrap();
    store.kv_set("rota:v2:anthropic:bot-fixture:outra-concessao", &cifrado).unwrap();
    let outro = Vinculo { bot: "bot-fixture", concessao: "outra-concessao" };
    assert_eq!(cofre::CofreDoHub::novo(&store).abrir(&outro).unwrap_err(), RecusaCofre::Recusado);
}

#[test]
fn aad_do_vinculo_e_versionada_e_estavel() {
    let vinculo = vinculo();
    let aad = cofre::aad_do_vinculo(&vinculo).unwrap();
    assert_eq!(aad, "rota:v2:anthropic:bot-fixture:concessao-fixture");
    assert!(aad.contains(":v2:"));
    assert!(cofre::aad_do_vinculo(&Vinculo { bot: "", concessao: "c" }).is_none());
    assert!(cofre::aad_do_vinculo(&Vinculo { bot: "b", concessao: "../c" }).is_none());
}

#[test]
fn nada_de_segredo_em_erro_debug_ou_codigo() {
    let segredo = Segredo::novo(CHAVE.to_owned()).unwrap();
    assert_eq!(format!("{segredo:?}"), "<credencial>");
    let falhas = [
        Falha::VinculoInvalido,
        Falha::PedidoInvalido,
        Falha::SemTeto,
        Falha::TetoIndisponivel,
        Falha::TetoEsgotado,
        Falha::UsoIncerto,
        Falha::SemPreco,
        Falha::Cofre(RecusaCofre::Ausente),
        Falha::Cofre(RecusaCofre::Indisponivel),
        Falha::Cofre(RecusaCofre::Recusado),
        Falha::Cofre(RecusaCofre::FormatoInvalido),
        Falha::Cofre(RecusaCofre::VinculoInvalido),
        Falha::Transporte,
        Falha::Provedor { status: Some(500) },
        Falha::Provedor { status: None },
        Falha::RespostaInvalida,
        Falha::ConteudoNaoTexto,
        Falha::ParadaAnormal,
        Falha::CredencialRepetida,
        Falha::UsoNaoConfirmado,
        Falha::Limite,
    ];
    for falha in falhas {
        let texto = format!("{falha} {falha:?} {}", falha.codigo());
        assert!(!texto.contains(CHAVE), "{falha:?}");
        assert!(!texto.contains("sk-ant"), "{falha:?}");
        assert!(falha.codigo().bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{falha:?}");
    }
}

#[test]
fn segredo_recusa_login_pessoal_e_texto_livre() {
    assert!(Segredo::novo("sk-ant-oat-fixture".to_owned()).is_err());
    assert!(Segredo::novo("texto livre".to_owned()).is_err());
    assert!(Segredo::novo(String::new()).is_err());
}
