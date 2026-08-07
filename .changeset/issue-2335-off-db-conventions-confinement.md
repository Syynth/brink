---
"@brink-lang/web": patch
---

Editor: the off-db analysis road (`session.analysis()` /
`IdeSnapshot::analyze`) now runs the `[project] conventions`
confinement/unconfigured checks (`E169`), matching the db-direct road
(issue #2335).

A pattern-claiming `@[convention(claims = "…", order = …)]` handler declared
outside the project's configured conventions module — or declared at all
when no `[project] conventions` is configured — used to be silently
accepted by this analysis path (`analyze_with_modules` never read
`opts.conventions`), even though the identical db-direct query
(`conventions_confinement_diagnostics_query`) already flagged it. Any
consumer reading the session's off-db analysis result (rather than querying
the db directly) now sees the same `E169` the db-direct road always did.
