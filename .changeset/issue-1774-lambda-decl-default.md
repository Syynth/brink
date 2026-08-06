---
"@brink-lang/web": patch
---

A native `var`/`const` declaration default may now be a lambda literal
(`const twice = |x| x * 2`), not just a bare-name function reference
(#1862). Previously this raised `E083` ("declaration default is not a
compile-time-constant expression") — RULED 2026-08-01 (`docs/decision-log.md`
#1774), the gate is lifted: a file-scope lambda has no enclosing frame to
capture from, so the creation-site-capture concern that justifies gating a
lambda everywhere else never applies here. The lambda still folds through
the same lambda-lifting machinery (#1709) as any other lambda, just handed
an empty enclosing frame.
