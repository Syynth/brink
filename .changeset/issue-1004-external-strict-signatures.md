---
"@brink-lang/web": patch
---

Strict typed mode now consumes host-manifest external signatures on the
compile path, and the compile/warnings diagnostic channel gained `code` and
real `range` fields (#1004).

Under `dialect = brink, types = strict`, a `compileProject` (or
`compileFragment`) whose host manifest types an `EXTERNAL`'s params no longer
reports those params as escaping strict inference — the manifest
`ManifestParam.ty` resolves the param the same way it already did for
hover/pickers. Each registered `EXTERNAL` declaration is escape-checked
against the exact `collect_external_sigs` resolution that seeds call-site
argument checking (one shared helper across the analysis and compile paths),
so a manifest-typed external stays clean while a genuinely unresolvable
declared type (an empty `ty`, or one naming a semantic type absent from the
manifest `types` vocabulary) is still reported — anchored at that external's
own declaration span rather than collapsing onto one arbitrary line. An
`EXTERNAL` with no manifest entry at all stays unchecked, as before.

Additive wire-shape change on the `CompileResult.warnings[]` diagnostic
objects (also `compile` / `compileFragment`): each entry now carries

- `code` — the structured diagnostic code string (e.g. `"E065"`), so
  consumers can filter/group programmatically instead of string-matching
  `message`; and
- `start` / `end` populated from the diagnostic's real source span (external
  escapes previously would have anchored at a fallback location).

Existing fields (`message`, `start`, `end`, `severity`, `file`) are unchanged;
`code` is purely additive.
