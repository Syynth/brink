---
"@brink-lang/web": patch
---

#1355 (follow-up to #1349/#1286): `lower_native::module::was_old_path`
(`crates/internal/brink-ir/src/hir/lower_native/module.rs`) now accepts
**both** spellings of the `@[was(...)]` rename-migration arg — the original
quoted string (`@[was("old::path")]`) and the unquoted `::`-path form
`@[was(old::path)]` whose grammar #1349 shipped. Previously the unquoted
form parsed cleanly but was diagnosed `E132` ("malformed migration
directive") here, so the native module-rename migration (#1286) was not
usable end-to-end with the unquoted spelling despite both PRs having
landed.

Reachable through any `@brink-lang/web` session that lowers a
`.brink`-extensioned file whose `@[was(...)]` arg is unquoted: the
diagnostics change (E132 → none) and `hir.module.was` — and therefore the
`brink-analyzer` alias table that maps a pre-rename `DefinitionId` to its
current one — are populated exactly as they already were for the quoted
form.
