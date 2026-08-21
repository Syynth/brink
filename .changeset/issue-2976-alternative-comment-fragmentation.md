---
"@brink-lang/web": patch
---

Fixed #2976: a mid-line comment (`/* ... */`) inside an inline-alternative
branch (`{a|b}`, `{cond: a|b}`, sequences, and multiline conditional
branch bodies) no longer fragments the alternative into a destroyed parse
(the `|` becoming an `ERROR` node and the closing `}` becoming
`STRAY_CLOSING_BRACE`). `inline::branch_content`'s catch-all arm and the
two `multiline_branch_text` call sites (`branchless_cond_body`,
`multiline_branch_body`) now retry past an elided comment the same way
`content::mixed_content` (#2366/#2958) and `choice::choice_content_elements`
(#2960/#2974) already do, reusing the same `Parser::skip_comment_tokens`
helper. Observable through `@brink-lang/web` as fewer/different parse
diagnostics and CST shape for `.ink` sources with a mid-line comment
inside an alternative.
