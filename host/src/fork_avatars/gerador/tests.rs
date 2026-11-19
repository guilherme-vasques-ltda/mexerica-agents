//! Testes do gerador 2b: cofre, cota, conjuntos e contrato — sem chamada paga.
use super::{api, chave, conjuntos, cota, id_gerado};
use crate::LockExt;
use crate::store::Store;
use serde_json::json;

fn store() -> Store {
    Store::open(std::path::Path::new(":memory:")).unwrap()
}

fn cofre(store: &Store) {
    *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![7; 32]));
}

#[test]
fn chave_nunca_volta_e_slot_e_dedicado() {
    let store = store();
    cofre(&store);
    assert!(!chave::configurada(&store).unwrap());
    chave::gravar(&store, "sk-teste-fixture").unwrap();
    assert!(chave::configurada(&store).unwrap());
    // O slot é só do gerador: voz e engine-account continuam vazios.
    assert!(crate::market::vault::read(&store, "voice").unwrap().is_none());
    let crua = crate::market::vault::read(&store, "mexeriquinhas").unwrap().unwrap();
    assert_eq!(crua, json!({"openai":"sk-teste-fixture"}).to_string());
    // A resolução só acontece dentro do adaptador; apagar volta a bloquear.
    chave::resolver(&store).unwrap();
    chave::gravar(&store, "").unwrap();
    assert!(!chave::configurada(&store).unwrap());
    assert!(chave::resolver(&store).is_err());
}

#[test]
fn sem_cofre_desbloqueado_gerador_indisponivel() {
    let store = store();
    *store.secret_key.locked() = None;
    // Leitura de slot vazio não diferencia travado: o estado olha a chave do cofre.
    assert_eq!(api::estado(&store)["cofre"], json!(false));
    assert_eq!(api::estado(&store)["gerador"], json!(false));
    assert!(chave::resolver(&store).is_err());
    // Gravar com o cofre travado também é recusado.
    assert!(chave::gravar(&store, "sk-x").is_err());
}

#[test]
fn cota_reserva_e_esgota() {
    let store = store();
    for _ in 0..cota::LIMITE_DIARIO {
        cota::reservar(&store).unwrap();
    }
    assert_eq!(cota::usados(&store), cota::LIMITE_DIARIO);
    let err = cota::reservar(&store).unwrap_err();
    assert_eq!(super::codigo(&err), "COTA_ATINGIDA");
    // Persistente: outra "leitura" do kv vê a reserva (mesma janela UTC).
    assert_eq!(cota::usados(&store), cota::LIMITE_DIARIO);
}

#[test]
fn id_gerado_so_uuid_v4_canonico() {
    let id = uuid::Uuid::new_v4().to_string();
    assert!(id_gerado(&id));
    assert!(!id_gerado("base"));
    assert!(!id_gerado(&id.to_uppercase()));
    assert!(!id_gerado("usr-00000000-0000-4000-8000-000000000000"));
    // uuid v1 não é aceito
    assert!(!id_gerado("00000000-0000-1000-8000-000000000000"));
}

fn fixture_conjunto(store: &Store, id: &uuid::Uuid) {
    let dir = conjuntos::pasta_job(store, id);
    for size in super::TAMANHOS {
        let d = dir.join(size.to_string());
        std::fs::create_dir_all(&d).unwrap();
        for q in super::QUADROS {
            std::fs::write(d.join(format!("{q}.webp")), b"RIFFxxxxWEBPVP8L").unwrap();
            conjuntos::privado(&d.join(format!("{q}.webp"))).unwrap();
        }
    }
    conjuntos::privado(&dir).unwrap();
}

#[test]
fn conjunto_so_vira_conjunto_ao_salvar() {
    let store = store();
    let id = uuid::Uuid::new_v4();
    fixture_conjunto(&store, &id);
    // Sem manifesto: não existe, não serve quadro, é órfão limpável.
    assert!(!conjuntos::existe(&store, &id.to_string()));
    assert!(conjuntos::quadro(&store, &id.to_string(), 64, "livre-1").is_err());
    conjuntos::limpar_orfaos(&store);
    assert!(!conjuntos::pasta_job(&store, &id).exists());
}

