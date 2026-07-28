---
"@brink-lang/web": patch
---

The fn-value verb layer, slice 2 (#1679): `filter_map`, and the ruled
effectful pair `each`/`map_each` (stdlib-spec §4). Observable through
`@brink-lang/web`, brink dialect only:

- **`filter_map(a, f) → [U]`** (`f: fn(T): Option[U]`) — the Option-mapper
  companion of `map`: keeps `f(x)` unwrapped when `some(v)`, drops it when
  `none`, in iteration order. Pure·silent-required, exactly like
  `map`/`filter`/`fold` — a non-Option callback return is a turn-terminating
  fault, the same posture as `filter`'s non-bool predicate return.
- **`each(a, f) → void`** (`f: fn(T)`) and **`map_each(a, f) → [U]`**
  (`f: fn(T): U`) — the ruled effectful spellings. Unlike the pure quartet,
  their callback's output reaches the transcript instead of being captured
  and discarded, and the dev-mode world-write guard is disarmed: a global
  write or RNG draw inside their callback is legal (in either mode), where
  the identical write inside `map`'s callback is an E119 compile error (a
  provable inline `#fn(target)`) or a `ComparatorWroteState` dev-mode fault
  (an opaque callback). Sequential in iteration order, never fused, and
  deliberately absent from E119's roster — their whole purpose is to be the
  legal home for the effects the pure quartet's gate rejects. The
  escaping-behavior faults (a callback that presents a choice, reaches
  `-> DONE`/`-> END`, or calls a host external) still apply to both — that
  limitation is architectural (no handler exists mid-opcode), not a purity
  rule.
- **No new opcode.** All six verbs share `SeqVerb` (0xA1); `filter_map`,
  `each`, and `map_each` add three more `SeqVerbOp` kind bytes to the
  three the pure trio shipped with.

The whole family is brink-dialect surface (strict-ink rejects it), so
vanilla-ink stories are unaffected and the oracle corpus is byte-identical.
