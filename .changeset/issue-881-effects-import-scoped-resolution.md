---
"@brink-lang/web": patch
---

Fix: T2-2's `#@effects(…)` `reads`/`writes`/`calls` clause resolution
(`resolve_cell`/`external_declared`) bypassed M-2d's import-scoped
resolution (issue #790), independently picking a flat, smallest-id
same-named candidate instead of routing through the shared
`ImportScope`/`lookup_by_name` machinery every other reference uses
(issue #881, tracked from #859; the #811 lesson: twin semantic checks
share one helper, never re-derive).

Under multi-module projects where two declared modules each publicly
export a same-name `VAR`/`CONST`/`EXTERNAL`, this could attribute a
`#@effects` assertion's clause to the *wrong* module's cell relative to
the one the asserting definition's body actually reads/writes/calls
(via the real import-scoped resolver) — producing a spurious `E103`
exceedance diagnostic, or, by luck of id ordering, silently masking a
real one.

`resolve_cell` and `external_declared` now resolve through
`brink-analyzer::resolve::lookup_by_name` with the asserting file's own
`ImportScope`, exactly like every other reference resolves — same-name
cross-module cells are now attributed per-importer, consistently with
what the body's own resolution binds.

Oracle byte-identical (5,577 episodes unmoved); single-module and
strict-ink projects are unaffected (the fast path is byte-identical
whenever there is at most one same-named candidate).
