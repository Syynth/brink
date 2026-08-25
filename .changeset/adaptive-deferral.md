---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Adaptive deferral for advisory paint (#3064 C2): in documents of 1,000+ lines, the HIR overlay and inlay hints map their decorations through each edit (positions stay exact) and rebuild content once the document has been quiet for ~120 ms — a typing burst pays one rebuild at its end instead of one per keystroke. Documents under the threshold rebuild synchronously exactly as before, so small-file behavior is byte-identical.
