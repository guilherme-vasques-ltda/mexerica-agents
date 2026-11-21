//! Trocar de provedor ou modelo pelo app. O ator do bot FECHA a conexão e abre
//! outra (session/load da mesma sessão); a conversa do Motor Mexerica tem de seguir. E trocar a conta apaga a
//! conversa guardada do bot. Ator real, adaptador real e motor real em Linux, com o provedor falso e sem
//! servidores MCP do Hub.
use crate::{
    LockExt,
    fork::motor_mexerica::{ACTOR_FAUX, credential_slot, historico},
    hub::Hub,
    store::Store,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::mpsc;

/// Um ator real com o motor real: o bot, o Hub e a pasta de trabalho dele.
struct Cenario {
    bot: &'static str,
    hub: Arc<Hub>,
    actor: super::Actor,
    results: mpsc::UnboundedReceiver<super::PromptDone>,
    done: mpsc::UnboundedSender<super::PromptDone>,
    work: PathBuf,
}

/// `None` fora do CI sem Node (a prova é obrigatória no CI: ver `REQUIRED_ENV`).
async fn cenario(test: &str, bot: &'static str) -> Option<Cenario> {
    crate::fork::motor_exec::test_support::case(test)?;
    let node = crate::fork::motor_mexerica::node_path().is_some();
    assert!(node || std::env::var_os(crate::fork::motor_mexerica::REQUIRED_ENV).is_none(), "node ausente no CI");
    if !node {
        return None;
    }
    ACTOR_FAUX.store(true, Ordering::Relaxed);
    let dir = crate::store::private_fixture_dir("motor-conversa");
    // A pasta de trabalho própria do bot (a única dentro dos dados do Hub que a validação aceita).
    let work = crate::service::data_dir().join("bots").join(bot).join("workspace");
    std::fs::create_dir_all(&work).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![42; 32]));
    let cfg = serde_json::from_value(
        json!({"id": bot, "name": "Teste", "backend": "mexerica", "model": "faux/faux-1", "cwd": work, "notify": false}),
    )
    .unwrap();
    store.save_bot(&cfg).unwrap();
    for provider in ["faux", "faux2"] {
        crate::market::vault::write(&store, &credential_slot(provider), "chave-de-prova-do-ator").unwrap();
    }
    let hub = Hub::new(
        store,
        "test".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "test".into(),
        1,
    );
    hub.start().unwrap();
    hub.shutdown().await;
    let mut actor = super::new_actor(hub.clone(), hub.store.bot(bot).unwrap().unwrap().config);
    actor.restore_dispatch().unwrap();
    let (done, results) = mpsc::unbounded_channel();
    Some(Cenario { bot, hub, actor, results, done, work })
}

impl Cenario {
    /// A próxima seleção da principal (D372): mesmo motor, outro modelo (outro provedor do motor).
    fn proximo_modelo(&self, model: &str) {
        let engine = json!({"schemaVersion": 2, "source": "own", "accountId": null, "backend": "mexerica",
            "model": model, "modelOptions": {}, "fastMode": false})
        .to_string();
        let sql = "UPDATE donna_bot_sessions SET engine_next=?1 WHERE id=?2";
        self.hub.store.db.locked().execute(sql, [engine, self.bot.to_owned()]).unwrap();
    }

    /// Um turno humano inteiro; devolve a resposta final do motor.
    async fn turno(&mut self, text: &str) -> String {
        let bot = self.bot;
        let lane = crate::store::Lane::main(bot);
        // Só vale a resposta final DESTE turno: uma anterior não pode fazer o teste passar por engano.
        let before: std::collections::HashSet<_> = self
            .hub
            .store
            .history_in_session(bot, Some(bot), i64::MAX, 50)
            .unwrap()
            .iter()
            .map(|e| e.id.clone())
            .collect();
        let turn = self.hub.store.alloc_turn(bot).unwrap();
        let data = json!({"text": text, "status": "queued", "contentOrigin": {"ingress": {"caller": "local"}}});
        let ids = [self.hub.store.insert_entry(&lane, crate::store::EntryKind::User, turn, &data).unwrap().id];
        self.hub.prepare_bot_turn(bot, None, |r| r.lane = Some(lane.clone())).unwrap();
        self.actor.start_turn(lane, &ids, text, false, &self.done).await.unwrap();
        // Como o laço do ator: as atualizações do motor viram entradas antes de o turno acabar.
        let actor = &mut self.actor;
        let results = &mut self.results;
        let finished = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                tokio::select! {
                    inc = super::recv_incoming(&mut actor.conn) => actor.on_incoming(inc).await,
                    done = results.recv() => break done.unwrap(),
                }
            }
        })
        .await
        .unwrap();
        while let Some(inc) = actor.conn.as_mut().and_then(|c| c.rx.try_recv().ok()) {
            actor.on_incoming(Some(inc)).await;
        }
        actor.finish_turn(&finished.result);
        let entries = self.hub.store.history_in_session(bot, Some(bot), i64::MAX, 50).unwrap();
        let answer = entries
            .iter()
            .rev()
            .find(|e| e.kind == "agent" && e.data["final"] == true && !before.contains(&e.id))
            .map(|e| e.data["text"].as_str().unwrap_or_default().to_owned());
        answer.unwrap_or_else(|| {
            let seen: Vec<_> = entries.iter().map(|e| (e.kind.as_str(), e.data.to_string())).collect();
            panic!("o turno não terminou com resposta do motor: {seen:?} resultado: {:?}", finished.result)
        })
    }

    async fn encerrar(mut self) {
        self.actor.conn.take().unwrap().acp.kill().await;
        std::fs::remove_dir_all(self.work).unwrap();
    }
}

