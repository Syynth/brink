---
"@brink-lang/web": patch
---

The off-db analysis road (`session.analysis()` / `IdeSnapshot::analyze`)
now runs the `[project] conventions` confinement/unconfigured checks
(`E169`), matching the db-direct road (issue #2335).

A pattern-claiming `@[convention(claims = "…", order = …)]` handler declared
outside the project's configured conventions module — or declared at all
when no `[project] conventions` is configured — used to be silently
accepted by this analysis path (`analyze_with_modules` never read
`opts.conventions`), even though the identical db-direct query
(`conventions_confinement_diagnostics_query`) already flagged it.

No `@brink-lang/web`-exported surface renders `session.analysis()` today —
`compile()`/`EditorSession::compile_project()` and `editor_dto::
diagnostic_to_js` all go through the db-direct `compile` road instead, so
`packages/ink-editor` and `packages/brink-studio` see no new squiggles from
this change. The real beneficiary is `brink-lsp`'s `analysis_loop`, which
calls the fixed function directly with no `ProjectDb` in between. This
changeset is still required because `@brink-lang/web` re-exports the fixed
function's behavior, but the observable delta for today's JS consumers is
nil.
