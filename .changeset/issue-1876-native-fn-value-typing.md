---
"@brink-lang/web": patch
---

Analyzer: native bare-name fn values now infer a real function type with the
target's effect row, in body/expression position (issue #1876).

On the native (`.brink`) surface a statically-named function in expression
position has been a fn value since #1862, but inference still typed the
reference `Unknown` — so nothing downstream could see it. It now types as
`fn(T…): R` built from the target's signature and carries that target's
effect row (`FnRow::of_target`), exactly as the ink `#fn(name)` spelling
already did, and is harvested as an ordinary fn-value creation site (a
call-graph edge plus the creation atom the effect fixpoint follows).

Author-visible consequence under `types = strict`: passing a bare function
name where a non-function type is declared is now an ordinary `E063`
type-mismatch at compile time — the typo hazard (`total(double)` for
`total(double(x))`) that the 2026-08-01 ruling accepted an unsigilled
spelling on the grounds the type checker catches. A bare name handed to a
declared `fn(T…): R` parameter still checks clean. Ink is unchanged: the
same bare name there is still a knot's visit count.

This does not cover a declaration initializer (`var f = double;`): that
position already produces a runtime fn value (lowering) but is not yet typed
(`signature::declared_fn_type` has no native/bare-name arm), so a bare name
in that position still types `Unknown` and can still misfire `E065` instead
of `E063`. Tracked separately at issue #1895.
