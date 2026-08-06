---
"@brink-lang/web": patch
---

Fix #1504: anonymous root-content container ids are now qualified by the file
that owns them, so two files with root-level weave content no longer collide.

Root-level weave content was scoped under an *empty* path in every file, and
address allocation is a pure hash of that path with no collision avoidance —
so the entry file's first root choice and an `INCLUDE`d file's first root
choice were both `c-0` and received the **same** `DefinitionId`. That id is
the linker's address key (last-write-wins) and the save key for visit counts,
so the collision was a live silent miscompile: the player was offered the
included file's choices, picked one, and the *entry* file's choice body ran.
The scope path is now qualified by the owning file's project path
(`hir::root_content_scope_path`). Per-file rather than per-module: an
`INCLUDE`d file with no `#@module` inherits its includer's module
(`docs/modules-spec.md` §1), so a module qualifier would leave the exact shape
#1504 was filed against still colliding.

The same change makes the synthesized root terminus content-pure: it was keyed
`#root-terminus.{file_id}`, the one address in `brink-ir` derived from a
`FileId` rather than from a path, so an editor/LSP session that registered a
sibling file before the entry minted a different id for the same source tree.

**Observable through `@brink-lang/web`**: `brink-web`'s compile session calls
`brink_compiler::compile` directly, so a playground/editor project whose entry
file and an `INCLUDE`d file both carry root-level weave now compiles instead
of failing with #1673's `E060` duplicate-`DefinitionId` guard — and the story
it compiles to runs the choice body the player actually picked.

⚠ **This is an identity break, not a plain bug fix.** It re-keys existing
definitions:

- **Anonymous visit counts and sequence positions in existing saves are
  invalidated**, with no migration path and no load-time diagnostic. Anonymous
  containers (`c-N`, `g-N`, `b-N`, `s-N`) have no author-visible name, so
  `#@was`/alias rebinding — which is name-based — cannot teach the loader the
  old id. The blast radius is bounded by construction: globals are name-keyed
  and an anonymous count is unreadable from author expressions, so this
  surfaces at most as a re-shown once-only choice or a restarted sequence.
- **Translations are *not* affected — verified, and this corrects the earlier
  sequencing note on #1504.** `brink-intl`'s export keys a translation scope on
  a `ScopeLineTable::scope_id`, and codegen opens a line table only for a
  scope-kind container (`Root`/`Knot`/`Stitch`); root-level choices and gathers
  inherit the **root** scope's id, which is the hash of the empty path and is
  not qualified by file. So no XLIFF unit id for a root-level line moves.
  Pinned by `root_content_translation_scope_id_is_unaffected_by_the_qualifier`
  in `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`. Worth
  stating explicitly because #1690's alias-aware rebinding could not have
  helped here: it rebinds by id through the alias table, and an anonymous
  container has no `#@was` site to populate an edge from.

Oracle conformance: 5,607 passing episodes before and after — no existing
episode changed. One tier-1 corpus case was added
(`tests/tier1/includes/root-weave-in-entry-and-included-file`) for the shape
that had no coverage at all; it compiles now (it tripped `E060` before) but
still fails on a separate, pre-existing divergence — brink does not accumulate
root-weave choices across the `INCLUDE` splice the way C# ink does, the same
gap `tier3/includes/choice-accumulation-across-include` and
`tier3/includes/root-content-splice-site` already record.

⚠ **Known limitation, flagged in review: the qualifier was not normalized —
fixed by [#1696](https://github.com/Syynth/brink/issues/1696).**
`hir::root_content_scope_path` used to qualify by the file's *raw registered
path*, not a normalized root-relative key, so anonymous root-content identity
was a function of how the entry (and any `INCLUDE`d file) happened to be
spelled when it was registered: `brink compile story.ink`, `./story.ink`, and
an absolute spelling of the same file minted three different anonymous
container-id sets for identical source, and `brink-lsp` (which keys by
absolute OS path) and the CLI (which keys by whatever spelling the caller
passed) disagreed on ids for the same tree beyond the registration-order
parity this PR restored (`root_content_ids_agree_between_discover_and_editor_
order` covers registration order only, with the same path spelling in both
orders — see its doc comment). #1696 derives the qualifier from a
root-relative key via `brink_driver::native_source_root` +
`brink_db::modules::root_relative_key` (the mechanism #1572 built for exactly
this hazard in native modules, extended to cover ink) — see
`.changeset/issue-1696-ink-root-content-key-normalization.md` for that
change's own save/translation-impact writeup and
`docs/root-content-identity-findings.md`'s "Known limitation" section for the
full history.
