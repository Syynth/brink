---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

Literal-whitespace editor presentation (ruled 2026-08-23): the editor no
longer imposes layout of its own. Removed: standalone-divert
right-align, the weave-depth artificial indent and its superscript
depth-sigil collapse (nested `* *` sigil runs now render as typed), the
screenplay character/parenthetical/dialogue indents and dialogue column
width, CHARACTER uppercase, and the 8.5in page cap/margins. Colors and
highlighting are unchanged, and the classification taxonomy (element
classes, `data-depth`, `brink-divert-standalone`) remains the host
contract — an embedder that wants a styled layout adds its own CSS over
those hooks. New: whitespace/tab indent guides
(@replit/codemirror-indentation-markers), themed from the `--bs-*`
tokens, spaced at the editor's 4-column indent unit; default on,
`indentGuides: false` to opt out. New: hanging indent for soft-wrapped
lines — continuation rows align even with the first row's text start
(not flush-left, not Inky's extra padding), carried by a `--line-indent`
custom property per line.
