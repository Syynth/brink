---
"@brink-lang/web": patch
---

The fn-value verb layer's pure trio (#1679): `map`, `filter`, `fold`
(stdlib-spec §4). Observable through `@brink-lang/web`, brink dialect only:

- **Three new verbs**, each taking a function value:
  `map(a, f) → [U]` (`f: fn(T): U`), `filter(a, pred) → [T]`
  (`pred: fn(T): bool`), and `fold(a, init, f) → U`
  (`f: fn(U, T): U`, left fold; an empty array returns `init` untouched —
  no absence case, so no `Option`). Callbacks are pure·silent-required
  (RULED 2026-07-18), which is what makes iteration order unobservable and
  lets the implementation fuse freely.
- **One new opcode** `SeqVerb` (0xA1 + kind byte: `map`, `filter`, `fold`)
  appears in disassembly. Each kind evaluates its callback re-entrantly per
  element with output isolated — the same machinery `SeqSortedBy` uses, so
  a callback that presents a choice, reaches `-> DONE`/`-> END`, calls a
  host external, or diverges is a turn-terminating fault, as is a
  non-array receiver, a non-function callback, or a non-bool `filter`
  predicate return.
- **E119 is now the shared pure-callback gate.** It already rejected a
  provably impure/unsilent `#fn(target)` comparator on
  `sort_by`/`sorted_by`; it now covers the trio's callbacks too, and its
  title changed from "sort comparator must be a pure, silent function" to
  "callback must be a pure, silent function". Per-site messages name the
  verb and its callback's role, so comparator diagnostics read the same as
  before apart from that title.

The trio is brink-dialect surface (strict-ink rejects it), so vanilla-ink
stories are unaffected and the oracle corpus is byte-identical.
