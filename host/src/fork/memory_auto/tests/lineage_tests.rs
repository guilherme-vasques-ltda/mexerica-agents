//! Sono 2b1: a linhagem fato/nota → mensagem nasce pelas rotas reais de escrita e sobrevive ao DELETE físico.
//! As primitivas de cada rota (editar, fixar, arquivar, ativar, restaurar, esquecer, limpar, notas) estão nos
//! testes de `codync_mem::lineage`; aqui ficam as portas do Hub que carregam evidência.
use super::*;

fn lineage(f: &Fixture, kind: &str, item: &str) -> Vec<String> {
    let db = f.hub.store.db.locked();
    let mut stmt =
        db.prepare("SELECT message FROM donna_memory_lineage WHERE kind=?1 AND item=?2 ORDER BY rev,message").unwrap();
    stmt.query_map([kind, item], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

fn fact(f: &Fixture, id: i64) -> Vec<String> {
    lineage(f, "fact", &id.to_string())
}

fn local() -> crate::fork::autoridade::Autoridade {
    crate::fork::autoridade::Autoridade::local()
}

/// Extração: cria (M), revisa em conflito com evidência só N, arquiva por N; o Desfazer da criação apaga o fato
/// e a linhagem fica (D409/D412).
#[test]
fn extracao_cria_revisa_em_conflito_arquiva_e_desfaz_sem_perder_a_linhagem() {
    let f = fixture(false);
    let r = success(&f);
    let ids = apply::complete(
        &f.hub,
        &r,
        ExtractionPlan {
            operations: vec![remember(&f, "Mora em Curitiba"), remember(&f, "Prefere café sem açúcar")]
        },
        |_, _| Ok(()),
    )
    .unwrap();
    let saved = f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap();
    let city = saved.iter().find(|e| e.text == "Mora em Curitiba").unwrap().clone();
    let coffee = saved.iter().find(|e| e.text == "Prefere café sem açúcar").unwrap().id;
    assert_eq!(fact(&f, city.id), std::slice::from_ref(&f.message));

    let notice = ids.iter().find(|id| f.hub.store.entry(id).unwrap().data["memory"]["entry"] == coffee).unwrap();
    assert_eq!(apply::undo(&f.hub, notice, &local()).unwrap()["entry"]["data"]["memory"]["status"], "undone");
    assert!(f.hub.store.memory_read(|c| mem::get(c, coffee)).unwrap().is_none(), "Desfazer da criação = DELETE");
    assert_eq!(fact(&f, coffee), std::slice::from_ref(&f.message), "o item apagado conserva a linhagem");

    let (n, job) = second_job(&f);
    let _ = queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false)
        .unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let revise = Operation::Revise {
        entry: city.id,
        expected_rev: city.rev,
        text: "Mora em Recife".into(),
        evidence: vec![n.clone()],
        nature: None,
    };
    apply::complete(&f.hub, &r, plan(revise), |_, _| Ok(())).unwrap();
    let conflict = f.hub.store.memory_read(|c| mem::get(c, city.id)).unwrap().unwrap();
    assert_eq!(conflict.evidence, std::slice::from_ref(&n), "a evidência atual é só N");
    assert_eq!(fact(&f, city.id), [f.message.clone(), n.clone()], "M, do texto conservado, continua na linhagem");

    f.hub.store.db.locked().execute("UPDATE donna_memory_auto_jobs SET status='waiting' WHERE id=?1", [&job]).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let archive =
        Operation::Archive { entry: city.id, expected_rev: conflict.rev, evidence: vec![n.clone()], nature: None };
    apply::complete(&f.hub, &r, plan(archive), |_, _| Ok(())).unwrap();
    assert_eq!(fact(&f, city.id), [f.message.clone(), n]);
}

/// MCP (D246): a proposta preparada aplicada depois do sucesso grava a linhagem na mesma porta.
#[test]
fn proposta_preparada_do_mcp_grava_a_linhagem() {
    let f = fixture(false);
    staged(&f, &remember(&f, "Mora em Curitiba")).unwrap();
    let r = success(&f);
    let apply::PreparedOutcome::Applied(_) = apply::apply_prepared(&f.hub, &r).unwrap() else {
        panic!("a proposta deve ser aplicada")
    };
    let saved = f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap();
    assert_eq!(fact(&f, saved[0].id), std::slice::from_ref(&f.message));
}

/// Nota da extração: a linhagem da nota sobrevive ao Desfazer e ao clearMemory (retenção D412).
#[tokio::test]
async fn nota_da_extracao_sobrevive_ao_desfazer_e_ao_clear_memory() {
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let bot = format!("linhagem-{}", uuid::Uuid::new_v4());
    let f = fixture_for_bot(false, &bot);
    let r = success(&f);
    let note = |name: &str| Operation::Note {
        file: name.into(),
        content: Some("---\ndescription: Preferências\n---\n- Prefere café sem açúcar.\n".into()),
        shared: false,
        expected_hash: None,
        evidence: vec![f.message.clone()],
        nature: None,
    };
    let ids = apply::complete(
        &f.hub,
        &r,
        ExtractionPlan { operations: vec![note("preferencias.md"), note("viagem.md")] },
        |_, _| Ok(()),
    )
    .unwrap();
    // D426: o item é o código opaco da nota, lido antes de o clearMemory apagar as versões (e o vínculo com o nome).
    let key = |name: &str| -> String {
        f.hub
            .store
            .db
            .locked()
            .query_row(
                "SELECT note_key FROM memory_file_versions WHERE bot=?1 AND name=?2 ORDER BY id LIMIT 1",
                params![bot, name],
                |r| r.get(0),
            )
            .unwrap()
    };
    let keys = [key("preferencias.md"), key("viagem.md")];
    assert_eq!(lineage(&f, "note", &keys[0]), std::slice::from_ref(&f.message));
    apply::undo(&f.hub, &ids[1], &local()).unwrap();
    crate::api::dispatch(&f.hub, &crate::api::devices::Caller::Local, "clearMemory", json!({"botId":bot}))
        .await
        .unwrap();
    for key in &keys {
        assert_eq!(lineage(&f, "note", key), std::slice::from_ref(&f.message));
    }
    let leaked: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*) FROM donna_memory_lineage WHERE item LIKE '%preferencias%' OR item LIKE '%viagem%'
             OR message LIKE '%preferencias%' OR message LIKE '%viagem%' OR kind LIKE '%viagem%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0, "D426: nenhum nome de nota na linhagem");
    let _ = std::fs::remove_dir_all(crate::chat::memory::files::folder(Some(&bot)).parent().unwrap());
}
