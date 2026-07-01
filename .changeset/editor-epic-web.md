---
"@brink-lang/web": minor
---
Unified all structural-op results into a single **breaking** `StructuralResult` (replaces `MoveResult`/`SymbolRenameResult`) with an op-wide safe-by-default breakage gate. Added `deleteSymbol`, atomic `rename_dir`, `extract_to_knot`/`extract_to_function`, document-agnostic `findReferencesAt`/`referencesToSymbol`, `resolve_code_action`, and auto-import ops. BREAKING: consumers of `MoveResult`/`SymbolRenameResult` migrate to `StructuralResult` (the `safe`/`introduced_diagnostics`/`cross_file_edits` fields are preserved).
