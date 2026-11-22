//! Fato interno restrito ao MESMO pipeline. Nenhum recibo ou autoridade wire.
use gst::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeStopError {
    Absent,
    Closed,
    Transition,
    NotNull,
    Lifecycle,
}

impl std::fmt::Display for NativeStopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Absent => "native_stop_absent",
            Self::Closed => "native_stop_closed_unknown",
            Self::Transition => "native_stop_transition",
            Self::NotNull => "native_stop_not_null",
            Self::Lifecycle => "native_stop_lifecycle",
        })
    }
}
impl std::error::Error for NativeStopError {}

pub(crate) fn stop_pipeline(pipeline: &gst::Pipeline) -> Result<(), NativeStopError> {
    let transition = pipeline.set_state(gst::State::Null);
    let (observed, current, pending) = pipeline.state(gst::ClockTime::ZERO);
    checked_null(transition, current, pending)?;
    checked_null(observed, current, pending)
}

fn checked_null(
    result: Result<gst::StateChangeSuccess, gst::StateChangeError>,
    current: gst::State,
    pending: gst::State,
) -> Result<(), NativeStopError> {
    match result {
        Err(_) => Err(NativeStopError::Transition),
        Ok(gst::StateChangeSuccess::Async) => Err(NativeStopError::NotNull),
        Ok(_) if current == gst::State::Null && pending == gst::State::VoidPending => Ok(()),
        Ok(_) => Err(NativeStopError::NotNull),
    }
}

#[cfg(test)]
#[path = "native_stop_tests.rs"]
pub(crate) mod tests;
