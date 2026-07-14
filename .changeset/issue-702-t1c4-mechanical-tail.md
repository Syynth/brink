---
"@brink-lang/web": patch
---

T1c-4 (#702) mechanical tail — corpus growth, a new "Function Values" book
chapter, and IDE polish. Only the IDE polish is observable through
`@brink-lang/web`:

- **Hover on a fn-value slot** (a `VAR`/`CONST`/`temp` bound directly to a
  `#fn(target, args…)` literal, at its declaration or a later plain
  assignment) now shows the bound signature display form — the same
  `fn heal(ref hp = player_hp, amount)` shape `string(f)` renders at
  runtime (spec §5), built statically from the HIR. Every other hover case
  is unchanged; a slot never bound to a direct `#fn(...)` literal (a
  `bind()` result, a copy of another variable, an ordinary value) shows
  nothing extra, same as before.
- **Completion after `#fn(`** now offers only statically-named function
  definitions (the same shape `#fn`'s E079 creation-site check requires),
  not the generic value-symbol list every other call-argument position
  offers. Completion everywhere else (including `#fn(name, ` — past the
  first argument) is unchanged.

Crates-only otherwise: the tier1-brink corpus wing grows (a triple-level
`bind`-of-`bind` chain, a wrong-typed-argument fault, the cross-flow
`#@local` `ref`-bind fault, and save/load with a live function value inside
an array/map), and grammar fuzzing extends to `#fn` in both dialects
(parser is dialect-agnostic, so this is parser-layer coverage) — none of
this changes any wasm-observable behavior.
