//! NS-A5 range-value opcode implementations (`docs/stdlib-spec.md` §7,
//! F7 ruled 2026-07-19; `docs/stdlib-sequencing.md` §2 Wave A5, issue
//! #1111).
//!
//! Ranges are a real `Value` kind: `start..end` (exclusive) / `start..=end`
//! (inclusive) over int bounds, v1 int-only. This module holds the **pure**
//! range ops — construction and the `non_empty` validator. The one range op
//! that draws (`rand::int` over a range) lives in [`crate::rand_ops`] with
//! the rest of the draw verbs, and is dispatched from `vm.rs`'s
//! `ConvertInt` arm (one value-directed `int(x)` verb, two legs).
//!
//! # Fault contract
//!
//! - `range_make`: non-int bounds fault (`StdlibWrongType`). Range bounds
//!   are ints by ruling — no numeric coercion, no bool promotion; a float
//!   bound is a malformed question, not a truncation opportunity.
//! - `range_non_empty`: a non-range operand faults (`StdlibWrongType`).
//!   The op itself is total over ranges — emptiness is the *answer*
//!   (`none`), never a fault. This is the parse-don't-validate boundary
//!   where dynamic bounds buy their `NonEmptyRange` evidence once.

use brink_format::Value;

use crate::error::RuntimeError;
use crate::story::Flow;

/// `RangeMakeExcl`/`RangeMakeIncl`: `[start, end]` → `Range`. Bounds must
/// both be ints (fault otherwise); an empty result (`start >= end` /
/// `start > end`) is a legal value — emptiness is load-bearing for
/// iteration (`for i in 0..n` with n = 0 runs zero times).
pub(crate) fn range_make(flow: &mut Flow, inclusive: bool) -> Result<(), RuntimeError> {
    let end_val = flow.pop_value()?;
    let start_val = flow.pop_value()?;
    let (Value::Int(start), Value::Int(end)) = (&start_val, &end_val) else {
        let bad = if matches!(start_val, Value::Int(_)) {
            &end_val
        } else {
            &start_val
        };
        return Err(RuntimeError::StdlibWrongType {
            verb: "range",
            expected: "int bounds",
            found: super::collection_ops::type_name(bad),
        });
    };
    flow.value_stack.push(Value::range(*start, *end, inclusive));
    Ok(())
}

/// `RangeNonEmpty`: `[r]` → `Option[Range]` — `some(r)` iff the range
/// denotes at least one element, else `none`. The checker types the `some`
/// payload as the inhabited-range refinement (`NonEmptyRange`, S2 ruled
/// 2026-07-19); at runtime the representation is the same `Range` value,
/// written form preserved — the refinement is a *view*, never a second
/// value kind (F7).
pub(crate) fn range_non_empty(flow: &mut Flow) -> Result<(), RuntimeError> {
    let v = flow.pop_value()?;
    let Some(len) = v.range_len() else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "non_empty",
            expected: "a range",
            found: super::collection_ops::type_name(&v),
        });
    };
    let result = if len > 0 {
        Value::some(v)
    } else {
        Value::none()
    };
    flow.value_stack.push(result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::story::PendingTerminal;
    use alloc::vec::Vec;

    /// Same minimal fixture shape as `rand_ops::tests` — these ops touch
    /// only `flow.value_stack`.
    fn test_flow() -> Flow {
        Flow {
            threads: Vec::new(),
            value_stack: Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
            ran_out_of_content_cause: crate::RanOutOfContentCause::default(),
            exec_mode: crate::story::ExecMode::default(),
            pure_callback: crate::story::PureCallbackState::default(),
            next_block_id: 0,
            pending_terminal: PendingTerminal::default(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn range_make_builds_both_forms() {
        for (inclusive, expected) in [
            (false, Value::range(1, 4, false)),
            (true, Value::range(1, 4, true)),
        ] {
            let mut flow = test_flow();
            flow.value_stack.push(Value::Int(1));
            flow.value_stack.push(Value::Int(4));
            range_make(&mut flow, inclusive).unwrap();
            let got = flow.pop_value().unwrap();
            assert_eq!(got.as_range(), expected.as_range());
        }
    }

    #[test]
    fn range_make_accepts_empty_and_backwards_bounds() {
        // Emptiness is a legal value at construction — `0..0` and `5..2`
        // both build (they iterate zero times, pick() them → none);
        // only the *refinement consumers* care.
        for (a, b) in [(0, 0), (5, 2)] {
            let mut flow = test_flow();
            flow.value_stack.push(Value::Int(a));
            flow.value_stack.push(Value::Int(b));
            range_make(&mut flow, false).unwrap();
            assert_eq!(flow.pop_value().unwrap().range_len(), Some(0));
        }
    }

    #[test]
    fn range_make_faults_on_non_int_bounds() {
        // Float end.
        let mut flow = test_flow();
        flow.value_stack.push(Value::Int(0));
        flow.value_stack.push(Value::Float(3.5));
        let err = range_make(&mut flow, false).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "range",
                found: "float",
                ..
            }
        ));
        // String start — the *start* is reported when both could be bad.
        let mut flow = test_flow();
        flow.value_stack.push(Value::from("a"));
        flow.value_stack.push(Value::Int(3));
        let err = range_make(&mut flow, true).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "range",
                found: "string",
                ..
            }
        ));
        // Bool bounds do NOT coerce (no int promotion at range bounds).
        let mut flow = test_flow();
        flow.value_stack.push(Value::Bool(false));
        flow.value_stack.push(Value::Int(3));
        assert!(range_make(&mut flow, false).is_err());
    }

    #[test]
    fn non_empty_mints_some_over_inhabited_ranges() {
        for r in [
            Value::range(1, 6, true),
            Value::range(0, 1, false),
            Value::range(5, 5, true), // single element: 5..=5
            Value::range(-3, 0, false),
        ] {
            let mut flow = test_flow();
            flow.value_stack.push(r.clone());
            range_non_empty(&mut flow).unwrap();
            let got = flow.pop_value().unwrap();
            // The payload is the SAME range, written form preserved — the
            // refinement is a view, not a rewrite.
            assert_eq!(got, Value::some(r.clone()));
            let Value::OptionVal(Some(inner)) = got else {
                unreachable!("non_empty over inhabited must be some");
            };
            assert_eq!(inner.as_range(), r.as_range());
        }
    }

    #[test]
    fn non_empty_returns_none_over_empty_ranges() {
        for r in [
            Value::range(0, 0, false),
            Value::range(5, 5, false),
            Value::range(7, 2, true),
        ] {
            let mut flow = test_flow();
            flow.value_stack.push(r);
            range_non_empty(&mut flow).unwrap();
            assert_eq!(flow.pop_value().unwrap(), Value::none());
        }
    }

    #[test]
    fn non_empty_faults_on_non_range() {
        let mut flow = test_flow();
        flow.value_stack.push(Value::Int(5));
        let err = range_non_empty(&mut flow).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "non_empty",
                found: "int",
                ..
            }
        ));
    }
}
