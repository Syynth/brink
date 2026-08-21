---
"@brink-lang/web": patch
---

Fixed #2366: a mid-line block comment (`Prose /* mid */ here`) no longer
fragments its `CONTENT_LINE` into two separate lines with the comment
hoisted to `SOURCE_FILE` level. `mixed_content`'s catch-all arm now retries
past a zero-progress stop the same way its `L_BRACE` sibling arm already
did — but narrower than a blanket trivia-skip: only the comment token(s)
are elided, so whitespace on either side of the comment survives (matches
inklecate's own output for the `astrochili__narrator` corpus's
`comments.ink`, which produces a double space where a comment used to sit,
not a single space). A stray non-trivia stop token (`}`, `|`, `\` before
newline) still breaks the loop rather than retrying, so this cannot spin.
Parse-tree shape and diagnostics are editor-observable through
`@brink-lang/web` — the previous spurious "expected newline at end of
content line" diagnostic on a mid-line block comment is gone, and the CST
now has one `CONTENT_LINE` instead of two.
