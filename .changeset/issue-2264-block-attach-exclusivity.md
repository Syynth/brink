---
"@brink-lang/web": patch
---

Issue #2264: a `@[convention(…, block, attach = StructName)]` handler
declaring both clauses on the same declaration now diagnoses `E186`
(mutually exclusive) instead of silently accepting the annotation with
`attach` inert. Before this fix, `block` always won `try_claim`'s dispatch
and the `attach` clause was parsed and stored but never consulted — no
event, no `OutputLine.element.data` merge, and no author signal at all.
Observable through `@brink-lang/web`: a project with this shape now
reports a new diagnostic where it previously analyzed clean.
