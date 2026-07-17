---
"@brink-lang/web": patch
---

T2-3 follow-up (#882): wire the ruled freeze semantics into `EffectRows`
emission. The section-local encoding version bumps 1 → 2 (still no format
`VERSION` bump) — every row gains a leading `is_entry` byte, so a compiled
`.inkb`/`.inkt` artifact's `EffectRows` bytes change even though runtime
behavior does not (the section remains additive metadata the linker never
reads; episodes stay byte-identical — oracle ratchet unchanged at 5,577).

- **Entry set respects visibility** — a `#@private` definition's row now
  ships with `is_entry: false`: it is not a legitimate host-lookup entry
  point (`docs/effects-spec.md` §10; host semantic lookup on it is refused
  per `docs/modules-spec.md` §4 rule 2). Every other definition defaults
  `is_entry: true`, unchanged from T2-3.
- **The row itself is never dropped.** `#@private` hides the *name*, not the
  *cell* (`docs/modules-spec.md` §4 rule 1) — a private knot/stitch/function
  can still be captured as a first-class fn-value token a *public* path
  holds, and the dispatch-narrowing machinery (§7) resolves such tokens by
  `DefinitionId`, not by name. So the `DefinitionId → row` table always
  carries every def's row regardless of `is_entry`; only host-facing lookup
  is gated by it. This is unconditional (not a reachability computation over
  whether a public path actually captured such a token today).
- **Writer and reader land together for both codecs** (`.inkb` + `.inkt`),
  each with its own round-trip test for both `is_entry: true` and
  `is_entry: false`, plus an end-to-end `ProjectDb`-level test proving a
  `#@private` def's row is excluded from the entry set but still resolvable
  in the table, alongside an unaffected public row.
