---
"@brink-lang/web": patch
---

Fixed the formatter silently dropping a comment attached to a
`~ { … }` logic block outside its body (observable via
`format_document`). A comment that is a direct child of the logic line —
trailing after the closing brace (`} /* note */`, `} // note`) or
leading between `~` and `{` (`~ /* c */ {`) — was deleted, because the
block body was rebuilt from the inner statement block alone. The block
renderer now emits leading comments on the header line and trailing
comments on the closing line. A leading comment on the opening line no
longer de-indents the body to column 0, and a single-line block that
carries a trailing comment now expands to the canonical multiline form
(matching the comment-free case) instead of being frozen verbatim.
