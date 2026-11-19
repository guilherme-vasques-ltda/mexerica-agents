#![allow(clippy::unwrap_used)]
use super::*;
use crate::fork::push_textos::{Motivo, Situacao};

fn finished(stop: Option<&str>, visible: bool) -> Observed {
    Observed { end: End::Finished(stop.map(str::to_owned)), visible_reply: visible, terminal: false }
}

/// `Status` legado não tem `Debug`; compara pela forma do fio.
fn st(status: Status) -> Value {
    serde_json::to_value(status).unwrap()
}

#[allow(clippy::unnecessary_wraps, reason = "o mesmo tipo que `resolve` recebe")]
fn decl(outcome: Declared, summary: Option<&str>) -> Option<Lenient<Declaration>> {
    Some(Lenient::Ok(Declaration { outcome, summary: summary.map(str::to_owned), at: 1 }))
}

#[test]
fn schema_v1_no_hub() {
    assert!(parse_declaration(&json!({"outcome":"no_news"}), 1).is_ok());
    for outcome in ["reported", "needs_owner", "failed"] {
        assert!(parse_declaration(&json!({"outcome":outcome}), 1).is_err(), "{outcome} sem resumo");
        assert!(parse_declaration(&json!({"outcome":outcome,"summary":" \n\t "}), 1).is_err(), "{outcome} vazio");
    }
    assert!(parse_declaration(&json!({"outcome":"no_news","extra":1}), 1).is_err());
    assert!(parse_declaration(&json!({"outcome":"done"}), 1).is_err());
    assert!(parse_declaration(&json!({"outcome":"reported","summary":"a".repeat(301)}), 1).is_err());
    let d = parse_declaration(&json!({"outcome":"reported","summary":"  linha\u{0007}um\n dois  "}), 1).unwrap();
    assert_eq!(d.summary.as_deref(), Some("linha um dois"));
}

#[test]
fn precedencia_falha_observada_terminal_ausente_e_declaracao() {
    let reported = decl(Declared::Reported, Some("ok"));
    // 1. Falha observada vence a declaração.
    for reason in [Reason::AgentError, Reason::Timeout] {
        let (status, o) = resolve(&Observed::failed(reason), reported.as_ref(), false);
        assert_eq!(
            (st(status), o.kind, o.source, o.reason),
            (st(Status::Failed), Kind::Failed, Source::Observed, Some(reason))
        );
    }
    // 2. `stopReason` ausente com declaração válida.
    let (status, o) = resolve(&finished(None, true), reported.as_ref(), false);
    assert_eq!(
        (st(status), o.kind, o.reason),
        (st(Status::Succeeded), Kind::Unconfirmed, Some(Reason::TerminalMissing))
    );
    // `stopReason` diferente de end_turn.
    let (status, o) = resolve(&finished(Some("max_tokens"), false), reported.as_ref(), false);
    assert_eq!((st(status), o.kind, o.reason), (st(Status::Failed), Kind::Failed, Some(Reason::EngineStop)));
    // 3. Cada declaração com end_turn.
    for (declared, status, kind) in [
        (Declared::NoNews, Status::Succeeded, Kind::NoNews),
        (Declared::Reported, Status::Succeeded, Kind::Reported),
        (Declared::NeedsOwner, Status::Failed, Kind::NeedsOwner),
        (Declared::Failed, Status::Failed, Kind::Failed),
    ] {
        let (s, o) = resolve(&finished(Some("end_turn"), false), decl(declared, Some("x")).as_ref(), false);
        assert_eq!((st(s), o.kind, o.source), (st(status), kind, Source::Declared));
    }
    // 4. Sem declaração.
    let (status, o) = resolve(&finished(Some("end_turn"), true), None, false);
    assert_eq!(
        (st(status), o.kind, o.reason, o.visible_reply),
        (st(Status::Succeeded), Kind::Unconfirmed, Some(Reason::NoDeclaration), true)
    );
    // 5. Conflito e declaração ilegível.
    let (_, o) = resolve(&finished(Some("end_turn"), false), reported.as_ref(), true);
    assert_eq!((o.kind, o.reason), (Kind::Unconfirmed, Some(Reason::Conflict)));
    let illegible = Some(Lenient::Illegible(json!(3)));
    let (_, o) = resolve(&finished(Some("end_turn"), false), illegible.as_ref(), false);
    assert_eq!((o.kind, o.reason), (Kind::Unconfirmed, Some(Reason::Conflict)));
    // Parada e interrupção.
    let stopped = Observed { end: End::Stopped, visible_reply: false, terminal: false };
    assert_eq!(st(resolve(&stopped, reported.as_ref(), false).0), st(Status::Cancelled));
    assert_eq!(st(resolve(&Observed::interrupted(), reported.as_ref(), false).0), st(Status::Interrupted));
}

#[test]
fn record_e_por_execucao() {
    let (mut slot, mut conflict) = (None, false);
    let d = |o| Declaration { outcome: o, summary: Some("s".into()), at: 1 };
    assert!(record(&mut slot, &mut conflict, d(Declared::Reported)));
    assert!(record(&mut slot, &mut conflict, Declaration { at: 9, ..d(Declared::Reported) }));
    assert!(!conflict);
    assert!(!record(&mut slot, &mut conflict, d(Declared::Failed)));
    assert!(conflict);
}

