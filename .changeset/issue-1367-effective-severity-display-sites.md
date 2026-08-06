---
"@brink-lang/web": patch
---

#1367 (follow-up to #1160/#1366): the four diagnostic **display** sites that
still read the raw `DiagnosticCode::severity()` default now render
`brink_analyzer::effective_severity` instead — `brink-web`'s
`diagnostic_to_js` (used by `compile`/`compile_fragment`/`EditorSession::
compile_project`), `brink-lsp`'s `diagnostic_to_lsp` (every publish site,
including a new `LanguageOptions.lints` field so a discovered `brink.toml`'s
`[lints]` table — previously resolved but never stored — actually reaches
the published severity), and `brink-ide`'s `structural_result::
introduced_diagnostics` (the safe-by-default breakage report `brink ide`'s
rename/move/delete commands and the wasm editor's `*_safe`/`gate` calls
both surface).

Reachable through `@brink-lang/web`: `EditorSession::compile_project`'s
`warnings[].severity` now promotes `E063` (annotation-vs-inference
mismatch) to `"Error"` under `types = strict`, matching the build-gating
severity `brink-db`'s partitioning already used — previously it always
showed `"Warning"` regardless of policy
(`compile_project_severity_reflects_strict_types_e063_promotion`).
`IdeSession`/`EditorSession` still have no `[lints]`-resolution input wired
(the #1160 changeset's tracked gap), so a `[lints]` override itself has no
observable effect through `@brink-lang/web` yet — only the `types`-driven
`E063` carve-out does. The plain `compile`/`compile_fragment` wasm entry
points always use `AnalysisOptions::default()` (no policy ever configured),
so their output is unchanged.
