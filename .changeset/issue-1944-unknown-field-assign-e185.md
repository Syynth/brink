---
"@brink-lang/web": patch
---

Analyzer: `E185`, a plain dotted assignment target naming an unknown struct
field (issue #1944).

`~ p.bogus = expr` — a plain assignment to a struct field the shape doesn't
declare — used to compile clean under `types = strict` with zero
diagnostics: `check_declared_field_assign_target` (PR #1939) deliberately
stays silent on an unresolvable field name ("Unknown never disagrees"), and
`ref_projection::check_strict`'s `E098` only covers an unknown segment in
`ref`-argument position, not a plain assignment target. Construction
literals already had this check (`structs::check`'s `E070`); plain
assignment targets did not.

`structs::check_field_assign_mismatch` now reports `E185` the moment it
resolves the receiver's declared shape and finds no field by the assigned
name — fired only once a real struct shape is known (an Unknown/untyped
receiver stays silent, unchanged), and only for a single-level dotted
target (`p.x = v`); a chained target (`o.i.a = v`) never reaches this check
at all, since LIR already rejects it outright with the non-suppressible
`E074` regardless of the field name. Reaches both `Stmt::Assignment` and
the T1b `~ { … }` `BlockStmt::Assignment` form, and both analysis roads
(`brink-db`'s db-direct `ProjectDb::diagnostics` and the off-db
`IdeSnapshot::analyze`), since both call the same
`brink_analyzer::strict_diagnostics` → `structs::check_assignments` seam.
