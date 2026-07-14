---
"@brink-lang/web": patch
---

Struct construction literals (`Name#{field: expr, …}`, TM-4c): fixes #675
and #676 per the ruling in decision-log "Struct construction literals:
source-order evaluation, duplicate field is a compile error" (2026-07-14).

- (#676) Initializers now evaluate in **source** order (left-to-right as
  written), not the shape's declaration order — codegen reorders only the
  already-evaluated *values* into shape offsets afterward. Previously, when
  the author's field order differed from the shape's declaration order and
  two or more initializers had observable side effects, those effects fired
  in shape order instead of source order.
- (#675) A duplicate field in a construction literal is now a real compile
  error (`E084`), naming the repeated field, under both `types = gradual`
  and `types = strict`. Previously a duplicate silently kept the last
  initializer's value while the earlier, shadowed initializer's expression
  — including any side effect — was dropped without lowering it at all.

Observable through `@brink-lang/web`: `compile_project`/`compile_fragment`
now return a diagnostic (`E084`) for a construction literal with a
duplicate field, and the compiled bytecode for a well-formed literal whose
source field order differs from its shape's declared order now evaluates
initializers in the order the author wrote them.
