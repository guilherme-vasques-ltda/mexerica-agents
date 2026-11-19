//! MX-264: prévia e lote atômico de skill/conector, prova das fronteiras do produtor.
#![allow(clippy::unwrap_used)]
use super::{CONFLICT, INDISPONIVEL, NOVOS_BOTS_RECUSADO, Tipo, UNSUPPORTED, aplicar_em_transacao};
use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope, Store, now_ms},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Ambiente {
    hub: Arc<Hub>,
    pasta: PathBuf,
}

impl Ambiente {
    fn novo() -> Self {
        let pasta = std::env::temp_dir().join(format!("donna-lote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&pasta).unwrap();
        // `:memory:` tem chave de cofre efêmera: o teste não depende de Secret
        // Service/D-Bus, que não existe no sandbox.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let hub = Hub::new(store, "hub-lote".into(), Identity::load_or_create(&pasta).unwrap(), "token".into(), 0);
        hub.start().unwrap();
        Self { hub, pasta }
    }

    fn bot(&self, id: &str, extra: &Value) {
        let mut body = json!({"id": id, "name": id, "backend": "codex", "cwd": self.pasta});
        if let (Some(dst), Some(src)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        let cfg: BotConfig = serde_json::from_value(body).unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
    }

    fn lista(&self, bot: &str, campo: &str) -> Vec<String> {
        let row = self.hub.store.bot(bot).unwrap().unwrap();
        let value = serde_json::to_value(&row.config).unwrap();
        value[campo].as_array().unwrap().iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
    }

    /// Space de equipe com `principal` opcional; bots do Space entram por `spaceId`.
    fn space(&self, id: &str, principal: Option<&str>) {
        let raw = self.hub.store.kv_read("donna.spaces.v1").unwrap().unwrap();
        let mut snap: Value = serde_json::from_str(&raw).unwrap();
        let mut space = snap["spaces"][0].clone();
        space["id"] = json!(id);
        space["kind"] = json!("team");
        space["name"] = json!("Time");
        space["principalBotId"] = principal.map_or(Value::Null, |p| json!(p));
        snap["spaces"].as_array_mut().unwrap().push(space);
        self.hub.store.kv_set("donna.spaces.v1", &serde_json::to_string(&snap).unwrap()).unwrap();
    }

    fn apagar(&self, id: &str) {
        self.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id=?1", [id]).unwrap();
    }

    fn instalar_skill(&self, nome: &str) -> String {
        let r = crate::fork::skill_catalog::custom(
            &self.hub.store,
            &json!({"name": nome, "description": "Prova MX-264", "instructions": "Texto passivo."}),
            || Ok(()),
        )
        .unwrap();
        r["skill"]["id"].as_str().unwrap().to_owned()
    }

    fn instalar_conector(&self, id: &str) {
        let connector: crate::market::Connector =
            serde_json::from_value(json!({"id": id, "name": id, "command": "unused"})).unwrap();
        crate::market::save_connectors(&self.hub.store, &[connector]).unwrap();
    }

    /// Conta de serviço do Marketplace (tem `servico`), no Space dado.
    fn instalar_conta_de_servico(&self, id: &str, space: &str) {
        let conta: crate::market::Connector = serde_json::from_value(
            json!({"id": id, "name": id, "command": "unused", "servico": "linear", "space": space}),
        )
        .unwrap();
        // `save_connectors` troca a lista inteira: acrescenta às já instaladas.
        let mut todas = crate::market::connectors(&self.hub.store).unwrap();
        todas.push(conta);
        crate::market::save_connectors(&self.hub.store, &todas).unwrap();
    }

    async fn previa(&self, tipo: &str, id: &str) -> anyhow::Result<Value> {
        crate::api::dispatch(
            &self.hub,
            &Caller::Local,
            "extensaoEmTodosPrevia",
            json!({"protocol": 1, "tipo": tipo, "id": id}),
        )
        .await
    }

    async fn aplicar(
        &self,
        caller: &Caller,
        tipo: &str,
        id: &str,
        bot_ids: &[&str],
        versao: &str,
    ) -> anyhow::Result<Value> {
        crate::api::dispatch(
            &self.hub,
            caller,
            "extensaoEmTodosAplicar",
            json!({"protocol": 1, "tipo": tipo, "id": id, "botIds": bot_ids, "versao": versao}),
        )
        .await
    }

    fn recebedores(previa: &Value) -> Vec<String> {
        previa["recebedores"].as_array().unwrap().iter().map(|b| b["botId"].as_str().unwrap().to_owned()).collect()
    }

    async fn definir(&self, caller: &Caller, tipo: &str, id: &str, ligado: bool) -> anyhow::Result<Value> {
        crate::api::dispatch(
            &self.hub,
            caller,
            "extensaoNovosBotsDefinir",
            json!({"protocol": 1, "tipo": tipo, "id": id, "ligado": ligado}),
        )
        .await
    }

    /// `createBot` do dono; `extra` mescla campos (omitir `skills` aciona o padrão).
    async fn criar(&self, nome: &str, extra: &Value) -> String {
        let mut body = json!({"name": nome, "backend": "codex", "cwd": self.pasta,
            "connectors": [], "logins": []});
        if let (Some(dst), Some(src)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        let r = crate::api::dispatch(&self.hub, &Caller::Local, "createBot", body).await.unwrap();
        r["bot"]["id"].as_str().unwrap().to_owned()
    }

    fn motivo(previa: &Value, bot: &str) -> Option<String> {
        previa["excluidos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["botId"] == bot)
            .map(|e| e["motivo"].as_str().unwrap().to_owned())
    }
}

impl Drop for Ambiente {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pasta);
    }
}

fn codigo(r: anyhow::Result<Value>) -> String {
    format!("{:#}", r.unwrap_err())
}

#[tokio::test]
async fn previa_de_skill_lista_recebedores_e_motivos_e_aplicar_so_neles() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Lote");
    a.bot("alvo", &json!({}));
    a.bot("ja-tem", &json!({"skills": [skill]}));
    a.bot("grupo", &json!({"kind": "group"}));
    a.bot("oculto", &json!({"hidden": true}));
    a.bot("apagado", &json!({}));
    a.apagar("apagado");
    a.space("time-1", None);
    a.bot("fora-do-pessoal", &json!({"spaceId": "time-1"}));

    let previa = a.previa("skill", &skill).await.unwrap();
    assert_eq!(previa["tipo"], "skill");
    assert_eq!(previa["id"], skill);
    assert_eq!(previa["versao"].as_str().unwrap().len(), 64);
    // O Hub cria a principal do Pessoal na abertura: ela também é recebedora.
    let recebidos = Ambiente::recebedores(&previa);
    assert!(recebidos.iter().any(|b| b == "alvo"));
    for ex in ["ja-tem", "grupo", "oculto", "apagado", "fora-do-pessoal"] {
        assert!(!recebidos.iter().any(|b| b == ex), "{ex} não devia receber");
    }
    assert_eq!(Ambiente::motivo(&previa, "ja-tem").as_deref(), Some("ja_tem"));
    assert_eq!(Ambiente::motivo(&previa, "grupo").as_deref(), Some("grupo"));
    assert_eq!(Ambiente::motivo(&previa, "oculto").as_deref(), Some("oculto"));
    assert_eq!(Ambiente::motivo(&previa, "apagado").as_deref(), Some("apagado"));
    assert_eq!(Ambiente::motivo(&previa, "fora-do-pessoal").as_deref(), Some("fora_do_pessoal"));

    let versao = previa["versao"].as_str().unwrap().to_owned();
    let ids: Vec<&str> = recebidos.iter().map(String::as_str).collect();
    let r = a.aplicar(&Caller::Local, "skill", &skill, &ids, &versao).await.unwrap();
    assert_eq!(r["aplicados"].as_array().unwrap().len(), recebidos.len());
    assert_eq!(a.lista("alvo", "skills"), [skill.as_str()]);
    for bot in ["ja-tem", "grupo", "oculto", "apagado", "fora-do-pessoal"] {
        let lista = a.lista(bot, "skills");
        assert!(lista.iter().all(|s| s != &skill) || bot == "ja-tem", "{bot} ganhou a skill");
    }
    // A versão das células rodou pelo trigger: a prévia antiga não reaplica.
    assert!(codigo(a.aplicar(&Caller::Local, "skill", &skill, &["alvo"], &versao).await).contains(CONFLICT));
}

#[tokio::test]
async fn aplicar_conector_liga_em_todos_e_so_neles_e_respeita_principal() {
    let a = Ambiente::novo();
    a.instalar_conector("mcp-lote");
    a.bot("alvo", &json!({}));
    a.bot("outro", &json!({}));
    a.bot("ja-tem", &json!({"connectors": ["mcp-lote"]}));
    a.space("time-1", Some("berga-space"));
    a.bot("berga-space", &json!({"spaceId": "time-1", "name": "Berga"}));
    a.bot("membro", &json!({"spaceId": "time-1"}));

    let previa = a.previa("conector", "mcp-lote").await.unwrap();
    let recebidos = Ambiente::recebedores(&previa);
    for esperado in ["alvo", "outro"] {
        assert!(recebidos.iter().any(|b| b == esperado), "{esperado} devia receber");
    }
    // Marketplace 1a: o conector é do Space pessoal; o bot de outro Space fica de fora do lote.
    assert!(!recebidos.iter().any(|b| b == "ja-tem" || b == "berga-space" || b == "membro"));
    assert_eq!(Ambiente::motivo(&previa, "membro").as_deref(), Some("space_diferente"));
    assert_eq!(Ambiente::motivo(&previa, "ja-tem").as_deref(), Some("ja_tem"));
    assert_eq!(Ambiente::motivo(&previa, "berga-space").as_deref(), Some("principal_de_space"));

    let versao = previa["versao"].as_str().unwrap().to_owned();
    let lista = recebidos.clone();
    let ids: Vec<&str> = lista.iter().map(String::as_str).collect();
    let r = a.aplicar(&Caller::Local, "conector", "mcp-lote", &ids, &versao).await.unwrap();
    assert_eq!(r["aplicados"].as_array().unwrap().len(), recebidos.len());
    for bot in ["alvo", "outro"] {
        assert_eq!(a.lista(bot, "connectors"), ["mcp-lote"], "{bot}");
    }
    assert_eq!(a.lista("membro", "connectors"), Vec::<String>::new());
    assert_eq!(a.lista("berga-space", "connectors"), Vec::<String>::new());
    assert_eq!(a.lista("ja-tem", "connectors"), ["mcp-lote"]);
}

#[tokio::test]
async fn lote_de_conta_de_servico_nao_liga_no_berga_do_pessoal() {
    let a = Ambiente::novo();
    // O Hub cria o Berga do Pessoal na abertura; o id vem de `team.settings`.
    let berga = a.hub.store.team_settings().unwrap().primary_bot_id.unwrap();
    let berga = berga.as_str();
    a.bot("alvo", &json!({}));
    a.instalar_conector("mcp-proprio");
    a.instalar_conta_de_servico("linear", "00000000-0000-4000-8000-000000000001");

    // Conta de serviço: o Berga fica de fora, com o motivo estável, na prévia.
    let previa = a.previa("conector", "linear").await.unwrap();
    let recebidos = Ambiente::recebedores(&previa);
    assert!(recebidos.iter().any(|b| b == "alvo"));
    assert!(!recebidos.iter().any(|b| b == berga));
    assert_eq!(Ambiente::motivo(&previa, berga).as_deref(), Some("principal_sem_contas"));
    let versao = previa["versao"].as_str().unwrap().to_owned();
    // Pedir o Berga à mão recusa com o código estável e não grava nada (nem no outro bot).
    let e = codigo(a.aplicar(&Caller::Local, "conector", "linear", &["alvo", berga], &versao).await);
    assert!(e.starts_with("principal_sem_contas"), "{e}");
    assert_eq!(a.lista("alvo", "connectors"), Vec::<String>::new());
    assert_eq!(a.lista(berga, "connectors"), Vec::<String>::new());
    // O lote só com quem a prévia confirmou funciona.
    a.aplicar(&Caller::Local, "conector", "linear", &["alvo"], &versao).await.unwrap();
    assert_eq!(a.lista("alvo", "connectors"), ["linear"]);
    assert_eq!(a.lista(berga, "connectors"), Vec::<String>::new());
    // MCP próprio do dono (sem ficha) segue como antes: o Berga do Pessoal ainda é recebedor.
    let mcp = a.previa("conector", "mcp-proprio").await.unwrap();
    assert!(Ambiente::recebedores(&mcp).iter().any(|b| b == berga));
}

#[tokio::test]
async fn lote_de_conta_de_servico_nao_liga_no_principal_de_space() {
    let a = Ambiente::novo();
    a.space("time-1", Some("berga-space"));
    a.bot("berga-space", &json!({"spaceId": "time-1", "name": "Berga"}));
    a.bot("membro", &json!({"spaceId": "time-1"}));
    a.instalar_conta_de_servico("linear-time", "time-1");

    let previa = a.previa("conector", "linear-time").await.unwrap();
    assert_eq!(Ambiente::recebedores(&previa), ["membro"]);
    assert_eq!(Ambiente::motivo(&previa, "berga-space").as_deref(), Some("principal_de_space"));
    let versao = previa["versao"].as_str().unwrap().to_owned();
    let e = codigo(a.aplicar(&Caller::Local, "conector", "linear-time", &["berga-space"], &versao).await);
    assert!(e.starts_with("principal_sem_contas"), "{e}");
    assert_eq!(a.lista("berga-space", "connectors"), Vec::<String>::new());
    a.aplicar(&Caller::Local, "conector", "linear-time", &["membro"], &versao).await.unwrap();
    assert_eq!(a.lista("membro", "connectors"), ["linear-time"]);
    assert_eq!(a.lista("berga-space", "connectors"), Vec::<String>::new());
}

#[tokio::test]
async fn versao_desatualizada_ou_lista_trocada_recusam_tudo() {
    let a = Ambiente::novo();
    a.instalar_conector("mcp-lote");
    a.bot("alvo", &json!({}));
    a.bot("outro", &json!({}));
    let previa = a.previa("conector", "mcp-lote").await.unwrap();
    let versao = previa["versao"].as_str().unwrap().to_owned();

    // Um bot novo muda o estado: a versão velha não aplica nada.
    a.bot("novo", &json!({}));
    let lista = Ambiente::recebedores(&previa);
    let ids: Vec<&str> = lista.iter().map(String::as_str).collect();
    assert!(codigo(a.aplicar(&Caller::Local, "conector", "mcp-lote", &ids, &versao).await).contains(CONFLICT));
    for bot in ["alvo", "outro", "novo"] {
        assert_eq!(a.lista(bot, "connectors"), Vec::<String>::new(), "{bot} gravou parcial");
    }

    // botId fora da lista confirmada também é conflito, mesmo com versão fresca.
    let previa2 = a.previa("conector", "mcp-lote").await.unwrap();
    let v2 = previa2["versao"].as_str().unwrap().to_owned();
    assert!(
        codigo(a.aplicar(&Caller::Local, "conector", "mcp-lote", &["alvo", "fantasma"], &v2).await).contains(CONFLICT)
    );
    assert_eq!(a.lista("alvo", "connectors"), Vec::<String>::new());

    // Extensão desinstalada entre prévia e aplicação: falha fechada, sem escrita.
    let previa3 = a.previa("conector", "mcp-lote").await.unwrap();
    let v3 = previa3["versao"].as_str().unwrap().to_owned();
    crate::market::save_connectors(&a.hub.store, &[]).unwrap();
    // Extensão que sumiu depois da prévia é conflito de estado: a tela refaz a
    // prévia. Nada é escrito.
    assert!(codigo(a.aplicar(&Caller::Local, "conector", "mcp-lote", &["alvo"], &v3).await).contains(CONFLICT));
    assert_eq!(a.lista("alvo", "connectors"), Vec::<String>::new());
}

#[tokio::test]
async fn extensao_nao_instalada_e_aparelho_sem_control_recusam() {
    let a = Ambiente::novo();
    a.bot("alvo", &json!({}));
    assert!(codigo(a.previa("skill", "sumiu").await).contains(INDISPONIVEL));
    assert!(codigo(a.previa("conector", "sumiu").await).contains(INDISPONIVEL));
    let versao = "0".repeat(64);
    assert!(codigo(a.aplicar(&Caller::Local, "skill", "sumiu", &["alvo"], &versao).await).contains(CONFLICT));

    // Aparelho sem Control não pré-visualiza nem aplica (permit + captura).
    let sem_controle = Caller::Device { key: "aparelho-sem-controle".into(), scopes: vec![], autoridade: None };
    assert!(
        codigo(
            crate::api::dispatch(
                &a.hub,
                &sem_controle,
                "extensaoEmTodosPrevia",
                json!({"protocol": 1, "tipo": "skill", "id": "x"}),
            )
            .await
        )
        .contains("This can only be done at the computer itself")
    );
    assert!(
        codigo(
            crate::api::dispatch(
                &a.hub,
                &sem_controle,
                "extensaoEmTodosAplicar",
                json!({"protocol": 1, "tipo": "skill", "id": "x", "botIds": ["alvo"], "versao": versao}),
            )
            .await
        )
        .contains("This can only be done at the computer itself")
    );
    // Ponte/Pairing/Bootstrap também não são autoridade do dono.
    for caller in
        [Caller::Pairing { key: "par".into() }, Caller::Bootstrap { key: "boot".into(), bootstrap_id: "b".into() }]
    {
        assert_ne!(
            codigo(
                crate::api::dispatch(
                    &a.hub,
                    &caller,
                    "extensaoEmTodosAplicar",
                    json!({"protocol": 1, "tipo": "skill", "id": "x", "botIds": ["alvo"], "versao": versao}),
                )
                .await
            ),
            ""
        );
    }
}

#[tokio::test]
async fn autoridade_revogada_ou_versao_malformada_nao_gravam() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Lote");
    a.bot("alvo", &json!({}));

    // Versão malformada/curta é entrada inválida, não conflito de estado.
    assert!(codigo(a.aplicar(&Caller::Local, "skill", &skill, &["alvo"], "abc").await).contains(UNSUPPORTED));
    assert!(codigo(a.aplicar(&Caller::Local, "skill", &skill, &[], &"0".repeat(64)).await).contains(UNSUPPORTED));

    // Aparelho Control revogado depois da autenticação: a captura original falha fechado.
    let aparelho = Device {
        key: "aparelho-lote".into(),
        name: "Teste".into(),
        platform: "teste".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    };
    a.hub.store.put_device(&aparelho).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&a.hub, &aparelho.key).unwrap();
    let previa = a.previa("skill", &skill).await.unwrap();
    let versao = previa["versao"].as_str().unwrap().to_owned();
    a.hub.store.remove_device(&aparelho.key).unwrap();
    assert!(a.aplicar(&caller, "skill", &skill, &["alvo"], &versao).await.is_err());
    assert_eq!(a.lista("alvo", "skills"), Vec::<String>::new());
}

