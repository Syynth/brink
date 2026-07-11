//! Replay recording: an in-memory log of external-call results captured during
//! a live run, replayed back during hot-reload reconstruction so a flow
//! re-walks the program with faithful external values instead of fallback
//! approximations.
//!
//! Records *every* external uniformly (no pure/query/effect distinction) and
//! replays the recordings, so replay re-executes nothing — effects don't
//! double-fire and reads stay faithful. The handlers ([`RecordingHandler`],
//! [`ReplayHandler`]) *compose* with a real [`ExternalFnHandler`] rather than
//! threading a recorder through the stepping hot loop. See
//! `docs/replay-recording-spec.md` (issue #189).
//!
//! Recordings live only in memory, for hot-reload — they are not serialized
//! (the transcript is the durable artifact). They are plain data, so they can
//! grow serialization later if a consumer ever needs to persist them.

use core::cell::RefCell;

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use brink_format::Value;

use crate::story::{ExternalFnHandler, ExternalResult};

/// Upper bound on recorded externals per flow (unbounded-growth guard). Beyond
/// it, [`ReplayRecorder::record`] drops the result and replay falls through to
/// the ink fallback body for the uncovered tail.
pub const RECORDING_CAP: usize = 16_384;

/// One recorded external-function result, captured in call order during a live
/// run.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedExternal {
    /// The ink-declared external name.
    pub name: String,
    /// Arguments passed, in declaration order.
    pub args: Vec<Value>,
    /// The value the external returned.
    pub result: Value,
}

/// How a replay obtains external values. Whole-flow granularity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplayMode {
    /// Default. Return the recorded result if the next entry matches (name +
    /// args), else the ink fallback body. Re-executes nothing, so effects don't
    /// re-fire and reads stay faithful.
    #[default]
    Recorded,
    /// Ignore recordings; run every external live (effects fire). The explicit
    /// "re-run against the current world" escape hatch — the consumer uses its
    /// real handler instead of [`ReplayHandler`].
    Live,
}

/// An append-only, capped log of external results for one flow, plus a replay
/// cursor. Recorded during the live run; consumed in order during replay.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayRecorder {
    log: Vec<RecordedExternal>,
    cursor: usize,
    diverged: bool,
}

impl ReplayRecorder {
    /// A fresh, empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a recorded external result, respecting [`RECORDING_CAP`]. Beyond
    /// the cap the result is dropped (replay falls through to fallback).
    pub fn record(&mut self, name: &str, args: &[Value], result: &Value) {
        if self.log.len() >= RECORDING_CAP {
            return;
        }
        self.log.push(RecordedExternal {
            name: name.to_owned(),
            args: args.to_vec(),
            result: result.clone(),
        });
    }

    /// Replay-cursor lookup: if the next recorded entry matches `name` + `args`,
    /// return its result and advance the cursor. On the first mismatch (the
    /// program path changed under us) or exhaustion, mark the recorder diverged
    /// so every subsequent lookup returns `None` (→ fallback), rather than
    /// feeding misaligned later recordings.
    pub fn take_recorded(&mut self, name: &str, args: &[Value]) -> Option<Value> {
        if self.diverged {
            return None;
        }
        match self.log.get(self.cursor) {
            Some(entry) if entry.name == name && entry.args.as_slice() == args => {
                self.cursor += 1;
                Some(entry.result.clone())
            }
            _ => {
                self.diverged = true;
                None
            }
        }
    }

    /// Reset the replay cursor and divergence flag to the start of the log, so
    /// the recording can drive another replay from the beginning.
    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
        self.diverged = false;
    }

    /// Number of recorded externals.
    #[must_use]
    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
}

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

    fn args(xs: &[i32]) -> Vec<Value> {
        xs.iter().map(|&x| Value::Int(x)).collect()
    }

    #[test]
    fn records_and_replays_in_order() {
        let mut r = ReplayRecorder::new();
        r.record("get_switch", &args(&[1]), &Value::Bool(true));
        r.record("get_var", &args(&[2]), &Value::Int(42));
        assert_eq!(r.len(), 2);

        assert_eq!(
            r.take_recorded("get_switch", &args(&[1])),
            Some(Value::Bool(true))
        );
        assert_eq!(
            r.take_recorded("get_var", &args(&[2])),
            Some(Value::Int(42))
        );
        // Exhausted → fallback.
        assert_eq!(r.take_recorded("get_var", &args(&[2])), None);
    }

    #[test]
    fn diverges_on_mismatch_and_stays_diverged() {
        let mut r = ReplayRecorder::new();
        r.record("a", &args(&[1]), &Value::Int(1));
        r.record("b", &args(&[2]), &Value::Int(2));
        assert_eq!(r.take_recorded("x", &args(&[1])), None);
        // Diverged latches: even a would-be match now returns None.
        assert_eq!(r.take_recorded("a", &args(&[1])), None);
    }

    #[test]
    fn arg_mismatch_diverges() {
        let mut r = ReplayRecorder::new();
        r.record("get_switch", &args(&[1]), &Value::Bool(true));
        assert_eq!(r.take_recorded("get_switch", &args(&[2])), None);
    }

    #[test]
    fn reset_cursor_replays_again() {
        let mut r = ReplayRecorder::new();
        r.record("a", &args(&[1]), &Value::Int(7));
        assert_eq!(r.take_recorded("a", &args(&[1])), Some(Value::Int(7)));
        r.reset_cursor();
        assert_eq!(r.take_recorded("a", &args(&[1])), Some(Value::Int(7)));
    }

    #[test]
    fn cap_drops_beyond_limit() {
        let mut r = ReplayRecorder::new();
        for _ in 0..RECORDING_CAP + 10 {
            r.record("a", &[], &Value::Null);
        }
        assert_eq!(r.len(), RECORDING_CAP);
    }

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
        assert!(matches!(h.call("x", &[]), ExternalResult::Fallback));
        assert!(matches!(h.call("a", &[]), ExternalResult::Fallback));
    }
}
