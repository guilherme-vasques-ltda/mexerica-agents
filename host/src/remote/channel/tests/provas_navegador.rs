//! Fatia 2/B1: quadros binários no canal protegido, com fonte falsa (sem Chromium).
use super::*;
use crate::fork::{
    browser::stream::{Frame as Quadro, Size, canal as navegador},
    spaces::PERSONAL,
};
use crate::store::BotConfig;

fn hub_com_bot() -> Arc<Hub> {
    let hub = temp_hub();
    let cfg: BotConfig = serde_json::from_value(json!({"id": "bot-nav", "name": "Nav", "spaceId": PERSONAL})).unwrap();
    hub.store.save_bot(&cfg).unwrap();
    hub
}

fn nova_sessao() -> String {
    format!("sess-{}", uuid::Uuid::new_v4())
}

fn quadro(sessao: &str, target: &str, geracao: u64, sequence: u64) -> Quadro {
    Quadro {
        session: sessao.into(),
        target: target.into(),
        sequence,
        target_generation: geracao,
        image: Size { width: 960, height: 540 },
        viewport: Size { width: 1280, height: 720 },
        jpeg: vec![0xff, 0xd8, u8::try_from(sequence).unwrap_or(1), 0xff, 0xd9],
    }
}

fn inscricao(hub: &Hub, sessao: &str) -> Value {
    json!({"hubId": hub.host_id, "spaceId": PERSONAL, "botId": "bot-nav", "session": sessao, "target": "aba-1"})
}

fn entrada(sessao: &str, sequence: u64, frame_sequence: u64) -> Value {
    json!({"session": sessao, "target": "aba-1", "frameSequence": frame_sequence, "targetGeneration": 0,
        "sequence": sequence, "epoch": 0, "controller": "agent", "x": 960.0, "y": 540.0,
        "displayWidth": 1920.0, "displayHeight": 1080.0})
}

enum Recebido {
    Json(Value),
    Binario(Vec<u8>),
}

impl Phone {
    async fn hello_caps(&mut self, hub: &Hub) -> Out {
        let ek = StaticSecret::from(crypto::random::<32>());
        let ek_pub = crypto::x25519_pub(&ek);
        let n = crypto::random::<32>();
        let dk = self.key.verifying_key().to_bytes();
        let cid = hub.identity.cid_raw();
        let sig = crypto::sign(&self.key, &crypto::hs1_input(&cid, &dk, &ek_pub, &n));
        let hello = json!({"t": "hello", "v": 1, "dk": self.dk(), "ek": crypto::b64(&ek_pub),
            "n": crypto::b64(&n), "sig": crypto::b64(&sig), "pair": false, "caps": [navegador::CAPACIDADE_QUADROS]});
        self.tx.send(hello).await.unwrap();
        let out = self.next().await;
        if let Out::Msg(w) = &out
            && w["t"] == "welcome"
        {
            let ek_h: [u8; 32] = crypto::unb64_n(w["ek"].as_str().unwrap()).unwrap();
            let th = crypto::transcript_hash(&cid, &dk, &ek_pub, &n, &ek_h);
            let keys = crypto::channel_keys(&crypto::x25519(&ek, &ek_h).unwrap(), &th);
            self.sealer = Some(Sealer::new(keys.d2h));
            self.opener = Some(Opener::new(keys.h2d, 16 << 20));
        }
        out
    }

    async fn proximo(&mut self) -> Recebido {
        loop {
            match self.next().await {
                Out::Msg(f) if f["t"] == "f" => {
                    if let Some(bytes) = self.opener.as_mut().unwrap().open(&f).unwrap() {
                        if navegador::eh_binario(&bytes) {
                            return Recebido::Binario(bytes);
                        }
                        return Recebido::Json(serde_json::from_slice(&bytes).unwrap());
                    }
                }
                Out::Msg(_) => {}
                Out::Close(code, _) => panic!("canal fechou com {code}"),
            }
        }
    }

    async fn chamar(&mut self, id: u64, metodo: &str, corpo: Value) -> Value {
        self.send(json!({"id": id, "m": metodo, "b": corpo})).await;
        loop {
            if let Recebido::Json(v) = self.proximo().await
                && v["id"] == id
                && (v.get("ok").is_some() || v.get("err").is_some())
            {
                return v;
            }
        }
    }

    async fn esperar_quadro(&mut self) -> Quadro {
        loop {
            if let Recebido::Binario(bytes) = self.proximo().await {
                return navegador::decodificar(&bytes).unwrap();
            }
        }
    }
}

async fn inscrito(hub: &Arc<Hub>, com_capacidade: bool) -> (Phone, String) {
    let key = paired_phone(hub).await;
    let mut phone = Phone::direct(hub, key);
    if com_capacidade {
        let Out::Msg(welcome) = phone.hello_caps(hub).await else { panic!("welcome esperado") };
        assert_eq!(welcome["caps"], json!([navegador::CAPACIDADE_QUADROS]));
    } else {
        assert!(matches!(phone.hello(hub, false).await, Out::Msg(v) if v["t"] == "welcome" && v.get("caps").is_none()));
    }
    let sessao = nova_sessao();
    let resposta = phone.chamar(1, "browserStreamSubscribe", inscricao(hub, &sessao)).await;
    assert!(resposta.get("ok").is_some(), "inscrição aceita: {resposta}");
    (phone, sessao)
}

