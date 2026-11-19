use super::*;
use crate::{
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Store},
};
use serde_json::json;

#[test]
fn valida_schema_e_recusa_cor_injetada_e_enum_desconhecido() {
    let original = serde_json::to_value(preset("", "", true)).unwrap();
    for color in ["#123", "#12345678", "red", "#GG0000", "#123456;display:none", "#123456\n"] {
        let mut config = original.clone();
        config["color"] = color.into();
        assert!(serde_json::from_value::<AvatarConfig>(config).is_err(), "{color}");
    }
    for key in SCHEMA["properties"].as_object().unwrap().keys().filter(|k| *k != "color") {
        let mut config = original.clone();
        config[key] = "desconhecido".into();
        assert!(serde_json::from_value::<AvatarConfig>(config).is_err(), "{key}");
    }
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("eye");
    assert!(serde_json::from_value::<AvatarConfig>(missing).is_err());
    let mut extra = original.clone();
    extra["style"] = "display:none".into();
    assert!(serde_json::from_value::<AvatarConfig>(extra).is_err());
    for role in ROLES.iter() {
        assert!(validate(&serde_json::to_value(role_config(role)).unwrap()).is_ok());
    }
    let mut custom = original;
    custom["color"] = "#aB09fF".into();
    assert!(serde_json::from_value::<AvatarConfig>(custom).is_ok());
}

