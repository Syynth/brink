//! NS-A3 (issue #1109, docs/stdlib-spec.md §9.6): the iterate protocol's
//! pull-shaped machine form over the CLOSED builtin iterable set.
//!
//! `iterate` is the registry's third entry — `next(ref Self): Option[T]`,
//! row ⊆ writes-receiver·silent·total, with laws attached: **every element
//! exactly once; `none` is terminal and sticky** (property-harness
//! enforced, `tests/law_iterate.rs`). Pull-shaped was RULED over
//! push/`each` because a push-desugared `for` body would be an fn-value
//! callback and functions never await — pull desugars inline and iterators
//! park across suspensions for free.
//!
//! v1 wires the protocol machinery over the builtin iterables only —
//! arrays (values) and maps (keys, insertion order, key set snapshotted at
//! iteration start per F10) — and `for` is the only consumer. The `for`
//! desugar itself compiles to an index walk over the same canonical
//! sequence (`CollectionKeys` + `IndexGet`; `brink-ir`'s
//! `lir::lower::blocks`), **not** to calls through this type: the desugar
//! is observably unchanged, and [`ValueIter`] is that same iteration
//! contract reified as a value. Both share one snapshot function
//! ([`crate::collection_ops::iteration_sequence`]) so they can never
//! drift; the law harness pins their agreement. This is the seam later
//! waves consume: A5's range iterators park in `FlowFrame` spills as
//! durable values, B2's `for k, v` desugar reads the same sequence, and
//! user-type impls join behind #1090.

use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::Value;

use crate::collection_ops::iteration_sequence;
use crate::error::RuntimeError;

/// A pull iterator over one builtin iterable's canonical sequence — the
/// iterate protocol's machine form: [`next`](Self::next) is
/// `next(ref Self): Option[T]` with the protocol's laws structural (the
/// cursor only advances, so every element is yielded exactly once and
/// exhaustion is terminal and sticky by construction).
///
/// Holds the snapshot the protocol's F10 ruling requires: constructing the
/// iterator snapshots a map's key set eagerly (arrays share their storage
/// — COW makes the snapshot free), so structural modification of the
/// source collection mid-iteration is invisible to an already-created
/// iterator.
#[derive(Debug, Clone)]
pub struct ValueIter {
    seq: IterSeq,
    idx: u64,
}

/// The snapshot behind a [`ValueIter`]: a materialized sequence for
/// arrays/maps, or the range bounds themselves (NS-A5, F7) — a range IS
/// its own canonical sequence, so iterating `0..1_000_000` never
/// materializes a million-element vector, mirroring the `for` desugar's
/// `CollectionKeys` identity pass-through for ranges.
#[derive(Debug, Clone)]
enum IterSeq {
    Seq(Arc<Vec<Value>>),
    Range { start: i64, len: u64 },
}

impl ValueIter {
    /// Snapshot `iterable`'s canonical sequence (arrays: values; maps:
    /// keys in insertion order; ranges: their own bounds — O(1)). Faults
    /// `NotIndexable` for anything outside the closed builtin iterable
    /// set — the same fault the `for` desugar's `CollectionKeys` raises.
    pub fn new(iterable: &Value) -> Result<Self, RuntimeError> {
        let seq = match (iterable.as_range(), iterable.range_len()) {
            (Some((start, _, _)), Some(len)) => IterSeq::Range {
                start: i64::from(start),
                #[expect(clippy::cast_sign_loss, reason = "range_len is never negative")]
                len: len as u64,
            },
            _ => IterSeq::Seq(iteration_sequence(iterable.clone())?),
        };
        Ok(Self { seq, idx: 0 })
    }

    /// How many elements remain to be pulled (saturated at `usize::MAX`
    /// for the degenerate 2³¹+-element ranges on 32-bit targets).
    #[must_use]
    pub fn remaining(&self) -> usize {
        let total = match &self.seq {
            IterSeq::Seq(seq) => seq.len() as u64,
            IterSeq::Range { len, .. } => *len,
        };
        usize::try_from(total.saturating_sub(self.idx)).unwrap_or(usize::MAX)
    }
}

