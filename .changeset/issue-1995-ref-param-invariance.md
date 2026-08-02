---
"@brink-lang/web": patch
---

Issue #1995/#1920 (ruled 2026-08-01): `ref` parameter arguments are now
checked **invariantly**, not covariantly. `assignable(Float, Int)` is
`true` (by-value widening), so `fn scale(ref x: float)` called with an
`int` cell used to be accepted — the callee then writes a `float` back
through a cell that is statically declared `int`, an unsound write-back.

Both by-ref call-checking sites now use a new invariant predicate
(`ref_assignable`, requiring the argument's type to match the parameter's
declared type exactly, still row-insensitive):

- The direct-call argument check (#1864/PR #1875).
- The UFCS-desugared argument/receiver check (#1881/PR #1914) — covers
  both the receiver slot (D5 auto-ref) and any later `ref` parameter.

This is a `.brink`-dialect-only, native-surface change (vanilla ink has
no `ref` parameters or UFCS calls to reach it) that **rejects some code
that compiled before this fix** — a widening `ref` argument now reports
`E063` under `types = strict`, the same code the covariant checks already
used. Observable through `@brink-lang/web` because the wasm package
re-exports the same diagnostics.
