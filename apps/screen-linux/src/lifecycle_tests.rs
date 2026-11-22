#![allow(clippy::unwrap_used)]
use super::*;
use crate::native_stop::tests::live_pipeline;
use gst::prelude::*;

#[test]
fn native_stop_old_pipeline_keeps_successor() {
    let sessions = Sessions::default();
    let old = sessions.admit("same").unwrap();
    let old_pipeline = live_pipeline();
    old.attach_pipeline(&old_pipeline).unwrap();
    assert_eq!(sessions.close("same"), Ok(()));
    let successor = sessions.admit("same").unwrap();
    let successor_pipeline = live_pipeline();
    successor.attach_pipeline(&successor_pipeline).unwrap();
    assert_ne!(old_pipeline, successor_pipeline);
    sessions
        .finish("same", &old, || {
            panic!("old callback must not notify successor")
        })
        .unwrap();
    assert!(Arc::ptr_eq(&sessions.admit("same").unwrap(), &successor));
    assert_eq!(old_pipeline.current_state(), gst::State::Null);
    assert_eq!(successor_pipeline.current_state(), gst::State::Playing);
    sessions.close("same").unwrap();
}

#[tokio::test]
async fn close_during_answer_before_registration_is_unknown() {
    let sessions = Arc::new(Sessions::default());
    // Same admission boundary as dispatch, before the request's first poll.
    let operation = sessions.admit("same").unwrap();
    let (created_tx, created_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let pending = operation.clone();
    let pending_sessions = sessions.clone();
    let answer = tokio::spawn(async move {
        let _attempt = Attempt {
            sessions: &pending_sessions,
            id: "same",
            operation: &pending,
            completed: false,
        };
        let pipeline = live_pipeline();
        created_tx.send(pipeline.clone()).unwrap();
        resume_rx.await.unwrap();
        let result = pending
            .attach_pipeline(&pipeline)
            .and_then(|()| pending.register());
        (result, pipeline)
    });
    let original = created_rx.await.unwrap();
    // No native object attached yet: closing pending is explicitly unknown.
    assert_eq!(sessions.close("same"), Err(NativeStopError::Absent));
    resume_tx.send(()).unwrap();
    let (result, pipeline) = answer.await.unwrap();
    assert_eq!(result, Err(NativeStopError::Closed));
    assert_eq!(pipeline, original);
    assert_eq!(pipeline.current_state(), gst::State::Null);
    assert!(!operation.is_registered());
    assert!(Arc::ptr_eq(
        &sessions.state.lock().unwrap().rows["same"],
        &operation
    ));

    // Same production negotiation/Attempt path, now after pipeline attachment
    // but before answer can register it. Channels delimit the cut, no sleeps.
    let operation = sessions.admit("negotiating").unwrap();
    let (created_tx, created_rx) = tokio::sync::oneshot::channel();
    let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let pending_sessions = sessions.clone();
    let pending = operation.clone();
    let answer = tokio::spawn(async move {
        let _attempt = Attempt {
            sessions: &pending_sessions,
            id: "negotiating",
            operation: &pending,
            completed: false,
        };
        pending
            .negotiate(&pending_sessions, "negotiating", async {
                let pipeline = live_pipeline();
                pending.attach_pipeline(&pipeline)?;
                created_tx.send(pipeline).unwrap();
                release_rx.await.unwrap();
                Ok(())
            })
            .await
    });
    let pipeline = created_rx.await.unwrap();
    sessions.close("negotiating").unwrap();
    assert!(answer.await.unwrap().is_err());
    assert_eq!(pipeline.current_state(), gst::State::Null);
    assert!(!operation.is_registered());
    assert!(
        !sessions
            .state
            .lock()
            .unwrap()
            .rows
            .contains_key("negotiating")
    );
}

#[tokio::test]
async fn close_cancels_negotiation_and_link_end_blocks_late_admission() {
    let sessions = Sessions::default();
    let operation = sessions.admit("same").unwrap();
    let pipeline = live_pipeline();
    operation.attach_pipeline(&pipeline).unwrap();
    let cancelled = operation.cancelled();
    sessions.close_all(true).unwrap();
    cancelled.await;
    assert_eq!(pipeline.current_state(), gst::State::Null);
    assert!(sessions.admit("late").is_err());
    assert_eq!(
        operation.with_pipeline(&pipeline, || Ok(())),
        Err(NativeStopError::Closed)
    );
}

#[tokio::test]
async fn failed_answer_stops_original_without_registering() {
    let sessions = Sessions::default();
    let operation = sessions.admit("failed").unwrap();
    let pipeline = live_pipeline();
    operation.attach_pipeline(&pipeline).unwrap();
    {
        let _attempt = Attempt {
            sessions: &sessions,
            id: "failed",
            operation: &operation,
            completed: false,
        };
        assert!(
            operation
                .negotiate(&sessions, "failed", async {
                    Err::<(), _>(anyhow::anyhow!("controlled_failure"))
                })
                .await
                .is_err()
        );
    }
    assert_eq!(pipeline.current_state(), gst::State::Null);
    assert!(!operation.is_registered());
    assert!(sessions.state.lock().unwrap().rows.is_empty());
}

#[test]
fn close_all_marks_every_answer_before_native_stop() {
    gst::init().unwrap();
    let sessions = Arc::new(Sessions::default());
    let first = sessions.admit("first").unwrap();
    let second = sessions.admit("second").unwrap();
    // Controlled real GstPipeline vfunc cuts, independent of HashMap order.
    for operation in [&first, &second] {
        let pipeline: crate::native_stop::tests::FailingPipeline = gst::glib::Object::new();
        pipeline.set_state(gst::State::Ready).unwrap();
        operation.attach_pipeline(pipeline.upcast_ref()).unwrap();
        let sessions = sessions.clone();
        let first = first.clone();
        let second = second.clone();
        pipeline.before_null(move || {
            // Reentry must progress: the map is not locked around native calls.
            // Both registrations must already be denied, even at the FIRST stop.
            assert_eq!(
                sessions.register("first", &first),
                Err(NativeStopError::Closed)
            );
            assert_eq!(
                sessions.register("second", &second),
                Err(NativeStopError::Closed)
            );
        });
    }
    sessions.close_all(false).unwrap();
    assert!(!first.is_registered());
    assert!(!second.is_registered());
    assert!(sessions.state.lock().unwrap().rows.is_empty());
}

#[test]
fn correlated_null_rechecks_object_and_connection() {
    gst::init().unwrap();
    for disconnect in [false, true] {
        let sessions = Arc::new(Sessions::default());
        let original = sessions.admit_answer("same", Some(7), None).unwrap();
        let pipeline: crate::native_stop::tests::FailingPipeline = gst::glib::Object::new();
        pipeline.set_state(gst::State::Ready).unwrap();
        original.attach_pipeline(pipeline.upcast_ref()).unwrap();
        sessions.register("same", &original).unwrap();
        let next = Arc::new(Operation::new());
        let cut_sessions = sessions.clone();
        let cut_next = next.clone();
        pipeline.before_null(move || {
            let mut registry = cut_sessions.state.lock().unwrap();
            registry.disconnected = disconnect;
            registry.rows.insert("same".into(), cut_next);
        });
        assert!(sessions.close_correlated("same", 7).is_err());
        assert!(Arc::ptr_eq(
            &sessions.state.lock().unwrap().rows["same"],
            &next
        ));
        assert_eq!(pipeline.current_state(), gst::State::Null);
    }
    let sessions = Sessions::default();
    assert_eq!(sessions.close_correlated("absent", 7).unwrap(), None);
    let original = sessions.admit_answer("same", Some(7), None).unwrap();
    assert_eq!(
        sessions.close_correlated("same", 8),
        Err(NativeStopError::Lifecycle)
    );
    assert!(original.session().is_ok());
    assert_eq!(
        sessions.close_correlated("same", 7),
        Err(NativeStopError::Absent)
    );
    assert!(original.session().is_err());
    // Um A antigo não pode parar o pipeline vivo de um sucessor B.
    let successor_sessions = Sessions::default();
    let successor = successor_sessions
        .admit_answer("same", Some(8), None)
        .unwrap();
    let successor_pipeline = live_pipeline();
    successor.attach_pipeline(&successor_pipeline).unwrap();
    successor_sessions.register("same", &successor).unwrap();
    assert_eq!(
        successor_sessions.close_correlated("same", 7),
        Err(NativeStopError::Lifecycle)
    );
    assert!(
        successor_sessions
            .admit_answer("same", Some(9), Some(7))
            .is_err()
    );
    assert_eq!(successor_pipeline.current_state(), gst::State::Playing);
    assert!(successor.is_registered());
    assert!(Arc::ptr_eq(
        &successor_sessions.state.lock().unwrap().rows["same"],
        &successor
    ));
    successor_sessions.close("same").unwrap();
}

#[test]
fn correlated_null_failure_never_returns_fact_or_reopens() {
    use gst::glib::subclass::prelude::*;
    use std::sync::atomic::Ordering;
    gst::init().unwrap();
    let sessions = Sessions::default();
    let original = sessions.admit_answer("same", Some(7), None).unwrap();
    let pipeline: crate::native_stop::tests::FailingPipeline = gst::glib::Object::new();
    pipeline.set_state(gst::State::Ready).unwrap();
    original.attach_pipeline(pipeline.upcast_ref()).unwrap();
    sessions.register("same", &original).unwrap();
    pipeline.imp().fail.store(true, Ordering::Relaxed);
    assert_eq!(
        sessions.close_correlated("same", 7),
        Err(NativeStopError::Transition)
    );
    assert!(!original.is_registered());
    assert!(
        original
            .with_pipeline(pipeline.upcast_ref(), || Ok(()))
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &sessions.state.lock().unwrap().rows["same"],
        &original
    ));
    pipeline.imp().fail.store(false, Ordering::Relaxed);
    assert_eq!(
        sessions.close_correlated("same", 7).unwrap(),
        None,
        "retry cannot replay eligibility"
    );
    assert_eq!(pipeline.current_state(), gst::State::Null);
}
