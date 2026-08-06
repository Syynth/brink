---
"@brink-lang/web": patch
---

Fix a live regression (since #2289): `EditorSession` (and `brink-lsp`) now
carries `brink.toml`'s `[project] conventions` pointer all the way into the
live analysis/compile db, instead of silently discarding it after
validation. Before this fix, every `IdeSession`-backed editor read the
pointer as unconditionally unset, which `conventions_confinement_diagnostics_query`
(#2289) treats as "misconfigured" rather than "nothing to check" — so `E169`
fired on every `@[convention]` handler in every native project opened in
the editor, and the resulting unclaimed scene headings dropped their whole
scene body from analysis (hover/go-to-def/completions inside them saw
content that did not exist).

`IdeSession` gains a `conventions: Option<String>` field + `set_conventions`
setter, mirroring the existing `set_language_dialect`/`set_type_policy`
wiring; `EditorSession::apply_parsed_config` and `brink-lsp`'s
`LanguageOptions`/`analysis_loop` now forward the resolved value the same
way. `@brink-lang/web`'s `explainMatch`/`explainMatchDoc` (issue #2113) are
also now reachable end to end for the first time — previously they always
reported "unconfigured" through the editor even when a real conventions
module was declared.
