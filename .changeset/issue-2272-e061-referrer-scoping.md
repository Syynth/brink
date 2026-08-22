---
"@brink-lang/web": patch
---

Analyzer: `E061` (unrecognized type name) is now referrer-scoped (issue
#2272, split out of #2249/PR #2271).

`annotations::check`'s struct-name recognition used to read a project-flat
`declared_struct_names` set with no `ImportScope`/std-exclusion — an
unimported std-only struct name (e.g. `~ temp c: Cue` with `Cue` never
imported) read as "recognized" even though `resolve::resolve_type_ref`
already silently excluded it from resolution by design. Net effect: that
shape raised no diagnostic anywhere. `check` now routes a bare `Named`
annotation through the same `ImportScope`/`Candidacy` lookup
`resolve_type_ref` uses, so `E061` fires — naming the module the struct
is declared in (and noting it isn't reachable from this file yet) when
the name is declared but out of scope, and falling back to the original
"not a recognized type" message when the name is unknown project-wide. `names.lists`/`names.handles` (the `List<L>`/`Handle<K>`
checks) and `check_reserved_type_names` (`E188`) are unaffected —
`resolve_type_ref` never scopes those vocabularies, so there was no
referrer-scoping precedent to mirror there.

Also: a knot/stitch/lambda parameter's and return-type annotation's bare
type name now registers a `RefKind::Type` reference (mirroring the
`VAR`/`CONST`/struct-field/`temp` registration issue #2249 already added),
closing the goto-def/rename/find-references gap for a struct used only as
a parameter or return type.
