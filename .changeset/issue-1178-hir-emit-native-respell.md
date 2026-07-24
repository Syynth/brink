---
"@brink-lang/web": patch
---

#1178 (B0.8b): adds `brink_ir::hir::emit_native` — a pretty-printer from
lowered `HirFile` back to `.brink` native source, and the new dev-only
`brink-respell` crate (`publish = false`, never shipped) that composes it
with the existing ink frontend to mechanically respell ink corpus fixtures
into `.brink`.

Not wired into any compile/analysis path — `emit_native` is called only by
`brink-respell`'s own tests, not by `brink-db`'s `lowered_query` or any
other seam a `@brink-lang/web` session reaches. No behavior change for any
existing `.ink` or `.brink` session; this is new, additive public API
surface on `brink-ir` with no live caller in the shipped pipeline.
