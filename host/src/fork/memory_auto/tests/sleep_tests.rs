//! Sono 2b1b: agendamento, reserva com prioridade da extração, checagens por mensagem, cursor na mesma
//! transação e o predicado da C7 (`sleep_checks::valid_for_cleanup`). Casos da linha 2b1 da tabela do desenho.
use super::*;
use crate::fork::memory_auto::{sleep, sleep_checks};

/// Depois da janela de 30 min, para o bot contar como parado.
fn later() -> i64 {
    crate::store::now_ms() + sleep::IDLE_MS + 60_000
}

/// Uma rodada do sono como o dreamer faria (sem o teste de turno vivo, que a fixture não tem).
fn round(f: &Fixture, now: i64) -> anyhow::Result<bool> {
    sleep::try_tick(&f.hub, "bot", now)?;
    Ok(sleep_jobs(f, "done") > 0)
}

fn sleep_jobs(f: &Fixture, status: &str) -> i64 {
    f.hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_memory_sleep_jobs WHERE status=?1", [status], |r| r.get(0))
        .unwrap()
}

fn checks(f: &Fixture) -> Vec<(String, String)> {
    let db = f.hub.store.db.locked();
    let mut stmt =
        db.prepare("SELECT message,status FROM donna_memory_sleep_checks ORDER BY message,checked_at_rev").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
}

fn cursor(f: &Fixture) -> i64 {
    sleep_checks::cursor(&f.hub.store.db.locked(), "bot", "bot").unwrap()
}

fn seq(f: &Fixture, id: &str) -> i64 {
    f.hub.store.entry(id).unwrap().seq
}

fn cleanable(f: &Fixture, message: &str) -> bool {
    sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", message).unwrap()
}

