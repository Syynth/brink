---
"@brink-lang/web": patch
---

Issue #628: `InferredType::List` (the phase-0 `signature()`/hover stub) now
carries the declaring LIST's name instead of dropping it. A VAR initialized
directly to a list literal (`VAR w = (sunny)`) previously fed
`infer::collect_globals` an `Unknown` type via a lossy `InferredType -> Ty`
conversion — weakening typed-mode inference for list VARs and, under
`types = strict`, spuriously tripping the Unknown-escape check (`E065`) for
anything assigned from such a VAR, unlike sibling nominal types
(`Ty::Struct`, `Ty::Handle`) which were already treated as clean.

Observable through `@brink-lang/web`: hovering a list-literal-initialized
VAR/CONST now shows its nominal type, e.g. `w: list<Weathers>`, instead of
the bare `w: list` it showed before.
