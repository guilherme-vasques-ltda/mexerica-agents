#![allow(clippy::unwrap_used)]

use super::{consulta, pendentes, privacidade::*, testes::Fixture, *};
use crate::fork::{control_tower::Observation, spaces::PERSONAL};
use crate::store::{EntryKind, Lane};
use regex::Regex;
use serde_json::json;
use std::path::{Path, PathBuf};

const OWNER: &str = "owner@example.invalid";
const MEMBER: &str = "member@example.invalid";
const OTHER: &str = "other@example.invalid";
const AUTH: EffectAuthority = EffectAuthority { hub_generation: 7, owner_epoch: 3, policy_revision: 2 };

fn owner_only() -> AudienceGrant {
    AudienceGrant::OwnerOnly { space_id: PERSONAL.into(), owner_principal_id: OWNER.into(), owner_epoch: 3 }
}

fn explicit() -> AudienceGrant {
    AudienceGrant::ExplicitPrincipals {
        space_id: PERSONAL.into(),
        allowed_principal_ids: vec![MEMBER.into()],
        allowed_bot_ids: vec!["a".into()],
        owner_epoch: 3,
        policy_revision: 2,
    }
}

fn caller(principal_id: Option<&str>) -> ReadAuthority<'_> {
    ReadAuthority {
        space_id: PERSONAL,
        principal_id,
        bot_id: None,
        owner_epoch: 3,
        policy_revision: 2,
        control: ControlAuthority::Validated,
    }
}

fn qualify(f: &Fixture, request_id: &str, audience: AudienceGrant, read: ReadProvenance) -> ExecutionTaint {
    let owner = match &audience {
        AudienceGrant::OwnerOnly { owner_principal_id, .. } => owner_principal_id.clone(),
        AudienceGrant::ExplicitPrincipals { .. } => OWNER.into(),
    };
    let mut taint = ExecutionTaint::new(owner, audience);
    taint.observe(read);
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    taint.qualify_na(&tx, request_id, AUTH).unwrap();
    tx.commit().unwrap();
    taint
}

#[test]
fn p01_broadcast_rotina_e_par_nao_leem_nem_metadados() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    qualify(&f, &id, owner_only(), ReadProvenance::PrivateRead);
    let q = load(&f.hub.store.db.locked(), &id).unwrap().unwrap();
    for reader in [
        BodyReaderAllowlist::BroadcastEventBus,
        BodyReaderAllowlist::RoutineOrWebhookConsumer,
        BodyReaderAllowlist::PeerBotDelivery,
    ] {
        for authority in [None, Some(&caller(Some(OWNER)))] {
            assert!(q.read_authorized(reader, authority).is_err());
        }
    }
}

#[test]
fn p02_solicitante_autenticado_recebe_apenas_estado() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    qualify(&f, &id, explicit(), ReadProvenance::PrivateRead);
    let fence = despachar(&f.hub.store, &id).unwrap().unwrap();
    let entry = f.entrada(&Lane::main("b"), EntryKind::Agent);
    let t = f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Entrada(entry.clone()));
    assert_eq!(
        registrar_fim(&f.hub.store, &id, "b", Andamento::EmExecucao(fence), Fim::Turno(t.fim)),
        Registro::CorpoRetido
    );

    let db = f.hub.store.db.locked();
    let mut auth = caller(Some(MEMBER));
    auth.bot_id = Some("a");
    let ler =
        |authority: Option<&ReadAuthority<'_>>| consulta::ler_autorizado_na(&db, "a", &f.execucao, &id, authority);
    let dto = ler(Some(&auth)).unwrap().1.json(&id);
    assert_eq!(dto["state"], "finished");
    assert_eq!(dto["outcome"], json!({"result":"completed", "reason":"end_turn"}));
    assert_eq!(dto.as_object().unwrap().len(), 5);
    assert!(!dto.to_string().contains(&entry));
    assert!(!dto.to_string().contains("SENTINELA"));
    for (principal, bot) in [(Some(MEMBER), Some("b")), (Some(OTHER), Some("a")), (None, Some("a"))] {
        let auth = ReadAuthority { bot_id: bot, ..caller(principal) };
        assert_eq!(ler(Some(&auth)).err().unwrap().to_string(), consulta::INDISPONIVEL);
    }
    assert_eq!(ler(None).err().unwrap().to_string(), consulta::INDISPONIVEL);
    let unavailable = ReadAuthority { control: ControlAuthority::Unavailable, ..auth };
    assert_eq!(ler(Some(&unavailable)).err().unwrap().to_string(), consulta::INDISPONIVEL);
}

