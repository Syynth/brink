---
"@brink-lang/web": patch
---

Analyzer: narrow effect rows at indirect/value call sites when the callee's
origin is statically known (issue #872, docs/effects-spec.md §8's
"read the concrete `EffectRow` off a stored `Ty::Fn`" precision rung).

Previously, any call through a function value (`f(args)`, `call(f, args…)`)
unconditionally forced the enclosing definition's effect row to the pessimal,
touches-everything floor — even when the value provably came from exactly one
`#fn(target, …)` creation site. Now, a call through a write-once local (or an
inline `#fn`/`bind(…)`-chain literal evaluated right at the call site) whose
origin is a single, statically-known def narrows to that def's real row
instead, pulled in through the same SCC effect fixpoint a direct call already
uses. The narrowing is proven sound before it's trusted: a local reassigned
more than once anywhere in its body, or an origin that can't be traced to a
single def, keeps the old pessimal floor unconditionally — conservative-total
is never traded for precision.

This is observable through `@brink-lang/web`'s effects-diff/hover surfaces
(brink-ide's `effects()` display) and `brink-db`'s emitted `EffectRows`
table: a definition that calls only through a known fn-value local now shows
a real, non-opaque row instead of "touches everything" where it previously
did.
