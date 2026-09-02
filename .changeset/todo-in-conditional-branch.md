---
"@brink-lang/web": patch
---

Fix: a `TODO:` line inside a multiline conditional's then-arm, `- else:`
arm, or a nested block (`{ cond: … - else: … }`) is now recognized as an
author note — it fires `E189` (Problems/TODO panel) like a weave-level
`TODO:` already did, and is no longer compiled as story prose printed to
the player at runtime.
