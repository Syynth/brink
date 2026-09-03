# E095 — `#@was` names the definition's own current name

`docs/autofix-spec.md` §9's first-wave `Safe` list: "E095 self-alias
`#@was` → delete". `#@was(town)` here names the file's own declared module
name (`#@module(town)`) — a self-alias that migrates nothing.

`crates/internal/brink-ir/src/hir/lower/structure/mod.rs`'s own doc: a
self-aliasing `#@was` is diagnosed and its payload is **never stored** —
`ModuleDecl.was` stays `None` for this occurrence, so no compiled artifact
(bytecode, alias table, exported line table) was ever built from it.
Deleting the stale tag is `Safe` by construction: nothing downstream reads
the value being removed.

Same shape reaches `E095` on a knot, a top-level or nested stitch, and a
`VAR`/`CONST`/`LIST`/`EXTERNAL` declaration — see
`crates/internal/brink-ide/src/stale_was_fix.rs`'s own test module for one
fixture per shape. This on-disk fixture is the one `assert_safe_fix`
(`brink_test_harness::fix`) actually compiles, replays, and diffs.
