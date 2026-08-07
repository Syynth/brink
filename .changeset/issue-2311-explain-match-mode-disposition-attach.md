---
"@brink-lang/web": patch
---

Explain-match wasm DTOs now carry `mode`, `disposition`, and the resolved
`attach` schema (issue #2311, #2113 follow-up). `ExplainClassifiedMatch`
(the winner/shadowed shape) gains `disposition` and `attach`; it already
carried `mode`. `ExplainAttempted` (the miss shape) gains all three —
`mode`, `disposition`, and `attach` — which it previously exposed none of.
`attach`, when present, is `{ kind: "resolved", name, fields }` or
`{ kind: "unresolved", name }`, mirroring `brink_ir::ConventionAttachSchema`;
its field types are the new recursive `ExplainSchemaTypeShape` mirroring
`brink_ir::SchemaTypeShape`. `attach` is omitted (not `null`) when the
handler declared no `attach = StructName` clause.

This also closes a gap one layer down: `brink_ir::ClassifiedMatch` (the
hit-case record `crates/internal/brink-ir/src/hir/classify.rs` produces)
did not carry the `attach` schema at all — only `ConventionProjectionEntry`
(the miss-case/attempted record) did. `ClassifiedMatch` now carries it
through from the projection entry unchanged, so both the hit and miss
wasm shapes can expose it.
