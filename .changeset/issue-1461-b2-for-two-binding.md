---
"@brink-lang/web": patch
---

#1461 (B2, `docs/b0-sequencing.md`/`docs/stdlib-spec.md` §5/§9 F10 ruling):
`brink-syntax-native`'s `for` grammar gains an optional second binding —
`for key, val in expr { … }`, two-binding map iteration — landing the one
additive HIR field the B0 fence reserved (`ForStmt.val_name`,
docs/b0-sequencing.md:356). Lowers to the F10-ruled desugar: key iteration
plus `let val = container[key]`, no pair shape ever materializes. The
existing single-binding form (`for name in expr`) is unaffected — same
LIR shape as before, byte-for-byte.

Reachable through any `@brink-lang/web` session that analyzes a
`.brink`-extensioned file (`brink-db`'s `lowered_query` → `lower_native`,
the same seam #1177's control-flow lowering used): `for k, v in m` now
parses and lowers instead of erroring on the comma, and the analyzer
binds `v`'s type from the map's value type when the iterable is a `[K:
V]`. The ink/brink-dialect `~ { for … }` grammar is untouched — it has no
two-binding syntax and never sets `val_name`.
