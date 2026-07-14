# Effects & ECS scheduling — the T2 round

Status: **language surface fully RULED** (sitting 1, 2026-07-13:
foundations; sitting 2, 2026-07-14: author-facing surface — see the
decision-log entries of those dates). Remaining OPEN items are
host-side only (§10 tail). Prereqs: value-model spec
(ratified), typed-mode spec (shipped), T1c spec (ratified; T1c-2+ in
flight). Implementation waits for the fine-grained salsa substrate
(#623 ruling: per-def effect rows must not ship on coarse
memoization).

## 1. What this buys, in one sentence

The host must make decisions **before** running ink — schedule,
prefetch, subscribe — and effect rows are the only way it can know
anything before running. Consumers, ascending: parallel flow
scheduling (access-disjoint flows advance concurrently), prefetch
(world queries known before entry resolve ahead, collapsing
park/resume round-trips), reactive sleep (a parked flow's dependency
set drives change-detection subscriptions — flows cost zero until an
input moves).

## 2. The layer model — RULED

Three layers, never conflated:

- **Atomic effects** are emitted by *expressions* when they run:
  `read cell`, `write cell`, `call external-kind`. Data never has
  effects; code emits them.
- **Rows** are static summaries of possible atoms:
  `{reads: CellSet, writes: CellSet, calls: KindSet}` — **unordered
  sets** (ordering is the journal's contract, not the row's). Every
  atom is absorbed into the *enclosing definition's* row.
- **Types**: `Ty::Fn` is the **only** row-carrying type constructor —
  a function value is the only data that encapsulates pending
  computation. `fn(int): int ⟨reads: gold⟩` means *calling it* reads
  gold; the returned int is just an int. Collections holding fn
  values carry rows via element types (containment, not
  effectfulness — reading the map is pure).

**Shipped entry rows contain what the host can act on**: World cells
per-cell + external call kinds. Temps die inside the frame and
`#@local` cells are flow-private by construction — neither can matter
to the host; both are excluded from shipped rows. Internal inference
keeps full per-cell precision regardless.

## 3. Soundness direction — RULED (ratified 2026-07-11, restated)

Rows may over-report, never under-report. Over-report = lost
parallelism or a spurious wakeup; under-report = an engine-level
race. Effects are **conservative-total**: the pessimal
touches-everything row is always available and always sound; "no
answer" is never an option (the asymmetry with gradual types, on the
record since the typed-mode round).

## 4. Inference — RULED

A definition's row coalesces exactly like its type: walk the body,
collect atoms, union; direct calls pull in the callee's row with
recursion handled by the **same per-SCC fixpoint as TM-1** (monotone
join, finite lattice — cells + kinds — terminates, no widening);
indirect calls pull in the callee *type's* row (§6). The row lives in
the signature object beside param/return types, so signature-firewall
/ Eq-cutoff economics apply unchanged. FG-2.1's `referenced_globals`
per-def pre-scan (landed) is the atom-collection pass.

Gradual mode: an `Unknown`-typed callee slot has no row to read →
pessimal row. Corollary on the record: **strict mode buys scheduler
precision**, not just error-catching.

## 5. Rows ride the unifier (the heap answer) — RULED

Rows are joined by the same unifier that joins types: a cell or
collection's element type accumulates the join of every fn value
assigned into it, through copies, parameters, returns, and nesting —
because *typing already follows values*. No separate points-to
machinery exists or is planned. The residual imprecision is
flow-insensitivity (the join covers the cell's whole lifetime), which
is the exact and only home of §8's refinements.

## 6. Flow through function values: the four mechanisms — RULED

1. **Higher-order params** (SHIPS): fn-typed params carry **row
   variables**, instantiated at call sites with concrete rows —
   shallow polymorphism, since every value's row is fixed at its
   creation site (`#fn` on a named target; `bind` copies rows;
   rows never grow after birth).
2. **Host-provided callbacks** (SHIPS): inference can't see native
   bodies; the manifest declares each binding's (and host-passed
   callback's) capability row. Symmetric with layer 2 of §9.
3. **The heap** (SHIPS): calls through stored values read the
   cell/element *type's* row — §5. Sound, coarse, improvable (§8).
4. **Runtime narrowing** (SPECIFIED; optional host optimization) —
   §7.

## 7. Runtime narrowing: selection, not inference — RULED

The runtime never computes a row. Every row that can exist is
computed at compile time (no runtime code synthesis, §11) and ships
in a `DefinitionId → row` table; a live fn value is a token; its row
is a **table lookup**. Narrowing, mechanically: at schedule-commit
the host may ask the VM for the fn tokens currently reachable from a
dispatch cell and substitute their looked-up rows for that dispatch's
static fallback.

Soundness gate (static, computed from flow-insensitive info we
already have): a dispatch is **narrowable only if its cell is not in
the entry's own write set** — a turn that can reassign the cell keeps
the static join. Cross-flow interference is the scheduler's existing
job: the dispatch cell is in the reads set, so concurrent writers
were already conflicts.

## 8. Refinement ladder — RULED (optimizer-not-gatekeeper doctrine)

Refinements only ever **narrow** rows; nothing downstream can break
when precision improves (tighter rows = more parallelism, fewer
wakeups, never new conflicts). Per the borrow-analysis precedent:
soundness never depends on any rung; each is adoptable when profiling
justifies it.

1. **Reachability slicing** — per entry point, join only `#fn` sites
   reachable from that entry's call/divert graph (call-graph pruning;
   candidate for v1 if cheap).
