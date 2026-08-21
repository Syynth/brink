---
"@brink-lang/web": patch
---

Fixed #2960: a mid-line comment (`/* ... */`) inside choice text (start
content before `[`, or between two `{...}` interpolations) no longer
fragments the choice into a spurious `expected newline after choice`
diagnostic with trailing text spilling into a bogus following content
line. `choice_content_elements` and `choice_content_element`'s `L_BRACE`
arm now retry past an elided comment the same way `content::mixed_content`
already does (#2366/#2958), reusing the same `Parser::skip_comment_tokens`
helper. Observable through `@brink-lang/web` as fewer/different parse
diagnostics and CST shape for `.ink` sources with a mid-line comment in
choice text.
