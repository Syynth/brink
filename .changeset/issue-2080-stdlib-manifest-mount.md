---
"@brink-lang/web": patch
---

#2080: `brink-environment`'s `Project::load` now mounts the built-in
stdlib source (`std/conventions/screenplay.brink`, embedded at compile
time via `include_str!`) into every `Environment`'s manifest, alongside a
project's own sources. This is the compiler's sole production compile
path (`brink_environment::compile`), which `@brink-lang/web`'s wasm
`compile.rs` entry point goes through directly — so every wasm compile
now sees one extra source key (`std/conventions/screenplay.brink`) join
the compilation universe, compiled as an ordinary native module alongside
the project's own files.

This is a **mount only**: the stdlib module is present in every
compiled project, and its native module identity mints exactly as it
would for a project file at that path — but nothing in it is marked
`pub` and no confinement rule scopes imports into it yet. There is no
`use std::…` surface reachable from a project's own source in this PR
(that needs #1582's pub marker and #2167's closure-scoped confinement,
tracked separately). A project whose own source happens to already use
the same key (`std/conventions/screenplay.brink`) is unaffected — its own
file wins over the embedded copy rather than being silently overridden.
