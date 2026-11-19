use super::*;
use crate::{
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Store},
};
use serde_json::json;

fn wire_fixture() -> Vec<Value> {
    serde_json::from_str(include_str!("../../../apps/desktop/src/shared/fork-mexeriquinha-wire-fixture.json")).unwrap()
}

#[test]
fn v2_e_fechado_e_recusa_html_urls_campos_extras_e_versoes_futuras() {
    let fixture = wire_fixture();
    for bot in &fixture {
        let avatar: AvatarConfig = serde_json::from_value(bot["avatarConfig"].clone()).unwrap();
        assert_eq!(serde_json::to_value(avatar).unwrap(), bot["avatarConfig"]);
    }
    let simples = fixture[0]["mexeriquinha"].clone();
    let pelucia = fixture[1]["mexeriquinha"].clone();
    let mut invalid = Vec::new();
    for (key, value) in
        [("v", json!(3)), ("estilo", json!("pelucia")), ("modeloId", json!("inexistente")), ("html", json!("<svg/>"))]
    {
        let mut config = simples.clone();
        config[key] = value;
        invalid.push(config);
    }
    for (key, value) in
        [("cor", json!("#123456")), ("olhos", json!("<script/>")), ("objeto", Value::Null), ("nome", json!("extra"))]
    {
        let mut config = simples.clone();
        config["aparencia"][key] = value;
        invalid.push(config);
    }
    let mut missing = simples.clone();
    missing["aparencia"].as_object_mut().unwrap().remove("objeto");
    invalid.push(missing);
    for id in ["https://evil.test/a.webp", "data:image/png;base64,AAAA", "../base", "novo-gerado"] {
        let mut config = pelucia.clone();
        config["conjuntoId"] = json!(id);
        invalid.push(config);
    }
    let mut excessive = pelucia;
    excessive["conjuntoId"] = json!("x".repeat(1025));
    invalid.push(excessive);
    for value in invalid {
        assert!(serde_json::from_value::<MexeriquinhaConfig>(value.clone()).is_err(), "{value}");
    }
}

