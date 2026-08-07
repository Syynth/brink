---
"@brink-lang/web": patch
---

Internal refactor, no observable behavior change: `EditorSession::apply_parsed_config`
now resolves `[project]`/`[lints]` into one `AnalysisOptions` and forwards
`dialect`/`types`/`lints`/`conventions` onto its `IdeSession` through the new
shared `IdeSession::apply_analysis_options` seam (issue #2334), instead of
four hand-copied setter calls each individually change-guarded. The same
field (`conventions`) had been dropped by this hand-written forwarding three
times running across three separate `IdeSession` producers (#1880 → #2316,
then #2317 → #2325) — routing every producer through one seam means a future
`AnalysisOptions` field only needs a forwarding decision made once. Verified
byte-identical against the full `brink-web` test suite (including the
`acceptance_gate`) and the `#1005`/`#1397` precedence-tier regression tests.
