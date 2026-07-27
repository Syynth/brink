---
"@brink-lang/web": patch
---

Analyzer: `use`/`IMPORT`'s trailing segment is now dual-reading (issue
#1592, ruled 2026-07-27). `use story::market::barter;` where `barter` is a
**module** (not an item) previously licensed nothing and produced no
diagnostic at all — `story::market` (a pure directory prefix holding
`barter`, never a file's own module) had no `declared_exports` entry to
check `barter` against, so the well-formedness check could neither confirm
nor refute it. Two changes:

- **A trailing segment that resolves to a real submodule now licenses that
  module** — its public exports become bare-referenceable in the importing
  file, exactly as an explicit `use story::market::barter;` written as a
  qualified import would grant. A trailing segment resolving to an item
  keeps today's behavior unchanged.
- **A trailing segment resolving to neither an item nor a module now raises
  `E088`** (previously silent) — the retired no-op.
- **Precedence, decided and documented**: when a trailing segment resolves
  as *both* an item of the parent module *and* a declared submodule, both
  readings apply — the item is bare-importable under its own name, and the
  submodule's exports are also licensed. No exclusion between the two
  (`resolve::import_coverage_for_file`'s doc comment has the full
  rationale).
- Self-import (`E090`) now also fires for the leaf-item shape when the
  resolved module is the importing file's own (previously only the
  qualified `import mod;` form was checked).

Ink's module names are flat identifiers (never `::`-joined), so this is a
structural no-op for `.ink`/`IMPORT` — the oracle corpus is unaffected.
Scoped by #1582 (native visibility, open/needs-design): native definitions
have no working `Public` marker yet, so an *all-native* project still
cannot prove this end-to-end; the mechanics are proven with an `.ink`
defining side (`#@public`), same limitation `native_use_import_scope.rs`
(#1581) already documented.
