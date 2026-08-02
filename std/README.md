# `std/`

Source for Brink's built-in `std::`-namespaced presets and conventions
modules, authored as ordinary `.brink` files per
`docs/prose-dialect-spec.md` §3.4/§3.5 ("Presets ship as modules... a
project *imports and extends* a preset with ordinary value code instead
of forking a JSON blob").

**Not yet a real import target.** No `std::`-namespaced module resolution
exists in the compiler today — native source discovery is
tree-is-universe (every `.brink` file under a project's root joins that
project; there is no notion of an importable standard-library path a
`use std::…` could resolve against). These files are the preset's
*authored source*, kept in the shape the eventual pipeline will consume,
not something any project can `use` yet.

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
