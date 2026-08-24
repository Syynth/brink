---
"@brink-lang/studio": minor
"@brink-lang/editor": minor
---

Search result cards (card-stack PR C). The Search panel's results render
as one card per match, in both text-search and references mode: a header
row (file:line, containing knot/stitch, `edited` badge, reveal ↗) over
the match's own small editable buffer — the match line plus a tunable
context window (default 1 above / 2 below), fully syntax-highlighted via
a per-file semantic-token cache. Cards collapse to a header preview
(per-card chevron, plus the binder-style expand/collapse-all buttons in
the summary row alongside the context knob and the snapshot ↻). The
list is virtualized: off-screen cards render as static HTML instead of
live editors. Card edits write through to the source and never remove
rows — the frozen snapshot flags them instead.

Also fixes cmd/ctrl-click goto/references from a real pointer: the
handler now binds mousedown (CodeMirror's own cmd-mousedown multi-cursor
preventDefault suppressed the browser click event a click-bound handler
was waiting for).
