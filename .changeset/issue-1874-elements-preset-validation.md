---
"@brink-lang/web": patch
---

`brink-analyzer`: a bare `[project] elements` preset name (issue #1874,
remainder of #1844's item 5) is now validated against a closed built-in-
preset-name set in `AnalysisOptions::apply_project_config`. An
unrecognized name (a typo, or a preset that hasn't shipped yet) now
surfaces a `ConfigWarning` instead of being accepted silently. A
path-shaped value (e.g. `"conventions.brink"`, `"scenes/conventions.brink"`)
is never rejected by this check — that shape is a project-relative
pointer to a custom conventions module, not a preset name.

The closed set is empty today: no built-in preset has shipped as a real
`std::conventions::*` module yet (#1720, the screenplay preset, is still
open), so even `elements = "screenplay"` itself is currently reported as
unrecognized — every mount that threads `brink.toml` through
`apply_project_config` (the CLI, `brink ide`, `brink-lsp`, and
`@brink-lang/web`'s `EditorSession::apply_project_config`) now surfaces
that warning where it previously surfaced nothing at all.
