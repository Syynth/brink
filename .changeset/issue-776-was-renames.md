---
"@brink-lang/web": patch
---

Added the M-3 renames surface (docs/modules-spec.md §5), completing the
modules spine: `#@was(old_name)` on modules and definitions, a compiled
old→new `DefinitionId` alias table in `.inkb`/`.inkt`, and a rehydration
miss-path lookup that rebinds saved state deterministically instead of
silently orphaning it under a stale id.

- **`#@was(old_name)` directive** — on a file-level `#@module` declaration
  (records the module's rename) and on any definition (VAR, CONST, LIST,
  EXTERNAL, knot, stitch). Brink-dialect-gated, like `#@module`/`#@private`.
  A self-alias (`old_name` equals the current name) warns "nothing to
  migrate" (E095); a missing/empty argument is E094.
- **Compiled `AliasTable` section** (`.inkb` format v5, section tag
  `0x0F`, since `0x0E` was independently claimed by the M-2b `Visibility`
  section) — one-byte section-locally-versioned old→new `DefinitionId`
  rows, sorted for the runtime's binary-search lookup. Matching `.inkt`
  text atoms (`(alias_table (alias $old -> $new))`). Empty for every story
  that uses no `#@was`, including the entire pre-M-3 corpus.
- **Rehydration miss-path lookup** — `Story::load_state`/the free
  `load_state` function now consult the alias table when a saved visit/
  turn-count id, or a divert-target/fn-token/closure-target id embedded in
  a saved global's value, doesn't match the current program. Still
  unresolved after that surfaces a teaching message in the new
  `LoadReport::unresolved_renames` field (only for a program that actually
  carries alias-table entries — an ordinary content edit with no `#@was`
  stays exactly as silent as before).
- Retrofits the pre-existing silent save-break on a plain knot rename (no
  module involved) with the same machinery.

Compat: the `.inkb` format version bumped 4 → 5 (a brand-new mandatory
section, not part of the v4 RFC's pre-reserved inventory) — checked-in
`.inkb` artifacts regenerate. `LoadReport` gained a field
(`unresolved_renames`), changing the JSON shape `StoryRunner::load`/
`load_bytes` return. The alias table itself is additive and brink-gated;
the entire pre-M-3 corpus emits an empty table and sees no behavior
change.
