---
"@brink-lang/web": patch
---

Fixed a duplicate `E046` diagnostic on directives with dynamic content
(`#@effects({expr})`, `#@was({expr})`, `#@private`/`#@public` with dynamic
content). `apply_scope_directives` had its own generic `d.dynamic` check
that fired for every directive, including ones with a dedicated handler
(`effects_assertion_from_directives`, `was_from_directives`,
`visibility_from_directives`) that independently re-checks `d.dynamic` and
emits its own `E046`. The generic check is removed in favor of the
dedicated handlers' own checks — unknown dynamic directives (no dedicated
handler) still get exactly one `E046` via the fallback arm.

Compat: strictly fewer diagnostics for an already-invalid construct
(dynamic content is never valid in a directive); no change for any
directive that isn't dynamic.
