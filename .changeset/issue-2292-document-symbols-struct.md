---
"@brink-lang/web": patch
---

Editor: `document_symbols`/`project_outline` now include `struct`
declarations (issue #2292).

`brink-ide::document::document_symbols` projected `manifest.structs` into
nothing — its top-level decl-group list only walked `variables`, `lists`,
and `externals`, so a native `.brink` file's `struct Cue { ... }` never
appeared in the outline, `textDocument/documentSymbol`, or the studio
Binder's `project_outline()` road, even though the symbol was already
correctly indexed everywhere else (cross-file resolution, hover, the LSP
`SymbolKind::STRUCT` mapping). Adding `(&manifest.structs,
SymbolKind::Struct)` to the decl-group list surfaces it alongside knots.