#[tokio::test]
async fn bot_novo_nasce_sem_extensao_e_pode_receber_no_lote() {
    // D91: instalar não liga nada; o lote só alcança quem a prévia confirmou.
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Lote");
    a.instalar_conector("mcp-lote");
    a.bot("antes", &json!({}));
    let skill_previa = a.previa("skill", &skill).await.unwrap();
    a.bot("depois", &json!({}));
    assert_eq!(a.lista("depois", "skills"), Vec::<String>::new());
    assert_eq!(a.lista("depois", "connectors"), Vec::<String>::new());
    // A prévia anterior não cobre o bot novo: conflito, nada gravado.
    let v = skill_previa["versao"].as_str().unwrap().to_owned();
    assert!(codigo(a.aplicar(&Caller::Local, "skill", &skill, &["antes"], &v).await).contains(CONFLICT));
    assert_eq!(a.lista("antes", "skills"), Vec::<String>::new());
    // Prévia nova inclui o bot criado depois e aplica nele também.
    let nova = a.previa("skill", &skill).await.unwrap();
    let lista = Ambiente::recebedores(&nova);
    assert!(lista.iter().any(|b| b == "depois"));
    let ids: Vec<&str> = lista.iter().map(String::as_str).collect();
    a.aplicar(&Caller::Local, "skill", &skill, &ids, nova["versao"].as_str().unwrap()).await.unwrap();
    assert_eq!(a.lista("depois", "skills"), [skill.as_str()]);
}

