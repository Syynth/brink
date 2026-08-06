# Modular artifacts — per-module bytecode, linking, DLC/UGC (V5 round)

**Status:** design draft, round convened 2026-07-23 (#1093). This is the
umbrella round that consolidates three parked threads into one coherent
design, made possible by the modules ruling (charter §13.2):

- **#717** — story-module dynamic linking (verified `.inkb` against a base) = the DLC/UGC half.
- **#848** — incremental artifact assembly / per-container splice = the build-perf half.
- **Translation stability** — per-module line tables / xliff units so a content patch to one module never churns another's translation state.

§13.2 made modules real units — filesystem-derived tree, absolute `story::`
paths as the wire's names, `(module, name)` `DefinitionId`s, the-tree-is-the-
universe. A module is already the **compilation** unit (#1296: the discovered
module set is the codegen closure) and the **addressing** unit. This round
makes it the **artifact** unit, aligning the format with the language.

This is a **draft**: the three anchor rulings below are settled (decision log
2026-07-23); everything under "Design surfaces" records a proposed direction
plus the open questions the round must close. Likely lands as a format V5 and
a charter §14.

---

## 1. Anchor rulings (settled 2026-07-23)

### 1.1 Load-time mounting is mandatory (not static-link-always)
The runtime must mount module artifacts **at load time**, not only assemble
them at build time. Rationale: UGC/mod content is discovered on the *player's*
machine (a mods folder, a workshop) — it is not known at ship time, so a base
cannot be statically re-linked against it. "Static link, always" can only ever
serve first-party, ship-time-known content; it is disqualified the moment UGC
is a goal. (Static assembly of the base remains valid as the *base's* build
step — see §1.2.)

### 1.2 The runtime invariant is a layered-precedence resolver
The immutable-after-`link()` `Program` gives way to a **layered module stack**:
the base at the bottom, modules layered above by precedence. Each
`DefinitionId` resolves to the **highest-precedence layer that defines it**.
The base may still be statically assembled into one artifact for its hot path;
mounting a module pushes a layer and (re)binds the affected `DefinitionId`s.

This is tractable because `link()` is **already** a `DefinitionId`→index map
build (containers, variables, addresses, line tables, list defs all keyed by
`DefinitionId`), and `DefinitionId = (module, name)` is globally unique and
save-stable. The same id denotes the same definition regardless of which
artifact carries it — the symbol table is already a global address space; the
format simply does not yet *split* along module lines, and the runtime does not
yet hold *more than one* layer.

### 1.3 Override policy: explicit patch markers + one-definition cells
Modules may **override/replace** base content (total-conversion is in scope),
but override is **opt-in and auditable**:

- A module that redefines an existing `DefinitionId` **must declare it**
  (`#@override story::foo::bar`, exact spelling TBD §2.4). An **undeclared**
  redefinition of an id another layer already defines is a **conflict error**
  (the one-definition rule, enforced at mount/admission).
- **Shared state cells (`VAR`) are one-definition, always.** The base owns the
  cell and its save slot; mods **reference** cells, never redefine them.
  (This is #717's "missing 10%" — two modules declaring `VAR gold` — resolved:
  cells are never a patch target; only code definitions (containers/flows) are.)
- Precedence among multiple *legitimate* overrides of the same id is a
  separate **ordering** concern — a priority/load-order manifest can layer on
  top later (§2.5). The one-definition rule makes the *default* case (no
  declared override) an error, so ordering only matters where authors opted in.

**Why explicit over silent last-wins:** in a verified-admission system,
override should be intentional and auditable — admission can verify exactly
what a mod replaces. Accidental collisions should fault, not silently resolve.
And cells must stay base-owned or saves break under module-set churn.

---

## 2. Design surfaces (open — the round closes these)

### 2.1 Per-module chunk format
**Direction:** a module artifact is a `StoryData`-shaped chunk scoped to *one
module's* definitions — its containers, variables, addresses, line tables, list
defs/items, effect rows, struct/frame shapes, visibility — all keyed by
`(this-module, name)` `DefinitionId`s, plus an **imports/exports table**
naming the cross-module `DefinitionId`s it references (imports) and exposes
(exports). The existing section-versioning + reserved-record runway (format V4
RFC) is the vehicle; a module chunk is a section-set with a module header.

**Open:**
- One file per module vs a container archive (base + modules in one bundle for
  first-party, separate files for UGC)?
- Does the chunk carry its own `name_table`/`literal_pool` (self-contained,
  larger) or reference a shared base pool (smaller, coupling)? Lean:
  self-contained per chunk for UGC independence; dedup at assembly for the base.
- Effect rows are content-hashed today (`brink-format` `id.rs`) — a module's
  rows stay content-addressed, so identical rows across modules coalesce for free.

### 2.2 Link-vs-load resolution & the runtime resolver
**Direction:** the base assembles statically (fast hot path). Mounting a module
runs a **relocation/admission pass**: verify imports resolve against the current
layer stack, check the override declarations against the one-definition rule,
bound the module's effects (§2.3), then push the layer and (re)bind affected
`DefinitionId`s in the resolver. The FG-4b/c symbolic-ref + relocation
machinery is the linker foundation and is oracle-proven.

**Open:**
- Is resolution eager at mount (rebuild the flat maps — O(total) per mount) or
  lazy via a layered lookup (resolve `DefinitionId` → walk layers top-down —
  O(layers) per lookup, cached)? Lean: eager rebind of *affected* ids only
  (mount touches the overridden/added ids, not the whole map) to keep the hot
  path a flat array read.
- What is the "linked program" type the VM holds now — `Program` becomes a
  base `Program` + an ordered `Vec<MountedModule>` + a rebind overlay?
- Unmounting: is it supported at all (see §2.7 save interaction)? Lean: mount is
  append-only within a session; unmount only between sessions (a different
  module set = a different load), which keeps mid-run resolution monotonic.

### 2.3 Cross-module verification / admission
**Direction:** mounting an untrusted (UGC) module is an **admission** event
(#717 verified-load, #912 per-marker admission, T2 capability bounds). The
module's effect rows are **bounded and checked at mount** — a UGC module cannot
call host bindings outside its declared, admitted capability set. Effect rows
union across the layer stack for scheduling.

**Open:**
- Signing/trust tiers: first-party (trusted, unbounded) vs UGC (sandboxed,
  capability-bounded)? Where does the trust boundary get declared — the mount
  API, a manifest, a signature?
- Admission granularity: whole-module vs per-marker (#912 precedent).
- What exactly is a UGC module *forbidden* — arbitrary host calls, overriding
  trusted base flows, redefining cells (already forbidden §1.3)?

### 2.4 Override spelling & the patch surface
**Direction:** `#@override story::foo::bar` on the overriding definition;
undeclared collision = conflict error at mount. Only code definitions
(containers/flows/functions) are patch targets; cells never (§1.3).

**Open:**
- Exact directive spelling and whether it names the target id or is inferred
  from the redefined `(module, name)`.
- Partial patch (splice into a container) vs whole-definition replace. Lean:
  whole-definition replace for V5; splice is #848's per-container concern and
  can follow.
- Can a mod override *another mod's* definition, or only the base? Lean: any
  lower-precedence layer, resolved by §2.5 ordering.

### 2.5 Per-module version/compat stamps
**Direction:** each module chunk carries a **version/compat stamp**; admission
checks a mounted module's expected-base-version against the actual base (a mod
built against base v1 mounted into v2). Interacts with #966's migration
facility — a version skew triggers a migration hook or a clean admission
refusal, never a silent mismatch.

**Open:**
- Semantic version vs content hash vs an explicit compat range.
- Priority/load-order manifest for override tie-breaks (§1.3) — is it per-mod
  metadata or host-supplied at mount?

### 2.6 Per-module translation units
**Direction:** line tables are already **per-scope** (`scope_id =
DefinitionId`), so they partition along module lines for free. A module's
xliff/translation unit is the set of line tables for its `DefinitionId`s. A
content patch to one module never churns another module's translation IDs — ID
stability becomes a **per-module property**. The intl pipeline
(`.ink`/`.brink` → `.inkb` → export-xliff → `.xlf`) gains a per-module unit
boundary.

**Open:**
- Does a UGC module ship its own `.xlf`s, or is UGC translation out of scope for
  V5?
- Base-vs-mod line-table id collisions (a mod overriding a flow re-emits its
  line table) — the override's line table shadows the base's, same precedence
  rule as code.

### 2.7 Mount API & save-state under module churn
**Direction:** DLC/UGC loading = mounting verified module artifacts into the
tree at runtime; **the-tree-is-the-universe becomes extensible at load.** The
mount API is the host's surface (bevy-brink asset mount, CLI `--module`, web).
Name-keyed `SaveState` + rehydration-mismatch faults already tolerate
module-set changes — under UGC, module-set churn is the **normal** case, not an
edge.

**Open:**
- Save made with mod X loaded, then X removed → base rebinds; a save position
  inside X's overridden flow is now dangling. Fault-and-recover policy (resume
  at the nearest base anchor?) vs refuse-to-load-without-X.
- Mount ordering determinism: the layer stack must be deterministic across
  runs given the same module set (charter determinism rule) — mount order is
  host-declared, not discovery-order-dependent.
- Is mid-session mount allowed, or only mount-at-load? Lean: mount-at-load for
  V5 (monotonic resolution), mid-session mount reserved.

---

## 3. Staging (what builds first)

1. **Split the format along module lines** — per-module chunk (§2.1) with an
   imports/exports table; assemble the base from module chunks (proves the
   split is behavior-preserving against today's monolithic `StoryData`; oracle
   must hold).
2. **Layered resolver** (§2.2) — `link()` over a base + zero mounted modules is
   byte-identical behavior to today (the null case), then a single trusted
   additive module mounts and resolves.
3. **Override + one-definition rule** (§1.3, §2.4) — patch markers, conflict
   errors, cell protection; differential tests for override precedence.
4. **Admission/verification** (§2.3) — capability bounds on a mounted module.
5. **Version stamps + save churn** (§2.5, §2.7).
6. **Per-module translation units** (§2.6) — the xliff boundary.

Steps 1–2 are pure build/runtime refactors with the oracle as the arbiter
(no behavior change until a second layer exists). Steps 3+ are the new surface.

## 4. Cross-references
- Consolidates #717 (DLC/UGC linking), #848 (incremental assembly), #1093 (this umbrella).
- Depends on: #1296 (native codegen closure = discovered module set), §13.2 (modules), FG-4b/c (symbolic-ref + relocation), format V4 RFC (section versioning + reserved records).
- Interacts with: #966 (migration facility), #912 (per-marker admission), T2 capability bounds, the intl pipeline (per-module xliff).
