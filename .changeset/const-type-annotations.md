---
"@brink-lang/web": patch
---

CONST declarations now accept a TM-2 inline type annotation
(#641, docs/typed-mode-spec.md §3: "optional anywhere"): `CONST name: type
= expr`, mirroring the `VAR` annotation surface end to end.

Superset grammar (`brink-syntax`): `const_declaration` now peeks for
`at_type_annotation` after the identifier, same discipline as
`var_declaration` — an unannotated `CONST` produces the exact same CST as
before this change. HIR (`brink-ir`) gains an `annotation: Option<TypeExpr>`
field on `ConstDecl`, lowered structurally with no validity checking.

Analysis (`brink-analyzer`): `dialect_gate` flags a `CONST` annotation as
`E051` under `strict-ink`, same as every other TM-2 annotation site.
Annotation *content* checks (`E061` unknown type name / `E062` reserved
`fn(...)` type) run through the same `finish_analysis`-gated call as `VAR`
— brink dialect only (maintainer ruling 2026-07-13), verified rather than
re-gated. `signature()`'s firewall now resolves a `CONST`'s annotation and
has it override the literal-inferred `value_type`, same annotation-wins
rule as `VAR`.

`brink-fmt` renders the annotation for free through the existing
single-line declaration renderer — idempotence tests added, no renderer
change. `brink-ide`'s parse → HIR → analyze → project pipeline doesn't
crash on annotated or reserved/unknown-type `CONST` sources. Grammar fuzz
coverage (`proptest_syntax.rs`) extended with a `CONST`-typed strategy
mirroring the existing `VAR`-typed one.

Oracle corpus is byte-identical (5,577 passing episodes) — none of it uses
brink-dialect annotation syntax, and the grammar addition is fully
optional/additive.
