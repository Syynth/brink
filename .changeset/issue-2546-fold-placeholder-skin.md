---
"@brink-lang/studio": patch
---

Collapsed fold placeholders now render as chips rather than loose glyphs (issue #2546).
`folding.ts` applies `brink-fold-pill`, its `-machinery`/`-narrative` kind classes,
`-icon`, `-summary`, `-count` and `brink-fold-decl-icon`, and none of them had a rule in
any stylesheet or CM6 theme in the workspace — so a collapsed machinery or narrative run
rendered as a bare `⚙`/`❞` followed by summary text and a count, reading as stray
characters spliced into the line. The studio's `editor.css` now skins all six from the
existing semantic tokens (both the latte and mocha themes), in the same inline-chip
language as `.brink-host-chip`/`.brink-value-chip`, with the summary as the pill's only
elastic part so the icon and count are never pushed out of view. The decl fold's kind
glyph is tinted from the same `--bs-symbol-*` tokens the binder's outline icons use.
