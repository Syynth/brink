---
"@brink-lang/web": patch
---

#1179 (B0.9, Q6(b)): a native `.brink` accept-list admission gate —
`brink_analyzer::validate_native_accept_list`, the inverse of the ink
`dialect_gate` reject-list. Enumerates the HIR shapes a well-formed native
lowering is allowed to produce and refuses anything else, loudly (a hard,
non-suppressible diagnostic, never a silent drop), at the same seam B0.3's
`validate_admission` runs at.

Four checks, each a fresh reserved `DiagnosticCode` (`E133`-`E136`):
`root_content` carrying anything other than empty or the synthesized `flow
main()` entry divert; any `IncludeSite` (native has no `INCLUDE` graph); a
`ThreadStart` outside the two legal splice positions B0.7's choice-point
lowering produces; a `ChoiceSet` carrying a non-neutral weave-fold value.

Reachable through any `@brink-lang/web` session that analyzes a
`.brink`-extensioned file (`brink-db`'s `lowered_query` → `lower_native_file`,
keyed off the producing frontend at the pipeline level, never a tree tag):
the gate now runs on every native lowering alongside B0.3's own admission
validator. In practice a real `.brink` file's lowering never produces any
of the four rejected shapes today — this is defense-in-depth, closing the
gap for a future B0.x slice (or a bug) that might.
