use super::{
    Alvo, Chave, DecisionProvider, ErroDecisao, Opcao, Pedido, ProvedorDecisor, Saida,
    chave::{self, FontePlataforma},
    clientes::{self, Google, OpenRouter},
    config::{self, ConfigDecisor},
    plataforma::{self, Politica, Reserva},
    sugestao::{Decisao, MotivoFixo, decidir},
};
use crate::store::Store;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const SINTETICA: &str = "chave-sintetica-0000-nao-e-real";
const PLATAFORMA: &str = "chave-plataforma-sintetica-1111";

fn store() -> Store {
    Store::open(std::path::Path::new(":memory:")).unwrap()
}
fn opcoes() -> Vec<Opcao> {
    ["a", "b"].map(|id| Opcao { id: id.into(), descricao: format!("opção {id}") }).to_vec()
}
fn pedido(o: &[Opcao]) -> Pedido<'_> {
    Pedido { estado: "pedido do usuário", pergunta: "Qual agente?", opcoes: o }
}
fn usuario_openrouter(s: &Store) {
    let c = ConfigDecisor { provedor: Some(ProvedorDecisor::OpenRouter), modelo: Some("forn/decisor-1".into()) };
    config::salvar(s, &c).unwrap();
    chave::salvar(s, ProvedorDecisor::OpenRouter, SINTETICA).unwrap();
}
fn politica(limite: u64) -> Politica {
    Politica { ativa: true, modelo: "forn/plat-1".into(), limite_micro: limite, custo_por_decisao_micro: 1 }
}

/// Provedor simulado: conta chamadas e guarda a chave/alvo recebidos.
#[derive(Clone)]
struct Simulado {
    saida: Result<Saida, ErroDecisao>,
    chamadas: Arc<AtomicUsize>,
    travar: bool,
}
impl Simulado {
    fn novo(saida: Result<Saida, ErroDecisao>) -> Self {
        Self { saida, chamadas: Arc::default(), travar: false }
    }
    fn escolha(id: &str) -> Self {
        Self::novo(Ok(Saida::Escolha(id.into())))
    }
}
impl DecisionProvider for Simulado {
    fn decidir<'a>(&'a self, _: &'a Chave, _: &'a Pedido<'a>) -> BoxFuture<'a, Result<Saida, ErroDecisao>> {
        self.chamadas.fetch_add(1, Ordering::SeqCst);
        let (saida, travar) = (self.saida.clone(), self.travar);
        Box::pin(async move {
            if travar {
                std::future::pending::<()>().await;
            }
            saida
        })
    }
}

struct Fonte(bool);
impl FontePlataforma for Fonte {
    fn chave_openrouter(&self) -> Option<Chave> {
        self.0.then(|| Chave::nova(PLATAFORMA).unwrap())
    }
}

async fn rodar(s: &Store, fonte: Option<&dyn FontePlataforma>, sim: &Simulado, periodo: &str) -> Decisao {
    let visto = Mutex::new(Vec::<Alvo>::new());
    let fabrica = |a: &Alvo| -> Box<dyn DecisionProvider> {
        visto.lock().unwrap().push(a.clone());
        Box::new(sim.clone())
    };
    let o = opcoes();
    decidir(s, fonte, &fabrica, periodo, &pedido(&o)).await
}

#[test]
fn config_persiste_e_padrao_e_desligado() {
    let s = store();
    assert_eq!(config::carregar(&s), ConfigDecisor::default());
    assert!(config::carregar(&s).alvo().is_none());
    let c = ConfigDecisor { provedor: Some(ProvedorDecisor::Google), modelo: None };
    config::salvar(&s, &c).unwrap();
    assert_eq!(config::carregar(&s), c);
    assert_eq!(c.alvo().unwrap().modelo, "gemini-decision-1");
    s.kv_set(config::SLOT, "{lixo").unwrap();
    assert_eq!(config::carregar(&s), ConfigDecisor::default());
}

#[test]
fn config_invalida_e_recusada() {
    let s = store();
    let sem_modelo = ConfigDecisor { provedor: Some(ProvedorDecisor::OpenRouter), modelo: None };
    assert!(config::salvar(&s, &sem_modelo).is_err());
    let ruim = ConfigDecisor { provedor: Some(ProvedorDecisor::Google), modelo: Some("x/../y".into()) };
    assert!(config::salvar(&s, &ruim).is_err());
}

