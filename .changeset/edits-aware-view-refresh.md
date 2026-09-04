---
"@brink-lang/editor": patch
---

`DocumentSessions` refreshes mounted views from a known edit list instead of
a whole-document replace (#3496): `applyEditsToViews(path, edits)` dispatches
a minimal, sorted `changes` set in one transaction — selection maps through
the change set (CM6's default) and no `scrollIntoView` effect is added, so a
mounted view's caret and scroll position survive an edit elsewhere in the
file. `invalidateFile`'s own fallback (used whenever no precise edit list is
available — a structural op's whole-file `new_source`, undo, an external
change) now applies a minimal common-prefix/suffix diff rather than
`{ from: 0, to: doc.length, insert: content }`, which previously collapsed
every existing selection to the insertion point and forced the whole
viewport to re-lay out.
