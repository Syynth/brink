---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Indent guides line up with the column they mark, and break between rows.

The guides were painted half a character right of their column — literal in
the upstream package, which appends `.5` to every gradient stop — so a caret
at that indent sat left of its own guide and read as needing one more space.
The shift is `0.5ch`, font-relative, because the editor font size is
user-settable.

Each row's guide is now slightly shorter than its row, leaving the small
vertical break between rows that Inky shows.

Two smaller fixes: Single File view remembers whether you hid the player
(it reopened on every reload and every switch back from Code view), and the
"not included in the project" banner can be dismissed — per file, for the
session, since what it states can stop being true.