/// Fixture com o fato literal do dono ("Você disse", não é duvidoso) aplicado a partir de M.
fn applied(f: &Fixture) -> codync_mem::store::Entry {
    // "Você disse" exige frase inteira: M diz "Moro em Curitiba." como frase própria (mesma revisão).
    let text = "Moro em Curitiba. Prefiro café sem açúcar.";
    let data = json!({"text":text,"contentOrigin":{"status":"appReported","report":{"channel":"desktopComposer"},
        "segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}});
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET data=?2 WHERE id=?1", params![f.message, data.to_string()])
        .unwrap();
    let r = success(f);
    apply::complete(&f.hub, &r, plan(remember(f, "Moro em Curitiba")), |_, _| Ok(())).unwrap();
    f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().remove(0)
}

/// Segundo turno (N) com o checkpoint de sucesso: o trabalho fica `waiting` na fila de extração.
fn second(f: &Fixture) -> (String, String) {
    let (n, job) = second_job(f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    (n, job)
}

#[test]
fn rodada_cobre_a_mensagem_aplicada_e_avanca_o_cursor_uma_vez_por_dia() {
    let f = fixture(false);
    let fact = applied(&f);
    assert_eq!(fact.nature, codync_mem::nature::KindNature::Said);
    assert!(!round(&f, crate::store::now_ms()).unwrap(), "bot com mensagem recente não está parado");
    assert!(round(&f, later()).unwrap());
    assert_eq!(checks(&f), [(f.message.clone(), "covered".to_owned())]);
    assert_eq!(cursor(&f), seq(&f, &f.message));
    assert!(cleanable(&f, &f.message));
    // Mesmo dia: não agenda de novo. Dia seguinte sem mensagem nova: nada a provar, nada gravado.
    assert!(
        !sleep::schedule(&f.hub.store, "bot", later(), &chrono::Local::now().format("%Y-%m-%d").to_string()).unwrap()
    );
    assert!(!sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    assert_eq!(checks(&f).len(), 1);
}

/// Interrupção antes do commit (falha no UPSERT do cursor): nem checagem, nem cursor, nem fim do trabalho; a
/// reserva volta a esperar e a rodada seguinte grava tudo uma vez só. Depois do commit, repetir não duplica.
#[test]
fn interrupcao_antes_e_depois_do_commit_nao_duplica_nem_pula() {
    let f = fixture(false);
    applied(&f);
    f.hub.store.db.locked().execute_batch(
        "CREATE TRIGGER sleep_fail BEFORE INSERT ON donna_memory_sleep_cursor BEGIN SELECT RAISE(ABORT,'queda'); END;",
    )
    .unwrap();
    assert!(sleep::try_tick(&f.hub, "bot", later()).is_err());
    assert_eq!((checks(&f).len(), cursor(&f), sleep_jobs(&f, "waiting")), (0, 0, 1), "nada gravado; volta a esperar");
    f.hub.store.db.locked().execute_batch("DROP TRIGGER sleep_fail").unwrap();
    assert!(round(&f, later()).unwrap());
    assert_eq!(checks(&f).len(), 1);
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap();
    assert!(r.is_none(), "rodada concluída não volta");
    assert_eq!((checks(&f).len(), cursor(&f)), (1, seq(&f, &f.message)));
}

#[test]
fn lembranca_duvidosa_segura_a_mensagem_que_a_sustenta() {
    let f = fixture(false);
    let r = success(&f);
    apply::complete(&f.hub, &r, plan(remember(&f, "Prefere café sem açúcar")), |_, _| Ok(())).unwrap();
    assert!(round(&f, later()).unwrap());
    assert_eq!(checks(&f), []);
    assert_eq!(cursor(&f), 0);
    assert!(!cleanable(&f, &f.message));
}

/// `expired` e `notQueued` não geram linha e seguram o cursor; `memory_blocked` vira `excluded`, deixa o cursor
/// passar, mas a C7 não apaga sozinha.
#[test]
fn expired_e_not_queued_seguram_o_cursor_e_bloqueada_vira_excluded() {
    for status in ["expired", "notQueued"] {
        let f = fixture(false);
        applied(&f);
        let (n, job) = second(&f);
        f.hub
            .store
            .db
            .locked()
            .execute("UPDATE donna_memory_auto_jobs SET status=?2 WHERE id=?1", params![job, status])
            .unwrap();
        assert!(round(&f, later()).unwrap(), "{status}");
        assert_eq!(checks(&f), [(f.message.clone(), "covered".to_owned())], "{status}");
        assert_eq!(cursor(&f), seq(&f, &f.message), "{status}: N segura o cursor");
        assert!(!cleanable(&f, &n));
    }
    let f = fixture(false);
    applied(&f);
    let (n, job) = second_job(&f);
    let blocked = r#"{"completion":{"turn":2,"memory_blocked":true}}"#;
    queue::checkpoint(&f.hub.store, "completion-2", blocked, Some(&job), true, false).unwrap();
    assert!(round(&f, later()).unwrap());
    assert!(checks(&f).contains(&(n.clone(), "excluded".to_owned())));
    assert_eq!(cursor(&f), seq(&f, &n));
    assert!(cleanable(&f, &f.message));
    assert!(!cleanable(&f, &n), "excluded vai para a lista à parte; a C7 não apaga sozinha");
}

#[test]
fn edicao_depois_da_checagem_segura_a_c7_sem_o_cursor_voltar() {
    let f = fixture(false);
    applied(&f);
    assert!(round(&f, later()).unwrap());
    let before = cursor(&f);
    {
        let db = f.hub.store.db.locked();
        let rev = crate::store::next_rev(&db).unwrap();
        db.execute("UPDATE entries SET rev=?2 WHERE id=?1", params![f.message, rev]).unwrap();
    }
    assert!(!cleanable(&f, &f.message));
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap(), "a editada é revista primeiro");
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().unwrap();
    sleep::run(&f.hub, &r, later(), None).unwrap();
    assert_eq!(cursor(&f), before);
    assert!(!cleanable(&f, &f.message), "a revisão nova não está em trabalho aplicado: segue retida");
}

/// R2-01: `covered` antiga + sugestão nova citando M, sem editar M.
#[test]
fn sugestao_nova_sobre_mensagem_checada_impede_a_c7() {
    let f = fixture(false);
    applied(&f);
    assert!(round(&f, later()).unwrap());
    assert!(cleanable(&f, &f.message));
    let op = serde_json::to_string(&remember(&f, "Prefere café")).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_memory_suggestions(id,job,bot,memory,operation,origin,nature,fact,created_at)
         VALUES('s1',?1,'bot','bot',?2,'{}','inferred','x',0)",
            params![f.job, op],
        )
        .unwrap();
    assert!(!cleanable(&f, &f.message));
}

/// R3-01/D409: M checada → conflito novo por N (evidência só N) → a C7 recusa apagar M.
#[test]
fn conflito_novo_por_outra_mensagem_impede_a_c7_de_apagar_m() {
    let f = fixture(false);
    let fact = applied(&f);
    assert!(round(&f, later()).unwrap());
    assert!(cleanable(&f, &f.message));
    let (n, job) = second(&f);
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let revise = Operation::Revise {
        entry: fact.id,
        expected_rev: fact.rev,
        text: "Moro em Recife".into(),
        evidence: vec![n],
        nature: None,
    };
    apply::complete(&f.hub, &r, plan(revise), |_, _| Ok(())).unwrap();
    let check = sleep_checks::check_valid(&f.hub.store.db.locked(), &f.message).unwrap();
    assert_eq!(check, None, "o fato da linhagem de M mudou depois da checagem (rev > checked_at_rev)");
    assert!(!cleanable(&f, &f.message));
}

/// D412: M → nota L e fato F; `covered` de M; a nota na linhagem retém M. E o fato de M esquecido retém M.
#[test]
fn nota_na_linhagem_ou_fato_esquecido_retem_a_mensagem() {
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let bot = format!("sono-{}", uuid::Uuid::new_v4());
    let f = fixture_for_bot(false, &bot);
    let r = success(&f);
    let note = Operation::Note {
        file: "viagem.md".into(),
        content: Some("---\ndescription: Viagem\n---\n- Curitiba.\n".into()),
        shared: false,
        expected_hash: None,
        evidence: vec![f.message.clone()],
        nature: None,
    };
    apply::complete(&f.hub, &r, plan(note), |_, _| Ok(())).unwrap();
    sleep::try_tick(&f.hub, &bot, later()).unwrap();
    let db = f.hub.store.db.locked();
    assert!(sleep_checks::check_valid(&db, &f.message).unwrap().is_some(), "a nota não segura a checagem");
    assert!(!sleep_checks::valid_for_cleanup(&db, &bot, &bot, &f.message).unwrap(), "mas retém na C7");
    drop(db);
    let _ = std::fs::remove_dir_all(crate::chat::memory::files::folder(Some(&bot)).parent().unwrap());

    let f = fixture(false);
    let fact = applied(&f);
    assert!(round(&f, later()).unwrap());
    f.hub.store.memory_write(|c, _, _| mem::forget(c, fact.id)).unwrap();
    assert!(!cleanable(&f, &f.message), "fato esquecido na linhagem retém");
}

/// R7/R2-02: sono `waiting` antes, extração `waiting` depois: a extração é reservada e o sono espera.
#[test]
fn extracao_esperando_passa_a_frente_do_sono() {
    let f = fixture(false);
    applied(&f);
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let (_, job) = second(&f);
    assert!(sleep::reserve(&f.hub.store, "bot", later()).unwrap().is_none(), "extração esperando no Hub");
    assert_eq!(queue::reserve(&f.hub.store, None).unwrap().unwrap().id, job, "o sono não bloqueia a extração");
    f.hub.store.db.locked().execute("UPDATE donna_memory_auto_jobs SET status='noChange' WHERE id=?1", [&job]).unwrap();
    assert!(sleep::reserve(&f.hub.store, "bot", later()).unwrap().is_some());
}

/// D389 `off`: não agenda, não reserva e não roda; a rodada já reservada devolve sem gravar nada.
#[test]
fn modo_off_nao_agenda_nem_roda() {
    let f = fixture(false);
    applied(&f);
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "off").unwrap();
    sleep::try_tick(&f.hub, "bot", later()).unwrap();
    assert_eq!(count(&f, "donna_memory_sleep_jobs"), 0);
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "auto").unwrap();
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().unwrap();
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "off").unwrap();
    sleep::run(&f.hub, &r, later(), None).unwrap();
    assert_eq!((checks(&f).len(), sleep_jobs(&f, "waiting")), (0, 1));
}

