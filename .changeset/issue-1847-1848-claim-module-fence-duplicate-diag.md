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

New diagnostic `E168`: two claiming handlers with byte-identical patterns —
the later one is provably unreachable under the interim first-match-wins
dispatch order. That interim order (declaration order, until issue #1840's
`fn conventions()` registration order supersedes it) is now documented at
the dispatch site and in `docs/prose-dialect-spec.md` §3.5b, along with the
known gap: a genuine overlap between two *different* (non-identical)
patterns is not yet detected.
