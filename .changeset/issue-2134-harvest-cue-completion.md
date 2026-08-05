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
the raw harvest index, so a completion request never forces a
project-wide re-merge sensitive to every text-range shift.
