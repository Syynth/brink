---
"@brink-lang/web": patch
---

Fixed a silent data drop in codegen (issue #3181): a content line that took
the `EmitContent`/`ChoiceOutput` *flattening* path (recognized-line
recognition declined it — e.g. text mixed with an inline
conditional/alternation) always shipped with `LineEntry.source_location:
None`, even when a real location was available from `hir::Content::ptr`.
Only the recognized-line path populated it before.

`lir::Content` now carries a `source_location` resolved the same way the
recognized path already does, threaded through to every `add_line` call in
the flattening path. This changes compiled output — `LineEntry
.source_location` (reachable through `EditorSession`/`StoryRunner`'s
`program_inkt()`, the Program Explorer's `.inkt` text dump) is now populated
for flattened-path lines that previously had none — and also fixes
`brink-intl`'s `lines.json`/XLIFF export for the same lines, which copies
`source_location` verbatim for the translation toolchain.

A content line inside a string-literal interpolation
(`lir::StringPart::Literal`) still has no location — that gap is deeper
than this fix reaches (HIR string literals carry no span at all today,
tracked separately, not folded into this PR) — and a tag's own line-table
entry (`ContentPart` inside `lir::Content::tags`) still has none either,
since `hir::Tag::ptr` is discarded when tags are lowered and reusing the
enclosing content's range would misattribute a tag's own byte span.
