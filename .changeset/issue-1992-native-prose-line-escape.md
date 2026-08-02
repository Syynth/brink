---
"@brink-lang/web": patch
---

Fix (issue #1992): the native `.brink` surface's `> text` code-ground line
escape ("charter §8.2, RULED 2026-07-23: `>` emits a prose line inside a
code body") used to be accepted by the parser but had no HIR lowering,
failing with a loud `E129` diagnostic for every token on the line. `>
text` inside a `fn`'s default (or a `flow`'s `~{ }` "Compound guard"
override) code-ground body now lowers to real content emission — the same
output the whole-body `>{ }` selector already produced, at line
granularity — the mirror image of issue #1991's `~ stmt` fix at the
opposite ground. The escape also parses inside a nested `if`/`while`/`for`
body, but still lowers loudly (`E129`) there in this slice — a
deliberately narrower first cut, not a silent gap. Observable through
`@brink-lang/web` since the wasm package re-exports the native
compiler/runtime pipeline.
