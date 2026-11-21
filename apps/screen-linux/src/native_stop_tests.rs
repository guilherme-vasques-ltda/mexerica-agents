#![allow(clippy::unwrap_used)]
use super::*;
use glib::subclass::prelude::*;
use gst::glib;

pub(crate) fn live_pipeline() -> gst::Pipeline {
    gst::init().unwrap();
    let pipeline = gst::parse::launch("videotestsrc is-live=true ! fakesink sync=false")
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
    pipeline.set_state(gst::State::Playing).unwrap();
    let (result, state, _) = pipeline.state(gst::ClockTime::from_seconds(5));
    result.unwrap();
    assert_eq!(state, gst::State::Playing);
    pipeline
}

#[test]
fn native_stop_same_pipeline_reaches_null() {
    let pipeline = live_pipeline();
    let original = pipeline.clone();
    assert_eq!(stop_pipeline(&pipeline), Ok(()));
    assert_eq!(pipeline, original);
    assert_eq!(original.current_state(), gst::State::Null);
}

#[test]
fn native_stop_error_or_absence_is_unknown() {
    let pipeline = live_pipeline();
    // Controlled native GstPipeline vfunc failure, not a mock stop implementation.
    let failing: FailingPipeline = glib::Object::new();
    failing.set_state(gst::State::Ready).unwrap();
    failing
        .imp()
        .fail
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        stop_pipeline(failing.upcast_ref()),
        Err(NativeStopError::Transition)
    );
    failing
        .imp()
        .fail
        .store(false, std::sync::atomic::Ordering::Relaxed);
    stop_pipeline(failing.upcast_ref()).unwrap();
    assert_eq!(
        checked_null(
            Err(gst::StateChangeError),
            gst::State::Null,
            gst::State::VoidPending
        ),
        Err(NativeStopError::Transition)
    );
    assert_eq!(
        checked_null(
            Ok(gst::StateChangeSuccess::Success),
            pipeline.current_state(),
            gst::State::VoidPending
        ),
        Err(NativeStopError::NotNull)
    );
    let operation = crate::lifecycle::Operation::new();
    operation.attach_pipeline(&pipeline).unwrap();
    let different = live_pipeline();
    assert_eq!(
        operation.with_pipeline(&different, || panic!(
            "must not act on a divergent pipeline"
        )),
        Err::<(), _>(NativeStopError::Lifecycle)
    );
    assert_eq!(
        operation.attach_pipeline(&different),
        Err(NativeStopError::Lifecycle)
    );
    assert_eq!(different.current_state(), gst::State::Playing);
    operation.stop().unwrap();
    assert_eq!(different.current_state(), gst::State::Playing);
    stop_pipeline(&different).unwrap();
    assert_eq!(
        crate::lifecycle::Operation::new().stop(),
        Err(NativeStopError::Absent)
    );
    assert_eq!(
        NativeStopError::Transition.to_string(),
        "native_stop_transition"
    );
}

mod imp {
    use super::*;
    use gst::subclass::prelude::*;

    #[derive(Default)]
    pub struct FailingPipeline {
        pub fail: std::sync::atomic::AtomicBool,
        pub before_null: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FailingPipeline {
        const NAME: &'static str = "DonnaNativeStopFailingPipeline";
        type Type = super::FailingPipeline;
        type ParentType = gst::Pipeline;
    }
    impl ObjectImpl for FailingPipeline {}
    impl GstObjectImpl for FailingPipeline {}
    impl BinImpl for FailingPipeline {}
    impl PipelineImpl for FailingPipeline {}
    impl ElementImpl for FailingPipeline {
        fn change_state(
            &self,
            transition: gst::StateChange,
        ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
            if transition == gst::StateChange::ReadyToNull {
                let cut = self.before_null.lock().unwrap().take();
                if let Some(cut) = cut {
                    cut();
                }
            }
            if transition == gst::StateChange::ReadyToNull
                && self.fail.load(std::sync::atomic::Ordering::Relaxed)
            {
                return Err(gst::StateChangeError);
            }
            self.parent_change_state(transition)
        }
    }
}

glib::wrapper! {
    pub struct FailingPipeline(ObjectSubclass<imp::FailingPipeline>)
        @extends gst::Pipeline, gst::Bin, gst::Element, gst::Object;
}

impl FailingPipeline {
    pub(crate) fn before_null(&self, cut: impl FnOnce() + Send + 'static) {
        *self.imp().before_null.lock().unwrap() = Some(Box::new(cut));
    }
}
