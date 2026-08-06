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
read straight off `HirFile::element_matches` via a live salsa query
(recomputed off the current revision, never a stored snapshot that
could lag an edit), not re-derived. It is present only when a compiled
`ElementMatch` for that same line exists and its handler agrees with
the live `winner` — absent (never a guess) on every `Unmatched` line,
on any `shadowed` runner-up (only the winning claim has a compiled
record to read a kind from), on an ink-dialect file (`element_matches`
is always empty there), or on a line the compiler structurally
declined to claim on its own (a heading carrying a `[slug]`/tags, or a
line folded into a block handler's captured run) even though the live
walk matched it.

`ExplainClassifiedMatch.kind` (`@brink/wasm-types`) is optional and
new; every other field on `ExplainMatch`/`ExplainClassifiedMatch`/
`ExplainAttempted` is unchanged. In practice `kind` cannot yet surface
`"cue"`, `"parenthetical"`, or `"bang_dispatch"`: the native frontend
hands a claiming handler's pattern only the inner `CUE_NAME`/`TEXT`
run (excluding the `@`/parens), which the built-in screenplay preset's
own `cue`/`parenthetical` patterns require and so never match against
the live raw-line walk, and `!name` dispatch handlers are registered
on a path the live walk never consults at all. Only `"content_line"`
and `"scene_heading"` are reachable today — see
`crates/brink-web/src/editor/explain_match.rs`'s own module doc.
