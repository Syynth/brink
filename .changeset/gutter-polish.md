---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Editor gutter polish: the fold gutter renders one fixed-slot SVG chevron for both states (collapsed = the same glyph rotated by CSS, so the marker never shifts; open-fold chevrons appear only while the pointer is over the gutter, collapsed markers stay visible and accented) via `brinkBasicSetup`, a drop-in copy of `basicSetup` with the brink fold gutter. The play-from-here ▶ is a centered SVG triangle with a hover pill instead of a font glyph. Structure rails carry hover tooltips naming what they mark (kind + nesting depth). The shell's corner menu button is an SVG aligned to the strip's icon axis.
