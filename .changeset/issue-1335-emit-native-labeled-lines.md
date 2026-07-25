---
"@brink-lang/web": patch
---

#1335 (B0.8b): `brink_ir::hir::emit_native` now respells two constructs it
previously refused — a labeled dissolved-gather continuation and a
genuinely mid-flow labeled content line (`Stmt::LabeledBlock`) — using
G-1's `(name)` content-line-label spelling (ruled 2026-07-20). Adds one
native-only round-trip fixture (`tests/tier1-brink-respell/labeled-mid-flow-gather/`).

Not wired into any compile/analysis path — `emit_native` is called only
by `brink-respell`'s own tests (dev-only, `publish = false`, never
shipped), the same posture #1178's changeset already recorded. No
behavior change for any existing `.ink` or `.brink` session; this only
shrinks the emitter's own refused-construct set.
