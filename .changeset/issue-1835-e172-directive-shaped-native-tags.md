---
"@brink-lang/web": patch
---

Compiler: a native (`.brink`) tag whose text begins with `@` — the shape
of an ink-dialect compiler directive (`#@private`, `#@was("…")`,
`#@local`, `#@module("…")`, `#@effects(…)`) — now raises a targeted
diagnostic (`E172`, issue #1835) instead of compiling silently as an
ordinary runtime tag.

`#@…` is not its own grammar production in either dialect — it is an
ordinary tag, and only ink's HIR lowerer gives a leading `@` special,
compile-time-consumed meaning. Native's tag lowering never checked for
it, so an author porting a file from ink, or splitting time between the
two dialects, got no error and no warning: the directive text silently
became literal tag content on the compiled story.

`E172` is `Warning`-severity and `@[allow(E172)]`-suppressible, not
`Error` — a project may legitimately want a literal `@`-led runtime tag,
so the diagnostic never blocks a compile that means it. The message
names the native `@[name(…)]` annotation equivalent to switch to when
one exists (`was`, `allow`, `effects`), and says plainly that there is
none when there isn't (`module`, `public`, `private`, `local`).

`brink-web` transitively depends on `brink-ir`'s native lowering
(`brink-db::lowered_query` dispatches `.brink`-extension files to native
parsing/lowering, non-optional), so this new diagnostic is
wasm-observable for `.brink` projects — an `@`-led tag now reports
`E172` in the editor instead of compiling silently as ordinary tag text.