#[tokio::test]
async fn padrao_de_skill_para_bots_novos_liga_desliga_e_explicit_ganha() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Padrao");
    // O padrão é do dono e a tela lê o estado na lista de skills.
    let r = a.definir(&Caller::Local, "skill", &skill, true).await.unwrap();
    assert_eq!(r["novosBots"], true);
    let lista = crate::api::dispatch(&a.hub, &Caller::Local, "skills", json!({})).await.unwrap();
    let item = lista["items"].as_array().unwrap().iter().find(|i| i["id"] == skill).expect("skill instalada na lista");
    assert_eq!(item["novosBots"], true);
    // Bot criado sem `skills` recebe o padrão instalado e verificado.
    let com = a.criar("ComPadrao", &json!({})).await;
    assert_eq!(a.lista(&com, "skills"), [skill.as_str()]);
    // `skills: []` explícito é escolha do dono: o padrão não entra.
    let explicito = a.criar("Explicito", &json!({"skills": []})).await;
    assert_eq!(a.lista(&explicito, "skills"), Vec::<String>::new());
    // Desligar o padrão: o próximo bot nasce sem a skill.
    a.definir(&Caller::Local, "skill", &skill, false).await.unwrap();
    let sem = a.criar("SemPadrao", &json!({})).await;
    assert_eq!(a.lista(&sem, "skills"), Vec::<String>::new());
}

