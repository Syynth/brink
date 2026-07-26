---
"@brink-lang/web": patch
---

IDE: renaming the head of a plain dotted field access (`p.x.y`, not a UFCS
call) no longer corrupts the reference site (issue #1560, the non-UFCS-call
half of the #1550/#1539 corruption class).

`resolve::lookup_variable`'s dotted-field-access fallback records a plain
field-access reference's resolved range as the *whole* `p.x.y` path, not
just the head segment — renaming `p` previously rewrote that whole span,
collapsing `p.x.y` into `newname` and silently dropping `.x.y`. `rename` and
`find_references` now narrow to the head segment's own range for this case,
the same way they already do for a UFCS call's receiver (`recv.verb(args)`,
#1550).
