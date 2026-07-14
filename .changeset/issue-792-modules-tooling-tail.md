---
"@brink-lang/web": patch
---

Added the M-4 modules tooling tail (docs/modules-spec.md §9): editor
affordances riding the existing code-action, folding, and formatting seams.

- **Auto-import quick-fix** — a cursor on an out-of-scope module reference
  (`E025`, import-required) now offers an *"Import `name` from `module`"*
  quick-fix that inserts the `IMPORT` line in the right place: below any
  existing `IMPORT` block, else below the `INCLUDE` block, else at the top
  under the `#@module` header. The offer is session-aware (it reads the
  module-qualified db that produces the live `E025` squiggle) and resolves
  as a pure source rewrite through the same `resolve_code_action` seam. It
  surfaces in both the wasm editor's code-action menu and the LSP.
- **Import-block folding** — a run of two or more leading `IMPORT`
  statements folds into a single `IMPORT … (N modules)` region, mirroring
  the `INCLUDE` block fold.
- **`IMPORT` formatting** — `brink fmt` canonicalizes import spacing:
  `IMPORT {  a , b  AS c } FROM  m` becomes `IMPORT { a, b AS c } FROM m`,
  and `IMPORT   mod` becomes `IMPORT mod`. Malformed (mid-edit) imports are
  left verbatim.

Compat: purely additive and brink-gated. Every trigger requires a
`#@module`/`IMPORT` construct absent from the entire pre-modules corpus, so
no existing story's diagnostics, folds, or formatting change.
