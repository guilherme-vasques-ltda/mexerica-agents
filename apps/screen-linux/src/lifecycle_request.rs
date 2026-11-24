//! Preparação ordenada do lifecycle e resposta individual correlacionada Linux.
use crate::{Helper, lifecycle, stream};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::sync::Arc;

#[cfg(test)]
// Only lifecycle actions are synchronous; input/portal never run under its locks.
pub(crate) fn prepare(
    h: &Helper,
    method: &str,
    p: &Value,
) -> Result<Option<Arc<lifecycle::Operation>>> {
    Ok(prepare_rpc(h, method, p, None)?.operation)
}

/// Reader captures this before spawn; a close result is never looked up again
/// by String in the asynchronous handler.
pub(crate) struct PreparedRpc {
    operation: Option<Arc<lifecycle::Operation>>,
    result: Option<Value>,
}

pub(crate) fn prepare_rpc(
    h: &Helper,
    method: &str,
    p: &Value,
    rpc_id: Option<i64>,
) -> Result<PreparedRpc> {
    // Missing means legacy; present but invalid must fail before any action.
    let expected = match p.get("answerId") {
        Some(value) if matches!(method, "answer" | "close") => Some(
            value
                .as_i64()
                .ok_or(crate::native_stop::NativeStopError::Lifecycle)?,
        ),
        _ => None,
    };
    let mut prepared = PreparedRpc {
        operation: None,
        result: None,
    };
    match method {
        "answer" => {
            prepared.operation = Some(h.sessions.admit_answer(
                p["session"].as_str().context("session is required")?,
                rpc_id,
                expected,
            )?);
        }
        "close" => {
            let id = p["session"].as_str().context("session is required")?;
            prepared.result = Some(if let Some(expected) = expected {
                match h.sessions.close_correlated(id, expected)? {
                    Some(answer) => json!({"nativeStop":{"answerId":answer,"state":"null"}}),
                    None => json!({}),
                }
            } else {
                h.sessions.close(id)?;
                json!({})
            });
        }
        "closeAll" => {
            h.sessions.close_all(false)?;
            prepared.result = Some(json!({}));
        }
        _ => {}
    }
    Ok(prepared)
}

pub(crate) async fn respond(
    h: &Arc<Helper>,
    method: &str,
    p: &Value,
    id: Value,
    prepared: Result<PreparedRpc>,
    notice_epoch: u64,
) -> Value {
    let result = match prepared {
        Ok(prepared) => match prepared.result {
            Some(result) => Ok(result),
            None => crate::handle(h, method, p, Ok(prepared.operation), notice_epoch).await,
        },
        Err(error) => Err(error),
    };
    match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":format!("{error:#}")}})
        }
    }
}

pub(crate) async fn answer(
    h: &Arc<Helper>,
    p: &Value,
    operation: Arc<lifecycle::Operation>,
) -> Result<Value> {
    let id = p["session"].as_str().context("session is required")?;
    let mut attempt = lifecycle::Attempt {
        sessions: &h.sessions,
        id,
        operation: &operation,
        completed: false,
    };
    let answer = async {
        let sdp = p["sdp"].as_str().context("sdp is required")?;
        let session = if let Some(session) = operation.session()? {
            session
        } else {
            let portal = h.portal().await?;
            let display = portal.display(p["display"].as_u64())?;
            let fd = portal.pipewire_fd().await?;
            let session = Arc::new(stream::Session::new(
                id.to_owned(),
                &display,
                fd,
                h.clone(),
                p,
                &operation,
            )?);
            operation.install(session.clone())?;
            session
        };
        let answer = session.answer(sdp, &operation).await?;
        Ok::<_, anyhow::Error>(json!({"sdp": answer}))
    };
    let result = operation.negotiate(&h.sessions, id, answer).await;
    if result.is_ok() {
        attempt.completed = true;
    }
    // The projection never includes SDP/ICE, native errors or portal details.
    result.map_err(|_| anyhow!("native_answer_unknown"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    pub(super) fn helper() -> Arc<Helper> {
        let (out, _) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(Helper {
            config: crate::fork_socket::Config::from_args(
                ["--socket", "/tmp/screen-linux-fixture/screen.sock"].map(std::ffi::OsString::from),
            )
            .unwrap(),
            portal: Arc::new(tokio::sync::Mutex::new(None)),
            status: tokio::sync::Mutex::default(),
            sessions: lifecycle::Sessions::default(),
            out,
            rt: tokio::runtime::Handle::current(),
        })
    }

    #[tokio::test]
    async fn close_before_answer_admission_is_legacy_control() {
        let h = helper();
        let params = json!({"session": "prepared"});
        // Hub prepare has reserved an id, but no answer has reached the helper.
        let prepared = prepare(&h, "close", &params);
        assert_eq!(
            crate::handle(&h, "close", &params, prepared, 0)
                .await
                .unwrap(),
            json!({})
        );

        // Reader-order answer admission changes the meaning of absence: an
        // existing pending operation without a pipeline must still be unknown.
        let operation = prepare(&h, "answer", &params).unwrap().unwrap();
        let prepared = prepare(&h, "close", &params);
        let error = crate::handle(&h, "close", &params, prepared, 0)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "native_stop_absent");
        assert!(operation.session().is_err());
        assert!(Arc::ptr_eq(
            &h.sessions.admit("prepared").unwrap(),
            &operation
        ));
    }

    #[tokio::test]
    async fn close_all_empty_is_legacy_control() {
        let h = helper();
        let params = json!({});
        let prepared = prepare(&h, "closeAll", &params);
        assert_eq!(
            crate::handle(&h, "closeAll", &params, prepared, 0)
                .await
                .unwrap(),
            json!({})
        );

        // Empty control is not a blanket conversion of pending/native errors.
        let operation = prepare(&h, "answer", &json!({"session": "pending"}))
            .unwrap()
            .unwrap();
        let prepared = prepare(&h, "closeAll", &params);
        let error = crate::handle(&h, "closeAll", &params, prepared, 0)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "native_stop_absent");
        assert!(operation.session().is_err());
    }
}