#[tokio::test]
async fn padrao_de_skill_desinstalada_e_ignorado_fail_closed() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Viva");
    // Estado gravado direto no kv: uma skill real e ids que não existem mais.
    a.hub.store.kv_set(super::KV_NOVOS_BOTS, &json!({"skills": [skill, "sumiu", "nunca-houve"]}).to_string()).unwrap();
    let bot = a.criar("Sobrevive", &json!({})).await;
    // Só a instalada entra; as fantasmas são ignoradas sem erro.
    assert_eq!(a.lista(&bot, "skills"), [skill.as_str()]);
    // Nem só fantasmas: bot nasce sem nada.
    a.hub.store.kv_set(super::KV_NOVOS_BOTS, r#"{"skills":["sumiu"]}"#).unwrap();
    let outro = a.criar("SoFantasma", &json!({})).await;
    assert_eq!(a.lista(&outro, "skills"), Vec::<String>::new());
}

#[tokio::test]
async fn conector_nao_vira_padrao_e_aparelho_sem_control_recusa() {
    let a = Ambiente::novo();
    a.instalar_conector("mcp-x");
    let skill = a.instalar_skill("Ok");
    // D91: padrão para conector é recusado com erro tipado próprio.
    assert!(codigo(a.definir(&Caller::Local, "conector", "mcp-x", true).await).contains(NOVOS_BOTS_RECUSADO));
    // Skill que não está instalada não vira padrão.
    assert!(codigo(a.definir(&Caller::Local, "skill", "sumiu", true).await).contains(INDISPONIVEL));
    // Aparelho sem Control, Ponte, Pairing e Bootstrap não definem padrão.
    let sem_controle = Caller::Device { key: "aparelho-sem-controle".into(), scopes: vec![], autoridade: None };
    assert!(
        codigo(a.definir(&sem_controle, "skill", &skill, true).await)
            .contains("This can only be done at the computer itself")
    );
    for caller in
        [Caller::Pairing { key: "par".into() }, Caller::Bootstrap { key: "boot".into(), bootstrap_id: "b".into() }]
    {
        assert!(a.definir(&caller, "skill", &skill, true).await.is_err());
    }
    // Nada foi gravado: bot novo segue sem a skill.
    let bot = a.criar("Intocado", &json!({})).await;
    assert_eq!(a.lista(&bot, "skills"), Vec::<String>::new());
}

