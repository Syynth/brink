---
"@brink-lang/web": patch
---

Issue #1263 (ruled #1260): the native `.brink` parser now warns when `<-`
appears outside a choice point instead of silently swallowing it as prose.
Charter §11 narrows threads to scoped splices inside `{? … }` choice
points, so a stray `<-` is almost always a misremembered ink thread — the
new `E131` diagnostic flags it at warning severity (never blocking
compilation, since `<-` can also be literal dialogue punctuation) and
raises confidence in its message when the tokens after `<-` are shaped
like a real knot/flow reference. Only affects native `.brink` sources;
ink `.ink` sources and the oracle corpus are unaffected.
