---
"@brink-lang/studio": patch
---

The TODO band survives the caret: with the cursor on a TODO line, `.cm-activeLine`'s background beat the band and left the dark ink invisible (0.2.0 regression). A highest-specificity restate keeps the goldenrod band under the active line.
