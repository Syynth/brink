//! Replay recording: a serializable log of external-call results captured
//! during a live run, replayed back during hot-reload reconstruction so a flow
//! re-walks the program with faithful external values instead of fallback
//! approximations.
//!
//! The design records *every* external uniformly (no pure/query/effect
//! distinction) and replays the recordings, so replay re-executes nothing —
//! effects don't double-fire and reads stay faithful. See
//! `docs/replay-recording-spec.md` (issue #189).

use serde::{Deserialize, Serialize};

use crate::Value;

/// Upper bound on recorded externals per flow (unbounded-growth guard). Beyond
/// it, [`ReplayRecorder::record`] drops the result and replay falls through to
/// the ink fallback body for the uncovered tail.
pub const RECORDING_CAP: usize = 16_384;

/// One recorded external-function result, captured in call order during a live
/// run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecordedExternal {
    /// The ink-declared external name.
    pub name: String,
    /// Arguments passed, in declaration order.
    pub args: Vec<Value>,
    /// The value the external returned.
    pub result: Value,
}

/// How a replay obtains external values. Whole-flow granularity.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplayMode {
    /// Default. Return the recorded result if the next entry matches (name +
    /// args), else the ink fallback body. Re-executes nothing, so effects don't
    /// re-fire and reads stay faithful.
    #[default]
    Recorded,
    /// Ignore recordings; run every external live (effects fire). The explicit
    /// "re-run against the current world" escape hatch.
    Live,
}

/// An append-only, capped log of external results for one flow, plus a replay
/// cursor. Recorded during the live run; consumed in order during replay.
///
/// Only the `log` is durable (serialized); the cursor and divergence flag are
/// transient replay state, reset per replay via [`reset_cursor`](Self::reset_cursor).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ReplayRecorder {
    log: Vec<RecordedExternal>,
    #[serde(skip)]
    cursor: usize,
    #[serde(skip)]
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

        // Name mismatch at cursor 0 → diverged.
        assert_eq!(r.take_recorded("x", &args(&[1])), None);
        // Even a would-be match now returns None (diverged latched).
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

    #[test]
    fn serde_roundtrip_preserves_log_resets_cursor() {
        let mut r = ReplayRecorder::new();
        r.record("a", &args(&[1]), &Value::Int(1));
        let _ = r.take_recorded("a", &args(&[1])); // advance cursor
        let json = serde_json::to_string(&r).expect("serialize");
        let mut back: ReplayRecorder = serde_json::from_str(&json).expect("deserialize");
        // Log preserved; cursor/diverged reset to default so replay starts fresh.
        assert_eq!(back.len(), 1);
        assert_eq!(back.take_recorded("a", &args(&[1])), Some(Value::Int(1)));
    }
}
