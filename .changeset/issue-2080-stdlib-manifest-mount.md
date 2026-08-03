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
the manifest. What that key's presence *does* on a given compile depends
on the entry's dialect: for a **native** (`.brink`) entry, discovery is
tree-is-universe, so the mounted module joins the compilation closure and
is compiled as an ordinary native module alongside the project's own
files. For an **ink** (`.ink`) entry — `@brink-lang/web`'s ordinary
case — the closure instead follows the entry's `INCLUDE` graph, which has
no edge into the mounted key, so it stays manifest-only: present, never
lowered, contributing nothing to that compile.

This is a **mount only**: the stdlib module is present in every
compiled project's manifest, and — for a native entry — its module
identity mints exactly as it would for a project file at that path; but
nothing in it is marked `pub` and no confinement rule scopes imports into
it yet. There is no `use std::…` surface reachable from a project's own
source in this PR (that needs #1582's pub marker and #2167's
closure-scoped confinement, tracked separately). A project whose own
source happens to already use the same key
(`std/conventions/screenplay.brink`) is unaffected — its own file wins
over the embedded copy rather than being silently overridden.
