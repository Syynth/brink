---
"@brink-lang/web": patch
---

A blank line (an empty-list or empty-string interpolation on a line of
its own) between a delivered line and a turn boundary — `-> END`,
`-> DONE`, a choice point — is no longer delivered as its own line,
matching ink, whose lookahead drops it; blank lines followed by content
are still delivered, and a turn's leading blank lines collapse to one
(#3533).
