---
"@brink-lang/web": patch
---

Fix #2238: a `STRUCT` shape table now supports two same-named shapes
coexisting (a project's own `struct Cue { … }` alongside a mounted std
preset's own same-named `struct Cue { … }`), resolved by referrer file —
the same rule #2197 already applies to knots/externals — instead of a
single project-wide bare-name winner. Previously the mounted preset's
shape could silently claim the bare name ahead of the project's own
declaration, and the project's construction literal would then bind
against the wrong (narrower) shape, faulting at runtime with `struct shape
id <u32::MAX> out of range`. Observable through `@brink-lang/web` for any
native project that both declares its own struct and mounts a std preset
declaring a same-named one.
