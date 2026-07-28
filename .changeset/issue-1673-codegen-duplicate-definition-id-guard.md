---
"@brink-lang/web": patch
---

#1673: codegen now refuses to emit a `StoryData` where two containers share
a `DefinitionId`, failing loudly with an `E060` internal-codegen-error
diagnostic instead of silently letting the linker's last-write-wins address
map drop one container's entry. This closes the exact failure mode #1504
demonstrated: two files with root-level weave content colliding on an
anonymous id, where a player picking a choice from one file's weave ran the
*other* file's choice body.

**Observable through `@brink-lang/web`**: `brink-web`'s compile session
(`session.rs`/`compile.rs`) calls `brink_compiler::compile` directly, so
any project shape that trips this guard now fails compilation there too,
surfaced as an `E060` diagnostic rather than compiling to a broken story.

The #1504 root cause (unqualified anonymous scope paths) is unchanged and
still blocked on the FG-4d identity ruling — this guard only changes the
failure mode from silent-wrong-output to loud-compile-error. One other
existing, source-reachable path now also trips it: two knots sharing an
author name (`E022`, warning-severity) collide on the same content-hashed
id and previously compiled silently; that shape now fails to compile too.
Whether `E022` itself should be promoted to a hard error is a separate,
undecided design question.
