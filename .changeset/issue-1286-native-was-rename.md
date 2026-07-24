---
"@brink-lang/web": patch
---

`brink-ir`/`brink-db`: native `@[was("old::module::path")]` module-rename
migration (issue #1286). A native `.brink` module's identity is derived
purely from its filesystem path (`native_module_path`) and folded into every
`DefinitionId`, so moving the file — or relocating the `brink.toml` root —
changes every id and breaks player saves keyed on the old ids. A file-level
`@[was("story::old::path")]` annotation declares the rename: `lower_native`
now parses it into `HirFile.module.was`, which the already-wired read path
(`brink-db::queries::module_map_query`) and alias-table codegen
(`brink-analyzer::manifest`) turn into an `AliasEntry { old, new }` so a save
carrying a pre-rename `DefinitionId` still resolves. Previously the
annotation was silently dropped (`hir.module.was` was always `None`), so no
migration was possible. A `@[was]` with no quoted old path is now diagnosed
(`E132`, warning) rather than silently ignored.

`brink-web` transitively depends on `brink-ir`/`brink-db` (non-optional) and
`brink-db::lowered_query` dispatches `.brink`-extension files through the
native frontend (the #1106 seam), so the new parse/lower is wasm-observable
for native files — most concretely the disappearance of the spurious `E129`
("construct parses but has no HIR lowering yet") the editor previously
reported on a top-level `@[was]` line, and the new `E132` for a malformed one.