#[test]
fn p03_leitura_sem_atestado_eleva_taint_sem_mudar_disposicao() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    let mut taint = qualify(&f, &id, owner_only(), ReadProvenance::UnattestedRead);
    for read in [ReadProvenance::AttestedInSpace, ReadProvenance::PrivateRead] {
        taint.observe(read);
        assert_eq!(taint.class(), PrivacyClass::UnknownTaint);
        assert_eq!(taint.disposition(), BodyDisposition::BodyRetained);
    }
    let row: (String, String) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT privacy_class,body_disposition FROM donna_session_privacy WHERE request_id=?1", [&id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("unknown_taint".into(), "body_retained".into()));
}

#[test]
fn p04_trigger_e_requalificacao_nao_rebaixam() {
    const REPLACE: &str = "INSERT OR REPLACE INTO donna_session_privacy(request_id,privacy_class,body_disposition,
           audience_json,owner_principal_id,owner_epoch,policy_revision,hub_generation,delivery_state,revoked_at)
         SELECT request_id,?2,body_disposition,audience_json,owner_principal_id,owner_epoch,policy_revision,
           hub_generation,'retained',0 FROM donna_session_privacy WHERE request_id=?1";
    let f = Fixture::new();
    for (read, class) in
        [(ReadProvenance::PrivateRead, "restricted_private"), (ReadProvenance::UnattestedRead, "unknown_taint")]
    {
        let id = f.aceitar().unwrap();
        qualify(&f, &id, owner_only(), read);
        qualify(&f, &id, owner_only(), ReadProvenance::AttestedInSpace);
        let mut db = f.hub.store.db.locked();
        assert_eq!(db.pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0)).unwrap(), 1);
        migrate(&db).unwrap();
        for sql in [
            "UPDATE donna_session_privacy SET privacy_class='public_in_space' WHERE request_id=?1",
            "UPDATE donna_session_privacy SET body_disposition='delivered' WHERE request_id=?1",
        ] {
            let err = db.execute(sql, [&id]).unwrap_err();
            assert_eq!(err.to_string(), "privacy_class_downgrade_forbidden");
        }
        for sql in [
            "UPDATE donna_session_privacy SET audience_json='{}' WHERE request_id=?1",
            "UPDATE donna_session_privacy SET owner_epoch=0 WHERE request_id=?1",
            "UPDATE donna_session_privacy SET policy_revision=0 WHERE request_id=?1",
            "UPDATE donna_session_privacy SET delivery_state='revoked_purged' WHERE request_id=?1",
        ] {
            assert!(db.execute(sql, [&id]).is_err());
        }
        let actual: String = db
            .query_row("SELECT privacy_class FROM donna_session_privacy WHERE request_id=?1", [&id], |r| r.get(0))
            .unwrap();
        assert_eq!(actual, class);
        let transition = "UPDATE donna_session_privacy SET privacy_class=?2 WHERE request_id=?1";
        if class == "unknown_taint" {
            assert_eq!(
                db.execute(transition, params![id, "restricted_private"]).unwrap_err().to_string(),
                "privacy_class_downgrade_forbidden"
            );
        } else {
            assert_eq!(db.execute(transition, params![id, "unknown_taint"]).unwrap(), 1);
        }

        let delete = "DELETE FROM donna_session_privacy WHERE request_id=?1";
        assert_eq!(db.execute(delete, [&id]).unwrap_err().to_string(), "privacy_row_delete_forbidden");
        assert_eq!(
            db.execute(REPLACE, params![id, "public_in_space"]).unwrap_err().to_string(),
            "privacy_row_delete_forbidden"
        );

        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        assert_eq!(revoke_matching_na(&tx, RevokeScope::Request(&id)).unwrap(), 1);
        tx.commit().unwrap();
        let snapshot = || -> (String, String, i64) {
            db.query_row(
                "SELECT privacy_class,delivery_state,revoked_at FROM donna_session_privacy WHERE request_id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        let revoked = snapshot();
        assert_eq!((revoked.0.as_str(), revoked.1.as_str()), ("unknown_taint", "revoked_purged"));
        assert!(revoked.2 > 0);
        assert_eq!(db.execute(delete, [&id]).unwrap_err().to_string(), "privacy_row_delete_forbidden");
        assert_eq!(
            db.execute(REPLACE, params![id, "unknown_taint"]).unwrap_err().to_string(),
            "privacy_row_delete_forbidden"
        );
        assert_eq!(snapshot(), revoked);
    }
}

