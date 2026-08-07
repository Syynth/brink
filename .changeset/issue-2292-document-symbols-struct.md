---
"@brink-lang/web": patch
---

Editor: `document_symbols`/`project_outline` now include `struct` and
`const` declarations (issue #2292).

`brink-ide::document::document_symbols` projected `manifest.structs` and
`manifest.constants` into nothing — its top-level decl-group list only
walked `variables`, `lists`, and `externals`, so a native `.brink` file's
`struct Cue { ... }` and `const MAX = 100` never appeared in the outline,
`textDocument/documentSymbol`, or the studio Binder's `project_outline()`
road, even though both symbols were already correctly indexed everywhere
else (cross-file resolution, hover, the LSP `SymbolKind::STRUCT`/`CONSTANT`
mapping). Adding `(&manifest.structs, SymbolKind::Struct)` and
`(&manifest.constants, SymbolKind::Constant)` to the decl-group list
surfaces both alongside knots.
