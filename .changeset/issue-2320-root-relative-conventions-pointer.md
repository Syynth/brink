---
"@brink-lang/web": patch
---

Fix #2320: a relative `[project] conventions` pointer resolved against the
process's current directory instead of the declared `native_root`, and an
unresolvable pointer was silently swallowed with no signal reachable from
wasm.

**The resolution fix lives at the pointer's read site, not in the shared
key normalizer.** `brink_db`'s `expected_conventions_module` routed the
pointer through `root_relative_key`, which absolutizes a relative path
against the **process's cwd** — correct for registered file keys (the ink
CLI registers the entry in its cwd-relative spelling, so a bare `main.ink`
compiled from `cwd = root/sub` must key as `sub/main.ink`; changing that
arm breaks CLI content identity by invocation cwd), but wrong for the
pointer: a relative `conventions` value is written in `brink.toml`, whose
directory defines the root, so relative means **root-relative by
definition**. This was reachable only through one-shot `brink compile`/
`brink check` (usually invoked with cwd == project root, masking it) until
PR #2316 wired `brink-lsp`'s persistent `analysis_loop` to resolve the
pointer for the server process's whole life: the LSP never calls
`std::env::set_current_dir`, so a session launched from a directory other
than the project root (e.g. `native_root=/project` launched from cwd
`/project/scenes`) silently confined against the wrong module for the
entire session. The pointer now resolves through its own
`conventions_pointer_key` (a relative pointer passes through untouched; an
absolute one still strips against the root), and `root_relative_key`'s
file-key semantics are untouched.

**`@brink-lang/web` is affected by the silent-swallow half.**
`EditorSession` never declares a `native_root` — its files are keyed by
already tree-relative virtual paths with no OS-filesystem anchor, so the
cwd bug could not bite there. But the silent-drop this issue reports for
`brink-web` — a `[project] conventions` pointer that resolves to no real
file in the project (most commonly: one discovered at a nested
`brink.toml` document key) — took the "does not match any file" arm, which
was a bare `tracing::warn!` returning zero diagnostics. `brink-web`'s wasm
build has no `tracing` subscriber at all, so that warning reached nothing
an embedder could ever observe: `compile_project`'s returned warnings
stayed empty, indistinguishable from "everything is fine," while
`ConventionsProjection` (what `explain_match` reads) stayed silently empty
too. Both `conventions_confinement_diagnostics` (the off-db road
`IdeSnapshot::analyze`/`EditorSession` actually run) and its db-direct
sibling `conventions_confinement_diagnostics_query` now push a real `E169`
diagnostic in this case instead of staying silent — one per declared claim
handler, anchored on each handler's annotation, worded to blame the
pointer/file mismatch itself ("does not match any file … fix the
`conventions` pointer or the project layout") and covering both the
typo'd/moved/deleted case and the nested-`brink.toml`-key case, never
`conventions_module_diagnostics`'s "move it there" message, since there is
no correct destination to name when the pointer doesn't resolve.

Pinned by `conventions_pointer_key_ignores_the_process_cwd` (unit, red
before this fix) plus the two-road e2e pair
`conventions_confinement_survives_a_relative_pointer_with_native_root_and_a_nested_lsp_cwd`
and `off_db_road_agrees_with_native_root_and_a_nested_lsp_cwd` (both
reproduce the LSP's exact `native_root`/nested-launch-cwd scenario and
assert the *confinement* arm specifically, so a resolution regression
cannot hide behind the new unresolvable-pointer diagnostic),
`unresolvable_pointer_is_e169_naming_the_pointer_not_a_destination`
(`brink-analyzer`), `an_unresolvable_conventions_pointer_is_e169_naming_the_pointer_not_a_destination`
(`brink-db`), and `compile_project_surfaces_an_unresolvable_conventions_pointer_as_e169`
(`brink-web`, exercised through the real wasm-exported `compile_project` entry
point `packages/ink-editor`'s `ProjectSession` calls).