/// The pull shape itself — `next(&mut self): Option<Value>` IS the
/// protocol method's machine form, so it lives on the standard trait:
/// `Some(element)` until the sequence is exhausted, then `None` forever
/// (terminal and sticky — the cursor never rewinds, making the fused-
/// iterator law structural).
impl Iterator for ValueIter {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        let item = match &self.seq {
            IterSeq::Seq(seq) => seq.get(usize::try_from(self.idx).ok()?).cloned()?,
            IterSeq::Range { start, len } => {
                if self.idx >= *len {
                    return None;
                }
                #[expect(
                    clippy::cast_possible_wrap,
                    clippy::cast_possible_truncation,
                    reason = "start + idx is an element of the range by construction, so it fits i32"
                )]
                Value::Int((start + self.idx as i64) as i32)
            }
        };
        self.idx += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining();
        (n, Some(n))
    }
}

#[cfg(test)]
mod tests {
    use brink_format::OrderedMap;

    use super::*;

    #[test]
    fn range_iterates_elements_in_order_without_materializing() {
        // NS-A5 (F7): ranges join the closed iterable set. Exclusive form.
        let r = Value::range(2, 5, false);
        let it = ValueIter::new(&r).unwrap();
        assert_eq!(it.remaining(), 3);
        let got: Vec<Value> = it.collect();
        assert_eq!(got, vec![Value::Int(2), Value::Int(3), Value::Int(4)]);
        // Inclusive form.
        let got: Vec<Value> = ValueIter::new(&Value::range(-1, 1, true))
            .unwrap()
            .collect();
        assert_eq!(got, vec![Value::Int(-1), Value::Int(0), Value::Int(1)]);
        // Empty ranges iterate zero times — emptiness is load-bearing.
        for r in [Value::range(0, 0, false), Value::range(5, 2, true)] {
            let mut it = ValueIter::new(&r).unwrap();
            assert_eq!(it.remaining(), 0);
            assert_eq!(it.next(), None);
            // Terminal and sticky.
            assert_eq!(it.next(), None);
        }
    }

    #[test]
    fn array_iterates_values_in_order() {
        let a = Value::array(vec![Value::Int(3), Value::Int(1), Value::Int(2)]);
        let mut it = ValueIter::new(&a).unwrap();
        assert_eq!(it.remaining(), 3);
        assert_eq!(it.next(), Some(Value::Int(3)));
        assert_eq!(it.next(), Some(Value::Int(1)));
        assert_eq!(it.next(), Some(Value::Int(2)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn map_iterates_keys_in_insertion_order() {
        let mut m = OrderedMap::new();
        m.insert(brink_format::MapKey::Str("z".into()), Value::Int(1));
        m.insert(brink_format::MapKey::Str("a".into()), Value::Int(2));
        m.insert(brink_format::MapKey::Int(7), Value::Int(3));
        let mut it = ValueIter::new(&Value::map(m)).unwrap();
        assert_eq!(it.next(), Some(Value::String("z".into())));
        assert_eq!(it.next(), Some(Value::String("a".into())));
        assert_eq!(it.next(), Some(Value::Int(7)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn none_is_terminal_and_sticky() {
        let a = Value::array(vec![Value::Int(1)]);
        let mut it = ValueIter::new(&a).unwrap();
        assert_eq!(it.next(), Some(Value::Int(1)));
        for _ in 0..16 {
            assert_eq!(it.next(), None);
        }
        assert_eq!(it.remaining(), 0);
    }

    #[test]
    fn non_iterable_faults_not_indexable() {
        let err = ValueIter::new(&Value::Int(3)).unwrap_err();
        assert!(matches!(err, RuntimeError::NotIndexable("int")), "{err:?}");
    }

    #[test]
    fn snapshot_is_immune_to_source_mutation() {
        // F10: the sequence snapshots at iterator creation — COW means a
        // later mutation of the source collection clones away from the
        // iterator's Arc, never through it.
        let mut a = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let mut it = ValueIter::new(&a).unwrap();
        if let Some(items) = a.array_make_mut() {
            items.push(Value::Int(3));
        }
        assert_eq!(it.next(), Some(Value::Int(1)));
        assert_eq!(it.next(), Some(Value::Int(2)));
        assert_eq!(it.next(), None);
    }
}
