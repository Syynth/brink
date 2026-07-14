---
"@brink-lang/web": patch
---

Added the M-1 module name model (docs/modules-spec.md §1/§5): every `.ink`
file is a module named by its stem, and a file-level `#@module(name)`
directive declares the module explicitly. `DefinitionId`s are now hashed
as `(module, name)` for **declared** modules; INCLUDE-glued files inherit
their includer's module. An undeclared file whose stem collides with a
declared module's name is a compile error (`E085`), and a malformed
`#@module` (missing/empty name, or a second declaration) is `E086`.
`#@module` is brink-dialect-only — under strict-ink it is rejected with
the standard `E051`-class diagnostic.

Identity is unchanged for the entire pre-modules world: an undeclared
stem-module contributes nothing to the hash, so every existing story's
`DefinitionId`s — and every saved game — stay byte-identical.
