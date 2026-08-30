---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Runtime-value hover (W12/#3305, RULED). While a session is live and
in-sync, hovering a variable in the editor appends its current runtime
value to the existing hover card — globals always, frame locals while
paused in the Debugger panel's selected frame's scope. Pairs with the
choice-point visualization: hover the failing condition's variable to
see exactly why it failed. No new wasm surface — the editor extracts
the identifier under the cursor and asks the host
(`getRuntimeValueNote`); the studio's policy suppresses under degraded
and outside a live session, never stale.
