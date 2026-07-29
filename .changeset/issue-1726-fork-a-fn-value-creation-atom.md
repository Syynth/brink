---
"@brink-lang/web": patch
---

Analyzer: collapse the effect row's opaque floor when every fn value reaching a
call site was created in-project (issue #1726, Fork A of #1680 —
docs/effects-spec.md §6.1a).

A new per-definition structural atom records the targets whose fn values a body
**creates** (`#fn(target, …)` literals, including through `bind(…)` chains),
harvested by the same body walk that already produces the direct-call edges and
referenced globals — empty globals, empty signatures, structural id sets only.
No inferred row or signature is ever consulted to decide an edge, so the call
graph stays row-independent and the SCC batching and effect fixpoint are
unchanged.

The user-visible effect is the narrowing this unlocks. Previously a call through
a local was narrowed only when that local was written **exactly once**; a local
reassigned to a second known `#fn` origin fell back to the pessimal,
touches-everything floor, where no `@[effects(…)]` assertion could cover it.
Now the row is the **join over every write's creation target**, which
over-reports at worst and so keeps the conservative-total direction. A
definition that calls through such a local shows a real, non-opaque row in the
effects-diff/hover surfaces (brink-ide's `effects()` display) and in
`brink-db`'s emitted `EffectRows` table, and an `@[effects(…)]` bound that
covers the join is now satisfied where it previously reported an `E103`
exceedance ("no effects assertion can cover this definition"), or `E108`/`E109`
against `silent`/`total`.

The guard is unchanged: a single write whose value did not trace to an
in-project creation site — a parameter, a call's return value, a heap load —
keeps the pessimal floor, because such a value can come from anywhere,
including a host callback. Lambda literals are out of scope (they have no index
symbol at HIR time).
