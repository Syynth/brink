# Epic #452 (instruction-level source mapping) vs the ruled HIR admission contract — evaluation memo

**Status: AGENT-AUTHORED EVALUATION — not a ruling.** Written 2026-07-19,
post-#1149, against: issue #452 (epic, filed 2026-07-07, `needs-design`,
unscheduled), `docs/hir-admission-contract.md` (batch-4 delegated ruling,
#1134), `docs/b0-sequencing.md` (PROPOSED), the 2026-07-07 decision-log
entry ("HIR span overlay in editor + source-compatible debugger
direction"), and the current code chain. B0.1 (`auto/b0-provenance`
worktree) had **zero commits** at evaluation time — every accommodation
below can still shape it. **Update 2026-07-19 (evening walkthrough): the
§7 questions Q-R1–Q-R4 are RULED — see the addendum immediately below;
the memo body is otherwise unmodified.**

---

## RULINGS addendum (2026-07-19, evening walkthrough)

- **Q-R1 — RULED (a):** a new **optional, strippable
  `SectionKind::DebugInfo`** (tag **0x11**, omit-when-empty) is the
  instruction-map carrier; the dormant `Opcode::SourceLocation` is
  **retired**.
- **Q-R2 — RULED: adopt.** Accommodations A1–A3 (with free riders A4/A5)
  as B0.1 scope constraints — already landed in B0.1.
- **Q-R3 — RULED: the epic IS scheduled now — this OVERRIDES the memo's
  recommendation** ("stay unscheduled; revisit after B0.4"). The epic
  enters the active queue rather than waiting for B0.4. §5's "the epic
  remains unscheduled" line reads accordingly.
- **Q-R4 — RULED as stated:** the debug section is **section-locally
  versioned**, with a **NodeId column reserved as the anticipated v2
  extension** (when Q2's endgame lands); the concrete v1 entry encoding
  stays with the epic's own design round.

**One-line verdict:** the ruled contract does most of the epic's hardest
upstream work for free, three cheap constraints on B0.1 keep it that way,
and the epic's pivotal format-carrier risk analysis is **inverted** — the
"new debug section" option (3a) is the safe one and the "no new section"
opcode option (3b) is the one that can actually perturb oracle episodes.

---

## 1. Verdict summary

| # | Epic workstream | Status | Note |
|---|---|---|---|
| 1 | LIR provenance (spans on `lir::Stmt`/`Expr`) | **partially delivered (issue #3183)** | `lir::Container` and `lir::Stmt` now carry a bare (non-`Option`) `Provenance`, reusing B0.1's `Provenance` type verbatim as this row anticipated. `lir::Expr` is deliberately deferred — no consumer needs per-expression granularity yet (D8 breakpoints and variable inspection are served by Container/Stmt granularity); see the issue #3183 PR's own scope-cut rationale for the blast-radius accounting (`lir::Expr` is a 90+-variant enum touching ~700+ sites vs. `Stmt`'s ~150). Cheaper than the epic assumed. |
| 2 | Codegen emission (offset → range map) | unchanged | Blocked on WS1 + the carrier ruling. Constraint A2 (projectable `kind_token`) is its enabler. |
| 3 | `brink-format` carrier (3a section vs 3b opcode) | **blocked-on-ruling** | Q-R1 below. Recommendation: 3a. Risk profile inverted vs the epic's framing (§4). |
| 4 | Runtime introspection API (`ContainerPosition` accessor) | **unblocked** | Contract-independent; accessor half startable any time. Resolver half follows WS3. |
| 5 | WASM bridge | unchanged | Follows WS4. |
| 6 | Studio debugger UI | unchanged | Follows WS5 + Track A overlay. |
| 7 | Editor overlay consumption / identity reconciliation | **watch** | Interacts with Q2's NodeId endgame (§6.3). Join-key future-proofing lands in the section format (Q-R1). |

| Epic risk | Status |
|---|---|
| Format change is oracle-corpus-affecting | **Largely dissolved for 3a** (§4 — the oracle never sees an additive strippable section). Still surfaced loudly per CLAUDE.md; the ruling is Q-R1. For 3b the risk is *worse* than the epic states. |
| Fidelity (`line:col` opcode vs byte-range) | **Settled by the contract.** §1.3 makes `{FileId, byte-range}` the identity currency; a `line:col`-no-file carrier is now contract-substandard. Recommend retiring dormant `Opcode::SourceLocation` (Q-R1). |
| Multi-file / include graph | **Confirmed unblocked by construction.** Q1(b) `Provenance { file, range, kind_token }` carries `FileId` on every node; `file_paths: LookupMap<FileId, String>` already reaches LIR lowering (`lir/lower/context.rs:207`). |
| Staleness | Unchanged. `StoryData.source_checksum` exists (`brink-format/src/story.rs:92`) and gates as the epic assumed. |
| Size / perf | **Pattern already established.** Omit-when-empty + section-locally-versioned sections are precedented (`Visibility` 0x0E, `FrameShapes` 0x10 — both landed with no VERSION bump, byte-identical for existing stories). |

---

## 2. Confirmed unblocked by the ruled contract / B0 slices

- **Multi-file identity (epic risk 3).** Opaque `Provenance` carries
  `FileId` on every HIR node. The epic's "the map must carry file
  identity" requirement is satisfied at the source of the chain by
  construction, for the ink frontend *and* the future native frontend —
  the epic predates the native surface and would otherwise have been
  welded to ink CST spans (the D1 dragon).
- **Garbage-span corruption of the debug map (implicit epic risk).**
  B0.3's admission validator makes range well-formedness (non-empty,
  in-bounds, unique per reference) a loud tier-1 check. Every span the
  debug map will ever carry has passed that gate before LIR sees it. No
  debug-map-specific validation is needed; B0.3 needs no new checks for
  this epic.
- **Fidelity granularity (epic risk 2).** Contract §1.3 rules ranges are
  identity keys, byte-precise, per-`FileId`. The question "which fidelity
  does the debug map use?" is no longer open — it uses the pipeline's
  identity currency. The dormant lossy `Opcode::SourceLocation(line,col)`
  (`brink-format/src/opcode.rs:1230`, no-op at `vm.rs:195`) is below
  contract standard and never emitted; it should be retired, not revived.
- **Expression-level granularity exists upstream.** The contract obliges
  every referencing `Expr` to carry a real byte range (F-I#1) — checked
  loudly from B0.3 on. The epic's "precise per-expression spans" input
  data is a contract obligation, not a hope.
- **Runtime side (WS4) was never blocked.** `ContainerPosition
  { container_idx, offset }` (`brink-runtime/src/story.rs:178`) is
  crate-private but precise; exposing an accessor + call-stack frames on
  `FlowInstance` touches no contract surface. `debug_snapshot()`
  (`debug.rs`) already reverse-maps to container granularity.

---

## 3. Cheap accommodations to bake into B0.x NOW

These are the payoff of evaluating pre-build. **A1–A3 touch B0.1's
in-flight scope and should reach that builder before its type lands.**

### A1 — `Provenance` must be a plain, publicly constructible value type (B0.1)
**What:** `Provenance { file: FileId, range, kind_token }` must be pure
data — public constructor from raw parts, public `file()`/`range()`/
`kind_token()` accessors — with the ink `AstPtr` living **only inside the
ink resolver** (keyed by range+kind), never embedded in `Provenance`
itself (e.g. no `enum Provenance { Ink(AstPtr<..>), … }` shortcut
migration). This is what Q1(b) says, but it is the tempting corner to cut
during a 15-site mechanical migration.
**Why the epic needs it:** the debugger's reverse path is
`(container_idx, offset)` → debug section → `(FileId, range, kind_token)`
→ **reconstruct a `Provenance`** → hand it to the same resolver trait →
live editor node. That path only exists if a `Provenance` can be built
from deserialized parts. It also makes WS1 trivial: LIR carries the same
type, no parallel span struct.
**Slice:** B0.1. **Delta:** ~zero — a design constraint, not extra code.
**Cost if not done:** a second full-surface migration of the type + all
~15 consumer sites later, plus a bespoke LIR span type in WS1 that then
has to be unified.

### A2 — `kind_token` gets a stable public numeric representation + reserved generic tokens (B0.1)
**What:** `kind_token` must be projectable into `brink-format` (which
must never depend on `brink-ir`): a public `u16`/`u32` repr with a
documented namespace, including a few frontend-agnostic generic values
(`Stmt`, `Expr` at minimum) that LIR provenance can stamp when no finer
token applies. Not an opaque token whose only consumer is the resolver.
**Why the epic needs it:** WS2/WS3 project provenance into the debug
section; the section wants a coarse node-kind per entry so the debugger
can classify positions (statement step vs expression step, breakpointable
or not) without a live HIR. If `kind_token` is resolver-private, codegen
needs a parallel mapping table forever.
**Granularity check:** for stepping/breakpoints the epic needs
*per-instruction ranges* (which come from `range`, and are contract-
guaranteed at expression level) plus a *coarse kind* — `kind_token` at
roughly `SyntaxKind` altitude is sufficient; nothing finer is needed on
`Provenance`.
**Slice:** B0.1. **Delta:** a few lines (repr + doc + 2–3 reserved
values). **Cost if not done:** WS2 blocked on a retrofit or a shadow
kind-mapping maintained by hand (a D8-shaped duplication).

### A3 — the resolver trait is keyed by `Provenance` value, and B0.1 documents the bytecode→node path as in-scope-by-design (B0.1)
**What:** the resolver's lookup contract must accept any well-formed
`Provenance` value (not only instances the frontend itself minted this
session) — i.e. resolution is `Provenance → Option<live node>` by
(file, range, kind) lookup, with "not resolvable" a normal answer
(headless compile, stale bytecode). One sentence in B0.1's doc noting
that runtime-reconstructed provenance is an intended caller.
**Why:** this is the difference between "HIR-node → node" (all B0.1
strictly needs) and "any recorded position → node" (what the debugger
needs). Same code either way *if stated now*; a resolver designed around
session-identity (e.g. keyed by `AstPtr` handed in) would foreclose it.
**Slice:** B0.1. **Delta:** zero code, one doc sentence + one test
(construct a `Provenance` from raw parts, resolve it).
**Cost if not done:** WS4's resolver becomes a second resolution
mechanism beside the trait Q1(b) just built.

### A4 — WS1 reuses `Provenance` verbatim; nothing in B0.1 may make it HIR-private (B0.1, trivial)
**What:** `Provenance` lives in `brink-ir` (or is re-exported at crate
root), visible to `lir::`. HIR and LIR are the same crate, so this is
free — just don't scope it `pub(in hir)` or bury it in a hir-only module.
**Slice:** B0.1. **Delta:** zero. **Cost if not done:** trivial to fix
later, but flagging prevents the accident.

### A5 — B0.2 ordering note (no change requested)
WS1 should start **after B0.2**, not merely B0.1: uniform provenance
stamping on LIR while `Return.ptr` presence still carries the
tunnel-return bit would recreate exactly the D5 trap B0.2 retires. No
scope change to B0.2 — just a sequencing dependency for the epic.

**Not needed:** B0.3 and B0.4 require no epic-motivated additions. B0.3's
existing checks already guard the debug map's inputs; B0.4 (manifest
projection) is orthogonal. B0.5's "CST provenance designed against opaque
`Provenance` from day one" already gives the native surface debugger
support by construction — worth stating in the epic when it's next
edited, not a B0 change.

---

## 4. The format-carrier decision (WS3) — framed for ruling

The epic's pivotal open decision: **(3a)** new debug `SectionKind`
(per-container offset→range table) vs **(3b)** interleave the existing
`Opcode::SourceLocation` inline. The epic framed 3a as the risky
oracle-corpus-class change and 3b as avoiding a new section. **Both
halves of that framing are now wrong.**

**What the oracle actually compares.** Oracle episodes are generated by
the C# ink runtime from `.ink` source; the harness
(`brink-test-harness/tests/oracle_snapshots.rs`) compiles the same `.ink`
through the brink pipeline, *executes* it, and diffs **episode output**
(text/choices/tags per choice-path, via `oracle::diff_oracle`). No test
in the gate compares `.inkb` bytes against the oracle; `.ink.json` is
consumed by nothing in-tree (post-#544). The oracle's compared surface is
runtime behavior, full stop.

**3a — new optional section.** Purely additive. The repo has an
established pattern for exactly this: `Visibility` (0x0E) and
`FrameShapes` (0x10) are **omitted entirely when empty**, section-locally
versioned, and landed with **no format VERSION bump** — existing stories
stayed byte-identical. A `DebugInfo` section (next free tag: **0x11**),
omitted when not requested and strippable for release, perturbs **zero
existing bytes**, changes no container bytecode, no addresses, no
instruction stream — and is therefore **invisible to the oracle by
construction**. No corpus regeneration. (Per CLAUDE.md this analysis is
surfaced loudly here rather than assumed silently — that is what this
section is.)

**3b — interleaved opcodes.** Mutates every container's bytecode:
addresses shift, jump encodings change, `.inkt` dumps churn — and, the
decisive finding: **the VM counts every executed opcode against the step
limit** (`story.rs:1095`: `stats.steps - step_start > step_limit`; the
`SourceLocation` arm at `vm.rs:195` is a no-op *after* being fetched and
counted). Interleaved debug opcodes inflate step counts and can flip
`StepLimitExceeded` outcomes on step-bounded episodes — i.e. **3b is the
option that can actually perturb oracle-compared behavior**, in addition
to being lossy (`line:col`, no file id) and thus below the contract's
§1.3 fidelity standard. It also plants instrumentation in the hot
dispatch loop, against the standing design principle ("instrumentation
doesn't belong in the production path").

**Recommendation: 3a**, specifically:
- New `SectionKind::DebugInfo = 0x11`, per-container offset→
  `(file_idx, range_start, range_end, kind_token)` table (file paths via
  a small file table in the section, mapping `FileId` through the
  existing `file_paths` map at codegen time), delta-encodable later.
- **Omitted entirely when not requested** (compile flag), strippable
  post-hoc; section-locally versioned (one prefix byte) so the entry
  format can grow — see §6.3 for why that matters.
- **Retire `Opcode::SourceLocation`** (never emitted, no-op, lossy —
  below contract standard). Its removal touches no real file ever
  produced.
- Note: like every section addition, *old* readers reject an `.inkb`
  containing the new tag (`SectionKind::from_u8` errors) — same
  compatibility posture as 0x0E/0x0F/0x10, all absorbed in-tree.

---

## 5. Revised epic sequencing

| Epic WS | Can start after | Notes vs epic's assumptions |
|---|---|---|
| 1 (LIR provenance) | **B0.2** | **Cheaper than the epic assumed.** The epic predates the contract and implicitly assumed threading `AstPtr`-derived spans (ink-welded, per-node-type). With A1/A4 it is "add one `Provenance` field per LIR stmt/expr and copy it through lowering" — one uniform value type, no per-node-kind plumbing, no new span struct, and B0.3 has already validated every range it will carry. |
| 2 (codegen emission) | WS1 + Q-R1 ruled | Unchanged size. A2 is its enabler. |
| 3 (format carrier) | Q-R1 ruled | Substantially **cheaper** under 3a than the epic's "oracle-corpus-class" framing: additive section, no regen, precedented pattern. |
| 4 (runtime accessors) | **now** (accessor half) | Never contract-blocked. The position→span resolver half follows WS3. |
| 5 (WASM bridge) | WS4 | Unchanged. Carries a `@brink-lang/web` changeset (wasm-observable). |
| 6 (studio UI) | WS5 + Track A overlay | Unchanged. |
| 7 (overlay upgrade + identity reconciliation) | WS2/WS3 | Watch §6.3: reconciling the overlay's range-derived synthetic-container handles with runtime identity is exactly where the NodeId endgame will land. Do not invent a third identity space here. |

**Still blocked:** WS2/3/5/6/7 in the dependency order above; nothing is
blocked on B0.3–B0.10 beyond what's listed. The epic remains unscheduled
— this memo changes what it will cost, not when it runs (Q-R3).

---

## 6. Contradictions / invalidated assumptions

1. **The epic's risk table is inverted on its pivotal decision** (§4):
   3a was framed as the oracle-risky option and 3b as format-safe. In
   fact 3a is oracle-invisible and 3b is the one with a real (if narrow)
   behavioral perturbation channel (per-opcode step accounting) plus
   guaranteed byte/address churn. The epic text should be corrected when
   next edited so a future implementer doesn't inherit the inversion.
2. **The epic's provenance-chain description is going stale.** "Spans
   exist upstream as `AstPtr`/`SyntaxNodePtr`" describes pre-B0.1
   reality. After B0.1 the upstream currency is `Provenance`; WS1 must
   thread *that*, not ptrs. (This memo's A1/A4 make that the cheap path;
   without them WS1 would plausibly have grown a parallel span type.)
3. **Ranges-as-identity vs the Q2 NodeId endgame.** Q2(a) ratifies
   byte-range equality as the v1 join key with NodeIds tracked as
   endgame. The debug map's *display* path (offset → file+range) is
   endgame-proof — a span is a span. But two epic consumers want a
   *persistent* key, not a positional one: edit-surviving breakpoints,
   and WS7's synthetic-container identity reconciliation (the 2026-07-07
   Track A ruling explicitly deferred persistent anchors *to this epic*).
   If Q2(b) lands, those should key on NodeId, and the debug section
   would want a NodeId column. **Mitigation is cheap and already
   recommended:** section-local versioning (§4) lets the entry format
   grow a NodeId without a format-wide event. The contradiction to avoid
   is ruling 3a's entry format as *frozen* range-only.
4. **No contract-side contradiction found.** Nothing in the epic
   invalidates the batch-4 rulings; the epic is a downstream beneficiary
   of Q1(b)/Q2(a)/Q7(a) and touches none of Q3–Q6. Per the standing
   caveat (rulings adopted from summaries), note this memo *relies* on
   Q1(b)'s opaque-Provenance shape; if B0.1 review overturns it, §3 here
   is void.

---

## 7. Questions for ruling (numbered, with recommendations)

**Q-R1 — Format carrier for the instruction map.**
3a (new optional `SectionKind::DebugInfo = 0x11`, omit-when-empty,
strippable, section-locally versioned) vs 3b (interleave
`Opcode::SourceLocation`).
**Recommend 3a**, + retire the dormant opcode. Basis: §4 — 3a is
oracle-invisible and precedented; 3b churns bytecode, is lossy below the
§1.3 fidelity standard, and can flip step-limited episode outcomes.

**Q-R2 — Adopt accommodations A1–A3 as B0.1 scope constraints** (A4/A5
are free riders). These amend a PROPOSED slice, not a ruling; the ask is
to bless them before B0.1's type lands so the in-flight builder treats
them as requirements.
**Recommend adopt** — combined delta is a few lines plus two doc
sentences; each has a second full-migration or shadow-mechanism cost if
skipped.

**Q-R3 — Does the epic stay unscheduled?** The contract makes WS1
markedly cheaper and WS4's accessor half is free-standing; there is a
temptation to ride WS1 behind the B0 spine.
**Recommend: stay unscheduled.** B0's exit criterion is the writer's
first scene; the debugger serves a different consumer. Take only Q-R1 +
Q-R2 now (they are the perishable decisions); revisit scheduling after
B0.4 lands.

**Q-R4 — Debug-section entry format future-proofing.** Rule only that
the section is section-locally versioned and that a NodeId column is the
anticipated v2 extension when Q2's endgame lands (§6.3); the concrete v1
entry encoding stays with the epic's own design round.
**Recommend adopt as stated.**
