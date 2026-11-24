use super::*;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

fn fixture(name: &str) -> Value {
    let data: Value = serde_json::from_str(include_str!("../../../fixtures/mail_read_offline_vectors.json")).unwrap();
    data["vectors"].as_array().unwrap().iter().find(|v| v["name"] == name).unwrap().clone()
}

#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
    mode: Mode,
    entered: Notify,
    resume: Notify,
    cancellation: Mutex<Option<CancellationToken>>,
    before_reply: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[derive(Default)]
enum Mode {
    #[default]
    Normal,
    Error,
    Timeout,
    Late,
}

fn date(s: &Value) -> DateTime<Utc> {
    s.as_str().unwrap().parse().unwrap()
}

impl ReadTransport for Fake {
    async fn list(
        &self,
        page_token: Option<&str>,
        limit: usize,
        cancel: CancellationToken,
    ) -> Result<ProviderPage, TransportFailure> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(limit, PAGE_SIZE);
        match self.mode {
            Mode::Error => {
                assert!(fixture("provider_error")["illustrative_provider_response"]["error"].is_string());
                return Err(TransportFailure);
            }
            Mode::Timeout => {
                *self.cancellation.lock().unwrap() = Some(cancel);
                std::future::pending::<()>().await;
            }
            Mode::Late => {
                *self.cancellation.lock().unwrap() = Some(cancel);
                self.entered.notify_one();
                self.resume.notified().await;
                if let Some(change) = self.before_reply.lock().unwrap().take() {
                    change();
                }
            }
            Mode::Normal => {}
        }
        let name = match page_token {
            None => "list_page_one",
            Some("illustrative-provider-page-token") => "list_page_two",
            Some(_) => return Err(TransportFailure),
        };
        let vector = fixture(name);
        let response = &vector["illustrative_provider_response"];
        Ok(ProviderPage {
            threads: response["threads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| ProviderRow {
                    id: row["threadId"].as_str().unwrap().to_owned(),
                    subject: row["subject"].as_str().map(str::to_owned),
                    last_message_at: date(&row["lastMessageAt"]),
                })
                .collect(),
            next_page_token: response["pageToken"].as_str().map(str::to_owned),
        })
    }

    async fn open(&self, thread_id: &str, cancel: CancellationToken) -> Result<ProviderThread, TransportFailure> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let vector = fixture(if matches!(self.mode, Mode::Late) {
            *self.cancellation.lock().unwrap() = Some(cancel);
            self.entered.notify_one();
            self.resume.notified().await;
            "late_response_after_generation_change"
        } else {
            "open_thread"
        });
        let response = &vector["illustrative_provider_response"];
        assert!(response["threadId"] == thread_id);
        Ok(ProviderThread {
            subject: None,
            messages: vec![ProviderMessage {
                sent_at: DateTime::UNIX_EPOCH,
                body_text: format!("{}\u{202e}", response["bodyText"].as_str().unwrap()),
            }],
        })
    }
}

fn context() -> Context {
    Context {
        person: Uuid::new_v4(),
        device: Uuid::new_v4(),
        workspace: Uuid::new_v4(),
        account: Uuid::new_v4(),
        installation: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    }
}

fn evidence(context: &Context) -> VerifiedState {
    VerifiedState {
        context: context.clone(),
        can_view: true,
        definition: Definition { protocol_version: "illustrative-v1".into(), schema_digest: [5; 32] },
        effective_scopes: Some(vec![READONLY.to_owned()]),
    }
}

fn setup(mode: Mode) -> (Context, Reader<Fake>) {
    let context = context();
    let state = evidence(&context);
    let reader =
        Reader::new(context.account, context.installation, state.definition.clone(), Fake { mode, ..Fake::default() });
    reader.replace_verified_state(Some(state)).unwrap();
    (context, reader)
}

