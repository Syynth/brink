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
tokens; default on, `indentGuides: false` to opt out.
