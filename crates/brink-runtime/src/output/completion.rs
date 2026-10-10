//! Incremental line-completion tracking for [`OutputBuffer`].
//!
//! `drive_to_terminal` asks [`OutputBuffer::has_completed_line`] after
//! **every** VM step, and the answer used to be computed from scratch each
//! time: a glue-marking pass over the whole unread transcript followed by a
//! walk over it. That made the question O(unread) per step — 27% of all
//! instructions on `crucible-8` and 19% on `hanoi-10` (measured with
//! callgrind on the baseline this module replaces), for a query whose answer
//! changes only when a part is appended to the transcript.
//!
//! [`LineCompletion`] keeps the walk's state up to date as parts are
//! pushed, so the per-step query is a field read. The state is defined by
//! the batch algorithm it replaces ([`OutputBuffer::has_completed_line_scan`],
//! kept as the test-only reference), and the two are held equal by a
//! property test over random part sequences and cursor moves
//! (`tests::incremental_matches_batch_scan`). The reasoning that makes the
//! incremental form exact:
//!
//! - Glue marking removes, for each `Glue`, every not-yet-removed
//!   `Newline` back to the nearest visible content part or `Tag` (issue
//!   #3535: ink's glue walks back over blank lines). Within a **run** — the
//!   parts between two such stopping points — a `Glue` therefore removes
//!   all of the run's newlines before it, and every newline after it in the
//!   run is in `after_glue` state.
//! - The walk finds the first surviving `Newline` not in `after_glue` state
//!   (`after_glue` is set by a `Glue` and cleared only by visible content).
//!   That newline is in the current run until the run ends, and a `Glue`
//!   later in the same run removes it.
//! - A found newline becomes final the moment its run ends: no later
//!   `Glue` can reach past visible content or a `Tag`. Until then it is
//!   tentative, and is dropped exactly when a `Glue` arrives in its run.
//! - The answer flips to `true` when visible content follows a found
//!   newline (blank content does not commit it, #3533 and #3535) and
//!   cannot flip back while the cursor stays put.
//!
//! Anything other than an append to the transcript at or past the cursor —
//! a cursor move, or `trim_function_end` removing parts — invalidates the
//! state, and [`OutputBuffer::rescan_completion`] rebuilds it from the
//! cursor. Those happen once per delivered line or per function return, not
//! once per step, so the per-step cost is what matters and it is now O(1).

use super::{OutputBuffer, OutputPart};

/// The walk's candidate newline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Found {
    /// No surviving newline yet.
    #[default]
    None,
    /// In the current run: a `Glue` in the same run removes it.
    Tentative,
    /// Its run has ended (a `Tag` followed it); no `Glue` can reach it.
    Final,
}

/// Walk state over `transcript[cursor..]`. See the module doc.
#[derive(Debug, Clone, Default)]
pub(crate) struct LineCompletion {
    /// The answer: a committed newline exists in the unread transcript.
    completed: bool,
    /// A `Glue` has been seen since the last visible content part.
    after_glue: bool,
    /// The found newline, if any.
    found: Found,
}

impl LineCompletion {
    /// Fold one more part — the one just appended to the transcript.
    pub(crate) fn feed(&mut self, part: &OutputPart) {
        if self.completed {
            return;
        }
        if part.is_visible() {
            if self.found != Found::None {
                self.completed = true;
            }
            self.after_glue = false;
            return;
        }
        match part {
            OutputPart::Newline if !self.after_glue && self.found == Found::None => {
                self.found = Found::Tentative;
            }
            OutputPart::Glue => {
                self.after_glue = true;
                if self.found == Found::Tentative {
                    self.found = Found::None;
                }
            }
            OutputPart::Tag(_) if self.found == Found::Tentative => self.found = Found::Final,
            _ => {}
        }
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.completed
    }
}

