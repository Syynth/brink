---
"@brink-lang/web": minor
---

Add the HIR structural projection to the editor session (#454 phase 2):
`getHirSpansDoc(doc)` returns nested semantic spans (kind, depth, resolved
`def_id`/`target_id` identity) plus a per-line container stack for rails, via
the new wasm `hir_spans_doc` export. New `HirSpan` / `HirLineContainer` /
`HirProjection` types.