#[test]
fn p05_revogacao_atomica_fecha_fila_callback_e_efeito() {
    let f = Fixture::new();
    let running = f.aceitar().unwrap();
    let queued = f.aceitar().unwrap();
    let other = f.aceitar().unwrap();
    for id in [&running, &queued] {
        qualify(&f, id, owner_only(), ReadProvenance::PrivateRead);
    }
    qualify(
        &f,
        &other,
        AudienceGrant::OwnerOnly { space_id: PERSONAL.into(), owner_principal_id: OTHER.into(), owner_epoch: 3 },
        ReadProvenance::PrivateRead,
    );
    let fence = despachar(&f.hub.store, &running).unwrap().unwrap();
    let guard = EffectFenceCheck { request_id: &running, fence, authority: AUTH };
    let mut db = f.hub.store.db.locked();
    let scope = RevokeScope::OwnerEpoch { owner_principal_id: OWNER, owner_epoch: 3 };
    let revoked_at = |c: &Connection, id: &str| -> i64 {
        c.query_row("SELECT revoked_at FROM donna_session_privacy WHERE request_id=?1", [id], |r| r.get(0)).unwrap()
    };
    {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        assert!(guard.revalidate_na(&tx, Some(AUTH)).is_ok());
        assert!(guard.revalidate_na(&tx, None).is_err());
        for current in [
            EffectAuthority { hub_generation: 8, ..AUTH },
            EffectAuthority { owner_epoch: 4, ..AUTH },
            EffectAuthority { policy_revision: 3, ..AUTH },
        ] {
            assert!(guard.revalidate_na(&tx, Some(current)).is_err());
        }
        assert!(EffectFenceCheck { fence: fence - 1, ..guard }.revalidate_na(&tx, Some(AUTH)).is_err());
        assert_eq!(revoke_matching_na(&tx, scope).unwrap(), 2);
        assert!(guard.revalidate_na(&tx, Some(AUTH)).is_err());
        assert!(revoked_at(&tx, &queued) > 0);
        tx.rollback().unwrap();
    }
    assert_eq!(revoked_at(&db, &queued), 0);
    let scope = RevokeScope::OwnerEpoch { owner_principal_id: OWNER, owner_epoch: 3 };
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    assert_eq!(revoke_matching_na(&tx, scope).unwrap(), 2);
    tx.commit().unwrap();
    assert!(revoked_at(&db, &running) > 0);
    assert_eq!(revoked_at(&db, &running), revoked_at(&db, &queued));
    assert_eq!(revoked_at(&db, &other), 0);
    let state: String = db
        .query_row("SELECT delivery_state FROM donna_session_privacy WHERE request_id=?1", [&running], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "revoked_purged");
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    assert!(guard.revalidate_na(&tx, Some(AUTH)).is_err());
    tx.commit().unwrap();
    drop(db);
    assert_eq!(f.estado(&running), ("cancelled".into(), Some("cancelled".into()), fence + 1));
    assert_eq!(f.estado(&queued), ("cancelled".into(), Some("cancelled".into()), 2));
    assert_eq!(f.estado(&other).0, "accepted");
    assert_eq!(despachar(&f.hub.store, &queued).unwrap(), None);
    let late = f.termino(&running, fence, MotivoFim::FimDoTurno, Conteudo::Entrada("tardia".into()));
    assert_eq!(
        registrar_fim(&f.hub.store, &running, "b", Andamento::EmExecucao(fence), Fim::Turno(late.fim)),
        Registro::Decidido
    );
    assert_eq!(f.resultados(), 0);
}

