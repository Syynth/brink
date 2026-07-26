---
"@brink-lang/web": patch
---

Compiler: `hir::Stitch` carries a `return_type` (issue #1509).

NG-C (#1489) widened `Knot` with a `: type` return-type annotation but left
`hir::Stitch` — a nested flow, the ruled general form of a stitch — without
the same field, so a return-typed nested flow stayed fenced behind `E129`
(native) or failed to parse at all (ink's `= name(params): type`).

Both frontends now parse and lower a stitch's return-type clause onto the
same `hir::TypeExpr` a knot's does, and a native nested flow that declares
one is exempted from the implicit `-> DONE` grace — the same
coroutine-vs-state toggle `Knot.return_type` already drives — instead of
having its return clause silently ignored or flagged as unlowered.
