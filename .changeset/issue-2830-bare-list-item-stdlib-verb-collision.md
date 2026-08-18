---
"@brink-lang/web": patch
---

Analyzer: resolve a `#fn(target)` literal naming a declared list item that
collides with a stdlib list verb (e.g. `pop`), instead of silently dropping
the reference (issue #2830).

`resolve_function`'s lookup chain (externals → knots → lists-by-full-name →
variables → locals) fell through to `is_t1b_stdlib_name`'s silent "handled
at LIR lowering" skip whenever a bare name matched both a real declared list
item and a T1b stdlib verb name — the ref was neither resolved nor
diagnosed, violating the `completeness` invariant every reference is either
resolved or diagnosed. A real declared list item now wins over the
stdlib-name fallback at a `#fn` literal site, mirroring `resolve_variable`'s
existing precedence for `RefKind::Variable` refs and the "author symbol
shadows builtin" doc comment already on `is_t1b_stdlib_name`.

Restricted to `#fn` literal sites (`arg_count: None`) — a real call site
(e.g. `push(arr, 5)`) keeps resolving to the stdlib verb regardless of a
same-named list item, so an author's `LIST` declaration can never silently
divert a stdlib call to a list-item lookup that faults at runtime.

Observable through `@brink-lang/web`: resolution results and diagnostics
change at these sites — go-to-definition/hover targets and what the
studio's Problems panel renders.