pub(crate) fn pedido_para_callback_tardio(store: &Store) -> i64 {
    use crate::LockExt;
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    tx.execute(
        "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,
           requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,
           admission_id,origin_entry_id,target_entry_id,attempt,fence,state,created_at,updated_at)
         VALUES('late',1,'inter_bot','wait','source','source','source','ref','bot','bot','bot','chain','admission',
           'origin','target',1,2,'running',0,0)",
        [],
    )
    .unwrap();
    let mut taint = ExecutionTaint::new(OWNER.into(), owner_only());
    taint.observe(ReadProvenance::PrivateRead);
    taint.qualify_na(&tx, "late", AUTH).unwrap();
    tx.commit().unwrap();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    assert_eq!(revoke_matching_na(&tx, RevokeScope::Request("late")).unwrap(), 1);
    tx.commit().unwrap();
    2
}

#[test]
fn p06_audiencia_filtra_antes_do_limite_e_da_contagem() {
    let f = Fixture::new();
    for _ in 0..=pendentes::LIMITE {
        let id = f.aceitar().unwrap();
        qualify(&f, &id, explicit(), ReadProvenance::UnattestedRead);
        assert!(encerrar(&f.hub.store, &id, Decisao::Prazo).unwrap());
    }
    let db = f.hub.store.db.locked();
    let roster = ["a", "b"];
    let filtro = pendentes::Filtro { desde: 0, limite: pendentes::LIMITE, manter: &|_| true };
    let outside = ReadAuthority { space_id: "outro-space", ..caller(Some(MEMBER)) };
    let unavailable = ReadAuthority { control: ControlAuthority::Unavailable, ..caller(Some(MEMBER)) };
    for auth in [None, Some(&caller(None)), Some(&caller(Some(OTHER))), Some(&outside), Some(&unavailable)] {
        let view = pendentes::ler_autorizado_na(&db, &filtro, auth).unwrap();
        assert!(view.linhas.is_empty());
        assert_eq!(view.total, 0);
    }
    let authorized = pendentes::ler_autorizado_na(&db, &filtro, Some(&caller(Some(MEMBER)))).unwrap();
    assert_eq!(authorized.total, pendentes::LIMITE + 1);
    assert_eq!(authorized.linhas.len(), pendentes::LIMITE);
    let Observation::Available { value, .. } = pendentes::torre_na(&db, PERSONAL, &roster).unwrap() else { panic!() };
    assert!(value.items.is_empty());
    assert_eq!(value.omitted, 0);
    assert!(matches!(pendentes::torre_na(&db, outside.space_id, &roster).unwrap(), Observation::Unavailable));
}

#[test]
fn p07_reinicio_invalida_geracao_sem_reenvio_ou_resultado() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    qualify(&f, &id, owner_only(), ReadProvenance::PrivateRead);
    let fence = despachar(&f.hub.store, &id).unwrap().unwrap();
    let check = EffectFenceCheck { request_id: &id, fence, authority: AUTH };
    let next = EffectAuthority { hub_generation: AUTH.hub_generation + 1, ..AUTH };
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    assert!(check.revalidate_na(&tx, Some(next)).is_err());
    assert_eq!(recuperar_na(&tx).unwrap(), 1);
    assert!(check.revalidate_na(&tx, Some(AUTH)).is_err());
    tx.commit().unwrap();
    drop(db);
    assert_eq!(f.estado(&id), ("interrupted".into(), Some("restart_uncertain".into()), fence + 1));
    assert_eq!(despachar(&f.hub.store, &id).unwrap(), None);
    assert_eq!(
        concluir(&f.hub.store, &f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Nenhum)).unwrap(),
        Conclusao::Vencida
    );
    assert_eq!(f.resultados(), 0);
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in dir.read_dir().unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs" || ext == "sql") {
            out.push(path);
        }
    }
}

fn statements<'a>(source: &'a str, strings: &Regex) -> impl Iterator<Item = &'a str> {
    strings.find_iter(source).flat_map(|m| m.as_str().split(';'))
}

