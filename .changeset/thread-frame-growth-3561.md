---
"@brink-lang/web": patch
---

`<- thread` no longer pushes a call frame. A gather that spawns one thread
per option and is looped back into from each choice body — the standard ink
game-loop idiom — used to retain one frame per turn forever: the boundary
frame `<-` pushed was captured in the choice's thread fork, and selecting the
choice installs that fork wholesale, so the boundary rode into the main call
stack and was never released. Call-stack depth grew with the turn count and,
because every fork copies the stack, the per-turn cost grew with it. ink
pushes no frame for a thread divert at all (inklecate compiles `<- opt(1)` to
a bare divert), and now neither does brink: the fork re-points its own copy of
the innermost frame at the target, binding the target's parameters there as a
plain `-> target` divert's would, and `Thread::base_depth` marks where the
parent's frames end.

This also fixes an output divergence the retained frame caused: a temp
declared in the looping knot was read through the thread frame's slot space
instead of its own, so a counter ink walks up from 1 read `1` on every turn.

Measured on `benchmarks/stories/hanoi-10` (5,000 turns): maximum call-stack
depth 2,501 → 1, every frame pushed now released, and the transcript is
byte-identical to inkjs 2.4.0's. The oracle ratchet is unchanged.