#[test]
fn leitura_tolerante_nao_falha_e_fica_conservadora() {
    #[derive(Deserialize, Serialize)]
    struct Holder {
        #[serde(default)]
        outcome: Option<Lenient<RunOutcome>>,
        #[serde(default)]
        declaration: Option<Lenient<Declaration>>,
    }
    let h: Holder = serde_json::from_value(json!({
        "outcome": {"kind":"futuro","source":"outro","reason":"novo"},
        "declaration": {"outcome":"talvez"}
    }))
    .unwrap();
    let Some(Lenient::Ok(o)) = &h.outcome else { panic!("objeto com valores desconhecidos é legível") };
    assert_eq!((o.kind, o.source, o.reason), (Kind::Unknown, Source::Unknown, Some(Reason::Unknown)));
    assert!(matches!(h.declaration, Some(Lenient::Illegible(_))));
    let h: Holder = serde_json::from_value(json!({"outcome": "texto"})).unwrap();
    assert_eq!(public(h.outcome.as_ref())["kind"], "unknown");
    // O dado ilegível é preservado como veio ao regravar.
    assert_eq!(serde_json::to_value(&h).unwrap()["outcome"], "texto");
    let h: Holder = serde_json::from_value(json!({})).unwrap();
    assert!(h.outcome.is_none() && public(None).is_null());
}

#[test]
fn publicacao_por_desfecho() {
    let publish =
        |o: RunOutcome| publication(Some(&Lenient::Ok(o)), Status::Succeeded, None, Some("Resposta"), "Diária");
    let base = |kind, source, reason, visible| RunOutcome {
        version: 1,
        kind,
        source,
        reason,
        summary: Some("resumo".into()),
        visible_reply: visible,
    };
    assert!(!publish(base(Kind::NoNews, Source::Declared, None, false)).is_some());
    let p = publish(base(Kind::Reported, Source::Declared, None, true));
    assert!(p.notice.is_none());
    assert!(matches!(&p.alert, Some((Alerta::Texto(t), AlertKind::Done)) if t == "Resposta"));
    let p = publish(base(Kind::Reported, Source::Declared, None, false));
    assert_eq!(p.notice.unwrap().0, "A rotina «Diária» terminou: resumo");
    assert!(matches!(
        &p.alert,
        Some((Alerta::Rotina { nome, situacao: Situacao::Terminou, detalhe }, AlertKind::Done))
            if nome == "Diária" && matches!(detalhe, Detalhe::Resumo(d) if d == "resumo")
    ));
    let p = publish(base(Kind::NeedsOwner, Source::Declared, None, false));
    assert!(matches!(
        &p.alert,
        Some((Alerta::Rotina { situacao: Situacao::Precisa, detalhe: Detalhe::Resumo(d), .. }, AlertKind::NeedsInput))
            if d == "resumo"
    ));
    let p = publish(base(Kind::Failed, Source::Observed, Some(Reason::Timeout), false));
    assert_eq!(p.notice.unwrap().0, "A rotina «Diária» não conseguiu terminar: passou do tempo limite e foi parada.");
    // O motivo observado é tipado: o push_textos traduz, o texto pt-BR fica só no aviso.
    assert!(matches!(
        &p.alert,
        Some((
            Alerta::Rotina { situacao: Situacao::Falhou, detalhe: Detalhe::Motivo(Motivo::Tempo), .. },
            AlertKind::Failed
        ))
    ));
    // Decisão 1 A: sem confirmação e sem resposta visível, aviso curto no chat e nenhum alerta.
    let p = publish(base(Kind::Unconfirmed, Source::None, Some(Reason::NoDeclaration), false));
    assert_eq!(p.notice.unwrap().0, "A rotina «Diária» terminou sem dizer como foi. Confira a conversa.");
    assert!(p.alert.is_none());
    assert!(!publish(base(Kind::Unconfirmed, Source::None, Some(Reason::NoDeclaration), true)).is_some());
    assert!(!publish(base(Kind::Unconfirmed, Source::Observed, Some(Reason::Stopped), false)).is_some());
    let p = publish(base(Kind::Unconfirmed, Source::Observed, Some(Reason::Interrupted), false));
    // `run.detail` é texto do Hub: no push cai no padrão traduzido, nunca no detalhe cru.
    assert!(matches!(
        &p.alert,
        Some((Alerta::Rotina { situacao: Situacao::Interrompida, detalhe: Detalhe::Padrao, .. }, AlertKind::Failed))
    ));
    // Nenhum aviso sai com texto vazio.
    for kind in [Kind::Reported, Kind::NeedsOwner, Kind::Failed, Kind::Unconfirmed] {
        if let Some((text, _)) = publish(base(kind, Source::Declared, None, false)).notice {
            assert_ne!(text.trim(), "");
        }
    }
}

#[test]
fn ferramenta_so_na_sessao_de_rotina() {
    assert!(offered(Some("routine-abc")));
    assert!(!offered(None) && !offered(Some("main")) && !offered(Some("group:x")));
    let listed = |slot| crate::routines::tools_for(slot).as_array().unwrap().iter().any(|t| t["name"] == TOOL);
    assert!(listed(Some("routine-abc")) && !listed(None));
}
