---
"@brink-lang/studio": patch
---

The Debugger and State View locals tables hide compiler-minted temps
(`DebugLocal.synthetic`, the #3395 lift-order hoist's `$liftN`), so an
author only sees the variables they wrote.