/// P2-1: o ramo de conectores espera `memory_boundary`/`emit_lock` ANTES de abrir
/// a transação do DB — a mesma ordem dos caminhos de memória. Com a ordem
/// invertida, a Tx segurava o DB enquanto a tarefa esperava o boundary.
#[tokio::test]
async fn lote_de_conector_espera_memory_boundary_sem_segurar_o_db() {
    let a = Ambiente::novo();
    a.instalar_conector("mcp-borda");
    a.bot("alvo", &json!({}));
    let previa = a.previa("conector", "mcp-borda").await.unwrap();
    let versao = previa["versao"].as_str().unwrap().to_owned();

    let boundary = a.hub.store.memory_boundary.locked();
    let hub = a.hub.clone();
    let aplicando = tokio::task::spawn_blocking(move || {
        let owner = crate::fork::autoridade::Autoridade::capturar(&hub, &Caller::Local).unwrap();
        aplicar_em_transacao(&hub, &owner, Tipo::Conector, "mcp-borda", &["alvo".to_owned()], &versao)
    });
    // Com o boundary preso, o DB deve seguir majoritariamente livre: o lote
    // espera no boundary, não dentro da transação. A sondagem é bloqueante de
    // propósito — o guard do boundary não atravessa nenhum `.await`.
    let mut livres = 0usize;
    for _ in 0..200 {
        if a.hub.store.db.try_lock().is_ok() {
            livres += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(boundary);
    let aplicados = aplicando.await.unwrap().unwrap();
    assert_eq!(aplicados, ["alvo"]);
    assert!(livres > 100, "lote segurou o DB esperando memory_boundary ({livres}/200 amostras livres)");
}

/// P2-2: a versão cobre os registros privados do cofre (digest, sem valores).
/// Trocar um valor de env mantém a projeção pública idêntica — só o digest
/// denuncia a mudança e a aplicação confirmada é recusada.
#[tokio::test]
async fn valor_secreto_do_conector_muda_a_versao_e_recusa_apply() {
    let a = Ambiente::novo();
    let conector = |valor: &str| {
        serde_json::from_value::<crate::market::Connector>(json!({
            "id": "mcp-secreto", "name": "mcp-secreto", "command": "unused",
            "env": {"TOKEN": valor},
        }))
        .unwrap()
    };
    crate::market::save_connectors(&a.hub.store, &[conector("a")]).unwrap();
    a.bot("alvo", &json!({}));
    let previa = a.previa("conector", "mcp-secreto").await.unwrap();
    let versao = previa["versao"].as_str().unwrap().to_owned();
    // Mesmas chaves de env → projeção pública idêntica; só o valor mudou.
    crate::market::save_connectors(&a.hub.store, &[conector("b")]).unwrap();
    let depois = a.previa("conector", "mcp-secreto").await.unwrap();
    assert_ne!(depois["versao"].as_str().unwrap(), versao, "digest do cofre não entrou na versão");
    assert!(codigo(a.aplicar(&Caller::Local, "conector", "mcp-secreto", &["alvo"], &versao).await).contains(CONFLICT));
    assert_eq!(a.lista("alvo", "connectors"), Vec::<String>::new());
}

/// P2-4: estado inválido de seleção é recusado como conflito — a mesma régua da
/// seleção de uma célula, nunca descarte silencioso de entradas.
#[tokio::test]
async fn selecao_invalida_e_recusada_como_conflito() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Valida");
    a.bot("alvo", &json!({}));
    // Versão de seleção ausente: a célula única recusaria; o lote também.
    a.hub.store.db.locked().execute("UPDATE bots SET donna_skills_version=NULL WHERE id='alvo'", []).unwrap();
    assert!(codigo(a.previa("skill", &skill).await).contains(CONFLICT));
    // Lista malformada (string no lugar de array): o gatilho rotaciona a versão
    // e o parser canônico recusa — nunca reescreve a lista descartando entradas.
    a.hub
        .store
        .db
        .locked()
        .execute("UPDATE bots SET config=json_set(config,'$.skills','\"nao-array\"') WHERE id='alvo'", [])
        .unwrap();
    assert!(codigo(a.previa("skill", &skill).await).contains(CONFLICT));
}

/// P2-3: a prévia carrega a autoridade original — revogação durante a espera da
/// fila falha fechado na reconferência, sem leitura privilegiada.
#[tokio::test]
async fn previa_reconfere_autoridade_depois_da_espera() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Revogavel");
    let device = Device {
        key: "fone-previa".into(),
        name: "Fone".into(),
        platform: "ios".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    };
    a.hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&a.hub, &device.key).unwrap();
    let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let hub = a.hub.clone();
        let fired = fired.clone();
        let key = device.key.clone();
        *a.hub.store.before_owner_write.lock().unwrap() = Some(Box::new(move || {
            fired.store(true, std::sync::atomic::Ordering::SeqCst);
            hub.revoke_device(&key).unwrap();
        }));
    }
    let r = crate::api::dispatch(
        &a.hub,
        &caller,
        "extensaoEmTodosPrevia",
        json!({"protocol": 1, "tipo": "skill", "id": skill.clone()}),
    )
    .await;
    assert!(r.is_err(), "prévia leu depois da revogação");
    assert!(fired.load(std::sync::atomic::Ordering::SeqCst), "gancho antes_da_gravar não disparou");
}

