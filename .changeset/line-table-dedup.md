---
"@brink-lang/web": patch
---

Line tables are deduplicated at emission

A line authored more than once within one scope (knot, stitch, or root)
is now a single line-table entry — one translation unit — instead of one
entry per occurrence site (`docs/intl-spec.md` §"Line-table
deduplication"). TheIntercept's `Lie`, previously 27 separate units, is
one. `linesTableOf` and `StoryRunner.linesTable` return the smaller
tables; `EmitLine` indices in the program model shift accordingly. Runtime
output is unchanged. Variant runs (line alternatives) are never merged.
