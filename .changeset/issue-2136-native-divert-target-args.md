---
"@brink-lang/web": patch
---

Issue #2136: native (`.brink`) HIR lowering now wires `-> knot(args)`
divert/tunnel-call/return-redirect arguments into `DivertTarget::args`
(and `Return::onwards_args` for `return -> knot(args)`) instead of
discarding them and raising a hard `E129` ("parses but has no HIR
lowering yet"). A native divert, tunnel call, or return-redirect with
arguments now compiles and runs, with the arguments reaching the target's
params exactly like the ink-dialect path already did — observable through
`@brink-lang/web`'s re-exported diagnostics (no more `E129` for this
construct) and compiled-story runtime behavior.
