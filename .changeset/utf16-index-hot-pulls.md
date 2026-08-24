---
"@brink-lang/web": patch
---

Performance (#3065, no behavior change — wire output byte-identical,
pinned by an every-offset equivalence test): the per-compile pulls'
byte→UTF-16 offset conversions (`getProjectOutline`, `getStoryGraph`,
`compileProject` diagnostics) now go through a per-file prefix-sum index
built once per pull instead of a linear scan from offset 0 per offset —
previously 17,744 scans per compile cycle on a studio-scale project,
making outline/story-graph O(symbols × file size).
