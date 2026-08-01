---
"@brink-lang/web": patch
---

Analyzer: fix a false `E065` on valid native code — a fn value in
declaration-initializer position now types (issue #1895).

`var f = double` on the native (`.brink`) surface has produced a real
runtime fn value since #1862 (`lir::lower::decls::fold_path_ref` folds it to
a `FnRef`), and `E080`'s `ref`-parameter obligation has always been checked
there. Typing was the odd one out: `signature::declared_fn_type` only
recognised the ink `#fn(…)` literal, so `f` got no declaration-derived type
at all, the global never reached `collect_globals`, and a later `f(3)`
classified as an unknown-callee value call — reporting `E065` under
`types = strict` on an otherwise-correct program.

The bare-name arm is now gated on the same two conjuncts lowering uses (the
declaring file is native, and the target is a statically-named function
definition), so the two sides can never disagree about which initializers
are fn values. `f` types as `fn(T…): R` from the target's signature and
carries the target's effect row, exactly as the body-position spelling does
since #1876 — a real mismatch through the global is now an ordinary `E063`
rather than an opaque `E065`. A bare name shadowed by a same-named
`VAR`/`CONST`/list item still declines to `Unknown`, because lowering
resolves that name to the shadowing global. Ink is unchanged: `VAR g = f`
there is still a knot's visit count, never a fn value.
