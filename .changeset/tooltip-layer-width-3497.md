---
"@brink-lang/studio": patch
---

Tooltips no longer collapse to one-word width in the studio (#3497). The tooltip portal layer introduced in #3349 was zero-width, and when CodeMirror falls back from fixed to absolute placement a tooltip sizes against that layer; the layer is now full-width, zero-height and click-through, so tooltips size against the editor root again.
