---
"@brink-lang/web": patch
---

#1366: `EditorSession::apply_project_config` (the wasm editor session's
`brink.toml` entry point) now merges the file's `[lints]` table / `deny-warnings`
flag onto the session's resolved lint policy via
`AnalysisOptions::apply_project_config` — the same merge point
`brink-driver`/`brink-cli`/`brink-lsp` already use (#1160) — and
`IdeSession` (the shared session type behind both `brink-web` and
`brink-cli`'s IDE surface) carries the resolved policy through
`set_lint_policy`/`analysis_options`/`snapshot` instead of a hardcoded
no-op `LintPolicy::default()`. A project's `[lints]` re-leveling of a
diagnostic's severity is now visible through `EditorSession::compile_project`'s
`severity` field, not just through the CLI compile path. Unknown or
non-overridable lint codes surface through the same warnings channel
`apply_project_config` already uses for unrecognized `[project]` keys.
Absent `[lints]` (the default, unchanged case) is byte-identical to prior
behavior.
