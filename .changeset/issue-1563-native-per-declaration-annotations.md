---
"@brink-lang/web": patch
---

Native `.brink` per-declaration `@[…]` annotations now lower instead of
hard-failing (#1563). `@[effects(pure, silent, total, reads(…), writes(…),
calls(…))]` above a `flow`/`fn` head populates the container's effects
assertion at both levels (top-level knot and nested stitch) and is checked
by the same exceedance pass that judges ink assertions (E103/E108/E109);
previously every such line was rejected with E129 ("parses but has no HIR
lowering yet"), so the whole surface was unreachable from a `.brink` file
compiled through `compile_project`. Misplaced or unknown annotations are now
diagnosed on the channel's own codes (E111 unknown name, E112 unrecognized
placement, E100/E101/E048 for the assertion grammar) rather than the blanket
E129. The E111 and E112 diagnostic *messages* also changed on both surfaces
to name the native placement alongside ink's. Ink-dialect behavior and the
oracle corpus are unaffected.
