---
"@brink-lang/web": patch
---

#1162: a new `Severity` tier — `Info`/`Hint` — sits below `Warning`, plumbed
through `brink_ir::Severity`, `brink-project-config`'s `LintLevel`
(`"info"`/`"hint"` alongside `"allow"`/`"warn"`/`"deny"`), and
`brink_analyzer::effective_severity` (a `[lints]` entry can now down-level a
`Warning`-default code to either advisory tier; both are immune to
`deny-warnings`, same as `allow`). No existing diagnostic code's *default*
severity changes — this only adds the tier and the opt-in mechanism to reach
it.

Reachable through `@brink-lang/web`: `EditorSessionHandle::setLintOverrides`
now accepts `"info"`/`"hint"` as per-code levels (previously rejected as
unrecognized), and `diagnostic_to_js` renders `"Info"`/`"Hint"` for a code
configured that way — both proved by
`set_lint_overrides_hint_relevels_e014_and_still_compiles` and
`diagnostic_to_js_renders_info_and_hint_tiers`. `brink-lsp`'s
`severity_to_lsp` maps `Info`/`Hint` to LSP's `INFORMATION`/`HINT`
respectively (not collapsed onto `WARNING`), and
`initializationOptions.lints` accepts the same two new strings.
