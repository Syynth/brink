---
"@brink-lang/web": patch
---

Analyzer: strict-mode void-return inference for functions with no explicit
return value (issue #1028).

A function whose body never carries a value-returning `return <expr>` —
whether it falls off the end or only ever bare-`return`s, e.g. a wrapper
that calls a void external and returns nothing — now infers its return type
as void, matching what an explicit `): void ===` annotation already did,
instead of escaping as `Unknown` (`E065`) under `types = strict`. A function
with a real return path (even one whose value's type inference can't pin
down) is unaffected — void inference reads "never returns a value", never
"returns a value inference gave up on".

typed-mode-spec §3 documents `void` as the annotation for a no-return
function but is silent on what the same-shaped body should infer as when
unannotated; this closes that gap with the conservative, non-escaping
reading (spec gap flagged in the PR).
