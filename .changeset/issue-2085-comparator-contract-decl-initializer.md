---
"@brink-lang/web": patch
---

`comparator_contract`'s `E119` pure-callback-verb gate (`sort_by`/
`sorted_by`/`map`/`filter`/`fold`/`filter_map`, issue #1110/#1679) now
checks file-level `VAR`/`CONST` initializer expressions, including a
decl-default lambda's own body (`const doIt = || map(xs, impureCallback)`,
legal since #1774). Previously `collect_sites` started only from
`root_content` + knot/stitch bodies, so an impure named callback written
directly in a declaration initializer — or nested inside a decl-default
lambda's body — silently compiled clean instead of being refused. Issues
#2085/#1769.
