---
"@brink-lang/web": patch
---

Implemented `\!`/`\@` as the prose-dialect's ruled line-start escapes (#1744,
`docs/prose-dialect-spec.md` §8d.6). Previously any leading `\!`/`\@` on a
native `.brink` content line hit the same "backslash before anything else is
a compile error" diagnostic as an unrecognized inline escape; now they
produce a literal `!`/`@` as the first character of the line, matching the
ruling. `\@NAME` at line start no longer opens a `CUE` — it stays plain text.

Observable through `@brink-lang/web`: native-dialect source that previously
failed to compile with an "invalid escape sequence" diagnostic on a
line-start `\!`/`\@` now compiles and runs. Anywhere else in a line, `\!`/`\@`
are unaffected and remain the same compile error.
