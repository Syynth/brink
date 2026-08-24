---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Two-range model for container spans (#3054): `HirSpan` gains `content_end_line` — the TIGHT end (last line of actual content, trailing whitespace and the next declaration's doc block excluded) alongside the structural `end_line` that runs to the next sibling. Rails and their tooltips use the tight range, so a two-line function no longer paints (or reports) itself through the next function's docs; choice rails get eight golden-step color buckets so siblings are distinct; conditional-branch tooltips show the condition.
