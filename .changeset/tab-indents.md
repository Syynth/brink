---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Tab indents and Shift-Tab dedents (by the 4-space indent unit), like every other editor. The built-in Tab/Shift-Tab line-conversion cycle (choice→body→gather→choice, character→parenthetical→dialogue, the double-blank `@:<>` template) is stripped for now (ruled 2026-08-24) — previously the keys were swallowed even where no conversion applied. Dialect-DECLARED transition rows keep first claim on Tab (#395 consumer contract; the default at-cue preset declares none). Enter/Shift-Enter transitions are untouched.
