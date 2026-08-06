---
"@brink-lang/web": patch
---

#1366: `EditorSession::apply_project_config` (the wasm editor session's
`brink.toml` entry point) now merges the file's `[lints]` table / `deny-warnings`
flag onto the session's resolved lint policy via
`AnalysisOptions::apply_project_config` — the same merge point
`brink-driver`/`brink-cli`/`brink-lsp` already use (#1160) — and
`IdeSession` carries the resolved policy through
`set_lint_policy`/`analysis_options`/`snapshot` instead of a hardcoded
no-op `LintPolicy::default()`. (`brink-web`'s `EditorSession` is the only
caller of `set_lint_policy` as of this PR; `brink-cli`'s IDE surface does
not yet forward a resolved policy into its own `IdeSession`.)

This delta is bigger than a re-rendered `severity` string: `IdeSession::compile`
feeds the resolved lints into the same closure-diagnostics partitioning
`brink compile` uses, so a `[lints]` entry that promotes a diagnostic to
`Error` — a per-code `"deny"` override or `deny-warnings = true` — now
makes `EditorSession::compile_project` return `ok: false` with
`story_bytes: null` for a file that previously compiled successfully with
only a warning. Callers that apply a `brink.toml` with `[lints]` should
expect this compile-failure outcome, not just a relabeled diagnostic.
Unknown or non-overridable lint codes surface through the same warnings
channel `apply_project_config` already uses for unrecognized `[project]`
keys. Absent `[lints]` (the default, unchanged case) is byte-identical to
prior behavior.
