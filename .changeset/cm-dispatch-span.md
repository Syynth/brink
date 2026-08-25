---
"@brink-lang/editor": patch
---

New `cm.dispatch` perf span times the whole synchronous CodeMirror transaction cycle (state update + extensions + DOM sync) on the main editor view, with the transaction count as meta — added to decompose per-keystroke handler time that no existing `cm.*` span accounted for.
