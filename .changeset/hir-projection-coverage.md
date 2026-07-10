---
"@brink-lang/web": minor
"@brink-lang/wasm-types": minor
---

Extend the HIR projection's coverage (#463): new span kinds
`divert_terminal` (`-> END` / `-> DONE` — no longer unprojected, and never
flagged unresolved), `logic` (assignments and returns), and
`conditional` / `sequence` (whole-construct extents, non-container). A bare
labeled gather (`- (g)` with no continuation content) now projects its
gather container, so its rail renders.