#[tokio::test]
async fn troca_de_modelo_pelo_ator_do_bot_mantem_a_conversa_do_motor_mexerica() {
    let Some(mut c) = cenario(
        "agent::bot::motor_conversa_tests::troca_de_modelo_pelo_ator_do_bot_mantem_a_conversa_do_motor_mexerica",
        "bot-troca",
    )
    .await
    else {
        return;
    };
    c.turno("MEU-CODIGO-42 vale hoje").await;
    let antiga = c.actor.conn.as_ref().map(|conn| conn.acp.clone()).unwrap();

    // Troca pelo app: outro provedor do motor. O ator fecha a conexão e abre outra com session/load.
    c.proximo_modelo("faux2/faux-1");
    let resposta = c.turno("/historico").await;
    assert_eq!(c.actor.cfg.model.as_deref(), Some("faux2/faux-1"));
    let nova = c.actor.conn.as_ref().map(|conn| conn.acp.clone()).unwrap();
    assert!(!Arc::ptr_eq(&antiga, &nova), "a troca de modelo tinha de fechar a conexão e abrir outra");
    assert!(
        resposta.starts_with("historico:") && resposta.contains("MEU-CODIGO-42 vale hoje"),
        "a conversa se perdeu na troca pelo app: {resposta:?}"
    );

    // E de volta ao primeiro provedor.
    c.proximo_modelo("faux/faux-1");
    let resposta = c.turno("/historico").await;
    assert!(resposta.starts_with("historico:") && resposta.contains("MEU-CODIGO-42 vale hoje"), "{resposta:?}");
    c.encerrar().await;
}

#[tokio::test]
async fn trocar_a_conta_do_bot_apaga_a_conversa_guardada_e_comeca_sem_copia() {
    let Some(mut c) = cenario(
        "agent::bot::motor_conversa_tests::trocar_a_conta_do_bot_apaga_a_conversa_guardada_e_comeca_sem_copia",
        "bot-conta",
    )
    .await
    else {
        return;
    };
    c.turno("CONTA-ANTIGA-777 vale hoje").await;
    assert_eq!(historico::stored_for("bot-conta"), 1, "a conversa da conta antiga está guardada");
    // Outra conversa do mesmo bot (a de uma rotina): a troca da sessão principal não pode apagá-la.
    historico::seed("bot-conta", "sessao-da-rotina");
    assert_eq!(historico::stored_for("bot-conta"), 2);

    // A conta (carimbo) com que as sessões do bot abriram mudou: ele recomeça sem a cópia anterior. O carimbo
    // da principal fica na chave do bot ou na própria, conforme a seleção; os dois mudam.
    let antigo = "{\"account\":\"conta-antiga\",\"revision\":1,\"backend\":\"mexerica\"}";
    for key in ["account.stamp.bot-conta", "account.stamp.bot-conta/session:bot-conta"] {
        c.hub.store.kv_set(key, antigo).unwrap();
    }
    let resposta = c.turno("/historico").await;
    assert!(!resposta.contains("CONTA-ANTIGA-777"), "a conversa da conta antiga voltou: {resposta:?}");
    assert_eq!(
        historico::stored_for("bot-conta"),
        2,
        "a conversa antiga da sessão trocada foi apagada; ficam a nova e a da rotina, que não foi tocada"
    );
    c.encerrar().await;
}
