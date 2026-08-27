---
"@brink-lang/studio": patch
---

Settings and the Story Graph now take over the editor area instead of opening
as tabs, so they are reachable from every view. Previously they were tabs,
which only works in a view that has tabs — in Continuous view, opening
Settings put it behind the manuscript where it never appeared. A document
type opts into this with `takeover` on its descriptor. The takeover has a
header with a close button, choosing any view dismisses it, and it is not
restored on reload: consulting the graph is an interruption, not a place.
The view commands are also renamed to "View mode: Code" / "View mode: Single
File" / "View mode: Continuous", and they update the same setting the
Settings picker shows.
