//! Replay handlers: [`ExternalFnHandler`]s that *compose* with a real handler
//! to record external results during a live run, and to replay them during
//! hot-reload reconstruction.
//!
//! This is the runtime half of the shared replay-recording primitive (#189);
//! the serializable data model ([`ReplayRecorder`]) lives in `brink-format`.
//! Recording composes (a wrapping handler) rather than threading a recorder
//! through the stepping hot loop, per the project's instrumentation principle.
//! See `docs/replay-recording-spec.md`.

use std::cell::RefCell;

use brink_format::{ReplayRecorder, Value};

use crate::story::{ExternalFnHandler, ExternalResult};

/// Wraps an [`ExternalFnHandler`] and records every inline-`Resolved` external
/// result into a [`ReplayRecorder`] during a live run.
///
/// Pure/command bindings resolve inline and are captured here. World-access /
/// async bindings resolve *out of band* (the handler returns
/// [`ExternalResult::Pending`] and the value arrives later via
/// `resolve_external`), so the consumer records those itself when it supplies
/// the value — it has the name, args, and result at that point.
pub struct RecordingHandler<'a, H: ExternalFnHandler + ?Sized> {
    inner: &'a H,
    recorder: RefCell<&'a mut ReplayRecorder>,
}

impl<'a, H: ExternalFnHandler + ?Sized> RecordingHandler<'a, H> {
    /// Wrap `inner`, recording its inline-`Resolved` results into `recorder`.
    pub fn new(inner: &'a H, recorder: &'a mut ReplayRecorder) -> Self {
        Self {
            inner,
            recorder: RefCell::new(recorder),
        }
    }
}

impl<H: ExternalFnHandler + ?Sized> ExternalFnHandler for RecordingHandler<'_, H> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        let result = self.inner.call(name, args);
        if let ExternalResult::Resolved(value) = &result {
            self.recorder.borrow_mut().record(name, args, value);
        }
        result
    }
}

/// Replays recorded external results (`ReplayMode::Recorded`).
///
/// For each call, returns the next recorded result if it matches (name + args),
/// else [`ExternalResult::Fallback`] — the ink fallback body — for
/// uncovered / divergent / past-cap calls. Re-executes nothing, so effects
/// don't re-fire and reads stay faithful.
///
/// For `ReplayMode::Live`, don't use this handler: supply the consumer's real
/// handler instead so everything runs live.
pub struct ReplayHandler<'a> {
    recorder: RefCell<&'a mut ReplayRecorder>,
}

impl<'a> ReplayHandler<'a> {
    /// Build a replay handler over `recorder`, resetting its cursor so replay
    /// starts from the first recorded result.
    pub fn new(recorder: &'a mut ReplayRecorder) -> Self {
        recorder.reset_cursor();
        Self {
            recorder: RefCell::new(recorder),
        }
    }
}

impl ExternalFnHandler for ReplayHandler<'_> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        match self.recorder.borrow_mut().take_recorded(name, args) {
            Some(value) => ExternalResult::Resolved(value),
            None => ExternalResult::Fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub handler: `Resolved` for names in its table, else `Fallback`.
    struct Stub(Vec<(&'static str, Value)>);
    impl ExternalFnHandler for Stub {
        fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
            self.0
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(ExternalResult::Fallback, |(_, v)| {
                    ExternalResult::Resolved(v.clone())
                })
        }
    }

    #[test]
    fn recording_captures_resolved_passes_through_fallback() {
        let mut rec = ReplayRecorder::new();
        let inner = Stub(vec![("get", Value::Int(5))]);
        {
            let h = RecordingHandler::new(&inner, &mut rec);
            assert!(matches!(h.call("get", &[]), ExternalResult::Resolved(_)));
            assert!(matches!(h.call("nope", &[]), ExternalResult::Fallback));
        }
        // Only the Resolved call was recorded.
        assert_eq!(rec.len(), 1);
    }

    #[test]
    fn replay_returns_recorded_then_fallback() {
        let mut rec = ReplayRecorder::new();
        rec.record("get", &[], &Value::Int(5));
        let h = ReplayHandler::new(&mut rec);
        assert!(matches!(
            h.call("get", &[]),
            ExternalResult::Resolved(Value::Int(5))
        ));
        // Exhausted → fallback.
        assert!(matches!(h.call("get", &[]), ExternalResult::Fallback));
    }

    #[test]
    fn record_then_replay_roundtrip() {
        let mut rec = ReplayRecorder::new();
        let inner = Stub(vec![("a", Value::Int(1)), ("b", Value::Bool(true))]);
        {
            let h = RecordingHandler::new(&inner, &mut rec);
            let _ = h.call("a", &[]);
            let _ = h.call("b", &[]);
        }
        let h = ReplayHandler::new(&mut rec);
        assert!(matches!(
            h.call("a", &[]),
            ExternalResult::Resolved(Value::Int(1))
        ));
        assert!(matches!(
            h.call("b", &[]),
            ExternalResult::Resolved(Value::Bool(true))
        ));
    }

    #[test]
    fn replay_diverges_to_fallback_on_mismatch() {
        let mut rec = ReplayRecorder::new();
        rec.record("a", &[], &Value::Int(1));
        let h = ReplayHandler::new(&mut rec);
        // Different name at cursor 0 → fallback (and latches diverged).
        assert!(matches!(h.call("x", &[]), ExternalResult::Fallback));
        assert!(matches!(h.call("a", &[]), ExternalResult::Fallback));
    }
}
