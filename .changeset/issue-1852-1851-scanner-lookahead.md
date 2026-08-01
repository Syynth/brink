---
"@brink-lang/web": patch
---

Parser: two native-scanner lookahead fixes.

`content::tag()` and `element::cue_name()` now count consecutive raw
`BACKSLASH`es before an `L_BRACE` instead of checking only the
immediately preceding token, so `\\{` (an escaped backslash followed by
a real, unescaped brace) is depth-counted correctly instead of being
mistaken for an escaped brace (issue #1852).

`element::cue_name()` now guards its `COLON` stop with the same
depth-zero check already used for `R_BRACE`, so a colon inside a
balanced `{…}` (e.g. `@NAME {a:b}`) is treated as part of the
interpolation rather than the cue name's terminator (issue #1851).
