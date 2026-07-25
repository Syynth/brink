---
"@brink-lang/web": patch
---

#1160: `brink.toml` gains a `[lints]` table — per-code severity overrides
(`deny`/`warn`/`allow`) plus a `deny-warnings` flag, mirroring Rust's own
`[lints]` table. Resolved through the same `apply_project_config` seam the
wasm editor session already calls, so a project's `[lints]` table now
changes which diagnostics `EditorSession`/`compile` report as errors vs.
warnings. Absent `[lints]` (the default, unchanged case) is byte-identical
to prior behavior. Only codes whose default severity is `Warning` are
overridable — a hard-error-by-default diagnostic is never consulted
against the table, so it can never be downgraded.
