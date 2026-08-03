---
"@brink-lang/web": patch
---

Issue #2136: native (`.brink`) HIR lowering now wires `-> knot(args)`
divert/tunnel-call arguments into `DivertTarget::args` instead of
discarding them and raising a hard `E129` ("parses but has no HIR
lowering yet"). A native divert or tunnel call with arguments now
compiles and runs, with the arguments reaching the target's params
exactly like the ink-dialect path already did — observable through
`@brink-lang/web`'s re-exported diagnostics (no more `E129` for this
construct) and compiled-story runtime behavior.
