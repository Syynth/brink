---
"@brink-lang/web": minor
---

`getDiagnosticRegistry()` — every diagnostic code the compiler knows, with
its title, default severity, whether `[lints]` can override it, its written
explanation when one exists, and an author-facing category for the codes a
project can actually configure.

Read this rather than keeping a code list in TypeScript: a hand-maintained
copy is wrong the moment a code is added, and wrong silently.

The `overridable` flag matters more than it looks: only 30 of the 189 codes
can be overridden at all — the analyzer refuses every code whose default
severity is not `warning`. A UI that ignores it offers a level picker for a
code the analyzer then discards.
