---
"@brink-lang/web": patch
---

Editor: a native project's own declarations no longer spuriously collide
with the mounted stdlib's same-named declarations when the project's own
`brink.toml` is loaded into the same session (issue #2318).

`std/conventions/screenplay.brink` declares `struct Cue`, `fn cue`, and
`fn heading`. Under #2245's peer-root ruling `std::` is a peer of `story::`,
not a parent, so a project declaring its own `Cue`/`cue`/`heading` must
coexist with the mount rather than collide with it — `brink compile` on
such a project already exits `0`. Only `EditorSession`'s off-db analysis
disagreed, and only when the project's `brink.toml` shared the session (as
`EditorSession`'s real callers load it, so the Binder can list/edit it):
`ProjectDb::is_all_native` — the gate `IdeSession`'s M-2d cross-declared-
module coexistence check reads — used to answer `false` the moment the db
held even one non-`.brink` file, including a `brink.toml` config document,
disabling the exemption for what was, in every sense a compile cares about,
a fully native project. The visible symptom was a self-contradictory pair
of diagnostics for any name shared with the mount: reported as both a
duplicate definition and as undeclared outside `use std::…` in the same
run.

`ProjectDb::is_all_native`/`project_is_all_native` now ignore any tracked
file with neither a `.brink` nor an `.ink` extension when deciding whether
a project is "all native" — such a file (a `brink.toml`, or any other
non-source document a host's file tree loads into the same session) no
longer counts as "an ink file" against the check.
