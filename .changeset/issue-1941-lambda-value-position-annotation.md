---
"@brink-lang/web": patch
---

Analyzer: a lambda's value-position read of an annotated param now exports
that param's declared type instead of `Unknown` under `types = strict`
(issue #1941).

PR #1938 fixed `fn f(t: content) { return t; }` — a `fn`'s `return` reading
an annotated param straight back out now exports the param's declared type.
The structurally parallel lambda shape was not covered:
`|t: content| { t }` (a block-bodied lambda's tail) and `|t: content| t`
(an expression-bodied lambda's sole expression) both still typed `Unknown`,
even though both are exactly the same "hand a param straight back out" read
as `return t;`.

`infer::body::InferPass::infer_lambda` now runs both value-position reads
through `or_own_annotation` — #1168's read-site annotation fallback,
already applied to `some(x)`, `get(m, k)`'s return shape, a `for` loop's
iterable, and (since #1938) a `fn`'s `return`. Unlike a plain `fn`/`flow`,
which gets its `annotated` fallback map seeded from `def.params` for free
at pass-creation time, nothing ever seeded a *lambda's* own param
annotations into that map — `infer_lambda` only ever shadowed (cleared)
whatever an enclosing same-named local's annotation left behind. The fix
also seeds `self.annotated` with the lambda's own resolvable param
annotations for the duration of its body walk, restored via the same
shadow/restore mechanism issue #1910 already uses for every other
frame-scoped map this function touches.

This seed's reach is the whole body walk, not only the two value-position
read sites above: `self.annotated` is consulted by `own_annotation`'s
bare-name fallback at every `or_own_annotation`/`annotated_callee_ty`
consumer reachable during the walk (an intrinsic's argument-position
overlay, a `for` loop's iterable, a direct-call callee's own annotated
type), exactly like a `fn`/`flow`'s own pass-creation seed already covers
its whole body, not only its `return`s. One exclusion: a param name the
lambda's own body re-binds via a fresh same-spelled `TempDecl`/`if`/
`while`/`for` binding is never seeded — `check_declared_assign_target`'s
`SymbolKind::Temp` arm reads this same map for its own mismatch report and
cannot tell the param's annotation apart from the fresh local's (absent)
one, so seeding it would falsely flag the fresh local's own assignment
against the shadowed param's type.

The TM-2 annotation firewall is unchanged: the fallback overlays an
`Unknown` only, so a lambda body that genuinely constrains its param still
exports its own independent derivation, and a lambda's own explicit return
annotation (`|t|: T { … }`) still overlays only when the tail/expression
comes back `Unknown`. `docs/typed-mode-spec.md` §2 now carries the rule.