#[tokio::test]
async fn lists_pages_and_opens_with_opaque_references() {
    let (context, reader) = setup(Mode::Normal);
    let first = reader.list(&context, None).await.unwrap();
    let expected = fixture("list_page_one");
    let expected = expected["expected_projection"].as_array().unwrap();
    assert_eq!(first.threads.len(), expected.len());
    for (row, expected) in first.threads.iter().zip(expected.iter().rev()) {
        assert_eq!(row.thread_id.get_version_num(), 4);
        assert_eq!(row.subject.as_deref(), expected["subject"].as_str());
        assert_eq!(row.last_message_at, date(&expected["lastMessageAt"]));
    }
    let cursor = first.next_cursor.unwrap();
    assert_eq!(cursor.get_version_num(), 4);
    let second = reader.list(&context, Some(cursor)).await.unwrap();
    let expected = fixture("list_page_two");
    assert_eq!(second.threads.len(), 1);
    assert_eq!(second.threads[0].last_message_at, date(&expected["expected_projection"][0]["lastMessageAt"]));
    assert!(second.next_cursor.is_none());
    let id = first.threads[1].thread_id;
    let opened = reader.open(&context, id).await.unwrap();
    assert_eq!(opened.thread_id, id);
    assert_eq!(opened.messages.len(), 1);
    assert!(
        opened.messages[0].body_text
            == fixture("open_thread")["illustrative_provider_response"]["bodyText"].as_str().unwrap()
    );
    assert!(!opened.truncated);
    assert_eq!(reader.transport.calls.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn invalid_evidence_and_crossed_references_never_dispatch() {
    let (context, reader) = setup(Mode::Normal);
    reader.replace_verified_state(None).unwrap();
    assert!(matches!(reader.list(&context, None).await, Err(ReadError::Denied)));
    for edit in [
        |s: &mut VerifiedState| s.can_view = false,
        |s: &mut VerifiedState| s.context.person = Uuid::new_v4(),
        |s: &mut VerifiedState| s.context.device = Uuid::new_v4(),
        |s: &mut VerifiedState| s.context.workspace = Uuid::new_v4(),
        |s: &mut VerifiedState| s.context.account = Uuid::new_v4(),
        |s: &mut VerifiedState| s.context.installation = Uuid::new_v4(),
        |s: &mut VerifiedState| s.context.generation = Uuid::new_v4(),
        |s: &mut VerifiedState| {
            assert!(fixture("effective_scope_missing")["effective_scope_evidence"].is_null());
            s.effective_scopes = None;
        },
        |s: &mut VerifiedState| s.effective_scopes.as_mut().unwrap().push("gmail.modify".into()),
    ] as [fn(&mut VerifiedState); 9]
    {
        let mut state = evidence(&context);
        edit(&mut state);
        reader.replace_verified_state(Some(state)).unwrap();
        assert!(matches!(reader.list(&context, None).await, Err(ReadError::Denied)));
    }
    for version_mismatch in [false, true] {
        let mut state = evidence(&context);
        if version_mismatch {
            state.definition.protocol_version.push('2');
        } else {
            state.definition.schema_digest[0] ^= 1;
        }
        reader.replace_verified_state(Some(state)).unwrap();
        assert!(matches!(reader.list(&context, None).await, Err(ReadError::Unavailable)));
    }
    assert_eq!(reader.transport.calls.load(Ordering::Relaxed), 0);

    reader.replace_verified_state(Some(evidence(&context))).unwrap();
    let page = reader.list(&context, None).await.unwrap();
    let cursor = page.next_cursor.unwrap();
    let id = page.threads[0].thread_id;
    let mut other = context.clone();
    other.workspace = Uuid::new_v4();
    reader.replace_verified_state(Some(evidence(&other))).unwrap();
    assert!(matches!(reader.open(&other, id).await, Err(ReadError::Denied)));
    assert!(matches!(reader.list(&other, Some(cursor)).await, Err(ReadError::CursorInvalid)));
    assert_eq!(reader.transport.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn projection_keeps_latest_messages_in_chronological_order_with_stable_ties() {
    let projected = project_thread(
        Uuid::new_v4(),
        ProviderThread {
            subject: None,
            messages: (0..=MAX_MESSAGES)
                .rev()
                .map(|i| ProviderMessage {
                    sent_at: DateTime::from_timestamp(i.min(MAX_MESSAGES - 1) as i64, 0).unwrap(),
                    body_text: i.to_string(),
                })
                .collect(),
        },
    )
    .unwrap();
    let expected = (1..MAX_MESSAGES - 1).chain([MAX_MESSAGES, MAX_MESSAGES - 1]).map(|i| i.to_string());
    assert!(projected.truncated);
    assert_eq!(projected.messages.len(), MAX_MESSAGES);
    assert!(projected.messages.iter().map(|m| m.body_text.as_str()).eq(expected));
}

#[test]
fn projection_budgets_newest_body_first_and_cuts_at_utf8_boundary() {
    let newest = "x".repeat(MAX_TEXT_BYTES - 4);
    let projected = project_thread(
        Uuid::new_v4(),
        ProviderThread {
            subject: None,
            messages: vec![
                ProviderMessage { sent_at: DateTime::UNIX_EPOCH, body_text: "éa🙂ท้าย".into() },
                ProviderMessage { sent_at: DateTime::from_timestamp(1, 0).unwrap(), body_text: newest.clone() },
            ],
        },
    )
    .unwrap();
    assert!(projected.truncated);
    assert!(projected.messages[0].body_text == "éa");
    assert!(projected.messages[1].body_text == newest);
    assert_eq!(projected.messages.iter().map(|m| m.body_text.len()).sum::<usize>(), MAX_TEXT_BYTES - 1);
}

#[test]
fn projection_rejects_oversized_input_instead_of_returning_partial_data() {
    let project = |count, body_bytes, subject_bytes| {
        let mut messages = (0..count)
            .map(|_| ProviderMessage { sent_at: DateTime::UNIX_EPOCH, body_text: String::new() })
            .collect::<Vec<_>>();
        messages[0].body_text = "x".repeat(body_bytes);
        project_thread(Uuid::new_v4(), ProviderThread { subject: Some("s".repeat(subject_bytes)), messages })
    };
    assert!(project(100, 4 * MAX_TEXT_BYTES, MAX_TEXT_BYTES).unwrap().truncated);
    for (count, body, subject) in
        [(101, 0, 0), (MAX_MESSAGES + 1, 4 * MAX_TEXT_BYTES + 1, 0), (1, 0, MAX_TEXT_BYTES + 1)]
    {
        assert!(matches!(project(count, body, subject), Err(ReadError::Unavailable)));
    }
}

#[tokio::test(start_paused = true)]
async fn provider_error_and_timeout_are_unavailable() {
    for mode in [Mode::Error, Mode::Timeout] {
        let (context, reader) = setup(mode);
        assert!(matches!(reader.list(&context, None).await, Err(ReadError::Unavailable)));
        if let Some(cancel) = reader.transport.cancellation.lock().unwrap().as_ref() {
            assert!(cancel.is_cancelled());
        }
        assert!(reader.state.lock().unwrap().references.is_empty());
        assert_eq!(reader.transport.calls.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn revocation_drops_inflight_open_and_stale_results_cannot_mint_references() {
    let (context, mut reader) = setup(Mode::Normal);
    let page = reader.list(&context, None).await.unwrap();
    let id = page.threads[1].thread_id;
    reader.transport.mode = Mode::Late;
    let (opened, ()) = tokio::join!(reader.open(&context, id), async {
        reader.transport.entered.notified().await;
        reader.replace_verified_state(None).unwrap();
        reader.replace_verified_state(Some(evidence(&context))).unwrap();
        reader.transport.resume.notify_one();
    });
    assert!(matches!(opened, Err(ReadError::Denied)));
    assert!(reader.transport.cancellation.lock().unwrap().as_ref().unwrap().is_cancelled());
    assert!(reader.state.lock().unwrap().references.is_empty());

    let reader = Arc::new(reader);
    let weak_reader = Arc::downgrade(&reader);
    let restored = evidence(&context);
    *reader.transport.before_reply.lock().unwrap() = Some(Box::new(move || {
        let reader = weak_reader.upgrade().unwrap();
        reader.replace_verified_state(None).unwrap();
        reader.replace_verified_state(Some(restored)).unwrap();
    }));
    let (listed, ()) = tokio::join!(reader.list(&context, None), async {
        reader.transport.entered.notified().await;
        reader.transport.resume.notify_one();
    });
    assert!(matches!(listed, Err(ReadError::Denied)));
    assert!(reader.state.lock().unwrap().references.is_empty());
    assert_eq!(reader.transport.calls.load(Ordering::Relaxed), 3);
}