#[test]
fn migracao_preenche_presets_padrao_e_donna_sem_sobrescrever_edicao() {
    let dir = std::env::temp_dir().join(format!("donnabot-avatar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bots.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute("DELETE FROM kv WHERE k='mexeriquinhas.internal.v2.migrated'", []).unwrap();
    c.execute("INSERT INTO kv VALUES('team.settings.v1', ?1)", [json!({"primaryBotId":"primary"}).to_string()])
        .unwrap();
    let customized = serde_json::to_value(preset("Amazon", "", false)).unwrap();
    let converted = |avatar: AvatarConfig| serde_json::to_value(mexeriquinhas::from_legacy(&avatar, false)).unwrap();
    let converted_custom = converted(preset("Amazon", "", false));
    for (id, name, description, avatar) in [
        ("primary", "Minha assistente", "", None),
        ("amazon", "Amazon", "", None),
        ("role", "Minha analista", "Especialista em Meta Ads", None),
        ("default", "Sem função conhecida", "", None),
        ("legacy", "Jurídico", "", None),
        ("mentions_donna", "Assistente da Donna", "Reporte à Donna um resumo diário.", None),
        ("explicit_donna", "Escolha do usuário", "", Some(serde_json::to_value(preset("", "", true)).unwrap())),
        ("saved", "Donna", "", Some(customized.clone())),
    ] {
        let mut config = json!({"id":id,"name":name,"description":description});
        if id == "legacy" {
            config["avatarColor"] = "green".into();
            config["avatarShape"] = "cloud".into();
            config["futureSetting"] = json!({"preserve":true});
        }
        if let Some(avatar) = avatar {
            config["avatarConfig"] = avatar;
        }
        c.execute("INSERT INTO bots(id, rev, config) VALUES(?1, 0, ?2)", params![id, config.to_string()]).unwrap();
    }
    for color in crate::store::AVATAR_COLORS {
        let id = format!("legacy_{color}");
        let config = json!({"id":id,"name":"Zeca","avatarColor":color,"avatarShape":"cloud"});
        c.execute("INSERT INTO bots(id, rev, config) VALUES(?1, 0, ?2)", params![id, config.to_string()]).unwrap();
    }
    drop(c);
    let store = Store::open(&path).unwrap();
    let avatar = |id| serde_json::to_value(store.bot(id).unwrap().unwrap().config.mexeriquinha).unwrap();
    assert_eq!(avatar("primary"), serde_json::to_value(principal_mexeriquinha()).unwrap());
    assert_eq!(avatar("amazon"), converted_custom);
    assert_eq!(avatar("role"), converted(preset("Meta Ads", "", false)));
    assert_eq!(avatar("default"), converted(legacy_config("blob", "blue")));
    assert_eq!(avatar("legacy"), converted(legacy_config("cloud", "green")));
    assert_eq!(avatar("mentions_donna"), converted(legacy_config("blob", "blue")));
    assert_eq!(avatar("explicit_donna"), converted(preset("", "", true)));
    assert_eq!(crate::store::AVATAR_COLORS.len(), 11);
    for color in crate::store::AVATAR_COLORS {
        let cfg = store.bot(&format!("legacy_{color}")).unwrap().unwrap().config;
        assert_eq!(cfg.mexeriquinha, Some(mexeriquinhas::from_legacy(&legacy_config("cloud", color), false)));
        assert!(crate::store::AVATAR_COLORS.contains(&cfg.avatar_color.as_str()));
        assert_eq!(cfg.avatar_shape, "blob");
    }
    let c = Connection::open(&path).unwrap();
    let raw: String = c.query_row("SELECT config FROM bots WHERE id='legacy'", [], |r| r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap()["futureSetting"], json!({"preserve":true}));
    drop(c);
    assert_eq!(avatar("saved"), converted_custom);
    store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    store.set_team_settings(&json!({"primaryBotId":"amazon"})).unwrap();
    assert_eq!(
        serde_json::to_value(store.bot("saved").unwrap().unwrap().config.mexeriquinha).unwrap(),
        converted_custom
    );
    assert_eq!(
        serde_json::to_value(store.bot("amazon").unwrap().unwrap().config.mexeriquinha).unwrap(),
        converted_custom
    );
    let rev = store.current_rev();
    assert!(rev > 0);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.current_rev(), rev, "migração idempotente");
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn somente_usuario_altera_avatar_e_sync_leva_o_valor_validado() {
    let dir = std::env::temp_dir().join(format!("donnabot-avatar-api-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let member: BotConfig = serde_json::from_value(json!({"id":"member","name":"Membro"})).unwrap();
    store.save_bot(&member).unwrap();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":"group","name":"Grupo","kind":"group","members":["member"]})).unwrap();
    store.save_bot(&cfg).unwrap();
    let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
    let config = serde_json::to_value(preset("", "", true)).unwrap();
    let patch = json!({"id":"group","avatarConfig":config});
    assert!(hub.update_bot(&patch).is_err());
    assert!(user_only(&json!({"avatarColor":"red"})).is_err());
    let value = hub.update_bot_by_user(&patch).unwrap();
    assert_eq!(value["avatarConfig"], config);
    let saved = hub.store.bot("group").unwrap().unwrap().config;
    let (color, shape) = legacy_avatar(&saved);
    assert_eq!(value["avatarColor"], color);
    assert_eq!(value["avatarShape"], shape);
    assert_ne!(color, "blue");
    assert_eq!(hub.bots_json(0).unwrap().iter().find(|bot| bot["id"] == "group").unwrap()["avatarConfig"], config);
    assert!(hub.update_bot_by_user(&json!({"id":"group","avatarConfig":null})).is_err());
    let mut invalid = config.clone();
    invalid["eye"] = "desconhecido".into();
    assert!(hub.update_bot_by_user(&json!({"id":"group","name":"Não salvar","avatarConfig":invalid})).is_err());
    assert_eq!(hub.store.bot("group").unwrap().unwrap().config.name, "Grupo");
    assert_eq!(hub.bot_value_for_test("group")["avatarConfig"], config);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migracao_preserva_leitura_tolerante_de_settings_e_perfis_antigos() {
    let dir = std::env::temp_dir().join(format!("donnabot-avatar-legacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bots.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute("INSERT INTO bots(id,rev,config) VALUES('ok',0,?1)", [json!({"id":"ok","name":"Amazon"}).to_string()])
        .unwrap();
    c.execute("INSERT INTO bots(id,rev,config) VALUES('broken',0,'{')", []).unwrap();
    for raw in ["{", "{\"primaryBotId\":42}"] {
        c.execute("INSERT INTO kv(k,v) VALUES('team.settings.v1',?1) ON CONFLICT(k) DO UPDATE SET v=?1", [raw])
            .unwrap();
        c.execute("UPDATE bots SET avatar_config=NULL WHERE id='ok'", []).unwrap();
        c.execute("DELETE FROM kv WHERE k='mexeriquinhas.internal.v2.migrated'", []).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store.bot("ok").unwrap().unwrap().config.mexeriquinha,
            Some(mexeriquinhas::from_legacy(&preset("Amazon", "", false), false))
        );
        assert!(store.bot("broken").unwrap().is_none());
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(store.bot("ok").unwrap().is_some(), "reabrir banco já migrado também funciona");
    }
    drop(c);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn padrao_distinto_e_funcao_por_palavras_inteiras_em_portugues_e_ingles() {
    let default = preset("", "", false);
    let donna = preset("", "", true);
    assert_ne!(default.0["shape"], donna.0["shape"]);
    assert_ne!(default.0["color"], donna.0["color"]);
    for name in ["Pesquisadora", "Researcher", "Minha pesquisadora"] {
        assert_eq!(preset(name, "", false), preset("Fonte", "", false));
    }
    assert_eq!(preset("Marketing", "calendário", false), preset("Hype", "", false));
    assert_eq!(preset("Maria", "Cuida do marketing: calendário de conteúdo e campanhas", false), default);
    assert_eq!(preset("XresearcherX", "devagar", false), default);
    assert_eq!(preset("Maria", "Responsável pelas atas", false), preset("Ata", "", false));
    assert_eq!(preset("Maria", "Cuida de customer support", false), preset("Ajuda", "", false));
}

#[test]
fn mencoes_a_donna_e_nomes_de_persona_nas_instrucoes_nao_escolhem_preset() {
    let default = preset("", "", false);
    for (name, description) in [
        ("Donna", ""),
        ("Assistente da Donna", ""),
        ("Zeca", "Reporte à Donna um resumo diário."),
        ("Assistente geral", "Atue como assistente geral."),
        ("Zeca", "Atue como general assistant."),
    ] {
        assert_eq!(preset(name, description, false), default, "{name}: {description}");
        let mut cfg: BotConfig =
            serde_json::from_value(json!({"id":"other","name":name,"description":description})).unwrap();
        settle(&mut cfg);
        assert_eq!(cfg.mexeriquinha, Some(mexeriquinhas::from_legacy(&legacy_config("blob", "blue"), false)));
    }
    for (id, description) in [
        ("psicologa", "Mantenha a calma ao responder."),
        ("suporte", "Revise contratos e peça ajuda quando tiver dúvida."),
        ("research", "Sempre cite a fonte."),
    ] {
        let role = ROLES.iter().find(|role| role["id"] == id).unwrap();
        assert_eq!(preset("Zeca", description, false), default, "{description}");
        assert_eq!(preset(role["name"].as_str().unwrap(), "", false), role_config(role));
        assert_eq!(preset("Zeca", role["role"].as_str().unwrap(), false), role_config(role));
    }
}

#[test]
fn escrita_projeta_todas_as_formas_e_cores_para_clientes_legados() {
    for role in ROLES.iter() {
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"other","name":"Outro"})).unwrap();
        cfg.avatar_config = Some(role_config(role));
        settle(&mut cfg);
        assert!(crate::store::AVATAR_COLORS.contains(&cfg.avatar_color.as_str()));
        assert!(crate::store::AVATAR_SHAPES.contains(&cfg.avatar_shape.as_str()));
        assert_eq!(cfg.avatar_config, Some(role_config(role)));
    }
    let mut cfg: BotConfig =
        serde_json::from_value(json!({"id":"legacy","name":"Jurídico","avatarColor":"green","avatarShape":"cloud"}))
            .unwrap();
    settle(&mut cfg);
    assert_eq!(cfg.mexeriquinha, Some(mexeriquinhas::from_legacy(&legacy_config("cloud", "green"), false)));
    assert!(crate::store::AVATAR_COLORS.contains(&cfg.avatar_color.as_str()));
    assert_eq!(cfg.avatar_shape, "blob");
}
