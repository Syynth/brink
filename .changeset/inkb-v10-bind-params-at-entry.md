---
"@brink-lang/web": patch
---

`.inkb` format v10: parameters are bound by the VM at entry

Codegen no longer emits the leading `DeclareTemp` prologue that bound a
parameterized container's arguments; the runtime binds them into the call
frame at every entry instead (`docs/compiler-spec.md` §"Parameter
binding"). Story behaviour is unchanged, but compiled bytecode is not: a
function, parameterized knot, tunnel, or thread starts at its first real
instruction now, so `EmitLine` and jump offsets shift and the program model
shows no `declare_temp` run at the top of those containers. Every `.inkb`
carries version 10, and a stored v9 artifact handed to `StoryRunner` or
`linesTableOf` is rejected with an unsupported-version error and must be
recompiled — deliberately, because a v9 prologue would otherwise decode
cleanly and re-bind parameters from an empty stack.