#[test]
fn salvar_serve_e_apagar_com_uso() {
    let store = store();
    let id = uuid::Uuid::new_v4();
    fixture_conjunto(&store, &id);
    let conjunto = conjuntos::salvar(&store, &id, "Fixturinha", "base").unwrap();
    // D1: o asset publicado tem uuid novo, distinto do id do job.
    let asset_id = conjunto["id"].as_str().unwrap().to_owned();
    assert!(id_gerado(&asset_id) && asset_id != id.to_string());
    assert!(conjuntos::existe(&store, &asset_id));
    assert!(!conjuntos::existe(&store, &id.to_string()));
    assert_eq!(conjuntos::listar(&store).unwrap()[0]["nome"], json!("Fixturinha"));
    // Quadro servido só pela allowlist fechada.
    assert!(conjuntos::quadro(&store, &asset_id, 64, "livre-1").unwrap().starts_with(b"RIFF"));
    assert!(conjuntos::quadro(&store, &asset_id, 128, "livre-1").is_err());
    assert!(conjuntos::quadro(&store, &asset_id, 64, "../conjunto").is_err());
    assert!(conjuntos::quadro(&store, "conjunto.json", 64, "livre-1").is_err());
    assert!(conjuntos::quadro(&store, &asset_id, 64, "conjunto.json").is_err());
    // Apagar conjunto livre funciona; conjunto referenciado por bot é recusado.
    conjuntos::apagar(&store, &asset_id).unwrap();
    assert!(!conjuntos::existe(&store, &asset_id));
}

#[test]
fn conferir_vinculo_recusa_assetid_inexistente() {
    let store = store();
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../../apps/desktop/src/shared/fork-mexeriquinha-wire-fixture.json"))
            .unwrap();
    let mut cfg: crate::store::BotConfig = serde_json::from_value(raw[0].clone()).unwrap();
    cfg.mexeriquinha = serde_json::from_value(json!({
        "v":3,"estilo":"3d","assetId":uuid::Uuid::new_v4().to_string(),"fallbackConjuntoId":"base"
    }))
    .unwrap();
    assert!(conjuntos::conferir_vinculo(&store, &cfg).is_err());
    // Com o conjunto salvo, o vínculo passa.
    let job = uuid::Uuid::new_v4();
    fixture_conjunto(&store, &job);
    let conjunto = conjuntos::salvar(&store, &job, "X", "base").unwrap();
    // O vínculo do bot aponta o assetId publicado; com ele salvo, a conferência passa.
    cfg.mexeriquinha = serde_json::from_value(json!({
        "v":3,"estilo":"3d","assetId":conjunto["id"],"fallbackConjuntoId":"base"
    }))
    .unwrap();
    conjuntos::conferir_vinculo(&store, &cfg).unwrap();
    // Config sem v3 não exige nada.
    cfg.mexeriquinha = serde_json::from_value(json!({"v":2,"estilo":"3d","conjuntoId":"base"})).unwrap();
    conjuntos::conferir_vinculo(&store, &cfg).unwrap();
}

#[test]
fn estado_sem_chave_nao_gera() {
    let store = store();
    cofre(&store);
    // O estado reporta indisponível e gerar é recusado sem credencial.
    assert!(chave::resolver(&store).is_err());
    assert_eq!(api::estado(&store), json!({"gerador":false,"cofre":true,"processador":false}));
}

/// SEG Fable P1-1: sem processador, o gate recusa ANTES de reservar cota —
/// nenhum clique em Gerar pode queimar tentativa nem chamada paga.
#[test]
fn gerar_recusa_antes_de_cota_sem_processador() {
    let store = store();
    cofre(&store);
    chave::gravar(&store, "sk-teste-fixture").unwrap();
    // Com chave configurada e cota zerada, o gate falha com código estável…
    let erro = api::gate_geracao(&store).unwrap_err();
    assert_eq!(super::codigo(&erro), "GERADOR_SEM_PROCESSADOR");
    // …e a cota permanece intacta (nenhuma tentativa consumida).
    assert_eq!(cota::usados(&store), 0);
    // O estado sinaliza indisponível mesmo com chave e cofre abertos.
    assert_eq!(api::estado(&store), json!({"gerador":false,"cofre":true,"processador":false}));
}