/// A chave inclui o momento da checagem (a antiga vira histórico) e o predicado usa sempre a MAIS RECENTE da
/// revisão atual: `covered` antiga + `excluded` mais nova → não apaga.
#[test]
fn predicado_usa_a_checagem_mais_recente_da_mensagem() {
    let f = fixture(false);
    applied(&f);
    assert!(round(&f, later()).unwrap());
    assert!(cleanable(&f, &f.message));
    let rev = f.hub.store.entry(&f.message).unwrap().rev;
    {
        let db = f.hub.store.db.locked();
        crate::store::next_rev(&db).unwrap();
        sleep_checks::record(&db, ("bot", "bot", "outra"), (&f.message, rev), sleep_checks::Status::Excluded, "teste")
            .unwrap();
    }
    assert_eq!(checks(&f), [(f.message.clone(), "covered".to_owned()), (f.message.clone(), "excluded".to_owned())]);
    assert!(!cleanable(&f, &f.message));
}

/// Extração presa (já tentada e devolvida por falta de seleção) não segura o sono; a recém-chegada, com o mesmo
/// motivo `noSelection` e sem tentativa, segura.
#[test]
fn extracao_presa_sem_selecao_nao_segura_o_sono_mas_a_recem_chegada_segura() {
    let f = fixture(false);
    applied(&f);
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let (_, job) = second(&f);
    let reason: (String, i64) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT reason,attempts FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(reason, ("noSelection".to_owned(), 0), "recém-chegada: mesmo motivo, sem tentativa");
    assert!(sleep::reserve(&f.hub.store, "bot", later()).unwrap().is_none(), "a recém-chegada segura o sono");
    // Tentada e devolvida sem seleção, como `worker::release(NoSelection)`.
    let r = queue::reserve(&f.hub.store, None).unwrap().unwrap();
    assert_eq!(r.id, job);
    worker::release(&f.hub, &r, worker::Unavailable::NoSelection).unwrap();
    assert!(sleep::reserve(&f.hub.store, "bot", later()).unwrap().is_some(), "presa: o sono roda");
}

