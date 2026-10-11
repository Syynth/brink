---
"@brink-lang/web": patch
---

A struct named inside a generic or function type (`Array<Guest>`,
`Map<string, Guest>`, `fn(Guest): int`) is now a real reference. Go to
definition, find references and rename reach it, where before only a bare
top-level annotation did (#2992).
