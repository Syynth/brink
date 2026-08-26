---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

The editor's prose no longer slides sideways when a file opens. The
structure-rails gutter was sized by its content, and that content only
exists once the HIR projection arrives a few hundred milliseconds later, so
the column grew from nothing and the compensating content padding — which is
the text's offset — was rewritten by the same delta. The column is now a
fixed one-lane width that does not depend on the open file's nesting depth
or on when the projection lands, so there is no growth to compensate for.
Deeper stacks paint their extra lanes over the neighbouring play gutter,
which is empty except on the hovered line; the bars live in an
absolutely-positioned layer and still render every lane at full size. Also
reclaims 10px of permanently blank gutter on every file.
