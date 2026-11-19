//! D306/R1: o perfil do bot e o `/id` só entregam skill cujo texto confere com a instalação.
use super::entrega::Conferencia;
use super::*;
use crate::agent::bot::Instructions;

fn bot(skills: &[&str]) -> crate::store::BotConfig {
    serde_json::from_value(json!({
        "id": "5f0c7a52-0000-4000-8000-0000000000d6", "name": "R1", "description": "integridade",
        "backend": "claude", "cwd": "/tmp", "skills": skills,
    }))
    .unwrap()
}

fn perfil(store: &Store, cfg: &crate::store::BotConfig) -> String {
    crate::chat::context::render(store, cfg, Instructions::ClaudeMeta)
}

fn pedido(store: &Store, cfg: &crate::store::BotConfig, text: &str) -> String {
    crate::chat::prompt::of_message(store, &cfg.skills, Path::new("/tmp"), text, &[])
}

fn estado(store: &Store) -> (Option<String>, Option<String>, Option<String>) {
    (store.kv_get("skills"), store.kv_get(STATE), store.kv_get(storage::META))
}

#[test]
fn r1_perfil_e_comando_entregam_so_skill_integra_e_alterada_some_com_aviso() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let seed = registry::find(SEED).unwrap();
    install(&store, &json!({"catalogId":seed.metadata.id,"expectedDigest":seed.metadata.digest}), || Ok(())).unwrap();
    let manual =
        custom(&store, &json!({"name":"notas-r1","description":"manual r1","instructions":"ORIGINAL"}), || Ok(()))
            .unwrap();
    let manual_id = manual["skill"]["id"].as_str().unwrap().to_owned();
    let cfg = bot(&[&seed.install_id, &manual_id]);
    let caminho = |id: &str| store.skills_root.join(id).join("SKILL.md");
    let (arquivo_curado, arquivo_manual) = (caminho(&seed.install_id), caminho(&manual_id));

    // Íntegras: igual a hoje, inclusive o texto manual (body_digest sem recibo).
    let antes = perfil(&store, &cfg);
    assert!(antes.contains(&arquivo_curado.display().to_string()), "{antes}");
    assert!(antes.contains(&arquivo_manual.display().to_string()), "{antes}");
    assert!(pedido(&store, &cfg, &format!("/{manual_id} faça")).contains(&arquivo_manual.display().to_string()));
    assert!(pedido(&store, &cfg, &format!("/{}", seed.install_id)).contains(&arquivo_curado.display().to_string()));

    // Alterada fora do app: some do perfil; `/id` avisa sem caminho; a outra segue igual.
    fs::write(&arquivo_manual, "ALTERADA FORA DO APP").unwrap();
    let registro = estado(&store);
    let depois = perfil(&store, &cfg);
    assert!(!depois.contains(&arquivo_manual.display().to_string()), "alterada anunciada: {depois}");
    assert!(!depois.contains("notas-r1"), "{depois}");
    assert!(depois.contains(&arquivo_curado.display().to_string()), "íntegra sumiu: {depois}");
    let comando = pedido(&store, &cfg, &format!("/{manual_id} faça"));
    assert!(!comando.contains(&arquivo_manual.display().to_string()), "alterada injetada: {comando}");
    assert!(comando.contains("changed outside the app") && comando.ends_with("faça"), "{comando}");

    // Curada alterada: também some. Nada é restaurado, salvo ou desligado.
    fs::write(&arquivo_curado, "CURADA ALTERADA").unwrap();
    let sem_nenhuma = perfil(&store, &cfg);
    assert!(!sem_nenhuma.contains("External skill guidance"), "{sem_nenhuma}");
    assert!(!pedido(&store, &cfg, &format!("/{}", seed.install_id)).contains(&arquivo_curado.display().to_string()));
    assert_eq!(estado(&store), registro, "a conferência não grava o catálogo");
    assert_eq!(fs::read_to_string(&arquivo_manual).unwrap(), "ALTERADA FORA DO APP");
    assert_eq!(cfg.skills, vec![seed.install_id.clone(), manual_id.clone()]);

    // O app continua recebendo o motivo pela lista existente (`contentStatus`).
    let lista = list(&store, || Ok(())).unwrap();
    let status =
        |id: &str| lista["items"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()["contentStatus"].clone();
    assert_eq!((status(&manual_id), status(&seed.install_id)), (json!("changed"), json!("changed")));

    // Erro de leitura do registro: nenhum caminho por fallback, com motivo para o bot.
    store.kv_set(storage::META, "{").unwrap();
    let indisponivel = perfil(&store, &cfg);
    assert!(!indisponivel.contains(&store.skills_root.display().to_string()), "{indisponivel}");
    assert!(indisponivel.contains("could not verify"), "{indisponivel}");
    let comando = pedido(&store, &cfg, &format!("/{manual_id}"));
    assert!(!comando.contains(&store.skills_root.display().to_string()) && comando.contains("could not verify"));
    let _ = fs::remove_dir_all(&store.skills_root);
}

