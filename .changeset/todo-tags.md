---
"@brink-lang/studio": patch
---

TODO notes can carry a tag — `TODO(audio): mix the vault door` — and the
TODOs panel turns each distinct tag into a chip you can toggle to filter.

The tag needs no language support: the ink parser already takes everything
after `TODO` to end of line, so the tag arrives as part of the note's text
and is split off for display. A note's tag renders as a chip on its row
rather than staying in the text, since `(audio)` repeated down a column is
noise.

The panel's title bar gains the two controls the Problems panel has: a
funnel that folds out the filter row — now holding the text filter *and*
the tag chips — and a group-by-file toggle for switching between the
per-file sections and a flat list.

Grouping persists; the tag selection deliberately does not. A tag is a
property of one project's notes, so restoring `(audio)` into a project
without it would filter the panel empty with no visible cause. Closing the
filter row clears the selection for the same reason.
