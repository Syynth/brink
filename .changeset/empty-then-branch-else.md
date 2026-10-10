---
"@brink-lang/web": patch
---

A conditional whose then-branch is empty and whose only arm is `- else:`
(`{cond:` then `- else:`) now takes the empty then-branch when the condition
is true, as ink does, instead of always running the else arm (#3510).