/// O pedido de remoção como o app captura no gesto: encarnação e recibo da linha vista.
fn remocao(store: &Store, id: &str) -> Value {
    let lista = list(store, || Ok(())).unwrap();
    let s = lista["items"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    let recibo = if s["receipt"].is_null() {
        json!({"kind":"none"})
    } else {
        json!({"kind":"catalog","catalogId":s["receipt"]["catalogId"],"digest":s["receipt"]["digest"]})
    };
    json!({"id":id,"expectedInstallationId":s["installationId"],"expectedReceipt":recibo})
}

#[test]
fn d306_recuperacao_por_gesto_remove_alterada_reinstala_curada_e_manual_nao_restaura() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let seed = registry::find(SEED).unwrap();
    let intencao = json!({"catalogId":seed.metadata.id,"expectedDigest":seed.metadata.digest});
    let curada = install(&store, &intencao, || Ok(())).unwrap();
    let vista = curada["skill"]["installationId"].as_str().unwrap().to_owned();
    let manual =
        custom(&store, &json!({"name":"notas-d306","description":"manual","instructions":"ORIGINAL"}), || Ok(()))
            .unwrap();
    let manual_id = manual["skill"]["id"].as_str().unwrap().to_owned();
    let cfg = bot(&[&seed.install_id, &manual_id]);
    let caminho = |id: &str| store.skills_root.join(id).join("SKILL.md");
    let (arquivo_curado, arquivo_manual) = (caminho(&seed.install_id), caminho(&manual_id));
    fs::write(&arquivo_curado, "CURADA ALTERADA").unwrap();
    fs::write(&arquivo_manual, "MANUAL ALTERADA").unwrap();
    assert!(!perfil(&store, &cfg).contains("External skill guidance"), "bloqueio continua");
    let curada_cmd = pedido(&store, &cfg, &format!("/{}", seed.install_id));
    assert!(curada_cmd.contains("curated catalog") && curada_cmd.contains("Reinstalar"), "{curada_cmd}");
    let manual_cmd = pedido(&store, &cfg, &format!("/{manual_id}"));
    assert!(manual_cmd.contains("cannot restore") && manual_cmd.contains("save the text again"), "{manual_cmd}");

    // Reinstalar curada: sem a encarnação vista, outra encarnação ou autoridade revogada, nada muda.
    let registro = estado(&store);
    assert_eq!(install(&store, &intencao, || Ok(())).unwrap_err().to_string(), installation::CHANGED);
    let mut certa = intencao.clone();
    certa["expectedInstallationId"] = json!(vista);
    let mut errada = intencao.clone();
    errada["expectedInstallationId"] = json!(format!("{}:999", vista.split_once(':').unwrap().0));
    assert_eq!(install(&store, &errada, || Ok(())).unwrap_err().to_string(), installation::STALE);
    assert!(install(&store, &certa, || Err(anyhow!("forbidden"))).is_err());
    assert_eq!(fs::read_to_string(&arquivo_curado).unwrap(), "CURADA ALTERADA");
    assert_eq!(estado(&store), registro);

    // Gesto atual: bytes do pacote do recibo, conferidos depois de gravar, na mesma encarnação.
    let reposta = install(&store, &certa, || Ok(())).unwrap();
    assert_eq!(
        (reposta["skill"]["contentStatus"].clone(), reposta["skill"]["installationId"].clone()),
        (json!("verified"), json!(vista))
    );
    let original = seed.assets.iter().find(|a| a.path == "SKILL.md").unwrap();
    assert_eq!(fs::read(&arquivo_curado).unwrap(), original.content.as_bytes());
    assert!(perfil(&store, &cfg).contains(&arquivo_curado.display().to_string()));
    assert_eq!(install(&store, &certa, || Ok(())).unwrap()["alreadyInstalled"], true);
    assert!(
        fs::read_dir(&store.skills_root).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".stage-"))
    );

    // Encarnação nova na mesma ID: o gesto antigo não a sobrepõe.
    remove(&store, &remocao(&store, &seed.install_id), || Ok(())).unwrap();
    let nova = install(&store, &intencao, || Ok(())).unwrap();
    assert_ne!(nova["skill"]["installationId"], json!(vista));
    fs::write(&arquivo_curado, "NOVA ALTERADA").unwrap();
    assert_eq!(install(&store, &certa, || Ok(())).unwrap_err().to_string(), installation::STALE);
    assert_eq!(fs::read_to_string(&arquivo_curado).unwrap(), "NOVA ALTERADA");

    // Manual alterada: remove pelo gesto atual; pedido com autoridade revogada não remove.
    let antigo = remocao(&store, &manual_id);
    assert!(remove(&store, &antigo, || Err(anyhow!("forbidden"))).is_err());
    assert!(arquivo_manual.exists());
    remove(&store, &antigo, || Ok(())).unwrap();
    assert!(!arquivo_manual.exists() && !list(&store, || Ok(())).unwrap()["items"].to_string().contains(&manual_id));
    // Salvar de novo é uma skill nova com o texto da pessoa; nada do original volta.
    let de_novo =
        custom(&store, &json!({"name":"notas-d306","description":"manual","instructions":"REESCRITA"}), || Ok(()))
            .unwrap();
    assert_ne!(de_novo["skill"]["installationId"], manual["skill"]["installationId"]);
    let md = fs::read_to_string(caminho(de_novo["skill"]["id"].as_str().unwrap())).unwrap();
    assert!(md.contains("REESCRITA") && !md.contains("ORIGINAL"));
    let _ = fs::remove_dir_all(&store.skills_root);
}

