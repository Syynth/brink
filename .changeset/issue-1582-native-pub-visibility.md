---
"@brink-lang/web": patch
---

Issue #1582 (RULED 2026-08-03): the native `.brink` grammar gains a `pub`
visibility marker — `pub flow`, `pub fn`, `pub var`, `pub const`,
`pub struct`, `pub extern`, `pub flags`. Absent `pub`, a declaration stays
Private (the already-ratified 2026-07-23 default, unchanged). `pub`
produces the existing `VisibilityMark::Public`, so `effective_visibility`
and every downstream cross-module gate (`brink-analyzer::modules`) are
unchanged — this is a grammar + lowering change, not an analyzer change.

**Grammar-level break, worth naming even though harmless today:** `pub`
becomes a reserved word on the native surface (confirmed zero occurrences
as an identifier across every in-tree `.brink` source before this change).
Any consumer that tokenizes `.brink` text independently of this crate
(none currently in-tree) would need to account for it.

`import`/`use`/`module` do not take `pub` (no `VisibilityMark` slot on
their HIR shapes); neither do the ink dialect's own knot/stitch
declarations (a different grammar, untouched).
