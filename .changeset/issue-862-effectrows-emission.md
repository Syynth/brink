---
"@brink-lang/web": patch
---

T2-3: first real emission into the reserved `EffectRows` `.inkb` section
(docs/effects-spec.md §11, format-v4-rfc §2). The wire surface grows —
compiled `.inkb` artifacts now carry a factored effect-row table — even
though the runtime does not consume rows yet (additive metadata; the
linker never reads them, so episodes stay byte-identical).

- **Section graduated** — `EffectRows` (tag `0x0D`) moves from reserved
  (count-0) to a real, section-locally-versioned section (version byte
  bumped, no format `VERSION` bump — the reservation existed for exactly
  this). Writer and reader land together, with `.inkt` text atoms and
  per-codec round-trips (inkb + inkt).
- **Factored rows** — each entry ships a direct part (reads / writes /
  call atoms / opaque) plus a per-dispatch list (`{cell, narrowable-bit,
  static fallback}`, empty in v1 — a flat row would foreclose §7
  narrowing). Every knot/stitch ships its container row (the host's
  resume-scheduling estimate, §12.1), keyed in a `DefinitionId → row`
  table.
- **Reserved parameter slots** — each call atom carries a
  capability-parameter slot populated `(any)` in v1 (component-granular;
  path-granular #826 is the later consumer) and a reserved
  handle-parameter slot (t1d-spec §7), left `None` in v1.
