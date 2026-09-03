---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

A delivered line's `source` now spans every source line that contributed text to it (glue, a prose-dialect cue + aside + dialogue), and the editor's follow/hover bands cover all of those lines (`ExecutionHighlight.endLine`).
