---
"@brink-lang/web": patch
---

#1397: `AnalysisOptions::apply_project_config`'s `[lints]` handling now
**replaces** the resolved lint policy (per-code overrides plus
`deny-warnings`) with whatever `config` currently resolves, instead of
merging `config`'s entries key-by-key into whatever was already resolved.
A code (or `deny-warnings`) present in an earlier call but omitted from the
current one now reverts to its base severity, instead of staying stuck.

This is observable through `@brink-lang/web`: `EditorSession`
(`apply_project_config`/`discover_project_config`) is a long-lived session
that re-applies `brink.toml` on every change (#1366) — previously, deleting
a `[lints]` entry (or the whole table) from `brink.toml` left the
previously-applied override permanently stuck on that session, since
nothing ever removed it. It now reverts correctly on the next apply.
`brink-cli`, `brink-lsp`, and `brink-environment`/`bevy-brink` build a
fresh `AnalysisOptions` on every call to this function already, so this is
a no-op behavior change for them.
