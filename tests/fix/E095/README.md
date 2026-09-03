# E095 — `#@was` names the definition's own current name

`docs/autofix-spec.md` §9's first-wave `Safe` list: "E095 self-alias
`#@was` → delete". `#@was(town)` here names the file's own declared module
name (`#@module(town)`) — a self-alias that migrates nothing.

`crates/internal/brink-ir/src/hir/lower/structure/mod.rs`'s own doc: a
self-aliasing `#@was` is diagnosed and its payload is **never stored** —
`ModuleDecl.was` stays `None` for this occurrence, so no compiled artifact
(bytecode, alias table, exported line table) was ever built from *the
module's own* alias-table codegen from it. Deleting the stale tag is `Safe`
here by construction: nothing in that codegen path reads the value being
removed. (The flat `hir.was_directives` sweep still sees the occurrence and
feeds `dialect_gate`'s `E051`/`emit_native`'s unsupported-channel refusal —
deleting the tag only removes those, it doesn't hide a compiled-output
difference from `assert_safe_fix`.)

This particular fixture is the one case where deleting is unconditionally
safe: `#@was(town)` names the same thing as `#@module(town)`, and the line
attaches to no following declaration at all, so there is no *other* owner
whose reading of this physical line could be a live rename instead of a
coincidental self-alias. `stale_was_fix.rs`'s own doc ("Why this is
mechanically Safe") explains the one overlap where that isn't true — a
file-level `#@was` sitting directly above a `VAR`/`CONST`/`LIST`/`EXTERNAL`
can self-alias one owner while being a live rename for the other — and its
test module's `no_fix_when_the_module_self_alias_line_also_feeds_a_differently_named_declaration`
/ `no_fix_when_the_declaration_self_alias_line_also_feeds_a_differently_named_module`
tests (`#3425` review) cover both directions of that shape by withholding
the fix rather than deleting.

Same shape reaches `E095` on a knot, a top-level or nested stitch, and a
`VAR`/`CONST`/`LIST`/`EXTERNAL` declaration — see
`crates/internal/brink-ide/src/stale_was_fix.rs`'s own test module for one
fixture per shape, plus the narrowing refusals above. This on-disk fixture
is the one `assert_safe_fix` (`brink_test_harness::fix`) actually compiles,
replays, and diffs.
