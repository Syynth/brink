---
"@brink-lang/web": patch
---

Analyzer: `E063`/`E185` now also reach a dotted field-assignment target
whose receiver is an unannotated `~ temp` initialized from a construction
literal (issue #2906).

`~ temp p = Point#{x: 0.0, y: 0.0}` followed by `~ p.bogus = 1` or
`~ p.x = "s"` used to compile clean under `types = strict` — the
dotted-assignment recording site (`check_declared_field_assign_target`)
only ever resolved a Temp root's shape from an explicit ascription
(`self.annotated`), never from the initializer's own inferred type, even
though `~ temp p: Point = …` (the annotated spelling) already resolved and
fired both diagnostics correctly. The fallback now consults the
initializer's own inferred `Ty::Struct` shape whenever there is no explicit
ascription, feeding it through the exact same
`structs::check_field_assign_mismatch` fact/check seam `E063`/`E185`
already use for an annotated temp or a `VAR`.

Conservative by construction: an unannotated temp reassigned anywhere in
its def's body — to a different concrete struct, or to anything the
analyzer can't resolve at all (an unannotated `EXTERNAL` call's return,
say) — withdraws the inferred shape rather than risk a false positive; a
genuinely unresolved receiver (an unannotated function parameter, an
unknown call result) still stays silent, unchanged. Reaches both
`Stmt::Assignment` and the T1b `~ { … }` `BlockStmt::Assignment` form, and
both analysis roads (`brink-db`'s db-direct `ProjectDb::diagnostics` and
the off-db `IdeSnapshot::analyze`).
