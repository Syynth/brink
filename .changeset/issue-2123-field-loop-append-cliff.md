---
"@brink-lang/web": patch
---

Issue #2123: the loop-append COW cliff #576 closed at the root persisted
one struct field deeper — `push(a.items, v)`/`insert`/`remove`/`remove_at`/…
on a single-level struct-field projection (`a: Bag`, `Bag.items: Array<int>`)
paid a fresh `Arc::make_mut` copy on *every* call instead of mutating in
place, an O(n²) cliff in a loop. Fixed without adding a new opcode: the
lowering now drops the record's own reference to the mutated field (via the
existing `RecordSet`) before the RMW runs, and takes rather than clones the
RMW's own operand and result temps, so the field's `Arc` becomes the sole
owner whenever nothing else aliases it.

Observable behavior change (brink dialect only — this mutator shape is a
T1b/TM-4 extension, unreachable from vanilla ink): a mid-RMW fault
(`insert`/`remove`/`remove_at` on an author-supplied key/index that's
invalid) now leaves the struct's *mutated field* — not the whole record —
as `Value::Null`. The struct itself stays a structurally valid record with
every other field untouched; previously the field mutator's take/write-back
ordering left the whole root completely unchanged on the same fault. This
is a narrower version of the trade-off the root-level `push`/`insert`/…
mutators (`lower_bare_mutator`, issue #576) already document and test.
