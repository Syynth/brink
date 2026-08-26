---
"@brink-lang/editor": patch
---

Make the HIR rails gutter a fixed one-lane column instead of sizing it to
the open file's nesting depth. The spacer that sizes the column was `5n + 2`
px for an n-deep container stack, so the column's width depended on which
file was open and on when the HIR projection arrived — and since the
detached-gutter layout pays gutter width back as the content's padding, both
slid the prose sideways. Deeper stacks now paint their extra lanes over the
neighbouring play gutter, which is empty except on the hovered line; the bars
live in an absolutely-positioned layer, so they are unaffected by the width.
Reclaims 10px of permanently blank gutter on every file.
