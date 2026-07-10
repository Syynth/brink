---
"@brink-lang/web": minor
---

Weave structure is now foldable (#476): choice branches fold from their
choice line (full-branch extent) and gather continuations fold from their
gather line, derived from the HIR projection's container extents. Choice
folds were previously dead code (single-line CST ranges), so story weave
never folded at all. Conditional/sequence folds are unchanged. Known
limitation: an unlabeled gather whose own line is prose gets no fold yet
(ptr-less line content; upstream lowering gap).
