---
"@brink-lang/web": patch
---

Issue #2134: `EditorSession::completions`/`completions_doc` now offer every
`@NAME` cue harvested anywhere in the project — not just the active
document — right after typing `@` at the start of a line
(`CompletionContext::CueName`). This is the completion-UI consumer the
#2114 harvest index landed for but nothing yet called: a cue declared only
in a sibling file, never imported, now completes while editing an unrelated
file, with no conventions handler or host manifest required
(`docs/prose-dialect-spec.md` §5, "harvest by default"). Reads a new
range-free projection (`ProjectDb::harvest_completion_names`) instead of
the raw harvest index — the same `Eq`-cutoff seam `resolution_index_query`
gives the symbol index. Correction (review finding): the projection still
depends on the whole-project harvest merge (`harvest_index_query`), so a
completion request does NOT skip that merge — what the projection buys is
an `Eq`-stable value a *memoized downstream* consumer could backdate on
across a pure range-shifting edit. No such consumer exists yet today (both
`brink-lsp` and `brink-web` read `harvest_completion_names()` directly,
per request), so the measured present-day incrementality benefit is zero;
the seam is there for whoever memoizes on top of it next.
