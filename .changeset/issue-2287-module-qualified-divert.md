---
"@brink-lang/web": patch
---

Fix #2287: a module-qualified divert (`-> barter::haggle`, after
`use story::market::barter;`) now resolves — the native lowering was
normalizing `::` to `.`, making it indistinguishable from ink's own dotted
`knot.stitch` addressing, so it could never match. The over-permissive flip
side is also fixed: a bare `-> haggle` after only a module-qualified import
now correctly stays unresolved (`E025`/unresolved-divert), rather than
silently accepting a name only a symbol-level or glob `use` should bring
into scope.