#[cfg(test)]
mod correlation_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::native_stop::tests::live_pipeline;
    use gst::prelude::*;

    use super::tests::helper;

    fn admit(h: &Helper, id: Option<i64>, expected: Value) -> Arc<lifecycle::Operation> {
        let mut p = json!({"session":"same"});
        if expected != json!({}) {
            p["answerId"] = expected;
        }
        prepare_rpc(h, "answer", &p, id).unwrap().operation.unwrap()
    }

    #[tokio::test]
    async fn reader_origin_restart_and_null_envelope() {
        let h = helper();
        let original = admit(&h, Some(7), json!({}));
        let pipeline = live_pipeline();
        original.attach_pipeline(&pipeline).unwrap();
        let session = Arc::new(stream::Session::for_test(pipeline.clone()));
        original.install(session.clone()).unwrap();
        original
            .negotiate(&h.sessions, "same", async { Ok(()) })
            .await
            .unwrap();
        let restart = admit(&h, Some(8), json!(7));
        assert!(Arc::ptr_eq(&original, &restart));
        assert!(Arc::ptr_eq(&restart.session().unwrap().unwrap(), &session));
        assert!(original.with_pipeline(&pipeline, || Ok(())).is_ok());
        let params = json!({"session":"same","answerId":7});
        let prepared = prepare_rpc(&h, "close", &params, Some(9));
        let reply = respond(&h, "close", &params, json!(9), prepared, 0).await;
        assert_eq!(
            reply,
            json!({"jsonrpc":"2.0","id":9,"result":{"nativeStop":{"answerId":7,"state":"null"}}})
        );
        assert_eq!(pipeline.current_state(), gst::State::Null);
        assert!(
            prepare_rpc(&h, "answer", &params, Some(10)).is_err(),
            "restart cannot recreate A"
        );
        assert_eq!(
            respond(
                &h,
                "close",
                &params,
                json!(11),
                prepare_rpc(&h, "close", &params, Some(11)),
                0,
            )
            .await["result"],
            json!({}),
            "no positive replay"
        );
    }

    #[tokio::test]
    async fn opening_invalid_and_legacy_never_confirm() {
        for stage in 0..3 {
            let h = helper();
            let original = admit(&h, Some(7), json!({}));
            let pipeline = live_pipeline();
            if stage > 0 {
                original.attach_pipeline(&pipeline).unwrap();
            }
            if stage == 2 {
                original
                    .install(Arc::new(stream::Session::for_test(pipeline.clone())))
                    .unwrap();
            }
            let p = json!({"session":"same","answerId":7});
            let result = prepare_rpc(&h, "close", &p, Some(9));
            if stage == 0 {
                assert!(result.is_err());
                assert!(original.attach_pipeline(&pipeline).is_err());
            } else {
                assert_eq!(result.unwrap().result.unwrap(), json!({}));
            }
            assert_eq!(pipeline.current_state(), gst::State::Null);
            assert!(h.sessions.register("same", &original).is_err());
        }
        for invalid in [json!(null), json!("7"), json!(7.0), json!(false), json!(8)] {
            let h = helper();
            let original = admit(&h, Some(7), json!({}));
            let pipeline = live_pipeline();
            original.attach_pipeline(&pipeline).unwrap();
            h.sessions.register("same", &original).unwrap();
            let p = json!({"session":"same","answerId":invalid});
            assert!(prepare_rpc(&h, "close", &p, Some(9)).is_err());
            assert!(prepare_rpc(&h, "answer", &p, Some(8)).is_err());
            assert_eq!(pipeline.current_state(), gst::State::Playing);
            h.sessions.close("same").unwrap();
        }
        for rpc_id in [json!("7"), json!(null), json!(7.0)] {
            let h = helper();
            let original = admit(&h, rpc_id.as_i64(), json!({}));
            let pipeline = live_pipeline();
            original.attach_pipeline(&pipeline).unwrap();
            h.sessions.register("same", &original).unwrap();
            let p = json!({"session":"same", "answerId":7});
            let reply = respond(
                &h,
                "close",
                &p,
                json!(9),
                prepare_rpc(&h, "close", &p, Some(9)),
                0,
            )
            .await;
            assert_eq!(reply["error"]["message"], "native_stop_lifecycle");
            assert_eq!(pipeline.current_state(), gst::State::Playing);
            h.sessions.close("same").unwrap();
        }
        for id in [None, Some(7)] {
            let h = helper();
            let original = admit(&h, id, json!({}));
            let pipeline = live_pipeline();
            original.attach_pipeline(&pipeline).unwrap();
            h.sessions.register("same", &original).unwrap();
            let p = json!({"session":"same"});
            assert_eq!(
                prepare_rpc(&h, "close", &p, Some(9))
                    .unwrap()
                    .result
                    .unwrap(),
                json!({})
            );
            assert_eq!(pipeline.current_state(), gst::State::Null);
        }
    }
}
