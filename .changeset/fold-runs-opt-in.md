---
"@brink-lang/web": minor
"@brink-lang/editor": minor
---

Machinery/narrative fold runs are now opt-in (#479). `foldingRanges` /
`folding_ranges_doc` return structural folds only unless the host enables
run computation via the new session-level `setFoldRunsEnabled(true)`
(mirrors `setDialect`; also on `DocumentHandle`), and the editor's default
active fold kinds are `structural` only — hosts implementing prose/logic
view modes activate `machinery`/`narrative` with `setActiveFoldKinds` and
collapse with `foldAllOfKind`. Runs are additionally bounded by weave
containers (choice branches / gather continuations), so a run fold never
crosses weave structure; conditional scaffold + arms still fold as one
pure-routing region, and inline `{a|b}` alternatives don't fragment
narrative runs.
