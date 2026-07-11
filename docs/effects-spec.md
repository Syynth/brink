# Effects & ECS scheduling — T2 design round skeleton

Status: **SKELETON — discussion scaffold for the T2 round** (nothing
ruled beyond the direction ratified 2026-07-11: inferred rows
internally, declared/frozen at entry points via the `#@` channel).
Prereqs: value-model spec (ratified), Tier-1 T1a–T1c landed enough
that closures' creation-site effect binding exists in practice.

## 1. What an effect row is (proposed shape)

Per definition (knot/stitch/function/closure):
`{ reads: CellSet, writes: CellSet, calls: ExternalKindSet }`
- CellSet granularity — OPEN: per-cell (precise, bigger rows) vs
  storage-class groups (World/Local/temps — cheap, coarser wakeups).
  Likely answer: per-cell for globals, grouped for temps.
- Commands (fire-and-forget bindings) — OPEN: are they ordered effects
  in the row (host cares about ordering?) or unordered call-kinds?
- "May fault" is NOT in v1 rows (errors ruled infallible/host-events);
  reserved as a future row member if recoverable errors ever land.

## 2. Inference

`effects(def)` — a salsa query beside `signature(def)`: body-local
effects ∪ transitive closure over the call/divert graph; closure env
refs bind at creation site (ruled); `ref` params are row variables
instantiated at call sites (effect polymorphism). Cutoff: rows are
small `Eq` values — the signature-firewall economics apply.

## 3. The entry-point firewall

- Inferred everywhere internally; **declared/frozen at flow entry
  points** via the `#@` channel — syntax OPEN: `#@effects(reads: gold,
  writes: alarm_raised, calls: audio)` vs a compiler-emitted lockfile
  (`.brinkeffects`) checked in CI vs both.
- Drift policy OPEN: hard error (declared ≠ inferred) vs
  ratchet-style (inferred ⊆ declared, error only on exceedance —
  allows declaring headroom).
- Which defs are entry points — OPEN: `#@entry`-marked only, every
  knot, or host-manifest-listed.

## 4. The ECS join (bevy-brink)

Manifest declares per-external ECS access (components/resources —
vocabulary extends the existing host semantic types). Join:
entry point → row → union of externals' access sets + cell sets →
a Bevy-legible access description. Consumers, ascending ambition:
1. **Parallel flow scheduling** — access-disjoint batches advance
   concurrently.
2. **Prefetch/batching** — world-queries known before entry resolve
   ahead/batched, collapsing park/resume round-trips.
3. **Reactive sleep** — parked condition's dependency set drives
   change-detection subscriptions; ambient flows wake only when their
   inputs change. API shape OPEN (component on the flow entity? host
   callback registry?).

## 5. Interactions to pin during the round

- Closures as host callbacks: row travels with the value (ruled);
  host-side representation of "this callback's access set" — OPEN.
- Handles: dereference happens host-side, so handle-typed args add
  the *binding's* declared access, nothing more.
- Speculation/scratch evals: effect-free by construction w.r.t. the
  live world — assert or exploit?
- Directive-channel reservation: `@effects` (and any sub-syntax)
  claims names in the reserved `@` namespace — coordinate with #497.

## 6. Open questions checklist (the round's finish line)

1. Row shape + cell granularity (§1)
2. Command ordering semantics (§1)
3. Declaration syntax + lockfile question (§3)
4. Drift policy (§3)
5. Entry-point definition (§3)
6. Manifest access-vocabulary shape (§4)
7. Reactive-sleep API shape (§4.3)
8. Callback access-set surface (§5)