#[tokio::test]
async fn round_trip_com_fonte_falsa_e_entrada_confirmada() {
    let hub = hub_com_bot();
    let (mut phone, sessao) = inscrito(&hub, true).await;
    assert!(navegador::oferecer(quadro(&sessao, "aba-1", 0, 1)));
    assert_eq!(phone.esperar_quadro().await.sequence, 1);
    let resposta = phone.chamar(2, "browserStreamInput", entrada(&sessao, 1, 1)).await;
    assert_eq!(resposta["ok"]["accepted"], true);
    assert!(navegador::oferecer(quadro(&sessao, "aba-1", 0, 2)));
    assert_eq!(phone.esperar_quadro().await.sequence, 2);
    let executadas = navegador::drenar_entradas(&sessao);
    assert_eq!(executadas.len(), 1, "entrada sai antes do quadro seguinte");
    assert_eq!((executadas[0].sequence, executadas[0].x, executadas[0].y), (1, 640.0, 360.0), "escala 1920×1080");
    assert_eq!(phone.chamar(3, "browserStreamAck", json!({})).await["ok"]["acknowledged"], true);
}

#[tokio::test]
async fn cliente_antigo_sem_capacidade_nunca_recebe_quadro() {
    let hub = hub_com_bot();
    let (mut phone, sessao) = inscrito(&hub, false).await;
    let resposta = phone.chamar(2, "browserStreamSubscribe", inscricao(&hub, &sessao)).await;
    assert_eq!(resposta["err"]["status"], 403, "sessão já é do primeiro canal");
    assert!(!navegador::oferecer(quadro(&sessao, "aba-1", 0, 1)), "sem capacidade, a fonte nem enfileira");
}

#[tokio::test]
async fn outro_space_hub_errado_e_segundo_aparelho_sao_recusados() {
    let hub = hub_com_bot();
    let (_dono, sessao) = inscrito(&hub, true).await;
    let key = paired_phone(&hub).await;
    let mut outro = Phone::direct(&hub, key);
    outro.hello_caps(&hub).await;
    let mut corpo = inscricao(&hub, &sessao);
    let resposta = outro.chamar(1, "browserStreamSubscribe", corpo.clone()).await;
    assert_eq!(resposta["err"]["status"], 403, "sessão já tem dono");
    corpo["spaceId"] = json!("outro-space");
    corpo["session"] = json!(nova_sessao());
    assert_eq!(outro.chamar(2, "browserStreamSubscribe", corpo.clone()).await["err"]["status"], 403, "outro Space");
    corpo["spaceId"] = json!(PERSONAL);
    corpo["hubId"] = json!("outro-hub");
    assert_eq!(outro.chamar(3, "browserStreamSubscribe", corpo).await["err"]["status"], 403, "outro Hub");
    assert_eq!(
        outro.chamar(4, "browserStreamInput", entrada(&sessao, 1, 1)).await["err"]["status"],
        400,
        "sem inscrição, sem entrada"
    );
}

#[tokio::test]
async fn epoca_de_controle_recusa_comando_do_agente() {
    let hub = hub_com_bot();
    let (mut phone, sessao) = inscrito(&hub, true).await;
    assert!(navegador::oferecer(quadro(&sessao, "aba-1", 0, 1)));
    assert_eq!(phone.esperar_quadro().await.sequence, 1);
    let epoca = phone.chamar(2, "browserStreamControl", json!({"controller": "person"})).await;
    assert_eq!(epoca["ok"]["epoch"], 1);
    let mut corpo = entrada(&sessao, 1, 1);
    corpo["epoch"] = json!(1);
    assert_eq!(
        phone.chamar(3, "browserStreamInput", corpo.clone()).await["ok"]["accepted"],
        false,
        "agente com pessoa no controle"
    );
    corpo["controller"] = json!("person");
    corpo["epoch"] = json!(0);
    assert_eq!(phone.chamar(4, "browserStreamInput", corpo.clone()).await["ok"]["accepted"], false, "época velha");
    corpo["epoch"] = json!(1);
    assert_eq!(phone.chamar(5, "browserStreamInput", corpo).await["ok"]["accepted"], true, "pessoa na época atual");
}

