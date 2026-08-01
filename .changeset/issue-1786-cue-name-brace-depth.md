---
"@brink-lang/web": patch
---

Parser: `element::cue_name()` (the `@NAME` cue-name raw scan) now tracks
brace depth instead of stopping unconditionally at the first `}` (issue
#1786). A cue name containing a balanced `{…}` — e.g. `@NAME {gold}
coins.` inside a `flow f() { … }` body — was mistaking that balanced
`}` for the enclosing block's own closer, ending the block early and
turning otherwise-clean source into a parse error. Fixed the same way
`content::tag()` was fixed for the sibling case in #1777/#1728: an
`L_BRACE` preceded by an odd number of consecutive raw `BACKSLASH`es is
excluded from the depth counter, since `\{` is the literal-brace escape
(#1716), not a metacharacter — an even count means the backslashes
escape each other, leaving the brace unescaped and depth-counted
(#1852).