/// Rodada 2/3/4: a origem explícita atravessa o `spawn_blocking` e protege as
/// gravações internas dos resolvedores. Ordem garantida por barreira, sem
/// sleeps: a trava é confirmada adquirida, a sonda é POR CHAMADA (tipo+id, não
/// global), captura → reconferência → resolvedor dentro → revogação →
/// liberação → a escrita do efeito é recusada na própria transação.
#[cfg(test)]
async fn confirmar_resolvedor(espera: &super::sonda::Espera) {
    for _ in 0..2000 {
        if espera.rx.try_recv().is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("resolvedor não entrou no prazo");
}

fn device_control(chave: &str) -> Device {
    Device {
        key: chave.into(),
        name: "Fone".into(),
        platform: "ios".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    }
}

#[tokio::test]
async fn revogacao_enquanto_o_resolvedor_espera_o_lock_nao_grava() {
    let a = Ambiente::novo();
    let conector = serde_json::from_value::<crate::market::Connector>(json!({
        "id": "mcp-corrida", "name": "mcp-corrida", "command": "unused",
    }))
    .unwrap();
    crate::market::save_connectors(&a.hub.store, std::slice::from_ref(&conector)).unwrap();
    a.bot("alvo", &json!({}));
    let device = device_control("fone-corrida");
    a.hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&a.hub, &device.key).unwrap();
    let previa = crate::api::dispatch(
        &a.hub,
        &caller,
        "extensaoEmTodosPrevia",
        json!({"protocol": 1, "tipo": "conector", "id": "mcp-corrida"}),
    )
    .await
    .unwrap();
    let versao = previa["versao"].as_str().unwrap().to_owned();
    // Recém-depois da prévia, o cofre volta a texto puro: a leitura do resolvedor
    // detecta plaintext e PRECISA gravar a migração selada — é essa escrita que
    // a revogação tem de barrar.
    let seed = serde_json::to_string(&[conector]).unwrap();
    a.hub.store.kv_set("connectors", &seed).unwrap();

    // connector_lock confirmado adquirido antes do pedido: o resolvedor para
    // dentro dele depois da reconferência inicial.
    let (adquiriu_tx, adquiriu_rx) = std::sync::mpsc::channel::<()>();
    let (soltar_tx, soltar_rx) = std::sync::mpsc::channel::<()>();
    let hub_trava = a.hub.clone();
    let trava = std::thread::spawn(move || {
        let _g = hub_trava.store.connector_lock.locked();
        let _ = adquiriu_tx.send(());
        let _ = soltar_rx.recv();
    });
    let adquiriu = adquiriu_rx;
    let soltar = soltar_tx;
    adquiriu.recv_timeout(std::time::Duration::from_secs(10)).expect("connector_lock não adquirido");
    let espera = super::sonda::assinar("conector", "mcp-corrida".to_owned());
    let hub = a.hub.clone();
    let aplicando = tokio::spawn(async move {
        crate::api::dispatch(
            &hub,
            &caller,
            "extensaoEmTodosAplicar",
            json!({"protocol": 1, "tipo": "conector", "id": "mcp-corrida",
                   "botIds": ["alvo"], "versao": versao}),
        )
        .await
    });
    confirmar_resolvedor(&espera).await;
    a.hub.revoke_device(&device.key).unwrap();
    soltar.send(()).unwrap();
    trava.join().unwrap();
    let r = aplicando.await.unwrap();
    assert!(r.is_err(), "aplicar gravou depois da revogação");
    // A migração do cofre não commitou: o slot segue exatamente como semeado.
    assert_eq!(a.hub.store.kv_read("connectors").unwrap().as_deref(), Some(seed.as_str()));
    assert_eq!(a.lista("alvo", "connectors"), Vec::<String>::new());
}