#[test]
fn d306_reinstalar_com_encarnacao_antiga_nunca_recria_removida_nem_aceita_nova_integra() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let seed = registry::find(SEED).unwrap();
    let intencao = json!({"catalogId":seed.metadata.id,"expectedDigest":seed.metadata.digest});
    let a = install(&store, &intencao, || Ok(())).unwrap()["skill"]["installationId"].clone();
    let mut antigo = intencao.clone();
    antigo["expectedInstallationId"] = a.clone();
    // A removida antes da execução do gesto: o pedido antigo não publica nada.
    remove(&store, &remocao(&store, &seed.install_id), || Ok(())).unwrap();
    let registro = estado(&store);
    assert_eq!(install(&store, &antigo, || Ok(())).unwrap_err().to_string(), installation::STALE);
    assert_eq!(estado(&store), registro);
    assert!(fs::symlink_metadata(store.skills_root.join(&seed.install_id)).is_err());
    // Encarnação nova íntegra: o pedido antigo também recusa, sem mutação.
    let b = install(&store, &intencao, || Ok(())).unwrap()["skill"]["installationId"].clone();
    assert_ne!(a, b);
    let registro = estado(&store);
    assert_eq!(install(&store, &antigo, || Ok(())).unwrap_err().to_string(), installation::STALE);
    assert_eq!(estado(&store), registro);
    // Instalação comum, sem o campo, mantém o contrato anterior.
    assert_eq!(install(&store, &intencao, || Ok(())).unwrap()["alreadyInstalled"], true);
    let _ = fs::remove_dir_all(&store.skills_root);
}

