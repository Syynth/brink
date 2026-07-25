---
"@brink-lang/web": patch
---

#1160: `brink.toml` gains a `[lints]` table — per-code severity overrides
(`deny`/`warn`/`allow`) plus a `deny-warnings` flag. Resolved through
`AnalysisOptions::apply_project_config` and consulted by every
diagnostic-partitioning call site in `brink-db`/`brink-driver`, so
`brink compile`, `brink ide`, and `brink-environment::compile` now respect
it: a project's `[lints]` table can turn a previously-warning-only
diagnostic into a build failure. Absent `[lints]` (the default, unchanged
case) is byte-identical to prior behavior. Only codes whose default
severity is `Warning` are overridable — a hard-error-by-default diagnostic
is never consulted against the table. An unknown or non-overridable code in
`[lints]` is now reported as a warning rather than silently accepted and
ignored.

**No behavior reachable through `@brink-lang/web` changes in this PR.**
`EditorSession::apply_project_config` (the wasm editor session's own
config-file entry point) applies only `[project] dialect`/`types`; it does
not go through `AnalysisOptions::apply_project_config` and does not read
`[lints]`/`deny-warnings` at all. `IdeSession`'s `AnalysisOptions`
construction sites are hardcoded to a no-op `LintPolicy::default()`. (An
earlier draft of this changeset incorrectly claimed the wasm editor session
already picked this up through a shared seam — it doesn't; tracked as a
follow-up in Syynth/brink#1366.)
