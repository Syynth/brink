---
"@brink-lang/web": patch
---

Analyzer: strict-mode `E066` (Conflicted-escape) no longer spuriously fires
on a temp whose only "conflicting" use was a dotted field read (issue #994).

A dotted `Path` (`t.field`) whose head resolves to a `Param`/`Temp` reaches
the TM-4b resolution fallback (docs/typed-mode-spec.md §6), which maps the
whole multi-segment path's range to the *head* variable's `DefinitionId` —
there is no static field-type table yet, so `t.field` and bare `t` were
indistinguishable to the body-inference pass's usage-observation step. That
step was folding the field-read's usage-context type back into the *head*
temp's own accumulated type, manufacturing a `Conflicted` join (and a
spurious `E066`) whenever the two disagreed, even though the temp itself
was never actually misused. A dotted head resolving to a global
`VAR`/`CONST` never had this problem (cross-type-reassignment detection for
globals isn't implemented in this slice) — a `Param`/`Temp` head now gets
the same treatment: a dotted field read is never folded back into the
head's own type, only a bare (single-segment) reference is.