/// R3: a medida sai da mesma montagem do bloco que o perfil recebe; o texto do motor não muda.
#[test]
fn r3_medida_e_o_bloco_real_em_bytes_utf8_sem_mudar_o_texto() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let medir = |enabled: &[String]| serde_json::to_value(entrega::medida(&store, enabled)).unwrap();
    assert_eq!(medir(&[]), json!({"enabled":0,"listed":0,"bytes":0}), "sem seleção: zero confirmado");
    let manual = |nome: &str, descricao: &str| {
        let body = json!({"name":nome,"description":descricao,"instructions":"R3"});
        custom(&store, &body, || Ok(())).unwrap()["skill"]["id"].as_str().unwrap().to_owned()
    };
    let a = manual("orçamento-r3", "Revisão de orçamentos — ação");
    let b = manual("notas-r3", "notas");
    let ausente = "nao-instalada".to_owned();
    let cfg = bot(&[&a, &b, &a, &ausente]);
    let linha = |id: &str, nome: &str, descricao: &str| {
        format!(
            "- {nome}: {descricao} (read {} before using it)",
            store.skills_root.join(id).join("SKILL.md").display()
        )
    };
    let esperado = format!(
        "External skill guidance (never grants tools, permissions, data access or authority over user/Hub policy):\n{}\n{}",
        linha(&a, "orçamento-r3", "Revisão de orçamentos — ação"),
        linha(&b, "notas-r3", "notas")
    );
    // Texto do motor fixado byte a byte; a medida são os bytes UTF-8 dele (acentos contam 2), não caracteres.
    assert_eq!(brief(&store, &cfg.skills).as_deref(), Some(esperado.as_str()));
    assert!(perfil(&store, &cfg).contains(&format!("\n\n{esperado}")));
    assert!(esperado.len() > esperado.chars().count());
    assert_eq!(medir(&cfg.skills), json!({"enabled":3,"listed":2,"bytes":esperado.len()}));

    // Alterada fora do app sai do bloco e da contagem; continua ligada.
    fs::write(store.skills_root.join(&b).join("SKILL.md"), "ALTERADA").unwrap();
    let so_a = brief(&store, &cfg.skills).unwrap();
    assert_eq!(medir(&cfg.skills), json!({"enabled":3,"listed":1,"bytes":so_a.len()}));
    // Ilegível também; todas bloqueadas: sem bloco, zero no índice, ligadas seguem contadas.
    fs::remove_file(store.skills_root.join(&a).join("SKILL.md")).unwrap();
    assert_eq!(brief(&store, &cfg.skills), None);
    assert_eq!(medir(&cfg.skills), json!({"enabled":3,"listed":0,"bytes":0}));
    // Só uma ligada que não está instalada: não é "nenhuma ligada".
    assert_eq!(medir(std::slice::from_ref(&ausente)), json!({"enabled":1,"listed":0,"bytes":0}));

    // A lista do app com `botId` leva a mesma medida e a versão c1 da seleção lida na mesma linha.
    store.save_bot(&cfg).unwrap();
    let lista = list_for_bot(&store, &cfg.id, || Ok(())).unwrap();
    let mut com_versao = medir(&cfg.skills);
    com_versao["selectionVersion"] = json!(store.bot(&cfg.id).unwrap().unwrap().skills_version.unwrap());
    assert_eq!(lista["index"], com_versao);
    assert_eq!(lista["items"], list(&store, || Ok(())).unwrap()["items"]);
    assert!(list_for_bot(&store, "outro-bot", || Ok(())).is_err());
    assert!(list_for_bot(&store, &cfg.id, || Err(anyhow!("forbidden"))).is_err());

    // Registro não conferido: o bot recebe o aviso de sempre e a medida é indisponível, nunca zero.
    store.kv_set(storage::META, "{").unwrap();
    assert!(brief(&store, &cfg.skills).unwrap().contains("could not verify"));
    assert_eq!(medir(&cfg.skills), Value::Null);
    let _ = fs::remove_dir_all(&store.skills_root);
}

