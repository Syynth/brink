# E092 — redundant visibility directive (issue #3424)

`VAR score` has no declared module in this file, so the module default is
Public (`docs/modules-spec.md` §4). The `#@public` above it restates that
default — redundant — and `RedundantVisibilityFixer`
(`crates/internal/brink-ide/src/redundant_visibility_fix.rs`) deletes the
whole directive line. `#@public`/`#@private` is a compile-time-only tag
directive: it never reaches runtime tag output and it is never a
translatable content line, so deleting it changes neither the trace nor the
line table.
