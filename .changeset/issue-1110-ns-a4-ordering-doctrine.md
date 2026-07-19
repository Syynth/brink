---
"@brink-lang/web": patch
---

NS-A4 (#1110): the §4b ordering doctrine in the VM + dev/prod ExecMode.
Observable through `@brink-lang/web`, brink dialect only:

- **Four new verbs**: `sort(ref a)` / `sort_by(ref a, cmp)` (imperative,
  statement-only — E055/E056/E058 postures) and `sorted(a)` /
  `sorted_by(a, cmp)` (functional twins, F0 ruled 2026-07-19). Two new
  opcodes (`seq_sorted` 0xF8, `seq_sorted_by` 0xF9) appear in
  disassembly. The doctrine order: int/float (numeric promotion), bool,
  string (USV-lexicographic), arrays lexicographic element-wise; stable.
  `min`/`max` gain the arrays-lexicographic roster leg too.
- **Dev/prod ExecMode** (runtime knob, default **Dev**): a float NaN
  comparand in `sort`/`sorted`/`min`/`max` is now a turn-terminating
  fault in dev mode (previously: A1's always-pinned placement); prod mode
  keeps the pinned non-fabricating total order (`-0 == +0` ties, NaN
  greatest). Hosts opt in via `Story::set_exec_mode` /
  `FlowInstance::set_exec_mode`. The mode is never embedded in `.inkb`
  and never persisted — rows stay mode-independent.
- **New diagnostic E119**: a `sort_by`/`sorted_by` comparator written as
  an inline `#fn(target)` whose inferred row provably breaks pure·silent
  (global reads/writes, external calls, emits, tags) is a compile error —
  exceedance-only; opaque comparators pass and fall to the runtime
  residual (`ComparatorEscaped` and friends).
- **F29(a)** (ruled by delegation 2026-07-19): a protocol
  `display`/`compare` impl whose row is provably total no longer inherits
  the conservative faults bit at the E114 contract gate — the
  conservative union applies only to opaque or genuinely fault-bearing
  impls.

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
