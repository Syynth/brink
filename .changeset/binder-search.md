---
"@brink-lang/studio": minor
---

Binder v2, part 5 (#3040): search. A header toggle opens a filter row —
one case-insensitive query over file names and structural names
(knots/stitches/functions). Matches keep their file context, a stitch
match survives as its knot's context, matching symbols reveal in BOTH
modes (Files mode included), the collapsed state is ignored while
searching, and Escape/× clears. The #tag namespace from the design is
deliberately deferred: the tag data does not exist at any layer yet —
#474 owns wiring per-flow tags through HIR → format → the wasm boundary,
and the binder search grows the third namespace when it lands.
