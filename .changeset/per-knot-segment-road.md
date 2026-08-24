---
"@brink-lang/web": patch
---

Ink files now lower through the per-knot segment road (#3084): a keystroke inside one knot re-lowers that knot's segment only — every other knot's lowering memo backdates, shifted-but-unchanged knots included — and the analysis path no longer pays a whole-file parse per edit. Large-file keystroke re-analysis drops accordingly (see `docs/per-knot-incremental-lowering-spec.md` for the measured before/after). Output is byte-identical (HIR, symbols, admission) with one declared exception: the per-file diagnostics ARRAY now arrives in a deterministic segment-major order instead of the old kind-grouped interleaving — the diagnostic set, ranges, codes, and messages are unchanged, only vector order moves, and only for files where multiple diagnostic sources interleave.