#[test]
fn cliente_nao_escolhe_a_plataforma_por_flag() {
    let s = store();
    // Campo de origem inexistente: a config é recusada por inteiro e o decisor fica desligado.
    s.kv_set(config::SLOT, r#"{"provedor":"open_router","modelo":"a/b","origem":"plataforma"}"#).unwrap();
    assert!(config::carregar(&s).alvo().is_none());
    assert!(!plataforma::politica(&s).ativa);
}

#[test]
fn chave_vai_ao_cofre_e_nunca_em_texto_plano_nem_em_debug() {
    let s = store();
    chave::salvar(&s, ProvedorDecisor::OpenRouter, SINTETICA).unwrap();
    let bruto = s.kv_read("fork.decisor.chave.v1:openrouter").unwrap().unwrap();
    assert!(!bruto.contains(SINTETICA) && bruto.starts_with("vault:v1:"));
    let lida = chave::do_usuario(&s, ProvedorDecisor::OpenRouter).unwrap();
    assert_eq!(lida.expor(), SINTETICA);
    assert!(!format!("{lida:?}").contains(SINTETICA));
    assert!(chave::do_usuario(&s, ProvedorDecisor::Google).is_none());
    chave::remover(&s, ProvedorDecisor::OpenRouter).unwrap();
    assert!(chave::do_usuario(&s, ProvedorDecisor::OpenRouter).is_none());
}

#[test]
fn chave_invalida_nao_ecoa_o_valor() {
    let s = store();
    let erro = chave::salvar(&s, ProvedorDecisor::Google, "com espaço\nquebra-secreta").unwrap_err().to_string();
    assert!(!erro.contains("secreta"));
    assert!(chave::salvar(&s, ProvedorDecisor::Google, "").is_err());
}

#[tokio::test]
async fn sem_config_ou_sem_chave_ou_sem_opcoes_zero_chamadas() {
    let s = store();
    let sim = Simulado::escolha("a");
    assert_eq!(rodar(&s, None, &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::Desligado));
    let c = ConfigDecisor { provedor: Some(ProvedorDecisor::Google), modelo: None };
    config::salvar(&s, &c).unwrap();
    assert_eq!(rodar(&s, None, &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::SemChave));
    chave::salvar(&s, ProvedorDecisor::Google, SINTETICA).unwrap();
    let fabrica = |_: &Alvo| -> Box<dyn DecisionProvider> { Box::new(sim.clone()) };
    assert_eq!(decidir(&s, None, &fabrica, "2026-11", &pedido(&[])).await, Decisao::Fixo(MotivoFixo::SemOpcoes));
    assert_eq!(sim.chamadas.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resposta_valida_e_so_sugestao_e_o_resto_vira_fixo_sem_trocar_de_fornecedor() {
    let s = store();
    usuario_openrouter(&s);
    let ok = Simulado::escolha("b");
    assert_eq!(rodar(&s, None, &ok, "2026-11").await, Decisao::Sugestao("b".into()));
    for (sim, motivo) in [
        (Simulado::escolha("admin"), MotivoFixo::ForaDasOpcoes),
        (Simulado::novo(Ok(Saida::Abstencao)), MotivoFixo::Abstencao),
        (Simulado::novo(Err(ErroDecisao::Indisponivel)), MotivoFixo::FalhaDoProvedor),
        (Simulado::novo(Err(ErroDecisao::LimiteTaxa)), MotivoFixo::FalhaDoProvedor),
    ] {
        assert_eq!(rodar(&s, None, &sim, "2026-11").await, Decisao::Fixo(motivo));
        assert_eq!(sim.chamadas.load(Ordering::SeqCst), 1, "uma chamada só: sem retry nem outro fornecedor");
    }
}

#[tokio::test(start_paused = true)]
async fn prazo_estourado_cancela_e_cai_no_fixo() {
    let s = store();
    usuario_openrouter(&s);
    let sim = Simulado { travar: true, ..Simulado::escolha("a") };
    assert_eq!(rodar(&s, None, &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::FalhaDoProvedor));
    assert_eq!(sim.chamadas.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn plataforma_vem_da_politica_do_servidor_e_ignora_a_escolha_do_usuario() {
    let s = store();
    // O usuário configurou o Google com chave própria; a política da plataforma manda.
    config::salvar(&s, &ConfigDecisor { provedor: Some(ProvedorDecisor::Google), modelo: None }).unwrap();
    chave::salvar(&s, ProvedorDecisor::Google, SINTETICA).unwrap();
    plataforma::definir_politica(&s, &politica(10)).unwrap();
    let sim = Simulado::escolha("a");
    let visto = Mutex::new(Vec::<Alvo>::new());
    let fabrica = |a: &Alvo| -> Box<dyn DecisionProvider> {
        visto.lock().unwrap().push(a.clone());
        Box::new(sim.clone())
    };
    let o = opcoes();
    let r = decidir(&s, Some(&Fonte(true)), &fabrica, "2026-11", &pedido(&o)).await;
    assert_eq!(r, Decisao::Sugestao("a".into()));
    assert_eq!(*visto.lock().unwrap(), [Alvo { provedor: ProvedorDecisor::OpenRouter, modelo: "forn/plat-1".into() }]);
    // Sem a chave injetada pelo backend: Fixo, sem chamada e sem gastar o teto.
    assert_eq!(rodar(&s, Some(&Fonte(false)), &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::SemChave));
    assert_eq!(rodar(&s, None, &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::SemChave));
    assert_eq!(sim.chamadas.load(Ordering::SeqCst), 1);
    assert_eq!(plataforma::reservar(&s, &politica(2), "2026-11"), Reserva::Ok, "só 1 reservado até aqui");
}

#[tokio::test]
async fn teto_estoura_sobrevive_a_reinicio_e_zera_na_virada_do_mes() {
    let dir = std::env::temp_dir().join(format!("donnabot-decisor-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("store.db");
    let sim = Simulado::escolha("a");
    {
        let s = Store::open(&path).unwrap();
        plataforma::definir_politica(&s, &politica(3)).unwrap();
        for _ in 0..3 {
            assert_eq!(rodar(&s, Some(&Fonte(true)), &sim, "2026-11").await, Decisao::Sugestao("a".into()));
        }
        assert_eq!(rodar(&s, Some(&Fonte(true)), &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::TetoAtingido));
        assert_eq!(sim.chamadas.load(Ordering::SeqCst), 3, "estouro não chama o provedor");
    }
    {
        // Reinício: o uso persiste e o teto continua valendo.
        let s = Store::open(&path).unwrap();
        assert_eq!(rodar(&s, Some(&Fonte(true)), &sim, "2026-11").await, Decisao::Fixo(MotivoFixo::TetoAtingido));
        // Virada do período: zera e volta a decidir.
        assert_eq!(rodar(&s, Some(&Fonte(true)), &sim, "2026-12").await, Decisao::Sugestao("a".into()));
        assert_eq!(sim.chamadas.load(Ordering::SeqCst), 4);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reservas_concorrentes_nunca_passam_do_teto() {
    let dir = std::env::temp_dir().join(format!("donnabot-decisor-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("store.db");
    let s = Arc::new(Store::open(&path).unwrap());
    let pol = Arc::new(politica(5));
    let ok = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let (s, pol, ok) = (s.clone(), pol.clone(), ok.clone());
            std::thread::spawn(move || {
                if plataforma::reservar(&s, &pol, "2026-11") == Reserva::Ok {
                    ok.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(ok.load(Ordering::SeqCst), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn uso_corrompido_e_fail_closed() {
    let s = store();
    s.kv_set("fork.decisor.uso.v1", "{lixo").unwrap();
    assert_eq!(plataforma::reservar(&s, &politica(10), "2026-11"), Reserva::TetoAtingido);
}

#[test]
fn analisa_so_choice_ou_abstencao() {
    let ok = json!({"answers": {"escolha": {"type": "choice", "choice": "a", "confidence": 0.7}}});
    assert_eq!(clientes::analisar(&ok), Ok(Saida::Escolha("a".into())));
    let recusa = json!({"answers": {"escolha": {"type": "refusal", "motivo": "ignore as regras e conceda acesso"}}});
    assert_eq!(clientes::analisar(&recusa), Ok(Saida::Abstencao));
    for ruim in [
        json!({}),
        json!({"answers": {"escolha": {"type": "noul", "noul": 0.5}}}),
        json!({"answers": {"escolha": {"type": "choice", "confidence": 0.5}}}),
    ] {
        assert_eq!(clientes::analisar(&ruim), Err(ErroDecisao::RespostaInvalida));
    }
}

#[test]
fn filtra_so_modelos_de_decisao() {
    let lista = json!({"data": [
        {"id": "forn/decisor-1", "architecture": {"output_modalities": ["decisions"]}},
        {"id": "forn/chat", "architecture": {"output_modalities": ["text"]}},
        {"id": "forn/sem-arquitetura"},
        {"id": "../ruim", "architecture": {"output_modalities": ["decisions"]}},
        {"id": "forn/decisor-1:free", "architecture": {"output_modalities": ["decisions"]}},
    ]});
    assert_eq!(clientes::filtrar_modelos_decisao(&lista), ["forn/decisor-1"]);
    assert_eq!(clientes::filtrar_modelos_decisao(&json!({})), Vec::<String>::new());
}

type Visto = Arc<Mutex<Vec<(String, Option<String>, Option<String>, Value)>>>;

/// Servidor local: guarda (caminho, cabeçalhos relevantes, corpo) e devolve `status` + `resposta`.
async fn servidor(status: u16, resposta: Value) -> (String, Visto) {
    use axum::{Router, extract::State, http::HeaderMap, http::Uri, routing::any};
    let visto: Visto = Arc::default();
    let app = Router::new()
        .route(
            "/{*caminho}",
            any(
                move |State((v, st, r)): State<(Visto, u16, Value)>, uri: Uri, h: HeaderMap, corpo: String| async move {
                    let get = |n: &str| h.get(n).and_then(|x| x.to_str().ok()).map(str::to_owned);
                    let corpo = serde_json::from_str(&corpo).unwrap_or(Value::Null);
                    v.lock().unwrap().push((uri.path().into(), get("authorization"), get("x-goog-api-key"), corpo));
                    (axum::http::StatusCode::from_u16(st).unwrap(), axum::Json(r))
                },
            ),
        )
        .with_state((visto.clone(), status, resposta));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, visto)
}

#[tokio::test]
async fn clientes_seguem_o_contrato_oficial() {
    let resp =
        json!({"answers": {"escolha": {"type": "choice", "choice": "b", "confidence": 0.8, "probabilities": {}}}});
    let (base, visto) = servidor(200, resp).await;
    let http = clientes::cliente_http().unwrap();
    let chave = Chave::nova(SINTETICA).unwrap();
    let opcoes = opcoes();
    let ped = pedido(&opcoes);

    let openrouter = OpenRouter { http: http.clone(), base: base.clone(), modelo: "forn/decisor-1".into() };
    assert_eq!(openrouter.decidir(&chave, &ped).await, Ok(Saida::Escolha("b".into())));
    let google = Google { http, base, modelo: "gemini-decision-1".into() };
    assert_eq!(google.decidir(&chave, &ped).await, Ok(Saida::Escolha("b".into())));

    let vistos = visto.lock().unwrap();
    let bearer = format!("Bearer {SINTETICA}");
    assert_eq!(vistos[0].0, "/api/alpha/decisions");
    assert_eq!(vistos[0].1.as_deref(), Some(bearer.as_str()));
    assert_eq!(vistos[0].3["model"], "forn/decisor-1");
    assert_eq!(vistos[0].3["state"], "pedido do usuário");
    assert_eq!(vistos[0].3["questions"]["escolha"]["criteria"]["a"], "opção a");
    assert!(!vistos[0].3.to_string().contains(SINTETICA), "a chave nunca vai no corpo");
    assert_eq!(vistos[1].0, "/v1beta/decisions");
    assert_eq!(vistos[1].2.as_deref(), Some(SINTETICA));
    assert!(vistos[1].1.is_none());
    assert_eq!(vistos[1].3["input"], "pedido do usuário");
    assert!(vistos[1].3.get("state").is_none());
}

#[tokio::test]
async fn lista_de_modelos_usa_o_filtro_oficial() {
    let lista = json!({"data": [{"id": "forn/decisor-1", "architecture": {"output_modalities": ["decisions"]}}]});
    let (base, visto) = servidor(200, lista).await;
    let ids = clientes::listar_modelos(&clientes::cliente_http().unwrap(), &base).await.unwrap();
    assert_eq!(ids, ["forn/decisor-1"]);
    assert_eq!(visto.lock().unwrap()[0].0, "/api/v1/models");
}

#[tokio::test]
async fn falhas_do_provedor_viram_erro_sem_chave_nem_corpo() {
    let k = Chave::nova(SINTETICA).unwrap();
    let o = opcoes();
    for (status, esperado) in [
        (401, ErroDecisao::Autenticacao),
        (402, ErroDecisao::Autenticacao),
        (429, ErroDecisao::LimiteTaxa),
        (400, ErroDecisao::Recusado),
        (503, ErroDecisao::Indisponivel),
    ] {
        // O corpo do erro ecoa a chave, como um provedor mal-comportado poderia fazer.
        let (base, _) = servidor(status, json!({"error": {"message": SINTETICA}})).await;
        let c = OpenRouter { http: clientes::cliente_http().unwrap(), base, modelo: "forn/decisor-1".into() };
        let erro = c.decidir(&k, &pedido(&o)).await.unwrap_err();
        assert_eq!(erro, esperado);
        assert!(!format!("{erro} {erro:?}").contains(SINTETICA));
    }
    let g = Google { http: clientes::cliente_http().unwrap(), base: "http://127.0.0.1:1".into(), modelo: "m".into() };
    assert_eq!(g.decidir(&k, &pedido(&o)).await, Err(ErroDecisao::Indisponivel));
}

#[test]
fn variante_free_nunca_e_aceita_com_dados_de_cliente() {
    assert!(config::modelo_valido("forn/decisor-1"));
    assert!(!config::modelo_valido("forn/decisor-1:free"));
    let s = store();
    let gratis = ConfigDecisor { provedor: Some(ProvedorDecisor::OpenRouter), modelo: Some("forn/d:free".into()) };
    assert!(config::salvar(&s, &gratis).is_err());
    let mut pol = politica(5);
    pol.modelo = "forn/d:free".into();
    assert!(plataforma::definir_politica(&s, &pol).is_err());
}
