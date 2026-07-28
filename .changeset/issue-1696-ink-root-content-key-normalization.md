---
"@brink-lang/web": patch
---

Fix #1696: an `.ink` entry's anonymous root-content container ids are now
qualified by a **root-relative key**, not the entry's raw registered path
spelling.

`hir::root_content_scope_path`'s qualifier (added by #1504) used whatever
string the caller passed as the entry — `brink compile story.ink`,
`./story.ink`, and an absolute spelling of the identical file minted three
different anonymous container-id sets for byte-identical source, and
`brink-lsp` (which keys its project database by absolute OS path) and the CLI
(which keys by whatever spelling the invocation used) disagreed on ids for
the same tree. `prepare_driver` now registers an ink project root
(`ProjectDb::ink_root`) via `brink_driver::native_source_root(entry)` — the
same `brink.toml`-walk-up-or-entry's-own-directory rule a native `.brink`
compile already used to root-relativize its own module identity (#1572) — and
every `file_paths` map the stamping/lowering passes read
(`normalized_stamped_query`, `chunk_lowering_ctx_query`, `lir_lowering_query`)
now strips that root before qualifying, via the renamed, now-shared
`brink_db::modules::root_relative_key` (previously `native_root_relative_key`,
native-only).

**Reachability, corrected (review finding on #1706, re-traced against the
real call graph):** `@brink-lang/web` is **not** reached by this mechanism.
`brink-web`'s compile entry point is `compile_over_tree`, which always goes
through `Project::load` + `brink_environment::compile` — never
`brink_compiler::compile`/`compile_path` directly (every call to those in
`crates/brink-web/src` is inside a `#[cfg(test)]` /
`#[cfg(all(test, target_arch = "wasm32"))]` module, exercising nothing
reachable from the published package). `brink_environment::compile` never
calls `set_ink_root`, and does not need to: `Project::load` already seeds
`ProjectDb` with root-relative source keys, so `root_relative_key` is the
identity function on that path (`ink_root` stays `None`) both before and
after this PR. The CLI's `brink compile` is the same story — it calls
`brink_environment::compile` too, via `compile_entry` in
`crates/brink-cli/src/main.rs`, not `brink_compiler::compile*`.
This changeset is filed per the standing "crates-only PRs need a
`@brink-lang/web` patch" policy (decision 2026-07-11) despite the traced nil
delta, so the release still carries a record of the identity-re-keying below
for anyone reading the changelog.

**The surfaces this PR actually re-keys** are the callers who use the
`brink-compiler` library's `compile`/`compile_path` entry points directly,
bypassing `brink_environment`/`Project::load`'s already-root-relative
registration: the oracle harness (`compile_path` in
`brink-test-harness`), `bevy-brink` (`brink_compiler::compile*` call sites
in `crates/bevy-brink/src/{request,ground_truth,source_loader,brkt,
test_support,replay,locale,capability}.rs` and `bindings/tests.rs`), and any
other external consumer of the `brink-compiler` crate — plus `brink-lsp`,
whose `register_native_root` now also calls `set_ink_root`.

⚠ **This is a second identity break on top of #1504's, not a plain bug fix,
for those surfaces.** It re-keys existing definitions again:

- **Anonymous visit counts and sequence positions in existing saves are
  invalidated a second time**, for any project whose entry is registered
  under a spelling other than the bare project-root-relative one — which
  includes a bare CLI invocation run from somewhere other than the resolved
  project root, an absolute-path CLI invocation, and *every* file the LSP
  holds, always, since it keys by `file://` URI. `root_relative_key` leaves a
  path that is already root-relative unchanged, so a CLI compile invoked from
  exactly the resolved project root with a bare relative entry is
  byte-identical to before this change; a compile invoked with an absolute
  or `./`-prefixed entry, or from elsewhere in the tree, is not. Same
  no-migration-path caveat as #1504: anonymous containers (`c-N`, `g-N`,
  `b-N`, `s-N`) have no author-visible name, so `#@was`/alias rebinding
  cannot teach the loader the old id.
- **Translation *scope* ids are not affected**, for the same reason #1504's
  changeset gives: `brink-intl`'s export keys a translation scope on
  `ScopeLineTable::scope_id`, and codegen opens a line table only for a
  scope-kind container (`Root`/`Knot`/`Stitch`); root-level choices and
  gathers inherit the **root** scope's id, the hash of the empty path, which
  no file qualifier — raw or root-relative — has ever touched. Pinned by
  `root_content_translation_scope_id_is_unaffected_by_the_qualifier` in
  `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`, unchanged
  by this PR.
- **Translation export's per-line `source.file` reference *does* change**
  (review finding on #1706 — narrowing an earlier, overbroad "translations
  are not affected" claim in this changeset). The same `file_paths` map
  `chunk_lowering_ctx_query`/`lir_lowering_query` now root-relativize also
  feeds `brink-ir`'s `build_source_location`
  (`crates/internal/brink-ir/src/lir/lower/recognize.rs`), which populates
  `LineEntry::source_location.file`; `brink-intl`'s `export_lines`
  (`crates/internal/brink-intl/src/export.rs`) emits that verbatim as
  `SourceJson.file` in `lines.json`/XLIFF. For any of the direct-library
  surfaces listed above whose entry was registered under a non-root-relative
  spelling, the `source.file` an exported translation unit points at changes
  from that raw spelling to the root-relative one — a metadata-only change
  (the export's scope/line *identity*, `scope_id` and `hash`, is untouched;
  only the human-readable source-file annotation moves). `@brink-lang/web`'s
  own translation export (`story_runner.rs`'s `export_lines`, over a
  `brink_environment`-compiled `StoryData`) is unaffected, per the
  reachability correction above.

Oracle conformance: the harness compiles every case through an *absolute*
entry path (`CARGO_MANIFEST_DIR`-derived), so this change does move every
oracle case's anonymous root-content ids from an absolute-path qualifier to
a root-relative one. That move is invisible to the oracle comparison, which
diffs `Line` output (text/tags/choices), never internal `DefinitionId`
values, and the normalization cannot introduce a new id collision (stripping
a shared root prefix is injective — two distinct raw paths under one root
stay distinct after stripping). See the PR body for the exact CASES/EPISODES
count re-run against this change.

Pinned by `root_content_ids_are_stable_across_entry_path_spellings` in
`crates/brink-compiler/tests/issue_1504_root_content_identity.rs` (flipped
from the `..._known_limitation` assertion #1693's review left in place) —
asserts `main.ink`, `./main.ink`, and an absolute spelling of the same file
now mint identical container ids.
