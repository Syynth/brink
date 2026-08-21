---
"@brink-lang/web": patch
---

New diagnostic: E187 rejects a write to a CONST (issue #2201).

`lir::lower::stmts::lower_assign_target` treated `SymbolKind::Constant`
identically to `SymbolKind::Variable`, with no distinction at all — a
story reassigning a declared `CONST` compiled clean, with zero
diagnostics anywhere in the pipeline, and the mutated value was
observable in the story's own output.

`E187` now rejects every write channel that resolves a `CONST` root:
plain/compound assignment, a postfix `++`/`--`, an indexed-assignment
root, a bare in-place mutator (`pop`/`heap_pop`/`push`/`insert`/`remove`/
`remove_at`, bare or indexed-lvalue), a struct-field write/mutator whose
root is a `CONST`, and passing the `CONST` by `ref`.
A `VAR` reassignment, a `CONST` read, and a local that merely shares a
`CONST`'s name all stay legal, unaffected by this change. Applies to
both `.ink` and `.brink` source, mirroring ink's own compile-time
rejection of `CONST` reassignment.
