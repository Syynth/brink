---
"@brink-lang/web": patch
---

#1348: the T1b ink dialect gate (`dialect_gate::check`, `E051`; and
`strict::config_error`, `E064`) no longer fires against native `.brink`
source. `dialect` is an ink-only axis (docs/t1b-surface-spec.md §1),
orthogonal to native's `Language` classification — a native project has no
"dialect" to be strict-ink about, so a native compile with `types = strict`
used to require a spurious, unrelated `dialect = brink` just to dodge `E064`
(surfaced by PR #1346's own strict-positive native test), and any native
construct the gate recognizes (`STRUCT` declarations, postfix indexing,
sigil literals, …) — all ordinary native syntax — could spuriously trip
`E051` under `dialect`'s `StrictInk` default.

`brink_analyzer::per_file_diagnostics` and `strict_diagnostics` both gained
an `is_native` flag; `brink-db`'s `per_file_diagnostics_query` and
`whole_project_diagnostics_query` compute it from the file's/project's
`Language` classification and skip the two ink-only checks accordingly. Ink
dialect gating is unaffected — `is_native = false` is byte-identical to
before this flag existed.

Reachable through any `@brink-lang/web` session that compiles a
`.brink`-extensioned entry: `EditorSession::compile_project` → `IdeSession::compile`
→ the same salsa `story_data()` seam `per_file_diagnostics_query` /
`whole_project_diagnostics_query` sit behind.