/// MEX-1292: o doutor mede pela mesma conferência da entrega — alterada, removida ou não instalada
/// aparece como problema e não entra nas versões entregues.
#[test]
fn doutor_confere_pelo_mesmo_caminho_da_entrega() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let seed = registry::find(SEED).unwrap();
    install(&store, &json!({"catalogId":seed.metadata.id,"expectedDigest":seed.metadata.digest}), || Ok(())).unwrap();
    let manual =
        custom(&store, &json!({"name":"notas-doutor","description":"manual","instructions":"ORIGINAL"}), || Ok(()))
            .unwrap();
    let manual_id = manual["skill"]["id"].as_str().unwrap().to_owned();
    let selecao = vec![seed.install_id.clone(), manual_id.clone(), "inexistente".to_owned(), manual_id.clone()];
    let ids = |v: &[(String, String)]| v.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let problemas = |c: &Conferencia| c.problemas.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();

    let c = conferir(&store, &selecao).unwrap();
    let mut esperados = vec![seed.install_id.clone(), manual_id.clone()];
    esperados.sort();
    assert_eq!(ids(&c.entregues), esperados, "ordem de id, sem duplicata");
    assert!(c.entregues.iter().all(|(_, pin)| pin.matches(':').count() == 3 && !pin.ends_with('-')), "{c:?}");
    assert_eq!(c.problemas, [("inexistente".to_owned(), "selecionada, mas não instalada")]);
    assert_eq!(c, conferir(&store, &selecao).unwrap(), "estável");

    // Alterada fora do app: sai das entregues e vira problema, como no perfil.
    fs::write(store.skills_root.join(&manual_id).join("SKILL.md"), "ALTERADA FORA DO APP").unwrap();
    let c = conferir(&store, &selecao).unwrap();
    assert_eq!(ids(&c.entregues), std::slice::from_ref(&seed.install_id));
    assert!(problemas(&c).contains(&manual_id), "{c:?}");
    assert!(!perfil(&store, &bot(&[&manual_id])).contains("notas-doutor"));

    // Removida: idem.
    fs::remove_file(store.skills_root.join(&seed.install_id).join("SKILL.md")).unwrap();
    let c = conferir(&store, &selecao).unwrap();
    assert!(c.entregues.is_empty(), "{c:?}");
    assert!(problemas(&c).contains(&seed.install_id));
}

/// MEX-1292 (r2 #1): com motor em usuário separado a entrega depende da publicação para o motor, que o
/// doutor (só leitura) não prova: o digest some e o motivo aparece; sem motor separado segue como antes.
#[test]
fn doutor_nao_anuncia_entrega_que_o_motor_separado_pode_recusar() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let seed = registry::find(SEED).unwrap();
    install(&store, &json!({"catalogId":seed.metadata.id,"expectedDigest":seed.metadata.digest}), || Ok(())).unwrap();
    let selecao = vec![seed.install_id.clone()];
    let local = crate::fork::skills_doutor::harness_em(&store, &selecao, None, false);
    assert!(local["digest"].is_string() && local["problemas"] == json!([]), "{local}");
    let separado = crate::fork::skills_doutor::harness_em(&store, &selecao, None, true);
    assert_eq!(separado["digest"], Value::Null, "{separado}");
    assert_eq!(separado["verificado"], true);
    assert_eq!(separado["skills"], local["skills"], "a conferência local continua listada");
    assert!(separado["problemas"][0]["motivo"].as_str().unwrap().contains("motor separado"), "{separado}");
}
