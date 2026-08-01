---
"@brink-lang/web": patch
---

Compiler: close two silent-drop/undocumented gaps in the natural-notation
element-claiming dispatch (issues #1847, #1848).

A claiming `@[element(claims = "…")]` `fn` declared inside a `module { … }`
block previously validated as a legal placement (it reads as un-nested by
`flow`/`fn`-depth alone) but was never scanned by the handler-collection
pass, so it silently registered nothing to claim with. It is now diagnosed
misplaced (`E112`), the same as a claim on a `flow` or a nested `fn`.

New diagnostic `E168`: two claiming handlers with byte-identical patterns,
where the later one never actually won a claim in the file — dead code
under the interim first-match-wins dispatch order. (A later byte-identical
twin is not *unconditionally* dead: it is the only handler that can claim a
line inside the earlier twin's own body, since a handler can never claim
inside its own declaration — so the check runs after the whole file is
lowered and only fires when the later twin produced zero actual claims.)
That interim order (declaration order, until issue #1840's `fn
conventions()` registration order supersedes it) is now documented at the
dispatch site and in `docs/prose-dialect-spec.md` §3.5b, along with the
known gap: a genuine overlap between two *different* (non-identical)
patterns is not yet detected.
