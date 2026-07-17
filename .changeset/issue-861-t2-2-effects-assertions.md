---
"@brink-lang/web": patch
---

T2-2: `#@effects(…)` author-facing assertion surface + the exceedance
compile error (docs/effects-spec.md §10, sitting 2 — 2026-07-14; issue
#861, tracked from #859). Builds on T2-1's advisory `effects(def)`
substrate (issue #860).

- **Grammar** (the `#@` directive channel, brink-dialect-gated → `E051`
  under strict-ink): `#@effects(reads: gold, writes: alarm, calls: audio)`
  declares an upper-bound effect row on a knot/stitch; `#@effects(pure)` is
  sugar for the empty row. Placement mirrors `#@local` — top of a
  knot/stitch body.
- **The only diagnostic is exceedance** (`E103`): the definition's inferred
  effect row is not covered by (⊄) its declared bound. Per the sitting-2
  ruling there is no drift policy — an inferred row *narrower* than its
  bound stays silent; nothing else warns.
- A clause naming an identifier that isn't a declared global `VAR`/`CONST`
  (`reads`/`writes`) or a declared `EXTERNAL` (`calls`) anywhere in the
  project is `E102`; malformed directive grammar (missing argument, unknown
  clause keyword, non-identifier value) is `E100`/`E101`.
- Wired lazily: an unannotated project never triggers effect-row inference
  — only defs that actually carry `#@effects(…)` cause `effects(def)` to be
  computed.

Oracle byte-identical (5,577 episodes unmoved) and the strict-ink corpus
untouched — this is a brink-dialect-only analysis surface with no format,
codegen, or runtime change. Ships as a `@brink-lang/web` patch because the
new diagnostic codes (`E100`–`E103`) are editor-observable (LSP/IDE
diagnostics) through the wasm analysis pipeline.