fn job_state(f: &Fixture, job: &str) -> (String, Option<String>, Option<String>) {
    let sql = "SELECT status,reason,proof FROM donna_memory_auto_jobs WHERE id=?1";
    f.hub.store.db.locked().query_row(sql, [job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
}

/// R1-01 = SEG-499-01: `off` entre a resposta e o commit fecha o pedido como `ineligible` (nada agora nem depois,
/// sem proposta guardada), nunca `noChange`; o sono não cobre essa mensagem, o cursor para antes dela e a C7 não a
/// apagaria. Religar não faz aprender o que foi lido desligado.
#[test]
fn off_entre_a_resposta_e_o_commit_fecha_ineligible_e_nada_e_aprendido() {
    let f = fixture(false);
    applied(&f);
    let (n, job) = second_job(&f);
    let op = Operation::Remember {
        text: "Prefere chá verde à tarde".into(),
        shared: false,
        evidence: vec![n.clone()],
        nature: None,
    };
    staged(&f, &op).unwrap();
    assert_eq!(count(&f, "donna_memory_auto_proposals"), 1, "proposta preparada no turno");
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "off").unwrap();
    let ids = apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap();
    assert_eq!(ids.len(), 0);
    assert_eq!(job_state(&f, &job), ("ineligible".to_owned(), Some("ineligible".to_owned()), None));
    assert_eq!(count(&f, "donna_memory_auto_proposals"), 0, "nenhuma proposta guardada");
    let texts = || -> Vec<String> {
        f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().into_iter().map(|e| e.text).collect()
    };
    assert_eq!(texts(), ["Moro em Curitiba"], "nenhuma memória gravada");
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "auto").unwrap();
    sleep::try_tick(&f.hub, "bot", later()).unwrap();
    assert!(queue::reserve_job(&f.hub.store, &job).unwrap().is_none(), "não volta à fila");
    assert_eq!(texts(), ["Moro em Curitiba"], "religar não aprende o que foi lido desligado");
    assert!(!checks(&f).iter().any(|(m, _)| *m == n), "zero covered para N");
    assert_eq!(cursor(&f), seq(&f, &f.message), "o cursor para antes de N");
    assert!(!cleanable(&f, &n));
}

