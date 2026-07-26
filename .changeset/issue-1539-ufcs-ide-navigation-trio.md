---
"@brink-lang/web": patch
---

Fix (#1539): `find_references` and `rename` now also resolve a UFCS
call site (`recv.verb(args)`) to the free function it dispatches to,
matching the fix #1534 already landed for hover/go-to-definition.

Before this fix, both editor operations keyed off `ResolutionMap`
alone, whose entry for a UFCS call spans the receiver — so querying
references from (or renaming) a free function called only via UFCS
syntax silently missed those call sites entirely. Renaming a free
function that had UFCS call sites produced a broken program: the
declaration moved to the new name, but every `recv.verb(...)` call
site was left referring to the old name.

Both operations now enumerate a target's UFCS call sites through the
same `ufcs_resolution_query` verdict table hover/go-to-definition
already read, narrowly scoped to each call's own method segment.
