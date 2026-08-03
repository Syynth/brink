---
"@brink-lang/web": patch
---

#2098: migrated six analyzer passes (`coalesce`, `contains_domain`,
`conversions`, `map_keys`, `structs`'s two visitors, `range_refinement`)
from a hand-rolled second walk of `VAR`/`CONST` initializer expressions
onto the shared `hir::visit::HirVisitor` entry point
(`visit_with_decl_initializers`), which now grows two new hooks
(`enter_var_decl`/`enter_const_decl`) so a stateful visitor can reset its
own per-declaration bookkeeping.

**No behavior change intended or observed.** This is a pure internal
refactor: every pre-existing test for all six passes' decl-initializer
diagnostics passes unchanged, a new regression test for `coalesce`'s
diagnostic-anchor bookkeeping was added and verified to fail without the
fix, and the oracle ratchet holds at exactly 5607/5607 episodes (compared
directly against unmodified `origin/main`, which shows the identical
365/397 case count — confirming the small case-level drift already on
`main` predates this PR). Filed as a patch changeset per this repo's
standing rule that any crates-only PR touching analyzer-pass internals
gets one, since `@brink-lang/web` re-exports the same diagnostic engine.

`comparator_contract.rs` and `ufcs.rs` are **not** migrated in this PR —
both were found, while scoping this work, to carry a real latent gap a
naive migration would silently paper over (documented in the PR body and
tracked as a follow-up).
