---
"@brink-lang/web": patch
---

#1342 (B0.9 close): the native strict-only enforcement point —
`brink_analyzer::native_strict_only_error`, a fresh `DiagnosticCode::E137`.
A native `.brink` file compiled with an explicit `types = gradual` knob is
now a hard error: gradual typing does not exist on the native surface
(decision-log 2026-07-19 "Typing posture ruled"), and this closes the gap
PR #1341 (issue #1179) left open — that slice delivered only the HIR-shape
accept-list (`E133`-`E136`), not "the strict-only ruling's enforcement
point" docs/b0-sequencing.md §B0.9 also discharges.

Wired at `brink-db`'s per-file diagnostics seam (`per_file_diagnostics_query`),
which already has both a file's `Language` classification and
`AnalysisOptions` access — `lower_native_file` cannot host this check (issue
#1179's finding: no `db`/`AnalysisOptions` there). Keyed on the *explicit*
`types` field, not the dialect-defaulted resolution, so a bare `.brink`
compile with no `types` config is unaffected; only an explicit
`types = gradual` (a CLI flag, a `brink.toml` entry, or a programmatic
`AnalysisOptions`) reaching a native file trips it.

Reachable through any `@brink-lang/web` session that compiles a
`.brink`-extensioned file with an explicit gradual `types` policy.
