---
"@brink-lang/web": patch
---

A function whose output ends in a value that renders as whitespace — an
empty list, `""`, a `none` — no longer leaves a blank line behind: the
function-end trim now treats such a value as the whitespace ink already
sees there, and trims the newline behind it too (#3536).
