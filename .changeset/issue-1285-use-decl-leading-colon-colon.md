---
"@brink-lang/web": patch
---

Fix (#1285): a `.brink` `use` line whose path starts with `::` (e.g.
`use ::foo;`) no longer partially parses as a malformed `USE_DECL` with
confusing errors, and no longer silently falls through as unremarked prose
either. `at_use_decl`'s lookahead now only commits to `USE_DECL` when the
token after `use` is an identifier; a leading `::` instead reports a
targeted diagnostic ("a `use` path cannot start with `::`") before falling
through, so the typo is surfaced instead of becoming player-facing text.
