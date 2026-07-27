---
"@brink-lang/web": patch
---

Fixes issue #1503: a `ChoiceSet` whose empty, unlabeled continuation sits
inside a knot or stitch (not the file's root content) no longer gets an
implicit `-> DONE`. Falling off the end of a knot/stitch without an
explicit `-> DONE`/`-> END` is a genuine ink runtime error
("ran out of content. Do you need a '-> DONE' or '-> END'?"), not a safe
implicit end — only root content gets the safe implicit end. Root-content
`ChoiceSet`s are unaffected; they keep emitting the implicit `-> DONE`.
Observable through `@brink-lang/web`: a story compiled from source with
this pattern used to run one extra (incorrect) step and end `Done`
instead of surfacing the runtime error.
