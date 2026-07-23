---
"@brink-lang/web": patch
---

`brink-syntax-native`: a divert target's call-style args (`-> knot("x")`)
are now captured on the `DIVERT_TARGET` node instead of silently orphaning
into an unrelated sibling `CONTENT_LINE` with zero parse errors (issue
#1265, bug #1196). Charter §11 keeps `-> knot(args)` verbatim from ink.
`DivertTarget::call_args()` reads the captured `ARG_LIST` back; the
existing `DivertTarget::path()` shape is unchanged (`ARG_LIST`, when
present, is a direct sibling of `PATH`, not wrapped in a `CALL_EXPR`).

Purely a native-surface parser fix — `brink-syntax-native` is off the ink
compiler pipeline, so vanilla-ink stories and the oracle corpus are
unaffected.