2. **Flow-sensitive per-def** — kill/gen over assignments within a
   body (backlog-with-doctrine).
3. **Runtime narrowing** — §7 (host-optional).

## 9. The ECS join and the trust tiers — RULED (direction)

**Two vocabularies, one join.** Brink-side rows speak cells + call
kinds (all the compiler can see; ships in `.inkb`). The host manifest
declares each binding's capability signature in engine vocabulary
(`get_position: reads Transform`). At load: entry row → look up call
kinds → union capability sets with world cells → a scheduler-native
access description. Absorbs/supersedes
`docs/host-capability-manifest.md` (Track B finally has its
consumer).

Trust tiers:

- **First-party `.inkb`**: rows inferred, trusted, zero checks (v1).
- **Foreign ink** (mods/DLC/host-generated): declared capability rows
  **verified against bytecode at load** (JVM-verifier style — cell
  and kind references are statically scannable; reject on
  exceedance). Zero runtime cost; the VM cap-mask (per-op check in
  foreign frames, fault on violation) is the named *fallback*, not
  the default. Direction only — not v1 implementation.
- **Host natives**: manifest declarations are an honesty contract
  with the host's own scheduler, not a security boundary (the host is
  the TCB).

The row table extends **only at load boundaries**, with
verified-or-vouched entries — this is the invariant that keeps
scheduling sound as content loads.

## 10. The author-facing surface — RULED (sitting 2, 2026-07-14)

- **No lockfile.** Inference is deterministic from source — there is
  no reproducibility problem for a pin artifact to solve. The shipped
  `.inkb` rows are the frozen record; a checked-in generated row file
  is rejected as compiler output cosplaying as input.
- **The only contract is the optional inline assertion**:
  `#@effects(reads: gold, calls: audio)` declares an upper bound;
  inference exceeding it is a compile error. `#@effects(pure)` is
  sugar for the empty row (the tooling-trust case). Nothing else
  errors or warns — there is no drift policy because there is nothing
  to drift against. Drift *visibility* is tooling: a `brink ide`
  effects-diff subcommand (CI-surfaceable as a PR comment) and IDE
  hover.
- **Default-public entry set.** Every knot/stitch ships its row — no
  `#@entry` marker exists (play-from-here already makes any knot a
  host entry). `#@private` opts out: not an entry point, row stays
  internal, host lookup fails at load. Its full visibility semantics
  belong to the **modules round** (the host is "outside every
  module" — one visibility concept, both axes). #657's effects half
  is closed by this; its types half stands (manifest-listed
  host-callable functions require annotations).

**Still OPEN — host-side sitting** (schedule when bevy-brink
implementation nears): manifest capability grammar (entity-granular
syntax space reserved), reactive-sleep API shape (component on the
flow entity vs host callback registry).

## 11. Invariants and non-goals

- **No runtime code synthesis** — the load-bearing invariant (not "no
  lambdas": lambdas-later get synthesized DefinitionIds, the same
  creation-freeze, and capture inherits the durable-cell rule; their
  save-stable anonymous identity is the lambda round's problem, not
  this spec's).
- Rows fixed at creation; refinements only narrow; the table extends
  only at load boundaries.
- **Format**: `EffectRows` (VERSION 4 reserved, section-locally
  versioned) ships **factored rows** — direct part + per-dispatch
  entries `{cell, narrowable, static fallback}` — plus the
  `DefinitionId → row` table. Flat rows are rejected: they
  structurally foreclose §7. Exact byte layout at implementation,
  inside the section version.
- Non-goals on record: tier-2 (foreign-ink) implementation,
  entity-granular capabilities, dynamic linking (#717, icebox), any
  author-facing effect syntax beyond the entry-point freeze (no
  monads, no handlers, no function coloring — interior effects are
  inferred, always).
