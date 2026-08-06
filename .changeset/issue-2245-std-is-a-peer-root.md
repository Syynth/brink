---
"@brink-lang/web": patch
---

Issue #2245: `std::` (and every future mounted library) is now a top-level
**peer root** of `story::`, never a subdirectory of it — correcting
`brink_db::modules::native_module_path`, which used to prefix every
derived native module path with the literal `"story"` unconditionally.
The #2080 stdlib mount (`std/conventions/screenplay.brink`) used to mint
`story::std::conventions::screenplay` — the standard library filed as a
subdirectory of the user's own project. It now mints
`std::conventions::screenplay`, a peer of `story`, matching the
2026-08-04 "peer roots" ruling (`docs/decision-log.md`). An ordinary
project file is unaffected: `market/barter.brink` still mints
`story::market::barter`.

Observable through `@brink-lang/web`: the real compile path
(`brink_environment::compile`, which `@brink-lang/web`'s `compile.rs`
calls) always mounts `std/` alongside a native project's own sources, and
`DefinitionId` is a hash of `(module, name)` — every std-declared
definition's id changes as a direct, expected consequence (ruled
time-bounded acceptable pre-release, decision-log addendum 2026-08-04; no
saves or `.inkb` artifacts in the wild depend on it yet). A project's own
(non-std) definitions keep byte-identical ids.

`is_std_module`'s string-prefix test (`story::std…`) — previously
reinvented independently in `brink-analyzer::resolve` and
`brink-ir::lir::lower::decls`, because those crates cannot share a helper
in that direction without a dependency cycle — is now `brink-ir::symbols`'s
own root-identity check (`std…`), consumed by both former call sites.
`native_module_path` derives its root the same structural way: a
root-relative key's leading path segment decides whether it qualifies
under `std` or `story`.

The oracle ratchet (`RATCHET_EPISODE_COUNT`, `crates/internal/
brink-test-harness/tests/oracle_snapshots.rs`) does not move: this is a
pure identity renumbering, and the ratchet compares episode content — text,
tags, choices, state — never ids.
