---
"@brink-lang/web": patch
---

Analyzer: `declared_shapes`'s struct-shape table is now referrer-scoped,
not a flat bare-name winner (issue #2241).

`brink-analyzer::structs::declared_shapes` used to return a flat
`BTreeMap<String, ShapeInfo>` populated by plain last-`insert`-wins — with
the stdlib mount (#2080), a project's own `struct Cue { … }` coexisting
with a same-named `struct Cue { … }` from a mounted std preset meant
whichever file was iterated last silently overwrote the other's
`ShapeInfo`, regardless of which one a checking site actually meant. This
table feeds real diagnostics (`E069`/`E070`/`E071` construction-literal
field checks, `E063` dotted-assignment field checks, `E098` `ref`
lvalue-path segment checks, and UFCS field-call/receiver-type
resolution), so a construction literal, assignment, or UFCS call could be
validated against the wrong struct's fields.

`declared_shapes` now returns a `ShapeTable`: `get_by_def` for a shape
already pinned to an exact `DefinitionId` (a construction literal's own
shape name, which the analyzer already resolves with full module-scope
`Candidacy` via `resolve::resolve_struct_ref`), and `resolve(name,
referrer, index)` for every other lookup — the candidate declared in the
referrer's own file, else whichever remains once mounted `std…`-declared
candidates are excluded. Every consumer (`structs::check`,
`structs::check_assignments`, `ref_projection::check_strict`,
`ufcs::resolve`) now resolves per its own referring file instead of
reading a global winner.
