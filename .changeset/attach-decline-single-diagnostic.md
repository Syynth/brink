---
"@brink-lang/web": patch
---

An attach-mode convention claim that is declined because its line carries
a trailing tag now reports only the decline (E129). It used to report an
E172 as well, for an `@`-leading tag it never lowered (#2399).
