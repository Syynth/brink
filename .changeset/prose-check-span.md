---
"@brink-lang/editor": patch
---

Prose checking is measured: the debounced check now records a permanent
`prose.check` perf span, annotated with the document length. Prose
checking landed four days after the perf baseline was taken and had no
span of its own, so its cost showed up in a scenario run only as an
unattributed long task — 651 ms on a real 1,125-line file, 4.8 s p95 on
the 8k-line perf fixture (#3491).
