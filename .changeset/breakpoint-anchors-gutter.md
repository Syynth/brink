---
"@brink-lang/studio": patch
"@brink-lang/editor": patch
---

Source-anchored breakpoints with a shared-column editor gutter
(W4/#3297). The editor's play gutter now renders breakpoint dots (bound
solid / unbound hollow / disabled dimmed, plus a hover preview) in the
same column as the play ▶ — click a plain line to toggle, header lines
keep play-from-here with "Set breakpoint here" in the symbol context
menu. The store keeps `(file, line)` anchors as the identity (range-keyed
per the debugger spec's v1 ruling), derives the runtime breakpoint set by
re-binding through `resolveSourceLine` on every compile/session change,
snaps no-code clicks to the nearest following bindable line, maps anchors
through document edits, and persists them per project.