#[tokio::test]
async fn troca_de_target_descarta_quadro_velho() {
    let hub = hub_com_bot();
    let (mut phone, sessao) = inscrito(&hub, true).await;
    let resposta = phone.chamar(2, "browserStreamTarget", json!({"target": "aba-2"})).await;
    assert_eq!(resposta["ok"]["targetGeneration"], 1);
    assert!(!navegador::oferecer(quadro(&sessao, "aba-1", 0, 1)), "target velho");
    assert!(!navegador::oferecer(quadro(&sessao, "aba-2", 0, 1)), "geração velha");
    assert!(navegador::oferecer(quadro(&sessao, "aba-2", 1, 1)));
    let recebido = phone.esperar_quadro().await;
    assert_eq!((recebido.target.as_str(), recebido.target_generation), ("aba-2", 1));
}

#[tokio::test]
async fn cliente_lento_recebe_so_o_quadro_mais_recente() {
    let hub = hub_com_bot();
    let (mut phone, sessao) = inscrito(&hub, true).await;
    for sequence in 1..=5 {
        assert!(navegador::oferecer(quadro(&sessao, "aba-1", 0, sequence)));
    }
    assert_eq!(phone.esperar_quadro().await.sequence, 5);
}

#[tokio::test]
async fn binaria_corrompida_fecha_e_gigante_nao_aloca() {
    let hub = hub_com_bot();
    let (mut phone, _sessao) = inscrito(&hub, true).await;
    let mut corrompido = navegador::codificar(&quadro("s", "a", 0, 1)).unwrap();
    *corrompido.last_mut().unwrap() = 0;
    for f in phone.sealer.as_mut().unwrap().seal(&corrompido).unwrap() {
        phone.tx.send(f).await.unwrap();
    }
    assert!(matches!(phone.next().await, Out::Close(4002, _)), "binária do aparelho fecha o canal");

    let (mut phone, _sessao) = inscrito(&hub, true).await;
    let gigante = vec![0u8; navegador::MAX_BINARIO + 1];
    for f in phone.sealer.as_mut().unwrap().seal(&gigante).unwrap() {
        phone.tx.send(f).await.unwrap();
    }
    assert!(matches!(phone.next().await, Out::Close(4013, _)), "acima do limite fecha sem montar quadro");
}

#[tokio::test]
async fn desconexao_encerra_a_sessao() {
    let hub = hub_com_bot();
    let (phone, sessao) = inscrito(&hub, true).await;
    assert!(navegador::oferecer(quadro(&sessao, "aba-1", 0, 1)));
    drop(phone);
    for _ in 0..200 {
        if !navegador::oferecer(quadro(&sessao, "aba-1", 0, 2)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("sessão deveria sumir após a desconexão");
}

type Corpo = fn() -> Value;
const CONTROLE_E_ALVO: [(&str, Corpo); 2] = [
    ("browserStreamControl", || json!({"controller": "person"})),
    ("browserStreamTarget", || json!({"target": "aba-2"})),
];

async fn assert_recusa_de_escopo(phone: &mut Phone, primeiro_id: u64) {
    for (i, (metodo, corpo)) in (primeiro_id..).zip(CONTROLE_E_ALVO) {
        let resposta = phone.chamar(i, metodo, corpo()).await;
        assert_eq!(resposta["err"]["status"], 403, "{metodo}: {resposta}");
        assert_eq!(resposta["err"]["message"], "browser_scope_unavailable", "{metodo}: {resposta}");
    }
}

#[tokio::test]
async fn controle_e_alvo_sem_inscricao_dao_403() {
    let hub = hub_com_bot();
    let key = paired_phone(&hub).await;
    let mut phone = Phone::direct(&hub, key);
    phone.hello_caps(&hub).await;
    assert_recusa_de_escopo(&mut phone, 1).await;
}

// Mover bot entre Spaces é recusado pelo Hub (`space_transfer_requires_explicit_contract`); o escopo da
// conexão só muda, portanto, por exclusão do bot, coberta abaixo. Bot de outro Space na inscrição: ver
// `outro_space_hub_errado_e_segundo_aparelho_sao_recusados`.
#[tokio::test]
async fn controle_e_alvo_com_bot_apagado_dao_403() {
    let hub = hub_com_bot();
    let (mut phone, _sessao) = inscrito(&hub, true).await;
    hub.store.delete_bot("bot-nav").unwrap();
    assert_recusa_de_escopo(&mut phone, 2).await;
}

#[tokio::test]
async fn controle_e_alvo_depois_de_revogar_o_aparelho_fecham_o_canal() {
    let hub = hub_com_bot();
    let (mut phone, _sessao) = inscrito(&hub, true).await;
    assert!(hub.revoke_device(&phone.dk()).unwrap());
    phone.send(json!({"id": 2, "m": "browserStreamControl", "b": {"controller": "person"}})).await;
    loop {
        match phone.next().await {
            Out::Close(code, _) => {
                assert_eq!(code, 4003, "aparelho revogado fecha o canal");
                break;
            }
            Out::Msg(f) if f["t"] == "f" => {
                let aberto = phone.opener.as_mut().unwrap().open(&f).unwrap();
                assert!(aberto.is_none_or(|b| !b.starts_with(b"{\"id\":2")), "sem resposta ao aparelho revogado");
            }
            Out::Msg(_) => {}
        }
    }
}
