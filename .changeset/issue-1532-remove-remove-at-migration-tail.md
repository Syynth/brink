---
"@brink-lang/web": patch
---

Issue #1532 (PR #1501 review follow-up on #1484's `remove`/`remove_at`
split): new compile diagnostic `E149` — a `remove(a, i)` call whose first
argument is statically known to be an array is now a compile error under
`types = strict` (the brink dialect's own implicit default), not just a
runtime `NotIndexable` fault. Only fires when the checker can prove the
receiver is an array from its own body-local uses (a `temp`/param, not a
`VAR` — a global's static type has no `Array`/`Map` representation in this
checker); `types = gradual` is unaffected, keeping the runtime fault as
its backstop. No behavior change for valid `remove(map, key)`/`remove_at`
call sites, and the oracle corpus (vanilla ink) is unaffected.
