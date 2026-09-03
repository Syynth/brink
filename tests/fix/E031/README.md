# E031 — value-call trim (issue #3428)

`greet` declares one param; the call over-supplies two (`"Al"`, `"Bob"`).
The classic `Opcode::Call` calling convention this diagnostic covers binds
the **trailing** supplied argument to the declared param (the callee's own
param-binding prologue pops off the shared value stack, LIFO) — see
`crates/internal/brink-ide/src/arity_trim_fix.rs`'s module doc for the full
account and the empirical repro that pinned it down. So the `Safe` trim here
removes the **leading** `"Al"` and keeps `"Bob"`: both sides print `Hi Bob`.
