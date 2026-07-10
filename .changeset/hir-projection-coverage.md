---
"@brink-lang/web": minor
---

Extend the HIR projection's coverage (#463): new span kinds
`divert_stmt` (whole divert/tunnel/thread statements, distinct from the
`divert` target reference inside them; suppressed for statements inside
inline logic in choice text), `divert_terminal` (`-> END` / `-> DONE` — no
longer unprojected, and never flagged unresolved), `logic` (assignments
and returns), and `conditional` / `sequence` (whole-construct extents,
non-container). Container extents now include gather/labeled-block labels,
so labeled gather lines (`- (g)`, `- (g) text`, nested labeled blocks) are
covered by their containers and render their rails. Multi-line
non-container spans that straddle a fragment view's start are dropped from
`getHirSpansDoc` instead of being clamped to the view's top-left.
