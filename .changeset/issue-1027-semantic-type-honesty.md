---
"@brink-lang/web": patch
---

Analyzer/IDE: semantic-type honesty for unregistered host types (issue
#1027).

`external_check::resolve_type` (hover, signature help, argument pickers) and
`infer::type_ref_to_ty` (strict inference) now classify a `TypeRef` through
one shared helper (`type_resolution::classify`), so a semantic-type name the
host manifest doesn't register resolves identically on both paths — never a
confidently-typed name on one side and `Unknown` on the other. That
divergence was the real story behind #1004: hover rendered `id: var_id`
with full confidence for an unregistered `var_id` while strict inference
correctly resolved it to `Unknown`.

Hover and signature help are also honest about it now: an unregistered
semantic type renders with an explicit warning marker and an `E040`
cross-reference (`id: var_id ⚠ unregistered semantic type — E040`) instead
of a bare, confident name. A registered type (base keyword or a name found
in the host manifest's `types`) renders exactly as before.
