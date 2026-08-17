---
"@brink-lang/web": patch
---

Fix #2772: the E116 `Option[T]`-condition-truthiness walk
(`option_conditions.rs`) now visits `hir.root_content` — the file-scope
content sitting before the first knot/stitch header. This is the third gap
found in the same walk after #2764/PR #2768 fixed the other two (no `Expr`
descent at all, and `check()` never walking `hir.variables`/`hir.constants`).
Previously a condition on an `Option[T]` value sitting in root content got
no E116 diagnostic at all, while the byte-identical condition written
inside a knot fired correctly.

Root content's own `~ temp` locals are now resolved through the same
synthetic `DefinitionId` scheme `strict.rs::body_def_ids` (issue #1903)
already established for this scope, rather than treating root content like
a bare declaration value with no locals of its own.

This makes new hard E116 errors (under `types = strict`) appear on `.brink`
files with an `Option[T]` condition sitting in file-scope content before
the first knot, in both the studio Problems panel and through
`EditorSession`/`IdeSnapshot::analyze`.
