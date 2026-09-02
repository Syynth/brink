---
"@brink-lang/web": patch
---

Internal: the auto-fix batching engine behind the fix surfaces
(`docs/autofix-spec.md` §5/§6.1) — `Select` picks the diagnostics of a
compilation, `applyRound`'s Rust counterpart turns them into one
non-overlapping edit set, and the fixpoint loop repeats to a hard cap of five
rounds, reporting a cap breach rather than swallowing it.

No wasm API changes in this release: `getFixes` / `applyFix` behave exactly as
before. The batch entry points reach `@brink-lang/web` in a later milestone.
