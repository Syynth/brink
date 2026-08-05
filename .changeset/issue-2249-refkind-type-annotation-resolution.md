---
"@brink-lang/web": patch
---

Compiler: a struct field's declared type and a `VAR`/`CONST`/`temp` TM-2
type annotation are now `RefKind::Type` references the analyzer resolves
against the referrer's module scope, instead of a private `brink-ir`
primitive re-deriving the answer (issue #2249, the remainder of #2246 left
open).

Before this issue, `hir::lower::types` registered no HIR reference at all
for a type annotation's nominal leaf — `symbols::project`'s own doc called
it "a nominal-only grammar, resolved later by a different mechanism". That
different mechanism was `ShapeTable::resolve`, a `brink-ir`-side lookup
re-implementing referrer scoping and std-exclusion on its own
(`decls::lookup_global`'s fallback, which excludes every std-declared
candidate unconditionally with no referrer-inside-std carve-out). Lowering
now consumes the analyzer's own `resolve::resolve_type_ref` resolution
directly for all four call sites this fed (`build_shape_table`'s field
loop, `build_struct_shape_data`'s identical loop,
`structs::record_global_annotation`, `context::LowerCtx::
record_temp_annotation`), and `ShapeTable::resolve` — with no production
caller left — is deleted.

**Observable delta:** a referrer *inside* a mounted std module referencing
a *sibling* std file's struct in a type annotation, with no explicit
import, now resolves (`lookup_by_name_direct`'s `InScope` tier) where it
previously could not (`lookup_global`'s unconditional std-exclusion had no
referrer-is-std carve-out) — the same static-offset (`RecordGet`/
`RecordSet`) chase issue #2246 already restored for a construction
literal's shape name. A TM-2 annotation naming an *unresolvable* type
(including a std-only struct an ordinary project file never imported)
still raises no diagnostic of its own — that annotation-content check
(`E061`, `brink-analyzer::annotations::check`) is unaffected and remains
project-flat.

Two sibling `brink-ir` lookups audited against the same question
(`collect_externals`' extern-to-fallback-fn pairing, `context::LowerCtx::
lookup_address_id`'s local-label addressing) were found **not** to fit this
pattern — both are self-declaration lookups with no corresponding
user-written reference to register a `RefKind` for — and are unchanged.
