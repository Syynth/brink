---
"@brink-lang/web": patch
---

Issue #2197: fixed a stdlib-mount codegen collision, and closed the
bare-name visibility gap it exposed. Observable through `@brink-lang/web`,
because the real compile path (`brink_environment::compile`, which
`@brink-lang/web`'s `compile.rs` calls) always mounts `std/` alongside a
native project's own sources.

- **The bug (worse than filed):** since #2080/#2190 mounted
  `std/conventions/screenplay.brink` into every native `Environment`, a
  project declaring its own same-named `extern`/`fn`/knot (exactly
  `tests/tier1-native/conventions-screenplay-preset/story.brink`'s shape —
  its own `scene_entered` extern + fallback + convention handlers, mirroring
  the shipped preset) hard-failed with `[E060] internal codegen error:
  duplicate DefinitionId … assigned to two different containers`. Root
  cause: several LIR-lowering/HIR-stamping self-identity lookups
  (`lir::lower::mod::lookup_container_id`, `lir::lower::decls::
  lookup_global`, `hir::stamp::lookup_label_id`) did a bare, file-blind
  `index.by_name` scan for "what id did the analyzer assign to the thing
  *this file* just declared" — correct when at most one candidate existed
  per name, but M-2d's cross-declared-module coexistence (#790) now lets a
  project's own declaration and the mounted std module's same-named one
  both live in the index, and the blind scan picked the same one for both
  files' lowering passes. Fixed by preferring the entry declared in the
  file currently being lowered (falling back to the old unscoped match when
  none exists, so every pre-#2197 corpus stays byte-identical).
- **The bare-name visibility gap (2026-08-03 SUBTRACTION RULING):** stdlib
  symbols are reachable only via an explicit `use std::…` — there is no
  implicit inclusion. That import mechanism doesn't exist yet (#1582/#2167),
  so today a std-mounted candidate is invisible to bare-name resolution,
  full stop — `brink-analyzer`'s `resolve::lookup_by_name_direct` now
  excludes any `Other`-classified (not-in-scope, not-imported) std-module
  candidate before it can win the flat-fallback tie-break, including when
  it is the *sole* candidate (previously silently reachable via the
  `!multiple` fast path). This narrows exactly one of M-2d's three
  resolution tiers (`Other`) for std candidates only; `InScope` (a std
  file referencing its own declarations) and `Imported` (a future real
  `use std::…`) are untouched.
- Added `brink-test-harness/tests/issue_2197_std_mount_module_qualification.rs`,
  compiling the golden fixture through `brink_environment::compile` (the
  real production path every oracle/tier1-3 corpus entry point bypasses)
  and asserting the project's own `scene_entered` keeps its exact
  `DefinitionId` across an isolated vs. mounted compile, plus a full
  transcript match — not merely that compilation no longer errors.