#[test]
fn p08_tripwire_sql_e_saida_de_eventos() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = Vec::new();
    rust_sources(&root, &mut paths);
    let tables = r"donna_session_(?:requests|results|privacy)\b";
    let target = format!(r"(?:\w+\s*\.\s*)?{tables}");
    let write = Regex::new(&format!(
        r"(?i)\b(?:insert(?:\s+or\s+\w+)?\s+into|replace\s+into|update(?:\s+or\s+\w+)?|delete\s+from)\s+{target}"
    ))
    .unwrap();
    let from = Regex::new(&format!(r"(?i)\b(?:from|join)\s+{target}")).unwrap();
    let body_column =
        Regex::new(r"\b(?:content|final_entry_id|body_\w+|payload_\w+|content_\w+|body|payload)\b").unwrap();
    let projection_tokens = Regex::new(r"'(?:''|[^'])*'|\b(?:select|from)\b|[()]").unwrap();
    let body_select = |sql: &str| {
        let mut depth = 0_usize;
        let mut selects = Vec::new();
        for token in projection_tokens.find_iter(sql) {
            match token.as_str() {
                "(" => depth += 1,
                ")" => {
                    depth = depth.saturating_sub(1);
                    selects.retain(|&(level, _)| level <= depth);
                }
                "select" => selects.push((depth, token.end())),
                "from" => {
                    if let Some(&(level, start)) = selects.last()
                        && level == depth
                    {
                        selects.pop();
                        let columns = sql[start..token.start()].replace("body_disposition", "disposition");
                        if body_column.is_match(&columns)
                            || columns.trim().trim_start_matches("distinct ").split(',').any(|column| {
                                let column = column.trim();
                                column == "*" || column.ends_with(".*")
                            })
                        {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    };
    let sql_comments = Regex::new(r"(?s)/\*.*?\*/|--[^\n]*").unwrap();
    let strings = Regex::new(r##"(?s)r#+".*?"#+|"(?:\\.|[^"\\])*""##).unwrap();
    let quoted = Regex::new(r#"["`\[\]]|\\\s*"#).unwrap();
    let normalize = |sql: &str| quoted.replace_all(&sql_comments.replace_all(sql, " "), "").to_ascii_lowercase();
    let own = Path::new("fork/retornos");
    let legacy_writers = [
        "fork/cadeias/descendencia_rotina_testes.rs",
        "fork/control_tower/delegated_permissions_tests.rs",
        "fork/delegacao/pausa.rs",
        "fork/teto/tests.rs",
    ];
    for path in paths {
        let relative = path.strip_prefix(&root).unwrap();
        if relative == own.join("provas_privacidade_testes.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let legacy = legacy_writers.iter().any(|name| relative == Path::new(name));
        let production = if relative == Path::new("fork/delegacao/pausa.rs") {
            source.split_once("#[cfg(test)]\nmod tests {").map_or(source.as_str(), |(production, _)| production)
        } else if legacy {
            ""
        } else {
            &source
        };
        let is_sql = path.extension().is_some_and(|ext| ext == "sql");
        let read_statements = |source: &str| {
            if is_sql {
                source.split(';').map(normalize).collect::<Vec<_>>()
            } else {
                statements(source, &strings).map(normalize).collect()
            }
        };
        if !relative.starts_with(own) {
            assert!(
                !read_statements(production).iter().any(|sql| write.is_match(sql)),
                "escritor fora da fronteira: {}",
                relative.display()
            );
        }
        if relative == own.join("privacidade.rs") {
            continue;
        }
        let legacy_read_fixture = relative == own.join("testes.rs")
            || relative == own.join("consulta_testes.rs")
            || relative == Path::new("chat/team/tests.rs");
        for sql in read_statements(&source) {
            if legacy_read_fixture
                && matches!(
                    sql.trim(),
                    "select content,final_entry_id,producer_execution_ref from donna_session_results"
                        | "select content,final_entry_id from donna_session_results"
                        | "select outcome,end_reason,content,final_entry_id from donna_session_results where request_id=?1"
                )
            {
                continue;
            }
            if relative == own.join("consulta_testes.rs")
                && sql
                    .contains("union select final_entry_id from donna_session_results where final_entry_id is not null")
            {
                let rest = sql.replace(
                    "union select final_entry_id from donna_session_results where final_entry_id is not null",
                    "",
                );
                assert!(!body_select(&rest));
                continue;
            }
            assert!(
                !(from.is_match(&sql) && body_select(&sql)),
                "leitura de corpo fora do portão: {}",
                relative.display()
            );
        }
    }

    let privacy_production = std::fs::read_to_string(root.join(own).join("privacidade.rs")).unwrap();
    let class_assignment = Regex::new(r"\bself\.class\s*=\s*[^=\s]").unwrap();
    assert_eq!(class_assignment.find_iter(&privacy_production).count(), 1);
    for method in ["set_class", "set_disposition", "downgrade", "declassify"] {
        assert!(!privacy_production.contains(&format!("fn {method}(")));
    }

    for sql in [
        "INSERT OR REPLACE INTO main.[donna_session_privacy] (request_id) VALUES ('r')",
        "UPDATE /* comentário */ `donna_session_requests` SET state='cancelled'",
        "DELETE FROM donna_session_results",
    ] {
        assert!(write.is_match(&normalize(sql)));
    }
    for sql in [
        "SELECT s.* FROM donna_session_results s",
        "SELECT r.id, s.final_entry_id FROM donna_session_requests r JOIN donna_session_results s ON s.request_id=r.id",
        "SELECT payload_text FROM donna_session_results",
        "SELECT (SELECT COUNT(*) FROM donna_session_results), r.final_entry_id FROM donna_session_results r",
    ] {
        assert!(from.is_match(&normalize(sql)) && body_select(&normalize(sql)));
    }
    assert!(!body_select(&normalize(
        "SELECT (SELECT COUNT(*) FROM donna_session_results), r.id FROM donna_session_requests r"
    )));

    // O broadcast legado de Hub::set_entry fica para uma integração dedicada.
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    let mut events = f.hub.events.subscribe();
    qualify(&f, &id, explicit(), ReadProvenance::PrivateRead);
    let fence = despachar(&f.hub.store, &id).unwrap().unwrap();
    let end = f.termino(&id, fence, MotivoFim::FimDoTurno, Conteudo::Nenhum);
    assert_eq!(
        registrar_fim(&f.hub.store, &id, "b", Andamento::EmExecucao(fence), Fim::Turno(end.fim)),
        Registro::CorpoRetido
    );
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    assert_eq!(revoke_matching_na(&tx, RevokeScope::Request(&id)).unwrap(), 1);
    tx.commit().unwrap();
    assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
}

#[test]
fn p09_sink_compativel_herda_taint_e_sink_mais_amplo_e_recusado() {
    let grant = owner_only();
    for read in [ReadProvenance::PrivateRead, ReadProvenance::UnattestedRead] {
        let mut taint = ExecutionTaint::new(OWNER.into(), grant.clone());
        taint.observe(read);
        let inherited = SinkGuard::Qualified {
            class: PrivacyClass::RestrictedPrivate,
            owner_principal_id: OWNER,
            audience: &grant,
        }
        .authorize(&taint)
        .unwrap();
        assert_eq!(inherited.class(), taint.class());
        assert_eq!(inherited.audience(), &grant);
        assert_eq!(inherited.disposition(), BodyDisposition::BodyRetained);
        for sink in [
            SinkGuard::Unqualified,
            SinkGuard::Qualified { class: PrivacyClass::PublicInSpace, owner_principal_id: OWNER, audience: &grant },
            SinkGuard::Qualified {
                class: PrivacyClass::RestrictedPrivate,
                owner_principal_id: OWNER,
                audience: &explicit(),
            },
            SinkGuard::Qualified {
                class: PrivacyClass::RestrictedPrivate,
                owner_principal_id: OTHER,
                audience: &grant,
            },
        ] {
            assert_eq!(sink.authorize(&taint).err().unwrap().to_string(), "taint_sink_denied");
        }
    }
}

#[test]
fn p10_owner_direct_view_exige_dono_e_autoridade_corrente() {
    let f = Fixture::new();
    let id = f.aceitar().unwrap();
    qualify(&f, &id, explicit(), ReadProvenance::PrivateRead);
    let q = load(&f.hub.store.db.locked(), &id).unwrap().unwrap();
    let read = |auth: &ReadAuthority<'_>| q.read_authorized(BodyReaderAllowlist::OwnerDirectView, Some(auth));
    assert_eq!(read(&caller(Some(OWNER))).unwrap(), ReadAccess::OwnerBody);
    for auth in [
        caller(Some(MEMBER)),
        caller(None),
        ReadAuthority { bot_id: Some("a"), ..caller(Some(OWNER)) },
        ReadAuthority { control: ControlAuthority::Unavailable, ..caller(Some(OWNER)) },
        ReadAuthority { space_id: "outro-space", ..caller(Some(OWNER)) },
        ReadAuthority { owner_epoch: 2, ..caller(Some(OWNER)) },
        ReadAuthority { policy_revision: 1, ..caller(Some(OWNER)) },
    ] {
        assert!(read(&auth).is_err());
    }
}
