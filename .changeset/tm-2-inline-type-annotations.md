---
"@brink-lang/web": patch
---

Added TM-2 inline type annotation syntax (#618, docs/typed-mode-spec.md
§3): `name: type` after knot/stitch params and `VAR`/`temp` declarations,
`): type ===` in the function-header return position, and the `~ temp
name: type = expr` ascription form. Type names are lowercase nominals —
`int`, `float`, `bool`, `string`, `divert`, `void`, `list<L>` (nominal per
a declared `LIST`), `array<T>`, `map<K, V>`; `fn(T…): R` function types
parse but are reserved until T1c (a targeted diagnostic, `E062`, fires on
any use). An unrecognized type name gets a targeted diagnostic (`E061`).

Superset grammar, same dialect-gate pattern as T1b: `brink-syntax` always
parses annotations regardless of dialect; under `strict-ink` every
annotation is a brink-extension diagnostic (`E051`) at its span, same as
every other T1b extension construct. `E061`/`E062` are unconditional in
both dialects (they check annotation *content*, independent of whether the
syntax itself is allowed).

Annotations feed `signature()`'s firewall: an annotated knot/stitch param
or knot return type is exposed on `Sig` (`param_annotations`,
`return_annotation`); an annotated `VAR` overrides its literal-inferred
`value_type` (annotation wins over inference) — the existing
`infer::collect_globals` seam picks this up with no further change. A new
`annotation_mismatches` function compares an annotation against TM-1 body
inference and reports a disagreement (`E063`, advisory/warning severity —
strict-mode policy is TM-3's call). `~ temp` ascriptions parse and lower to
HIR but aren't yet wired into body inference (that would touch
`infer::body::BodyCtx`, out of scope per #638).

`brink-fmt` renders annotations for free through its existing single-line
token-collapsing passes (knot headers, declarations, logic lines) — no
renderer changes were needed, only idempotence tests. `brink-ide`'s
parse → HIR → analyze → project pipeline doesn't crash on annotated or
reserved/unknown-type sources. Grammar fuzz coverage (`proptest_syntax.rs`)
extended with a depth-bounded type-expression strategy so the superset
parser never panics on type-annotated input.

Oracle corpus is byte-identical (5,577 passing episodes) — none of it uses
brink-dialect annotation syntax, and the grammar addition is fully
optional/additive at every position it touches.
