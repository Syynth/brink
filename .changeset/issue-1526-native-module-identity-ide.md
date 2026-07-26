---
"@brink-lang/web": patch
---

Fix (#1526): IDE analysis now mints the project database's `DefinitionId`s
for native `.brink` files, so editor hover stops silently dropping
db-backed detail on native projects.

A native file's module is its path (`market/barter.brink` →
`story::market::barter`) and always qualifies identity, but the analysis the
editor session runs was module-blind — it hashed every symbol by bare name.
The ids it handed to hover therefore missed in the db's per-definition
queries, so hovering a knot/stitch in a `.brink` file showed no effect row,
no TM-2 declared parameter/return annotations, and no inferred types, on
every native project (single- or multi-file). Most ink projects were
unaffected, since their undeclared stem-modules don't qualify identity and
both paths already agreed — but an ink file carrying a declared `#@module`
annotation qualifies identity the same way a native file's path does, so
those projects get the same fix: changed identity, plus import-scoped
resolution and cross-module coexistence in the editor.

The analysis pass is now fed the database's resolved module map, which is
where path-derived native identity is minted and stays minted — the
analyzer never recomputes it. Same fix reaches the LSP's background analysis
pass and the overlay/projection gates used by rename and move.
