---
"@brink-lang/web": patch
---

Issue #2310 (#2113 remainder, NS-T seam 3/6): `explainMatch`/
`explainMatchDoc`'s `winner` now carries `kind` — the claimed line's
compile-time structural shape (`content_line` / `scene_heading` /
`bang_dispatch` / `cue` / `parenthetical`).

`brink_ir::explain_match` itself still cannot derive this correctly
from its own bare-text inputs (one shape is chain-gated on the
*previous* line, which a standalone line of text can't answer — see
`brink-ir`'s `hir::explain` module doc). `kind` is composed one layer
up instead, in `EditorSession::explain_match`/`explain_match_doc`:
read straight off the last-compiled `HirFile::element_matches` for the
active document, not re-derived, and guarded against staleness — it is
present only when that compiled snapshot's handler agrees with the
live `winner`, and absent (never a guess) otherwise, including on
every `Unmatched` line and on any `shadowed` runner-up (only the
actual winning claim has a compiled record to read a kind from).

`ExplainClassifiedMatch.kind` (`@brink/wasm-types`) is optional and
new; every other field on `ExplainMatch`/`ExplainClassifiedMatch`/
`ExplainAttempted` is unchanged.
