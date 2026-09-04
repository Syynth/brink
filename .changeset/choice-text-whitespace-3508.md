---
"@brink-lang/web": patch
---

A choice's presented text keeps its interior whitespace runs verbatim, as
ink does (#3508): `* [a  0]` presents `a  0` (it presented `a 0`). Output
lines still collapse runs to one space. The line-table entries for choice
display text carry the verbatim text (visible in exported XLIFF for
choices whose text had runs); line identity hashes are unchanged.