/// Controle positivo: plano vazio legítimo com o aprendizado ligado → `noChange` com a prova → `covered`. O mesmo
/// `noChange` sem a prova (legado, `proof` NULL) não cobre.
#[test]
fn no_change_so_conta_com_a_prova_do_commit_e_o_legado_nao_cobre() {
    for legacy in [false, true] {
        let f = fixture(false);
        let r = success(&f);
        assert_eq!(apply::complete(&f.hub, &r, ExtractionPlan::default(), |_, _| Ok(())).unwrap().len(), 0);
        assert_eq!(job_state(&f, &f.job), ("noChange".to_owned(), None, Some("learningOn".to_owned())));
        if legacy {
            f.hub.store.db.locked().execute("UPDATE donna_memory_auto_jobs SET proof=NULL", []).unwrap();
        }
        sleep::try_tick(&f.hub, "bot", later()).unwrap();
        let covered = checks(&f).contains(&(f.message.clone(), "covered".to_owned()));
        assert_eq!(covered, !legacy, "legado={legacy}");
        assert_eq!(cleanable(&f, &f.message), !legacy);
    }
}

/// R1-02: o fato de M muda depois da checagem (fixar e desafixar); M, atrás do cursor, volta à rodada seguinte.
#[test]
fn checagem_invalidada_volta_a_rodada_seguinte() {
    let f = fixture(false);
    let fact = applied(&f);
    assert!(round(&f, later()).unwrap());
    for pinned in [true, false] {
        f.hub.store.memory_write(|c, now, rev| mem::pin(c, fact.id, pinned, now, rev)).unwrap();
    }
    assert_eq!(sleep_checks::check_valid(&f.hub.store.db.locked(), &f.message).unwrap(), None);
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap(), "M volta a ter o que provar");
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().unwrap();
    sleep::run(&f.hub, &r, later(), None).unwrap();
    assert!(sleep_checks::check_valid(&f.hub.store.db.locked(), &f.message).unwrap().is_some());
    assert_eq!(checks(&f).len(), 2, "a antiga fica de histórico");
}

/// R1-03: 500 respostas de cartão genUI (kind=user) antes não ocupam o lote; a mensagem humana aplicada depois entra.
#[test]
fn respostas_de_cartao_nao_ocupam_o_lote() {
    let f = fixture(false);
    applied(&f);
    for i in 0..500 {
        let data = json!({"text":format!("opção {i}"),"choice_response":{"card":"c","option":i}});
        f.hub.add_entry(&Lane::main("bot"), EntryKind::User, 1, &data).unwrap();
    }
    let (n, job) = second(&f);
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    apply::complete(&f.hub, &r, ExtractionPlan::default(), |_, _| Ok(())).unwrap();
    assert!(round(&f, later()).unwrap());
    assert!(checks(&f).contains(&(n.clone(), "covered".to_owned())));
    assert_eq!(cursor(&f), seq(&f, &n));
}