/// Mesma corrida para SKILLS: `load_initialized` do catálogo tem migração
/// pendente (trava de escritor legado derrubada) e espera o lock de arquivo do
/// catálogo. A reemissão tem de ser recusada na transação da autoridade.
#[tokio::test]
async fn revogacao_enquanto_o_catalogo_espera_o_lock_nao_grava() {
    let a = Ambiente::novo();
    let skill = a.instalar_skill("Corrida");
    a.bot("alvo", &json!({}));
    let device = device_control("fone-catalogo");
    a.hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&a.hub, &device.key).unwrap();
    let previa = crate::api::dispatch(
        &a.hub,
        &caller,
        "extensaoEmTodosPrevia",
        json!({"protocol": 1, "tipo": "skill", "id": skill.clone()}),
    )
    .await
    .unwrap();
    let versao = previa["versao"].as_str().unwrap().to_owned();
    // Depois da prévia: derruba uma trava de escritor legado → o próximo
    // load_initialized tem reemissão pendente (reescreve META e recria a trava).
    a.hub.store.db.locked().execute_batch("DROP TRIGGER d166_skill_v2_insert").unwrap();
    let meta_antes = a.hub.store.kv_read("donna.passiveSkills.v2").unwrap();

    // .catalog.lock confirmado adquirido: o resolvedor para dentro de
    // with_snapshot → root_lock, depois do MUTATION.
    let lock_path = a.hub.store.skills_root.join(".catalog.lock");
    let arquivo =
        std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&lock_path).unwrap();
    let (adquiriu_tx, adquiriu_rx) = std::sync::mpsc::channel::<()>();
    let (soltar_tx, soltar_rx) = std::sync::mpsc::channel::<()>();
    let trava = std::thread::spawn(move || {
        fs2::FileExt::lock_exclusive(&arquivo).unwrap();
        let _ = adquiriu_tx.send(());
        let _ = soltar_rx.recv();
    });
    let adquiriu = adquiriu_rx;
    let soltar = soltar_tx;
    adquiriu.recv_timeout(std::time::Duration::from_secs(10)).expect(".catalog.lock não adquirido");
    let espera = super::sonda::assinar("skill", skill.clone());
    let hub = a.hub.clone();
    let aplicando = tokio::spawn(async move {
        crate::api::dispatch(
            &hub,
            &caller,
            "extensaoEmTodosAplicar",
            json!({"protocol": 1, "tipo": "skill", "id": skill,
                   "botIds": ["alvo"], "versao": versao}),
        )
        .await
    });
    confirmar_resolvedor(&espera).await;
    a.hub.revoke_device(&device.key).unwrap();
    soltar.send(()).unwrap();
    trava.join().unwrap();
    let r = aplicando.await.unwrap();
    assert!(r.is_err(), "aplicar gravou depois da revogação");
    // A reemissão não commitou: META intacto e a trava continua derrubada.
    assert_eq!(a.hub.store.kv_read("donna.passiveSkills.v2").unwrap(), meta_antes);
    let trava_existe: Option<String> = a
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT name FROM sqlite_master WHERE type='trigger' AND name='d166_skill_v2_insert'", [], |r| {
            r.get(0)
        })
        .ok();
    assert!(trava_existe.is_none(), "reemissão recriou a trava sem autoridade");
    assert_eq!(a.lista("alvo", "skills"), Vec::<String>::new());
}

