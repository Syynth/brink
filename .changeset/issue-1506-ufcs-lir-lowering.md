---
"@brink-lang/web": patch
---

Issue #1506: LIR/codegen now consumes the `ufcs_resolution` verdict table
PR #1497 (#1482) shipped. A `.brink` method-call site (`recv.name(args)`)
that resolves cleanly at analysis now actually compiles and runs instead of
being refused with `E144` at lowering: field access wins over a same-named
free function and lowers as a call through the field's value, a free
function in ordinary lexical scope desugars to `name(recv, args)`, and a
T1b/NS stdlib prelude verb (or classic ink builtin) desugars the same way
through the existing builtin/stdlib dispatch. `E144` remains as a defensive
fallback for a resolved site with no recorded verdict (only reachable by a
caller that skips the analyzer's `ufcs` pass); it is no longer the blanket
refusal every resolved method call hit.

Web-observable through `compileProject`: a native `.brink` entry using
method-call syntax onto a field-typed-function, a free function, or a
prelude verb — previously always a compile refusal (`E144`) — now compiles
and its `StoryData` runs the call for real. `.ink` compiles are unaffected
(ink's own lowering cannot produce the multi-segment callee path this pass
keys on). Auto-ref (a free function reached through method syntax whose
first parameter is `ref`) stays out of scope — refused with `E143`,
pointing at #1462.
