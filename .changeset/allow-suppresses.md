---
"@brink-lang/web": patch
---

`[lints] Exxx = "allow"` now suppresses the diagnostic. It previously did
nothing at all — `effective_severity` returned the code's default severity
for `allow`, and every consumer reported it — so a project had no way to
turn a diagnostic off.

Any diagnostic whose default severity is not `Error` can be overridden,
including the advisory tier: `E189`, the ink `TODO:` note, is configurable.
