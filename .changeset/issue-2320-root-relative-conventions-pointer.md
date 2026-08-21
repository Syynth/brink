---
"@brink-lang/web": patch
---

Fix #2320: a relative `[project] conventions` pointer resolved against the
process's current directory instead of the declared `native_root`, and an
unresolvable pointer (typo'd/moved/deleted target) was silently swallowed
with no signal reachable from wasm.

`brink_db::modules::root_relative_key(native_root, pointer)` absolutized a
relative pointer via `std::path::absolute`, which resolves a relative path
against the **process's cwd**, not the `native_root` argument passed in.
This was reachable only through `brink compile`/`brink check` (a one-shot
process usually invoked with cwd == project root, so the divergence rarely
bit) until PR #2316 wired `brink-lsp`'s persistent `analysis_loop` to
resolve `[project] conventions` for the server process's whole life:
`brink-lsp` never calls `std::env::set_current_dir`, so a session launched
from a directory other than the project root (e.g. `native_root=/project`
launched from cwd `/project/scenes`) silently confined to the wrong module
for the entire session. `root_relative_key` now resolves a relative pointer
directly against `native_root` (join, then lexically normalize) instead of
falling through to `std::path::absolute`'s cwd-relative behavior — cwd no
longer enters the computation when a root is declared.

**`@brink-lang/web` is affected differently.** `EditorSession` never
declares a `native_root` at all — its files are keyed by already
tree-relative virtual paths with no OS-filesystem anchor, so threading a
real root through (the LSP's fix) does not translate cleanly here: setting
one would also retroactively re-derive every file's own module identity via
the same `root_relative_key` strip, which is not this session's intended
behavior (see the PR body for the full reasoning and why this route, not
that one, was chosen). Instead, the specific silent-drop this issue reports
for `brink-web` — a `[project] conventions` pointer that resolves to no
real file in the project (most commonly: one discovered at a nested
`brink.toml` document key) took the "does not match any file" arm, which
was a bare `tracing::warn!` returning zero diagnostics. `brink-web`'s wasm
build has no `tracing` subscriber at all, so that warning reached nothing an
embedder could ever observe — `compile_project`'s returned warnings stayed
empty, indistinguishable from "everything is fine," while
`ConventionsProjection` (what `explain_match` reads) stayed silently empty
too. Both `conventions_confinement_diagnostics` (the off-db road
`IdeSnapshot::analyze`/`EditorSession` actually run) and its db-direct
sibling `conventions_confinement_diagnostics_query` now push a real `E169`
diagnostic in this case instead of staying silent — worded to blame the
pointer itself ("does not match any file... fix the `conventions` pointer"),
never `conventions_module_diagnostics`'s "move it there" message, since
there is no correct destination to name when the pointer doesn't resolve.

Pinned by `root_relative_key_resolves_a_relative_pointer_against_root_not_cwd`
and `conventions_confinement_survives_a_relative_pointer_with_native_root_and_a_nested_lsp_cwd`
(both reproduce the LSP's exact `native_root`/nested-launch-cwd scenario,
red before this fix), `unresolvable_pointer_is_e169_naming_the_pointer_not_a_destination`
(`brink-analyzer`), `an_unresolvable_conventions_pointer_is_e169_naming_the_pointer_not_a_destination`
(`brink-db`), and `compile_project_surfaces_an_unresolvable_conventions_pointer_as_e169`
(`brink-web`, exercised through the real wasm-exported `compile_project` entry
point `packages/ink-editor`'s `ProjectSession` calls).
