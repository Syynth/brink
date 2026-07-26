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
method-call syntax onto a free function or a T1b/NS stdlib prelude verb
(including the collection mutators — `m.insert(k, v)`, `a.push(v)`, etc.,
lowered the same statement-only way their bare-call form is) — previously
always a compile refusal (`E144`) — now compiles and its `StoryData` runs
the call for real. The field-access `FieldCall` verdict also lowers for
real, but is not yet reachable from any native `.brink` source: the grammar
cannot yet spell a function-typed struct field
(`brink-syntax-native`'s `struct_field` parses only bare-path field types),
so this half is exercised by a hand-rewritten HIR fixture
(`brink-ir/tests/ufcs_field_call.rs`), not an end-to-end `.brink` case.
`.ink` compiles are unaffected (ink's own lowering cannot produce the
multi-segment callee path this pass keys on). Auto-ref (a free function
reached through method syntax whose first parameter is `ref`) was out of
scope here; it lands separately in #1462, in this same release.
