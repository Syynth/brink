---
"@brink-lang/web": patch
---

Issue #1531 (RULED 2026-07-27): frame-local projection receivers are
legal for UFCS auto-ref. `let g = Guest { … }; g.hp.heal(5)` — a `ref`
first-parameter method call whose receiver is a `let`/param-bound struct
field, one field level deep — now compiles and mutates the caller's
binding, instead of refusing with `E143`. A frame-local cell is a valid
projection root; the mutation needs no effect row because it is
unobservable outside the frame. LIR lowering never reuses the durable-only
`RefProjection`/`MakeProjection` machinery for this case — it splices a
read/call/write-back RMW sequence instead, the same discipline `g.hp = 5`
already rides. The durable-rooted case (`party.leader.heal(5)` where
`party` is a `VAR`) and its effect-row requirement are unchanged. A
frame-local projection more than one field deep still refuses with `E143`
(no lowering support beyond one level, matching plain assignment's `E074`
boundary).
