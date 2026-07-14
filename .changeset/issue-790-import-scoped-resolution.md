---
"@brink-lang/web": patch
---

M-2d: true import-scoped resolution (docs/modules-spec.md §2), relaxing the
#784/#793 `E096` stopgap.

Resolution now consults each file's own `IMPORT` list and declared module: a
bare reference with same-name candidates across different declared modules
binds to the module *this file* imported, rather than to the flat
duplicate-winner. Because same-name public definitions across declared
modules can now be disambiguated per-importer, they are **legal** — the
`E096` "duplicate definition declared in two different modules" hard error is
retired; two modules may each export `ambush` and two files may import
different ones, each binding its own.

- **Import-scoped `lookup_by_name`** (`brink-analyzer::resolve`) — a new
  per-file `ImportScope` (own declared module + imported modules) threads
  through every resolution lookup site. With zero or one candidate (all of
  strict-ink and every single-module project) the fast path is byte-identical
  to the previous flat resolver, so no existing corpus resolution moves.
- **Coexistence in the index** (`brink-analyzer::manifest`) — a cross-declared
  -module same-name/same-kind pair is no longer dropped as a duplicate; both
  are indexed. Within-module and legacy/undeclared duplicates keep the
  ordinary `E022`/`E023`/`E026` warning; strict-ink is untouched.

Byte-identical strict-ink and single-module resolution; oracle ratchet
(5,577) unchanged.
