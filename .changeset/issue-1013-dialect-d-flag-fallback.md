---
"@brink-lang/editor": patch
---

`dialect.ts`'s `ResolvedDialect.compile` no longer unconditionally constructs
its per-kind `RegExp`s with the `d` (`hasIndices`) flag (#1013). That flag
needs V8 9.0 / Chromium 90+ — NW.js-hosted embedders on older Chromium (e.g.
RPG Maker MZ's bundled NW.js is Chromium 88, with no newer official runtime)
threw `SyntaxError: Invalid flags supplied to RegExp constructor 'd'` at
construction, black-screening the embedder at boot before a single line was
ever classified.

Support is now feature-detected once at module scope. Modern engines keep the
`d`-flag path unchanged (indices read straight off the match). Older engines
fall back to a capture-group walk that reconstructs the same per-named-group
`[start, end)` spans by locating each group's captured text within its
nearest enclosing named group's span — correctly handling nested groups (e.g.
`parenthetical`'s `content` wrapping `content_inner`) — with no loss of
`DialectMatch` fidelity (`kind`/`attrs`/`hiddenSpans`/`contentSpan` are
byte-identical to the `d`-flag path on every input the at-cue conformance
corpus and generalization suite cover).
