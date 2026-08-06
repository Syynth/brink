---
"@brink-lang/web": patch
---

Compiler: new diagnostic `E170` (issue #1859) extends `E168`'s
byte-identical-pattern check to genuinely overlapping (non-identical)
`@[element(claims = "…")]` patterns. When a later-declared handler's
pattern is provably subsumed by an earlier one's — every string the later
pattern can match, the earlier one also matches — and the later handler
never actually won a claim in the file, it is unreachable under the
interim first-match-wins dispatch order.

Subsumption is proven by generating a set of candidate strings from the
later pattern's structure (recursing into named capture groups and
expanding every alternation branch) and checking that the earlier pattern
accepts every one of them — a sound-but-incomplete heuristic, so a missed
case is a false negative, never a false positive.