/// Rodada 4: origem `Recusada` (aparelho sem Control) nunca migra nem reemite
/// o catálogo — com escrita pendente, a leitura pura falha fechado e o banco
/// fica intacto.
#[tokio::test]
async fn origem_recusada_nao_migra_o_catalogo() {
    let a = Ambiente::novo();
    a.instalar_skill("Recusada");
    // Escrita pendente: trava de escritor legado derrubada → load_initialized
    // precisaria reemitir se rodasse como operação interna.
    a.hub.store.db.locked().execute_batch("DROP TRIGGER d166_skill_v2_insert").unwrap();
    let meta_antes = a.hub.store.kv_read("donna.passiveSkills.v2").unwrap();
    let sem_controle = Caller::Device { key: "tela".into(), scopes: vec![], autoridade: None };
    let hub_req = a.hub.clone();
    let hub_fut = a.hub.clone();
    let r = crate::fork::donna_fixa::pedido::com_pedido(&hub_req, &sem_controle, async move {
        crate::fork::skill_catalog::list(&hub_fut.store, || Ok(()))
    })
    .await;
    assert!(r.is_err(), "catálogo migrou para aparelho sem autoridade");
    assert_eq!(a.hub.store.kv_read("donna.passiveSkills.v2").unwrap(), meta_antes);
    let trava_existe: Option<String> = a
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT name FROM sqlite_master WHERE type='trigger' AND name='d166_skill_v2_insert'", [], |r| {
            r.get(0)
        })
        .ok();
    assert!(trava_existe.is_none(), "reemissão recriou a trava sem autoridade");
}
