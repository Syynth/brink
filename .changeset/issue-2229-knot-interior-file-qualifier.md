---
"@brink-lang/web": patch
---

`brink-ir`: two files that legitimately declare a same-named knot (M-2d —
`native_module_path` always differs per file, so `insert_symbol` lets them
coexist rather than raising a duplicate-definition diagnostic) no longer
fail to compile with `[E060] duplicate DefinitionId` when both knots hold
an anonymous container at the same structural position (issue #2229) —
whether that container is one the HIR stamping pass mints (unlabeled
choice/gather/conditional-branch/sequence-branch) or one minted at LIR
time (an inline-sequence wrapper, e.g. an alternation inside choice text).

Three id-affecting changes ship together, all inside the one ruled break
class (Option A, `docs/decision-log.md` 2026-08-20):

- `hir::stamp_container_ids`'s per-knot loop qualifies a knot's interior
  anonymous-container hashing scope with the same `#file:{path}` prefix
  root content already carried (#1504).
- `lir::lower_knot_chunk` gives the knot chunk's `IdAllocator` that same
  per-file prefix, covering the LIR-minted inline-sequence wrappers the
  stamping pass never sees (review finding — the stamping fix alone left
  this shape colliding).
- Synthesized choice path segments are spelled `c-{n}` (matching the
  documented `c-N`/`g-N`/`b-N`/`s-N` scheme) instead of the bare `c{n}`,
  which an authored knot legally named `c0` could equal — under the now
  shared `#file:` namespace that was a new single-file `E060` regression
  (review finding); a dashed segment can never equal an authored
  identifier.

Consequence (accepted, not a defect): anonymous-container `DefinitionId`s
shift — every anonymous choice container everywhere, and every
knot-interior anonymous container/wrapper — so saved visit counts keyed
to those old addresses detach on recompile (`LoadReport` degrades
tolerantly). Name-keyed state (labels, knots, stitches, globals) is
unaffected. `brink-web` transitively depends on `brink-ir`, so this is
wasm-observable for any `.brink`/`.ink` source reaching these shapes.
