---
"@brink-lang/web": patch
---

Faster project compiles and recompiles (#460). The per-knot LIR chunk memos
behind `compileProject` used to rebuild their whole knot-*invariant* lowering
environment — the flattened
resolution lookup over every project resolution, the reconstructed struct-shape
tables, and the file-id→path map — once per knot, so the LIR layer cost scaled
as (knots × project size). It is now built once per project revision and shared
by every knot.

On a 50-file × 20-knot project the per-knot LIR layer drops from 34.3 ms to
4.0 ms cold (0.8 ms → 0.2 ms on a one-line-edit recompile), and end-to-end cold
compile from ~341 ms to ~307 ms.

No observable output change: the compiled artifact is byte-identical (pinned by
new cold-vs-warm `.inkb` identity tests and the existing incremental fuzz
harness), diagnostics are unchanged, and no JS signature moves.
