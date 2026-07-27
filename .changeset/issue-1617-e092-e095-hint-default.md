---
"@brink-lang/web": patch
---

#1617: two of the four candidates #1162 named for the `Info`/`Hint`
advisory tier get their *default* severity moved — `E092` (a
`#@public`/`#@private` override that restates the module's own default)
and `E095` (`#@was(name)` naming the definition's own current name) now
default to `Hint`, down from `Warning`. Both are directives whose emission
site proves they change nothing about the compiled output regardless of
whether the author acts on them — `E092`'s override is discarded by
`effective_visibility` either way, and `E095`'s self-alias leaves
`ModuleDecl::was` unset exactly as if the directive were absent. `E038`
(malformed doc-comment tag) and `E043` (inapplicable doc-comment tag),
the other two named candidates, are left at `Warning`: unlike `E092`/
`E095`, their emission sites show the tagged doc content is silently
dropped from the parsed `DocBlock` — a real, actionable loss, not a pure
no-op — so softening them risked hiding a genuine defect.

Reachable through `@brink-lang/web`: the wasm editor session's
`diagnostic_to_js` (`brink-web/src/editor_dto.rs`) renders
`brink_analyzer::effective_severity`, so an unconfigured `E092`/`E095`
diagnostic now serializes as `"Hint"` instead of `"Warning"` wherever the
session applies `brink.toml`'s `[project]`/`[lints]` (issue #1366) — same
seam #1367 already wired through, just handed a code whose *default*, not
just an override, now resolves below `Warning`.

Fixes a latent bug this reclassification's own implementation surfaced:
`brink_analyzer::effective_severity`'s hard-error exemption
(`if base != Warning { return base; }`) predates any code defaulting to
`Info`/`Hint`, and would have silently made `E092`/`E095` permanently
un-overridable by `[lints]` the moment their default moved off `Warning`
— the check now only short-circuits on `Error`. The matching
`validate_lint_code` (`brink-analyzer`) and `@[allow(…)]` suppressibility
gate (`brink-ir::hir::lower_native::annotation::parse_allow`) had the
same `== Warning`/`!= Warning` conflation and are fixed the same way, so
an existing `[lints] E092 = "deny"` or `@[allow(E092)]` an author already
had keeps working unchanged.