#[tokio::test]
async fn hub_real_valida_persiste_e_emite_fixture_v2_sem_abrir_autoridade_ao_bot() {
    let dir = std::env::temp_dir().join(format!("mexeriquinha-wire-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    for bot in wire_fixture() {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":bot["id"],"name":bot["name"],"backend":"codex"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
    // Como a fixture do editor: o update real envia Reconfigure ao ator registrado.
    for row in hub.store.bots().unwrap() {
        hub.start_created_bot(&row.config);
    }
    for bot in wire_fixture() {
        let id = bot["id"].as_str().unwrap();
        let patch = json!({"id":id,"avatarConfig":bot["avatarConfig"],"mexeriquinha":bot["mexeriquinha"]});
        assert!(hub.update_bot(&patch).is_err(), "bot não ganha autoridade de editar aparência");
        let saved = hub.update_bot_by_user(&patch).unwrap();
        assert_eq!(saved["avatarConfig"], bot["avatarConfig"]);
        assert_eq!(saved["mexeriquinha"], bot["mexeriquinha"]);
        assert!(serde_json::from_value::<AvatarConfig>(saved["avatarConfig"].clone()).is_ok());
        let wire = hub.bots_json(0).unwrap().into_iter().find(|b| b["id"] == id).unwrap();
        assert_eq!(wire["avatarConfig"], bot["avatarConfig"]);
        assert_eq!(wire["mexeriquinha"], bot["mexeriquinha"]);
        let cfg = hub.store.bot(id).unwrap().unwrap().config;
        assert!(crate::store::AVATAR_COLORS.contains(&cfg.avatar_color.as_str()));
        assert!(crate::store::AVATAR_SHAPES.contains(&cfg.avatar_shape.as_str()));
        assert_ne!(label(&cfg), "");
        let (raw, avatar) = stored(&cfg).unwrap();
        assert!(serde_json::from_str::<Value>(&raw).unwrap().get("avatarConfig").is_none());
        assert_eq!(
            serde_json::to_value(restored(&raw, Some(&avatar)).unwrap().avatar_config).unwrap(),
            bot["avatarConfig"]
        );
        let rev = hub.store.current_rev();
        let mut invalid = bot["avatarConfig"].clone();
        invalid["url"] = json!("https://evil.test/");
        assert!(hub.update_bot_by_user(&json!({"id":id,"name":"Não salvar","avatarConfig":invalid})).is_err());
        assert_eq!(hub.store.current_rev(), rev);
        assert_eq!(hub.store.bot(id).unwrap().unwrap().config.name, bot["name"].as_str().unwrap());
        assert_eq!(hub.bot_value_for_test(id)["avatarConfig"], bot["avatarConfig"]);
        // Atualização explícita legada segue aceita e lida sem converter o contrato da biblioteca.
        let legacy = serde_json::to_value(preset("Amazon", "", false)).unwrap();
        assert_eq!(hub.update_bot_by_user(&json!({"id":id,"avatarConfig":legacy})).unwrap()["avatarConfig"], legacy);
        assert!(hub.store.bot(id).unwrap().unwrap().config.mexeriquinha.is_none());
    }
    hub.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migracao_v2_e_transacional_preserva_escolhas_e_incrementa_revision_uma_vez() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,rev INTEGER,deleted INTEGER DEFAULT 0,avatar_config TEXT); CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT); INSERT INTO kv VALUES('rev','10'); INSERT INTO kv VALUES('team.settings.v1','{\"primaryBotId\":\"a\"}');").unwrap();
    let legacy = serde_json::to_string(&preset("Developer", "", false)).unwrap();
    for id in ["a", "b"] {
        c.execute(
            "INSERT INTO bots(id,config,rev,avatar_config) VALUES(?1,?2,10,?3)",
            params![id, json!({"id":id,"name":id,"futureSetting":{"keep":true}}).to_string(), legacy],
        )
        .unwrap();
    }
    // Falha após a primeira atualização: nem a aparência anterior nem a revisão global podem ficar pela metade.
    c.execute_batch("CREATE TRIGGER reject_b BEFORE UPDATE ON bots WHEN old.id='b' BEGIN SELECT RAISE(ABORT,'falha de teste'); END;").unwrap();
    assert!(migrate(&c).is_err());
    let rev: String = c.query_row("SELECT v FROM kv WHERE k='rev'", [], |r| r.get(0)).unwrap();
    assert_eq!(rev, "10");
    for id in ["a", "b"] {
        let avatar: String = c.query_row("SELECT avatar_config FROM bots WHERE id=?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(avatar, legacy);
    }
    c.execute_batch("DROP TRIGGER reject_b;").unwrap();
    migrate(&c).unwrap();
    let read = |id: &str| -> (String, i64, String) {
        c.query_row("SELECT config,rev,avatar_config FROM bots WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
    };
    let a = read("a");
    let b = read("b");
    assert_eq!(serde_json::from_str::<Value>(&a.0).unwrap()["mexeriquinha"]["modeloId"], Value::Null);
    assert_eq!(serde_json::from_str::<Value>(&b.0).unwrap()["mexeriquinha"]["estilo"], "simples");
    assert_eq!(serde_json::from_str::<Value>(&a.0).unwrap()["futureSetting"], json!({"keep":true}));
    assert!(a.1 > 10 && b.1 > 10);
    migrate(&c).unwrap();
    assert_eq!(read("a"), a);
    assert_eq!(read("b"), b);
    // Principal com escolha v2 própria deve conservar aparência e revisão.
    let chosen = wire_fixture()[1]["avatarConfig"].to_string();
    c.execute(
        "UPDATE bots SET config=json_set(config,'$.mexeriquinha',json(?1)),avatar_config=?2 WHERE id='a'",
        params![wire_fixture()[1]["mexeriquinha"].to_string(), chosen],
    )
    .unwrap();
    let before = read("a");
    migrate(&c).unwrap();
    assert_eq!(read("a"), before);
    // Escolha legada explícita após a migração não é convertida de novo na reabertura.
    c.execute("UPDATE bots SET config=json_remove(config,'$.mexeriquinha'),avatar_config=?1 WHERE id='b'", [&legacy])
        .unwrap();
    let before = read("b");
    migrate(&c).unwrap();
    assert_eq!(read("b"), before);
}

#[test]
fn novos_sem_avatar_ja_sao_v2_e_principal_personalizada_preserva_aparencia() {
    let mut cfg: BotConfig = serde_json::from_value(json!({"id":"tool-created","name":"Outro"})).unwrap();
    settle(&mut cfg);
    assert_eq!(serde_json::to_value(cfg.mexeriquinha).unwrap()["v"], 2);
    let mut old = serde_json::to_value(preset("", "", true)).unwrap();
    old["color"] = json!("#0003cc");
    old["head"] = json!("helmet");
    let custom: AvatarConfig = serde_json::from_value(old).unwrap();
    let converted = mexeriquinhas::from_legacy(&custom, true);
    assert_eq!(converted, mexeriquinhas::from_legacy(&custom, false));
    assert_eq!(converted.0["aparencia"]["cor"], "celeste", "desempate por ID igual ao TS");
    assert_eq!(converted.0["aparencia"]["acessorio"], "capacete");
    assert!(converted.0.get("modeloId").is_none());
    assert_eq!(mexeriquinhas::from_legacy(&preset("", "", true), true), principal_mexeriquinha());
}

#[test]
fn downgrade_main_real_le_config_coluna_listagem_e_escrita_sem_ocultar_outros_bots() {
    use super::legacy_main_fixture as old;
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,avatar_config TEXT)").unwrap();
    for bot in wire_fixture() {
        let mut cfg: BotConfig = serde_json::from_value(bot.clone()).unwrap();
        settle(&mut cfg);
        let (profile, avatar) = stored(&cfg).unwrap();
        c.execute("INSERT INTO bots VALUES(?1,?2,?3)", params![cfg.id, profile, avatar]).unwrap();
    }
    let rows: Vec<(String, String)> = c
        .prepare("SELECT config,avatar_config FROM bots ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    for (profile, avatar) in &rows {
        // Exatamente o BotConfig completo da main, schema/Deserialize antigos congelados.
        let direct_old: old::model::BotConfig = serde_json::from_str(profile).unwrap();
        assert!(!direct_old.id.is_empty(), "os leitores diretos antigos não descartam o perfil");
        let direct_new: BotConfig = serde_json::from_str(profile).unwrap();
        assert!(direct_new.mexeriquinha.is_some(), "os leitores diretos novos conservam a chave tipada");
        let restored_old = old::restored(profile, Some(avatar)).unwrap();
        assert!(restored_old.avatar_config.is_some());
        let restored_new = restored(profile, Some(avatar)).unwrap();
        assert!(restored_new.mexeriquinha.is_some());
        let sql = old::profile_query("SELECT {profile} FROM bots WHERE id=?1");
        let old_projection: String = c.query_row(&sql, [&direct_old.id], |r| r.get(0)).unwrap();
        assert!(serde_json::from_str::<old::model::BotConfig>(&old_projection).is_ok());
        let new_projection: String = c
            .query_row(&profile_query("SELECT {profile} FROM bots WHERE id=?1"), [&direct_old.id], |r| r.get(0))
            .unwrap();
        assert!(serde_json::from_str::<BotConfig>(&new_projection).unwrap().mexeriquinha.is_some());
    }
    let (profile, avatar) = &rows[0];
    let mut old_bot = old::restored(profile, Some(avatar)).unwrap();
    let id = old_bot.id.clone();
    old_bot.name = "Editado no Hub antigo".into();
    let (old_profile, old_avatar) = old::stored(&old_bot).unwrap();
    c.execute("UPDATE bots SET config=?2,avatar_config=?3 WHERE id=?1", params![id, old_profile, old_avatar]).unwrap();
    let read_back = restored(&old_profile, Some(&old_avatar)).unwrap();
    assert_eq!(read_back.name, "Editado no Hub antigo");
    assert!(read_back.mexeriquinha.is_none(), "a regravação antiga descarta somente a chave desconhecida deste perfil");
    let other: (String, String) = c
        .query_row("SELECT config,avatar_config FROM bots WHERE id<>?1", [&id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(restored(&other.0, Some(&other.1)).unwrap().mexeriquinha.is_some());
}

#[test]
fn public_avatar_config_nunca_aceita_v2_conflito_nao_prevalece_e_aliases_sao_recusados() {
    let bot = wire_fixture().remove(0);
    assert!(serde_json::from_value::<AvatarConfig>(bot["mexeriquinha"].clone()).is_err());
    let mut conflicting = bot.clone();
    conflicting["avatarConfig"]["color"] = json!("#000000");
    assert!(validate_patch(&conflicting).is_err());
    let mut cfg: BotConfig = serde_json::from_value(conflicting.clone()).unwrap();
    assert!(crate::hub::validate(&mut cfg).is_err());
    let cfg = restored(&conflicting.to_string(), Some(&conflicting["avatarConfig"].to_string())).unwrap();
    assert!(cfg.mexeriquinha.is_none());
    assert!(validate_patch(&json!({"mexeriquinha":null})).is_err());
    for key in ["mexeriquinha", "Mexeriquinha", "mexeriquinha_config", "mexeriquinhaConfig", "mexeriquinha-config"] {
        let patch = json!({(key):bot["mexeriquinha"]});
        assert!(user_only(&patch).is_err());
        if key != "mexeriquinha" {
            assert!(validate_patch(&patch).is_err());
        }
    }
}

#[test]
fn projecao_legada_v2_tem_baseline_literal_cores_olhos_e_22_conjuntos() {
    // Baseline de compatibilidade: não deriva de DATA/defaultConfig, que também podem mudar.
    let expected = |color: &str, eye: &str, face: &str| {
        json!({
            "shape":"oval","color":color,"eye":eye,"head":"none","face":face,
            "ears":"none","chin":"none","neck":"none","cheeks":"none","badge":"none","badgeStyle":"light"
        })
    };
    for (cor, color) in [
        ("laranja", "#f28a1e"),
        ("lima", "#94c630"),
        ("limao", "#f2c41e"),
        ("toranja", "#ec5a5e"),
        ("roxo", "#8d56d8"),
        ("celeste", "#2f9cf0"),
    ] {
        let value: MexeriquinhaConfig=serde_json::from_value(json!({"v":2,"estilo":"simples","aparencia":{"cor":cor,"olhos":"ponto","roupa":"nenhuma","acessorio":"nenhum","objeto":"nenhum"}})).unwrap();
        assert_eq!(
            serde_json::to_value(mexeriquinhas::legacy_projection(&value)).unwrap(),
            expected(color, "auto", "none"),
            "{cor}"
        );
    }
    for (olhos, eye, face) in [
        ("ponto", "auto", "none"),
        ("sonolentos", "bar", "none"),
        ("brilhantes", "round", "none"),
        ("oculos-redondos", "auto", "glasses"),
        ("oculos-escuros", "auto", "shades"),
        ("piscando", "cat", "none"),
        ("oculos-verdes", "auto", "shades"),
    ] {
        let value: MexeriquinhaConfig=serde_json::from_value(json!({"v":2,"estilo":"simples","aparencia":{"cor":"laranja","olhos":olhos,"roupa":"nenhuma","acessorio":"nenhum","objeto":"nenhum"}})).unwrap();
        assert_eq!(
            serde_json::to_value(mexeriquinhas::legacy_projection(&value)).unwrap(),
            expected("#f28a1e", eye, face),
            "{olhos}"
        );
    }
    let sets = [
        ("base", "#f28a1e", "auto", "none"),
        ("ceo", "#f28a1e", "auto", "none"),
        ("assistente", "#f28a1e", "auto", "none"),
        ("financeiro", "#94c630", "auto", "glasses"),
        ("marketing", "#ec5a5e", "auto", "shades"),
        ("developer", "#8d56d8", "auto", "none"),
        ("pesquisa", "#f2c41e", "auto", "none"),
        ("vendas", "#2f9cf0", "cat", "none"),
        ("compras", "#94c630", "auto", "none"),
        ("rh", "#ec5a5e", "round", "none"),
        ("suporte", "#f2c41e", "auto", "none"),
        ("designer", "#8d56d8", "auto", "none"),
        ("amazon", "#2f9cf0", "auto", "none"),
        ("dono", "#f28a1e", "auto", "none"),
        ("chefe", "#ec5a5e", "auto", "none"),
        ("hacker", "#f28a1e", "auto", "shades"),
        ("berga", "#f28a1e", "auto", "none"),
        ("pomarca", "#f28a1e", "auto", "none"),
        ("colheitor", "#94c630", "auto", "none"),
        ("tesourango", "#f2c41e", "auto", "glasses"),
        ("holofruta", "#ec5a5e", "auto", "shades"),
        ("chiperica", "#2f9cf0", "auto", "none"),
    ];
    assert_eq!(sets.len(), 22);
    for (id, color, eye, face) in sets {
        let value: MexeriquinhaConfig = serde_json::from_value(json!({"v":2,"estilo":"3d","conjuntoId":id})).unwrap();
        assert_eq!(
            serde_json::to_value(mexeriquinhas::legacy_projection(&value)).unwrap(),
            expected(color, eye, face),
            "{id}"
        );
    }
}

fn invalid_optional_mexeriquinhas() -> Vec<Value> {
    let original = wire_fixture()[1]["mexeriquinha"].clone();
    let mut values = vec![Value::Null, json!(true), json!(42), json!("inválido"), json!([])];
    for version in [json!(3), json!(2.01), json!("2")] {
        let mut value = original.clone();
        value["v"] = version;
        values.push(value);
    }
    for id in ["inexistente", "https://evil.test/a.webp", "data:image/png;base64,AAAA"] {
        let mut value = original.clone();
        value["conjuntoId"] = json!(id);
        values.push(value);
    }
    let mut extra = original.clone();
    extra["extra"] = json!(true);
    values.push(extra);
    let mut oversized = original;
    oversized["conjuntoId"] = json!("x".repeat(1025));
    values.push(oversized);
    values
}

#[test]
fn somente_optional_do_perfil_tolera_matriz_invalida_e_outros_campos_continuam_estritos() {
    let legacy = wire_fixture()[1]["avatarConfig"].clone();
    let mut records = Vec::new();
    for (i, value) in invalid_optional_mexeriquinhas().into_iter().enumerate() {
        let raw =
            json!({"id":format!("read-{i}"),"name":"Continuo visível","avatarConfig":legacy,"mexeriquinha":value});
        let direct: BotConfig = serde_json::from_str(&raw.to_string()).unwrap();
        assert!(direct.mexeriquinha.is_none(), "{value}");
        assert_eq!(serde_json::to_value(direct.avatar_config).unwrap(), legacy);
        assert!(restored(&raw.to_string(), Some(&legacy.to_string())).unwrap().mexeriquinha.is_none());
        assert!(serde_json::from_value::<MexeriquinhaConfig>(value.clone()).is_err());
        assert!(validate_patch(&json!({"mexeriquinha":value})).is_err());
        records.push(raw);
    }
    let listed: Vec<Value> = serde_json::from_str(&serde_json::to_string(&records).unwrap()).unwrap();
    let readable: Vec<_> =
        listed.into_iter().filter_map(|raw| serde_json::from_str::<BotConfig>(&raw.to_string()).ok()).collect();
    assert_eq!(readable.len(), records.len(), "nenhum perfil some da listagem por sua aparência opcional");
    for bot in wire_fixture() {
        let direct: BotConfig = serde_json::from_str(&bot.to_string()).unwrap();
        assert_eq!(serde_json::to_value(direct.mexeriquinha).unwrap(), bot["mexeriquinha"]);
        assert_eq!(
            serde_json::to_value(
                restored(&bot.to_string(), Some(&bot["avatarConfig"].to_string())).unwrap().mexeriquinha
            )
            .unwrap(),
            bot["mexeriquinha"]
        );
    }
    assert!(serde_json::from_str::<BotConfig>("{").is_err());
    assert!(serde_json::from_value::<BotConfig>(json!({"id":"bad","name":42,"mexeriquinha":null})).is_err());
    assert!(
        serde_json::from_value::<BotConfig>(
            json!({"id":"bad","name":"Nome","avatarConfig":{"v":2},"mexeriquinha":null})
        )
        .is_err()
    );
}

#[tokio::test]
async fn leitura_tolerante_nao_relaxa_update_real_nem_autoridade_e_controle_valido_persiste() {
    let dir = std::env::temp_dir().join(format!("mexeriquinha-strict-write-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut raw = wire_fixture()[0].clone();
    raw["backend"] = json!("codex");
    store.save_bot(&serde_json::from_value::<BotConfig>(raw).unwrap()).unwrap();
    let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
    let id = wire_fixture()[0]["id"].as_str().unwrap().to_owned();
    hub.start_created_bot(&hub.store.bot(&id).unwrap().unwrap().config);
    let before = serde_json::to_value(hub.store.bot(&id).unwrap().unwrap().config).unwrap();
    let rev = hub.store.current_rev();
    for value in invalid_optional_mexeriquinhas() {
        let patch = json!({"id":id,"name":"Não gravar","mexeriquinha":value});
        assert!(hub.update_bot_by_user(&patch).is_err(), "{value}");
        assert!(hub.update_bot(&patch).is_err());
        assert_eq!(hub.store.current_rev(), rev);
        assert_eq!(serde_json::to_value(hub.store.bot(&id).unwrap().unwrap().config).unwrap(), before);
    }
    let control = wire_fixture()[1].clone();
    let saved=hub.update_bot_by_user(&json!({"id":id,"name":"Gravação válida","avatarConfig":control["avatarConfig"],"mexeriquinha":control["mexeriquinha"]})).unwrap();
    assert_eq!(saved["mexeriquinha"], control["mexeriquinha"]);
    assert_eq!(saved["name"], "Gravação válida");
    assert!(hub.store.current_rev() > rev);
    hub.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn versao_numerica_exata_e_canonica_sem_aceitar_string_ou_valor_futuro() {
    for version in ["2", "2.0", "2e0"] {
        let raw = format!(r#"{{"v":{version},"estilo":"3d","conjuntoId":"base"}}"#);
        let value: MexeriquinhaConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(serde_json::to_value(&value).unwrap(), json!({"v":2,"estilo":"3d","conjuntoId":"base"}));
        assert_eq!(value, serde_json::from_value(json!({"v":2,"estilo":"3d","conjuntoId":"base"})).unwrap());
        assert!(validate_patch(&json!({"mexeriquinha":serde_json::from_str::<Value>(&raw).unwrap()})).is_ok());
    }
    for version in ["2.01", "3", r#""2""#] {
        let raw = format!(r#"{{"v":{version},"estilo":"3d","conjuntoId":"base"}}"#);
        assert!(serde_json::from_str::<MexeriquinhaConfig>(&raw).is_err());
    }
}

#[test]
fn subconjunto_schema_fica_fechado_e_oneof_tem_discriminantes_disjuntos() {
    assert!(mexeriquinhas::supported_schema(&mexeriquinhas::SCHEMA));
    let original = mexeriquinhas::SCHEMA.clone();
    for (pointer, value) in [
        ("/unknown", json!(true)),
        ("/oneOf/0/minProperties", json!(1)),
        ("/oneOf/0/additionalProperties", json!(true)),
        ("/oneOf/0/type", json!("array")),
        ("/oneOf/0/properties/modeloId/oneOf", json!([{"const":"berga"}])),
        ("/oneOf/0/properties/modeloId/description", json!("metadata aninhada não suportada")),
        ("/oneOf/1/properties/estilo/const", json!("simples")),
    ] {
        let mut schema = original.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        schema.pointer_mut(parent).unwrap().as_object_mut().unwrap().insert(key.into(), value);
        assert!(!mexeriquinhas::supported_schema(&schema), "{pointer}");
    }
    let mut metadata = original;
    metadata["description"] = json!("metadado só da raiz");
    assert!(mexeriquinhas::supported_schema(&metadata));
}

#[test]
fn backup_legacy_e_atomico_duravel_primeiro_original_e_nao_fabrica_passado() {
    use super::legacy_main_fixture as old;
    let dir = std::env::temp_dir().join(format!("mexeriquinha-original-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("backup.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,rev INTEGER,deleted INTEGER DEFAULT 0,avatar_config TEXT); CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT); INSERT INTO kv VALUES('rev','10');").unwrap();
    let legacy = format!(" \n{} \n", serde_json::to_string(&preset("Developer", "", false)).unwrap());
    let mut existing_v2 = wire_fixture()[0].clone();
    existing_v2["id"] = json!("v2");
    let records = [
        (
            "sql-null",
            json!({"id":"sql-null","name":"A","avatarConfig":null,"avatarColor":"blue","avatarShape":"blob","description":"não copiar"}),
            None,
        ),
        ("json-null", json!({"id":"json-null","name":"B"}), Some("null".to_owned())),
        (
            "explicit",
            json!({"id":"explicit","name":"C","avatarConfig":serde_json::to_value(preset("Marketing","",false)).unwrap(),"avatarColor":"red","avatarShape":"hex","description":"não copiar"}),
            Some(legacy.clone()),
        ),
        ("absent", json!({"id":"absent","name":"D","avatarColor":"green"}), None),
        ("preexisting", json!({"id":"preexisting","name":"E"}), Some(legacy.clone())),
        ("v2", existing_v2, Some(wire_fixture()[0]["avatarConfig"].to_string())),
    ];
    for (id, raw, avatar) in &records {
        c.execute(
            "INSERT INTO bots(id,config,rev,avatar_config) VALUES(?1,?2,10,?3)",
            params![id, raw.to_string(), avatar],
        )
        .unwrap();
    }
    let preexisting = "original anterior intocado";
    c.execute("INSERT INTO kv VALUES('mexeriquinhas.legacy.original.preexisting',?1)", [preexisting]).unwrap();
    c.execute_batch("CREATE TRIGGER reject_explicit BEFORE UPDATE ON bots WHEN old.id='explicit' BEGIN SELECT RAISE(ABORT,'falha controlada'); END;").unwrap();
    assert!(migrate(&c).is_err());
    let count: i64 = c
        .query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'mexeriquinhas.legacy.original.%'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "backups criados na transação também voltam atrás");
    assert!(
        !c.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='mexeriquinhas.internal.v2.migrated')", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
    let rev: String = c.query_row("SELECT v FROM kv WHERE k='rev'", [], |r| r.get(0)).unwrap();
    assert_eq!(rev, "10");
    for (id, raw, avatar) in &records {
        let row: (String, Option<String>) = c
            .query_row("SELECT config,avatar_config FROM bots WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(row, (raw.to_string(), avatar.clone()));
    }
    c.execute_batch("DROP TRIGGER reject_explicit;").unwrap();
    migrate(&c).unwrap();
    drop(c);
    let c = Connection::open(&path).unwrap();
    for (id, raw, avatar) in &records {
        let backup: Option<String> = c
            .query_row("SELECT v FROM kv WHERE k=?1", [format!("mexeriquinhas.legacy.original.{id}")], |r| r.get(0))
            .optional()
            .unwrap();
        if *id == "v2" {
            assert!(backup.is_none());
            continue;
        }
        if *id == "preexisting" {
            assert_eq!(backup.as_deref(), Some(preexisting));
            continue;
        }
        let appearance: serde_json::Map<String, Value> = ["avatarConfig", "avatarColor", "avatarShape"]
            .into_iter()
            .filter_map(|key| raw.get(key).map(|v| (key.to_owned(), v.clone())))
            .collect();
        assert_eq!(
            serde_json::from_str::<Value>(&backup.unwrap()).unwrap(),
            json!({"avatar_config":avatar,"config":appearance}),
            "{id}"
        );
    }
    // A escrita pelo contrato antigo ignora a chave do perfil, mas não toca no backup em kv.
    let row: (String, String) = c
        .query_row("SELECT config,avatar_config FROM bots WHERE id='explicit'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    let mut old_cfg = old::restored(&row.0, Some(&row.1)).unwrap();
    old_cfg.name = "Nome no Hub antigo".into();
    let (profile, avatar) = old::stored(&old_cfg).unwrap();
    c.execute("UPDATE bots SET config=?1,avatar_config=?2 WHERE id='explicit'", params![profile, avatar]).unwrap();
    let before: String =
        c.query_row("SELECT v FROM kv WHERE k='mexeriquinhas.legacy.original.explicit'", [], |r| r.get(0)).unwrap();
    migrate(&c).unwrap();
    assert_eq!(
        c.query_row::<String, _, _>("SELECT v FROM kv WHERE k='mexeriquinhas.legacy.original.explicit'", [], |r| r
            .get(0))
            .unwrap(),
        before
    );
    assert!(restored(&profile, Some(&avatar)).unwrap().mexeriquinha.is_none());
    // Nem marcador apagado manualmente pode substituir um backup preexistente.
    c.execute("DELETE FROM kv WHERE k='mexeriquinhas.internal.v2.migrated'", []).unwrap();
    migrate(&c).unwrap();
    assert_eq!(
        c.query_row::<String, _, _>("SELECT v FROM kv WHERE k='mexeriquinhas.legacy.original.explicit'", [], |r| r
            .get(0))
            .unwrap(),
        before
    );
    // Bot legado posterior ao marcador: nada de migração retroativa ou original inventado.
    c.execute("INSERT INTO bots(id,config,rev,avatar_config) VALUES('after-marker','{\"id\":\"after-marker\",\"name\":\"Antigo\"}',20,?1)",[&legacy]).unwrap();
    migrate(&c).unwrap();
    assert!(
        !c.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='mexeriquinhas.legacy.original.after-marker')", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
    assert_eq!(
        c.query_row::<String, _, _>("SELECT avatar_config FROM bots WHERE id='after-marker'", [], |r| r.get(0))
            .unwrap(),
        legacy
    );
    drop(c);
    std::fs::remove_dir_all(dir).unwrap();
}
