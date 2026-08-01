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

The TM-2 annotation firewall is unchanged: the fallback overlays an
`Unknown` only, so a lambda body that genuinely constrains its param still
exports its own independent derivation, and a lambda's own explicit return
annotation (`|t|: T { … }`) still overlays only when the tail/expression
comes back `Unknown`. `docs/typed-mode-spec.md` §2 now carries the rule.
