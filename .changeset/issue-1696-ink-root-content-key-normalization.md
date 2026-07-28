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

**Observable through `@brink-lang/web`**: `brink-web`'s compile session calls
`brink_compiler::compile` directly, and every ink compile now unconditionally
registers this root — a playground/editor project's compiled `StoryData` gets
root-relative anonymous container ids where it previously got raw-path-
qualified ones.

⚠ **This is a second identity break on top of #1504's, not a plain bug fix.**
It re-keys existing definitions again:

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
- **Translations are not affected**, for the same reason #1504's changeset
  gives: `brink-intl`'s export keys a translation scope on
  `ScopeLineTable::scope_id`, and codegen opens a line table only for a
  scope-kind container (`Root`/`Knot`/`Stitch`); root-level choices and
  gathers inherit the **root** scope's id, the hash of the empty path, which
  no file qualifier — raw or root-relative — has ever touched. Pinned by
  `root_content_translation_scope_id_is_unaffected_by_the_qualifier` in
  `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`, unchanged
  by this PR.

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
