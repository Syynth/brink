---
"@brink-lang/web": patch
---

Internal: the observable-equivalence oracle ships (#3376,
`docs/observable-semantics-spec.md` §3/§3.1). The episode harness can now
compute the full host-facing trace of a run — output steps, choices by
order, external calls with their arguments, host-readable globals at every
turn boundary, host-invoked function results, terminal kind — and
`trace_diff(P, Q, runs)` replays the same runs on two compiled programs
(`.inkb` bytes) to report the first divergence. Tier 0's corpus
differential and tier 3a's mutation-sensitivity study run in CI.

No compiler or runtime behaviour changes: the story a host runs, and every
line and diagnostic it produces, are exactly what they were. This is the
mechanical checker that future "this transformation is safe" claims —
auto-fix's Safe tier, the optimizer — will have to pass.