impl OutputBuffer {
    /// Rebuild [`Self::completion`] from the cursor. Called after anything
    /// that is not an append at or past the cursor: a cursor move
    /// (`take_first_line`, `flush_lines`, `reset_cursor`) or a removal
    /// (`trim_function_end`).
    pub(crate) fn rescan_completion(&mut self) {
        let mut state = LineCompletion::default();
        for part in &self.transcript[self.cursor..] {
            state.feed(part);
        }
        self.completion = state;
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    use brink_format::{LineFlags, ListValue, Value};
    use proptest::prelude::*;

    use super::super::{OutputBuffer, OutputPart};

    /// Every `OutputPart` shape the completion state distinguishes: content
    /// that is visible, content that is not (issue #3533's blank values), the
    /// structural parts that are neither, and the two parts that carry the
    /// glue algebra.
    fn arb_part() -> impl Strategy<Value = OutputPart> {
        prop_oneof![
            Just(OutputPart::Newline),
            Just(OutputPart::Glue),
            Just(OutputPart::Spring),
            Just(OutputPart::Text("a".to_string())),
            Just(OutputPart::Text(" ".to_string())),
            Just(OutputPart::Tag("t".to_string())),
            Just(OutputPart::ValueRef(Value::Int(1))),
            Just(OutputPart::ValueRef(Value::String(Arc::from(" ")))),
            Just(OutputPart::ValueRef(Value::OptionVal(None))),
            Just(OutputPart::ValueRef(Value::List(Arc::new(ListValue {
                items: Vec::new(),
                origins: Vec::new(),
            })))),
            Just(OutputPart::LineRef {
                container_idx: 0,
                line_idx: 0,
                slots: Vec::new(),
                flags: LineFlags::empty(),
                site: None,
            }),
            Just(OutputPart::LineRef {
                container_idx: 0,
                line_idx: 0,
                slots: Vec::new(),
                flags: LineFlags::ALL_WS,
                site: None,
            }),
            Just(OutputPart::ElementAttach("k".to_string(), "v".to_string())),
            Just(OutputPart::ElementAttachEnd),
        ]
    }

    /// The three ways the transcript's unread region changes: an append
    /// (through the private `push_part`, so suppression in the public push
    /// methods does not narrow the sequences tried), a cursor move (what
    /// `take_first_line`/`flush_lines`/`reset_cursor` do, followed by the
    /// same rescan they call), and a function-end trim.
    #[derive(Debug, Clone)]
    enum Op {
        Push(OutputPart),
        MoveCursor(usize),
        Trim(usize),
    }

    fn arb_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            8 => arb_part().prop_map(Op::Push),
            1 => (0usize..64).prop_map(Op::MoveCursor),
            1 => (0usize..64).prop_map(Op::Trim),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2048))]

        /// The incremental answer equals the batch scan after every
        /// mutation of the unread transcript.
        #[test]
        fn incremental_matches_batch_scan(ops in proptest::collection::vec(arb_op(), 1..48)) {
            let mut buf = OutputBuffer::new();
            for (n, op) in ops.into_iter().enumerate() {
                match op {
                    Op::Push(part) => buf.push_part(part),
                    Op::MoveCursor(k) => {
                        buf.cursor = k.min(buf.transcript.len());
                        buf.rescan_completion();
                    }
                    Op::Trim(start) => buf.trim_function_end(start.min(buf.transcript.len())),
                }
                let batch = buf.has_completed_line_scan();
                prop_assert_eq!(
                    buf.has_completed_line(),
                    batch,
                    "after op {} the incremental state disagrees with the scan; \
                     cursor={} transcript={:?} state={:?}",
                    n,
                    buf.cursor,
                    &buf.transcript[buf.cursor..],
                    buf.completion
                );
            }
        }
    }

    /// The glue-run shapes the module doc reasons about, spelled out so a
    /// failure names the shape rather than a proptest seed.
    #[test]
    fn glue_run_shapes() {
        let nl = OutputPart::Newline;
        let glue = OutputPart::Glue;
        let a = || OutputPart::Text("a".to_string());
        let blank = || OutputPart::ValueRef(Value::String(Arc::from("")));
        let tag = || OutputPart::Tag("t".to_string());
        let cases: [(&str, Vec<OutputPart>, bool); 10] = [
            ("plain line", alloc::vec![a(), nl.clone(), a()], true),
            (
                "glue eats the newline",
                alloc::vec![a(), nl.clone(), glue.clone(), a()],
                false,
            ),
            (
                "glue after the newline eats the next one too",
                alloc::vec![a(), nl.clone(), glue.clone(), nl.clone(), a()],
                false,
            ),
            (
                "one glue eats every newline in the run",
                alloc::vec![a(), nl.clone(), nl.clone(), glue.clone(), a()],
                false,
            ),
            (
                "a later newline does not shield the first",
                alloc::vec![
                    a(),
                    nl.clone(),
                    nl.clone(),
                    glue.clone(),
                    nl.clone(),
                    glue.clone(),
                    a()
                ],
                false,
            ),
            (
                "blank content does not commit the line (#3535)",
                alloc::vec![a(), nl.clone(), blank()],
                false,
            ),
            (
                "glue walks back over blank content (#3535)",
                alloc::vec![a(), nl.clone(), blank(), nl.clone(), glue.clone(), a()],
                false,
            ),
            (
                "visible content after blank content commits",
                alloc::vec![a(), nl.clone(), blank(), nl.clone(), a()],
                true,
            ),
            (
                "a tag shields the newline behind it",
                alloc::vec![a(), nl.clone(), tag(), glue.clone(), a()],
                true,
            ),
            (
                "a tag alone does not commit",
                alloc::vec![a(), nl.clone(), tag()],
                false,
            ),
        ];
        for (name, parts, want) in cases {
            let mut buf = OutputBuffer::new();
            for p in parts {
                buf.push_part(p);
            }
            assert_eq!(buf.has_completed_line(), want, "{name}");
            assert_eq!(buf.has_completed_line_scan(), want, "{name} (batch)");
        }
    }
}
