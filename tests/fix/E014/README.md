# E014 — bare `~` with no expression

`docs/autofix-spec.md` §9 names "E014 bare `~` → delete the line" as a
first-wave **Safe** fixer, now `brink_ide::empty_logic_line_fix::EmptyLogicLineFixer`
(issue #3423). This fixture predates that fixer by design (it was written
ahead of it so `assert_safe_fix`, `brink_test_harness::fix`, had one on-disk
pair that genuinely cleared the §3 Safe bar before a fixer existed to
produce it) — the pre-fix source compiles, the two programs agree on every
explored run, and no translation unit moves.
`crate::fix::obligations::assert_fixture_matches_fixer` now also pins
`expected.ink` to be exactly what the registered fixer produces from
`before.ink`.

It is what the four fixtures beside it are measured against: E025, E063,
E080 and E081 all discharge **compilation-blocking** diagnostics, so their
pre-fix source has no program to preserve and the verdict is
`NoPreImage`. Without a positive case, the sweep in
`crates/internal/brink-test-harness/tests/fix_safe_obligations.rs` could
pass while proving nothing.

The fixture directory is keyed by diagnostic code and does **not** require
a registered fixer — the registry test's obligation runs the other way
(every `Safe`-max fixer needs a fixture, not every fixture needs a fixer).
