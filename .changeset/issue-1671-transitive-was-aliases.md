---
"@brink-lang/web": patch
---

Compiler: `#@was` on a knot or stitch now emits one compiled alias-table
entry per descendant re-keyed by the rename, not just one for the renamed
declaration itself (issue #1671).

Renaming a knot re-keys every stitch and label beneath it, because their
qualified names embed the knot's name — but `#@was` previously minted
exactly one alias entry (the knot's own), so a declared rename still lost
every descendant's saved visit count and translations. The compiler now
walks every stitch/label whose qualified name is prefixed by the renamed
container and mints a bridging entry for each, while it still knows every
descendant's path — the loader cannot recover this at load time, since a
`DefinitionId` is a hash and no path can be derived from one. Table growth
is bounded by the renamed container's subtree size.
