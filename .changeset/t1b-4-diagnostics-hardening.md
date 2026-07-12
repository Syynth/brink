---
"@brink-lang/web": patch
---

T1b-4 diagnostics/semantics hardening (#577, #578, #580, #581, #568):

- **#577**: `break`/`continue` used outside any enclosing `while`/`for`
  loop is now a targeted, Error-severity compile error (`E057`). Previously
  it lowered unconditionally to an unguarded jump, and codegen silently
  degraded that to a no-op (`Opcode::Nop`) instead of ever surfacing an
  error — the compiler would accept clearly-wrong ink and produce dead
  bytecode for it. The check runs at LIR-lowering time (the same layer as
  the T1b-3 mutator checks, `E055`/`E056`), so it is a real, non-suppressible
  compile error, not a suppressible analysis diagnostic.
- **#578**: an inline multiline conditional/sequence that keeps its
  `InlineConditional`/`InlineSequence` shape all the way to LIR lowering
  (rather than being lifted to a top-level statement by HIR normalization —
  reachable via a choice's own display/bracket/inner text, which
  normalization never touches, or via a second inline construct on one
  content line) could contain a `~ { … }` T1b logic block. Lowering that
  case hit an internal `debug_assert!`-guarded "unreachable" arm — a panic
  in debug builds, a silent statement drop in release. It now routes
  through the same real lowering path top-level blocks use.
- **#580** (RULED): `contains(map, needle)` with a `needle` outside the
  map-key domain (a float, array, map, …) now returns `false` instead of
  faulting — total on both the array and map branches, matching the array
  branch's existing behavior. Indexing/mutation faults on a bad key are
  unchanged (value-model-spec §6); `contains` never had a "the key isn't
  there" failure mode to escalate to a fault the way those do.
- **#581** (RULED): a collection mutator (`push`/`insert`/`remove`) called
  with the wrong argument count is now a targeted, Error-severity compile
  error (`E058`) naming the expected signature (e.g.
  `push(container, value)`), replacing the generic `E031` warning the arity
  check used to share with ordinary function-call arity checking. E031
  never blocked compilation, so a malformed mutator call used to silently
  vanish from the lowered bytecode with no compile failure. Pure-function
  arity checking is unchanged.
- **#568**: a debug-build `console.warn` diagnostic for the third lossy-leg
  failure mode at the `value_to_js` wasm boundary (alongside the existing
  key-coercion-collision (#555) and key-reordering (#564) diagnostics): a
  `Value::Float` map value whose lossless `f32` → `f64` widening would print
  with more digits in a real JS engine than the value's own shortest
  decimal (e.g. `0.1f32` widens to the `f64` whose shortest round-trip
  decimal is `0.10000000149011612`). No value precision is actually lost —
  the widening is exact — but the extra digits are a genuine
  "where-did-these-come-from" surprise. Diagnostic-only; `value_to_js`'s
  marshaling is unchanged.

Oracle corpus: unchanged, 5,577 passing episodes.
