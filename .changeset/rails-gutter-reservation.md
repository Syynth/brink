---
"@brink-lang/studio": patch
---

The editor's prose no longer jumps sideways a few hundred milliseconds
after a file opens. The structure-rails gutter is sized by its content and
that content only exists once the HIR projection arrives, so the column
grew from nothing and the compensating content padding — which is the
text's offset — was rewritten by the same delta. The column now reserves
its common width up front, so the rails populate space that was already
there.
