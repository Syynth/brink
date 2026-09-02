---
"@brink-lang/web": patch
---

Fix a divert (or `-> END` / `-> DONE`) at the end of a nested gather being silently dropped: when a choice body ended with a nested choice set, the inner gather's own terminator was overwritten by the exit to the outer gather, so `- -` followed by `-> target` printed the gather and then ran out of content. The inner gather now keeps its terminator and only receives the outer-gather exit when it has none (#3383).
