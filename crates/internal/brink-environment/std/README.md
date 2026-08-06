# `std/`

Source for Brink's built-in `std::`-namespaced presets and conventions
modules, authored as ordinary `.brink` files per
`docs/prose-dialect-spec.md` §3.4/§3.5 ("Presets ship as modules... a
project *imports and extends* a preset with ordinary value code instead
of forking a JSON blob").

**Lives inside `brink-environment`'s package, not at the repo root.**
`brink-environment` mounts this tree via `include_str!` (see
`crates/internal/brink-environment/src/lib.rs`'s `STDLIB_SOURCES`), and
`brink-environment` is a published crate — `include_str!` cannot reach
outside its own package root, so the canonical source has to live under
`crates/internal/brink-environment/std/`, not the repo's top-level `std/`
(a review finding on #2080 caught the earlier out-of-package path, which
built and tested clean locally but would have broken `cargo package`'s
verify build for every downstream consumer). There is no longer a
repo-root `std/` directory.

**Mounted, but not yet a real import target.** #2080 (ruled 2026-08-03)
mounts this directory's source into every compiled project's
`Environment` manifest (`crates/internal/brink-environment`) — the same
hash-addressed, string-keyed home every project source lives in, embedded
via `include_str!` so it mounts identically on hosts with no filesystem
(wasm). Native module identity mints `std::conventions::screenplay` for
`std/conventions/screenplay.brink` (`brink_db::modules::
native_module_path`) — a **peer root** of `story`, not a subdirectory of
it (issue #2245, decision-log 2026-08-04 "peer roots" ruling): an
ordinary project file at `market/barter.brink` still mints
`story::market::barter`, but a key whose leading segment is the reserved
`std` root mints under `std` instead. What's still missing: nothing
here is marked `pub`, and no confinement rule scopes what a project's own
`use` may reach into it — a real `use std::…` importing an item from this
module still needs #1582's pub marker and #2167's closure-scoped
confinement. These files are the preset's *authored source*, mounted into
every compile, but not yet something a project can `use`.

## `conventions/screenplay.brink`

The built-in screenplay preset (issue #1720, Track 1 step 8 of #1351).
See that file's own module doc for the element inventory it covers, what
it deliberately does not cover, and why. Its transcript-level regression
coverage lives at `tests/tier1-native/conventions-screenplay-preset/` (a
project that inlines the same handler declarations, since single-file
dispatch is the only mechanism landed today that can exercise them) —
keep the two in sync. `crates/internal/brink-test-harness/tests/
screenplay_preset_std_module.rs` reads this real file directly and
mechanically checks it lowers cleanly with exactly the four handlers
named above, so drift between the two no longer depends on this note
alone.
