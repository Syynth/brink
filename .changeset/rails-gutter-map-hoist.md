---
"@brink-lang/editor": patch
---

Performance (#3067, no behavior change): the HIR rails gutter's
span-by-handle map is built once per projection on the overlay state
instead of once per visible line inside `lineMarker` — scrolling a large
file paid O(spans × visible lines) for it (19.7 ms per gutter rebuild
batch, ~1.5 s per full scroll pass on the perf fixture).
