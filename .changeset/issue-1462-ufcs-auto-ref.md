---
"@brink-lang/web": patch
---

Issue #1462 (D5): UFCS method-call syntax now **auto-refs** its receiver when
the resolved free function's first parameter is declared `ref`. A `.brink`
source that writes `gold.bump(1)` against `fn bump(ref n, amount)` compiles
and runs the mutation for real — internally the desugar spells the reference
as `bump(ref gold, 1)` (desugar notation; the native surface has no
call-site `ref` keyword, so the spellable equivalent is the unmarked
`bump(gold, 1)`, and a dotted receiver becomes an explicit T1e projection,
`party.leader.heal(5)` → `heal(ref party.leader, 5)`) and rides the existing
ref-argument/projection lowering, so a `ref` parameter's write lands in the
receiver's own cell instead of a copy.

A non-`ref` first parameter is unchanged: plain by-value desugar, with no
lvalue requirement on the receiver. A receiver that cannot be written
through is refused with `E143` ("cannot mutate …") instead of being silently
desugared by value — a `CONST` receiver, or a projection rooted in a
frame-local (T1e's durable-root rule).

Web-observable through `compileProject`: a native `.brink` entry calling a
`ref`-first-parameter function through method syntax previously always
refused to compile (`E143`, "not supported yet"); it now compiles, and its
`StoryData` performs the mutation. `E143`'s message and title change with
it — the code now names the ruled refusal, not the missing feature. `.ink`
compiles are unaffected (ink's own lowering cannot produce the multi-segment
callee path this keys on).
