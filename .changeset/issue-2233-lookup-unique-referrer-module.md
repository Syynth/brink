---
"@brink-lang/web": patch
---

Analyzer: `lookup_unique_by_name`'s std-visibility gate no longer
disagrees with `lookup_by_name` for a referrer declared inside the std
tree itself (issue #2233).

`#2216` (PR #2224) taught `lookup_unique_by_name` — the scope-free lookup
`infer::body::infer_ufcs_free_fn_result` uses to type a UFCS-shaped call's
result — the same std-invisibility gate `lookup_by_name_direct` uses. But
`lookup_unique_by_name` has no `ImportScope` to consult, so its gate
excluded every std-mounted candidate unconditionally, including when the
*referrer* was itself declared inside std: `lookup_by_name`'s own
`InScope` tier keeps resolving a std file's own sibling references, so the
two lookups silently disagreed for that one case.

`BodyCtx` now carries a `referrer_module` hint (the referring def's own
declared module — the same string `ImportScope::file_module` would carry
for that file), threaded from `ProjectCtx::body_ctx`.
`lookup_unique_by_name` takes it as a new parameter and only excludes a
std candidate when its module differs from the referrer's own — the exact
`Candidacy::InScope` "referrer and candidate share a declared module"
rule, reproduced without a full `ImportScope`. A referrer inside std
looking up a *different* std submodule's candidate, or a name genuinely
ambiguous between a now-visible std sibling and a coexisting ordinary
candidate, both still resolve to `None` (declined) rather than guessed at —
this narrows the over-broad exclusion, it does not widen resolution.
