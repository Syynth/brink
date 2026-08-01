---
"@brink-lang/web": patch
---

Analyzer: returning an annotated parameter now exports that parameter's
declared type instead of `Unknown` under `types = strict` (issue #1912).

`fn passthru(t: content) { return t; }` reported `E065` — "return type
escapes strict inference as Unknown" — on a return type that is *exactly*
the annotated parameter type, while the annotated-return twin
`fn passthru(t: content): content { return t; }` was clean. Handing a
parameter straight back out lost its type: `ty_of_def` types a parameter
read from the body walk's own `locals` alone, which an annotation never
seeded. Filed against `content` (which only became a resolvable type in
issue #1846) but never `content`-specific — `int`, `float`, `bool` and
`string` all lost the same way.

`infer::body::InferPass::infer_return` now runs the returned value through
`or_own_annotation`, the read-site annotation fallback issue #1168 already
applies to `some(x)`, `get(m, k)`'s return shape and a `for` loop's
iterable. A `return` value is joined into the def's return type and never
`observe`d back onto the expression, so it meets that fallback's stated
contract: safe only at read sites that produce no counter-evidence.
`docs/typed-mode-spec.md` §2 now carries the rule.

The TM-2 annotation firewall is unchanged: the fallback overlays an
`Unknown` only, so a parameter the body genuinely constrains still exports
its own independent derivation and `E063` (annotation disagrees with
inferred usage) keeps comparing two derivations. One consequence is a new
true positive that could not fire before — `fn f(t: content): string
{ return t; }` now reports `E063`, where the body's `Unknown` used to be
silently overlaid by the return annotation.
