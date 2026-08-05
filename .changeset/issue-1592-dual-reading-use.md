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
  module** — its public exports become reachable via qualified access
  (`barter::haggle`, never bare `haggle`) in the importing file, exactly
  as an explicit `use story::market::barter;` written as a qualified
  import would grant. A trailing segment resolving to an item keeps
  today's behavior unchanged.
- **A trailing segment resolving to neither an item nor a module now raises
  `E088`** (previously silent) — the retired no-op. This guard also widened
  incidentally: `E088` now fires for a bare import naming an item of any
  **declared** module that exports nothing publicly at all (not just a
  pure-directory prefix), since the check now needs real visibility into
  the *module* (`known_module_names`, a strict superset of the old
  `declared_exports`-only guard) to validate the dual-reading in the first
  place. A private-but-existing item was previously silent for the same
  structural reason as the pure-directory case; it now diagnoses too.
- **Precedence, decided and documented**: when a trailing segment resolves
  as *both* an item of the parent module *and* a declared submodule, both
  readings apply — the item is bare-importable under its own name, and the
  submodule is also licensed for qualified access under its own name. No
  exclusion between the two (`resolve::import_coverage_for_file`'s doc
  comment has the full rationale).
- Self-import (`E090`) now also fires for the leaf-item shape when the
  resolved module is the importing file's own (previously only the
  qualified `import mod;` form was checked) — except when the trailing
  segment resolves as a **submodule of the importer's own module**
  (`story::market` writing `use story::market::barter;`), which is the
  import the `E025` import-required gate makes mandatory to reference the
  child's exports, not a self-import.
- **Aliasing a trailing segment that resolves as a module now raises
  `E129`** (`use a::b as c;` where `b` is a declared submodule) instead of
  silently dropping the alias while still licensing `b`'s exports under
  their original names — mirrors the existing `E129` rejection of the
  single-segment `use a as m;` module-alias shape.

`#@module(...)` places no structural constraint on an ink module's own
name (it accepts any non-empty string, `::`-joined or not); the oracle
corpus is unaffected because no `#@module`/`IMPORT`/`use` construct appears
anywhere in it, not because of any structural property of ink module names.
Scoped by #1582 (native visibility, open/needs-design): native definitions
have no working `Public` marker yet, so an *all-native* project still
cannot prove this end-to-end; the mechanics are proven with an `.ink`
defining side (`#@public`), same limitation `native_use_import_scope.rs`
(#1581) already documented.
