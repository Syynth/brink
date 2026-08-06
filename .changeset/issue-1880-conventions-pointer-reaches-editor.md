---
"@brink-lang/web": patch
---

Fix a live regression: `EditorSession` (and `brink-lsp`) now carries
`brink.toml`'s `[project] conventions` pointer all the way into the live
analysis/compile db, instead of silently discarding it after validation.
Before this fix, every `IdeSession`-backed editor and every `brink-lsp`
background analysis pass fed the db a hardcoded `None` regardless of what
`brink.toml` configured. On the `brink-lsp` path this meant
`external_claim_handlers_query` never saw the conventions module's
`@[convention]` handlers, so they claimed no prose outside their own file —
unclaimed scene headings elsewhere fell to `lower_native`'s `E129` arm and
dropped their whole scene body from analysis (hover/go-to-def/completions
inside them saw content that did not exist). The confinement gate
(`E169`, #2289) itself is a `brink-db`-only query neither the LSP's
background loop nor `IdeSession`'s off-db analyzer path ever calls, so it
was never reachable from either editor surface — this fix does not change
`E169` behavior.

`IdeSession` gains a `conventions: Option<String>` field + `set_conventions`
setter, mirroring the existing `set_language_dialect`/`set_type_policy`
wiring; `EditorSession::apply_parsed_config` and `brink-lsp`'s
`LanguageOptions`/`analysis_loop` now forward the resolved value the same
way. `@brink-lang/web`'s `explainMatch`/`explainMatchDoc` (issue #2113) are
also now reachable end to end for the first time — previously they always
reported "unconfigured" through the editor even when a real conventions
module was declared.
