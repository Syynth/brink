---
"@brink-lang/web": patch
---

Compiler: import aliases (`IMPORT { name AS alias } FROM mod` / `use
mod::name as alias;`) are now honored by resolution, not just recorded
(issue #1590).

`ImportItem.alias` used to be read only by the `E089` duplicate-import
check — `resolve::import_coverage_for_file` keyed bare-import coverage on
the source name and `lookup_by_name` only ever looked candidates up by
their own definition spelling, which never contains an alias. So `use
story::market::barter as b;` licensed `barter`, not `b`: a reference to
the alias reported unresolved (`E024`). Pre-existing on ink's `IMPORT …
AS`, but issues #1581/#1588 newly accept the native `use … as` spelling
(previously rejected as `E129`), which is what made this reachable from a
live native project for the first time.

`ImportScope` now carries an alias table, and `lookup_by_name` falls back
to it when a direct name lookup finds nothing. Ruling: the alias is
**additive**, not Rust's shadow-and-revoke — both the alias and the
original (source) name resolve through the same import afterward. This
follows from `lookup_by_name`'s existing "byte-identity guarantee" fast
path, which already returns a globally-unique name unconditionally,
ignoring `ImportScope` entirely; a strict revoke-on-alias rule would only
ever take effect in the rarer ambiguous-candidate case, so it would hold
sometimes and not others. Tested in both dialects, including the negative
case (a file that never imported the module gets neither the alias nor
the bare name).

Companion fix: the `E025` (import-required) diagnostic message no longer
hardcodes ink's `IMPORT { name } FROM mod` syntax — it never carried a
dialect signal to render the right one, and the native `use` spelling
reads wrong to native authors. `brink-ide::import_fix`'s `AddImport`
quick-fix (which does know the referring file's dialect, via
`ProjectDb::is_native`) now renders `use module::name;` for a native
referrer and `IMPORT { name } FROM module` for an ink one.
