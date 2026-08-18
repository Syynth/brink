---
"@brink-lang/web": patch
---

Issue #2856: fixed a silent-drop bug where an author-declared symbol
(`VAR`, knot, external, list, or local) sharing a name with a classic
uppercase ink built-in (`RANDOM`, `FLOOR`, `CHOICE_COUNT`, …) never
actually shadowed it — the reference was silently discarded at both the
analyzer's resolution pass and, separately, at LIR-lowering's call-site
codegen, with no diagnostic and a clean compile. `{RANDOM}` against a
declared `VAR RANDOM = 42` rendered as empty text instead of `42`; a knot
`=== function FLOOR(x) ===` called as `FLOOR(5)` ran the real built-in
instead of the author's knot. Both are now fixed: a declared symbol always
wins resolution first, matching the existing (and now-corrected doc
comments') stated behavior for the T1b lowercase stdlib names, and the
`E035` "name shadows a built-in function" warning at the declaration site
is what it always claimed to be — informational, not a lie about what
actually happens at the reference site. Also documents, and makes
enforceable via a named predicate, the `brink-analyzer` `completeness`
proptest's boundary around these reserved-but-shadowable names.
