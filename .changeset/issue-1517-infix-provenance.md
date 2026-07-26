---
"@brink-lang/web": patch
---

Issue #1517: HIR infix expressions (`lhs op rhs`) now carry their own
source `Provenance`, so an `or`-coalescing chain and its own left spine
are separately addressable in the analyzer's typing side table. Before
this, an infix node's only identity was the union of the ranges reachable
in its subtree, which a chain shared with its left spine whenever the
trailing operand carried no range of its own (`some(a) or f() or 99`), so
the analyzer had to drop *both* verdicts rather than risk serving one
node's verdict under another's key.

Web-observable effect is narrow but real: a `types = strict` brink-native
chain whose key previously collided lost its recorded shape verdict and
fell back to the runtime coalesce check; it now keeps the verdict and
lowers to the shape its types imply. No diagnostic, ink-dialect, or
bytecode change otherwise — the oracle corpus holds at 5,599 episodes.
