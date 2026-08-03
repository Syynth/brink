//! T2-1 effect-row inference substrate (docs/effects-spec.md §2/§4/§5, issue
//! #860 — tracked from #859). The soundness core of the T2 effects epic.
//!
//! Three layers, never conflated (spec §2):
//!
//! - **Atomic effects** are emitted by expressions when they run: `read cell`,
//!   `write cell`, `call external-kind`. Data never has effects; code does.
//! - **Rows** ([`EffectRow`]) are static summaries of possible atoms —
//!   `{reads, writes, calls}` as **unordered sets** (ordering is the journal's
//!   contract, not the row's). Every atom is absorbed into the enclosing
//!   definition's row, and a direct call pulls in the callee's whole row.
//! - **Types**: rows ride `Ty::Fn` (spec §5, the heap answer). Since issue
//!   #1680 step 3 the type does carry one — [`super::FnRow`], the structural
//!   set of creation targets §7's token lookup keys on — but **this walk
//!   cannot read it**: `def_effect_atoms` runs the body pass with empty
//!   globals and empty signatures (load-bearing for §6.1a's acyclicity), and
//!   a `#fn` literal types as `Unknown` under empty signatures. So a call
//!   through a value stored in a VAR/CONST cell (§6 mechanism 3, the heap)
//!   still stays **opaque** — the conservative floor, see
//!   [`EffectRow::opaque`] — which is sound. Wiring that rung means deciding
//!   which stratum reads the type-carried row (spec §6.1c). Three rungs
//!   narrow the floor today, each structural (no inferred row or signature
//!   ever decides a call-graph edge, §6.1a):
//!
//!   1. Issue #872: a call through a **local** whose every write traces to a
//!      `#fn(target, …)`/`bind(…)`-chain origin resolves to those origins
//!      (`InferPass::resolve_pending_value_calls` in `infer::body`). Fork A
//!      (`docs/decision-log.md` 2026-07-28, issue #1726) widened it from a
//!      single write-once origin to the **join over every traced write**, and
//!      added [`EffectAtoms::creates_fn_values`] — the structural record of
//!      which targets a body creates fn values for, harvested by the same
//!      walk with empty globals and empty sigs.
//!   2. Issue #1680 / §6.1: a call through a **fn-typed param** is a **row
//!      variable** — [`EffectRow::holes`], the "row with a hole" Fork C
//!      ruled. The definition's own row stays pessimal
//!      ([`EffectRow::is_pessimal`]); each *caller* instantiates the hole
//!      from its structurally-traced argument
//!      ([`EffectAtoms::call_fn_args`]) and so escapes the floor.
//!   3. The heap (VAR/CONST cells joined project-wide, §5's "sound, coarse,
//!      improvable") is still pessimal. The `Ty::Fn` row it needs now
//!      exists; what is missing is a stratum that can read it — see the
//!      **Types** bullet above and spec §6.1c (issue #1680's remainder).
//!
//! **Soundness direction (spec §3, conservative-total)**: rows may over-report,
//! never under-report. Over-report costs parallelism or a spurious wakeup;
//! under-report is an engine-level race. The pessimal touches-everything row
//! ([`EffectRow::pessimal`]) is always available and always sound; "no answer"
//! is never an option. The `conservative_total_*` property tests pin the
//! no-under-report invariant, mutually-recursive fixture included.
//!
//! **Inference (spec §4)**: a definition's row coalesces exactly like its type
//! — walk the body, collect atoms ([`EffectAtoms`], harvested by the same
//! `infer_def_body` walk FG-2.1's `referenced_globals` already drives), union;
//! a direct call to an inferable callee pulls in the callee's row with
//! recursion handled by the **same per-SCC fixpoint as TM-1's type solver**
//! ([`solve_scc_effects`] — monotone join, finite lattice of cells + kinds,
//! terminates, no widening). An `Unknown`/opaque callee has no row to read →
//! pessimal (spec §4's gradual-mode corollary).

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;

/// A static summary of the atomic effects a definition (and everything it
/// transitively calls) may perform — the spec §2 "row". Unordered sets over a
/// finite per-project lattice, so [`join`](Self::join) is a monotone
/// least-upper-bound and the per-SCC fixpoint terminates without widening.
///
/// `opaque` is the top element of that lattice: when set, the row is treated
/// as *touching every cell and calling every kind* regardless of the listed
/// members — the conservative-total floor (spec §3) for a call whose effects
/// inference cannot see (a call through a function value, an unresolved
/// callee). [`covers`](Self::covers) reads it as "⊒ everything".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "opaque + the NS-A2 emits/tags/faults dimensions are independent \
              lattice components of one row, not a state machine"
)]
pub struct EffectRow {
    /// Cells (VAR/CONST global [`DefinitionId`]s) this row may read. Seeded
    /// from FG-2.1's `referenced_globals` per-def read-set (spec §4).
    pub reads: BTreeSet<DefinitionId>,
    /// Cells this row may write (assignment targets resolving to a VAR/CONST).
    pub writes: BTreeSet<DefinitionId>,
    /// `EXTERNAL` binding *names* (the call-kinds, spec §2) this row may
    /// transitively call.
    pub calls: BTreeSet<String>,
    /// The pessimal top element (spec §3): this row performs a call whose
    /// effects inference cannot summarize — a call through a function value
    /// with no visible row, or an unresolved callee. An opaque row is sound
    /// against any concrete row it might stand in for.
    pub opaque: bool,
    /// NS-A2 (issue #1108, from #1087): the definition may produce **content**
    /// — narration/dialogue fragments a host renders (text, interpolations,
    /// glue-only output counts; ruled 2026-07-18). Tag-only lines do NOT set
    /// this — tags are the metadata channel, tracked by [`Self::tags`]
    /// (maintainer ruling refinement on #1087). Bool granularity v1.
    pub emits: bool,
    /// NS-A2 (issue #1108, from #1087's second ruling): the definition may
    /// touch the **tag channel** — line tags, tag-only lines, choice tags.
    /// Independent of [`Self::emits`]: a flow can be silent-but-annotating,
    /// narrating-but-untagged, both, or neither. Bool granularity v1.
    pub tags: bool,
    /// NS-A2 (issue #1108, from #1097): the definition may raise a
    /// **turn-terminating fault** — the designed domain-fault inventory
    /// (E078-lineage conversions, OOB indexing, missing-key reads,
    /// division by zero, the A1 `StdlibWrongType`/`NotOrderable` stdlib
    /// faults, projection invalidation, value-call dispatch faults). Bool
    /// granularity v1; per-fault-kind is the reserved refinement.
    pub faults: bool,
    /// NS-A4 / **F29(a)** (ruled by delegation 2026-07-19, stdlib-spec
    /// §4b): the *refined* faults bit — like [`faults`](Self::faults) but
    /// with charge sites **discharged by local type evidence** where the
    /// walk can prove the construct total (a wrong-type-only intrinsic
    /// over a provably-right-typed argument, float division, `for` over a
    /// provable collection, an int-bounded range literal). Invariant:
    /// `faults_refined → faults` (the refinement only ever *removes*
    /// charges). Consumed by the protocol-impl contract gate (E114): a
    /// `display`/`compare` impl whose row is provably total does NOT
    /// inherit the conservative bit; the conservative union applies only
    /// when the impl's own row is opaque or genuinely fault-bearing.
    /// Deliberately NOT part of [`covers`](Self::covers)/
    /// [`is_empty`](Self::is_empty) semantics (those stay anchored to the
    /// conservative bit — the ground-truth harness and assertion checks
    /// must keep the no-under-report property), and never serialized into
    /// the `.inkb` `EffectRows` section.
    pub faults_refined: bool,
    /// **§6.1 row variables — the "row with a hole"** (`docs/effects-spec.md`
    /// §6 mechanism 1 and §6.1b; Fork C of issue #1680, ruled 2026-07-28).
    /// Each member is the *declaration index* of one of this definition's own
    /// `fn`-typed params that the body **calls through**. The row is
    /// therefore parametric: its true effects are this row's listed atoms
    /// **⊔ the row of whatever fn value the caller passes in that position**.
    ///
    /// A hole is **not** a second opacity bit. [`opaque`](Self::opaque) stays
    /// the *intrinsic* floor (a call inference genuinely cannot see);
    /// [`is_pessimal`](Self::is_pessimal) is the effective floor every
    /// consumer must read, and it is `true` for any row with an unfilled
    /// hole. That keeps the conservative-total direction (spec §3) exactly as
    /// it was: a higher-order definition read on its own is still pessimal.
    /// The precision arrives one hop up, in [`solve_scc_effects`], which
    /// **instantiates** the hole from the caller's structurally-traced
    /// argument origins ([`EffectAtoms::call_fn_args`]) and so no longer
    /// inherits the callee's floor.
    ///
    /// Shallow by construction (§6.1: "every value's row is fixed at its
    /// creation site"): a hole is filled with ground rows, never with another
    /// hole — an argument that is itself a fn-typed param, or a target whose
    /// own row still holes, falls back to the floor rather than chaining.
    pub holes: BTreeSet<u32>,
}

impl EffectRow {
    /// The pessimal touches-everything row (spec §3) — always sound, the
    /// answer for an `Unknown`/opaque callee.
    #[must_use]
    pub fn pessimal() -> Self {
        Self {
            opaque: true,
            // F29: the conservative union applies to an opaque row — the
            // refined bit never claims totality for a row inference
            // cannot see.
            faults_refined: true,
            ..Self::default()
        }
    }

    /// The **effective** pessimal floor — the bit every consumer of a row
    /// must read in place of [`opaque`](Self::opaque).
    ///
    /// `opaque` records only *intrinsic* opacity (a call whose effects
    /// inference genuinely cannot see). A row that carries §6.1
    /// [`holes`](Self::holes) is equally unusable on its own: its true
    /// effects depend on an argument the definition has not been given yet.
    /// Reading such a row as non-pessimal would under-report — the one thing
    /// spec §3 forbids — so an unfilled hole tops the lattice exactly like
    /// `opaque` does. Only [`solve_scc_effects`], which can *fill* the hole
    /// from the call site, is entitled to look past it.
    #[must_use]
    pub fn is_pessimal(&self) -> bool {
        self.opaque || !self.holes.is_empty()
    }

    /// Whether this row lists (or subsumes) nothing at all — an empty,
    /// non-opaque row (a genuinely pure·silent·untagged·total definition).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.is_pessimal()
            && self.reads.is_empty()
            && self.writes.is_empty()
            && self.calls.is_empty()
            && !self.emits
            && !self.tags
            && !self.faults
    }

    /// Fold `other` into `self` — the lattice join (set union per component,
    /// `opaque`/`emits`/`tags`/`faults` are sticky). Monotone: `self` only
    /// ever grows, which is what makes the [`solve_scc_effects`] fixpoint
    /// converge over the finite cells + kinds universe.
    pub fn join(&mut self, other: &EffectRow) {
        self.join_atoms(other);
        // Holes are indices into the *declaring definition's* param list, so
        // unioning them is only meaningful between two rows describing the
        // same definition (e.g. a fixpoint round's old and new estimate).
        // Folding a *callee's* row into a caller goes through
        // [`Self::join_atoms`] instead — see its doc.
        self.holes.extend(other.holes.iter().copied());
    }

    /// [`join`](Self::join) minus the [`holes`](Self::holes) component — the
    /// join used when folding a **callee's** row into a caller.
    ///
    /// A hole is an index into the *callee's* own param list; carrying it up
    /// into the caller's row would silently reinterpret it against the
    /// caller's params, which is neither sound nor meaningful. The caller
    /// instead discharges each of the callee's holes explicitly (fill it from
    /// the call site's traced argument, or take the pessimal floor) — see
    /// [`solve_scc_effects`].
    pub fn join_atoms(&mut self, other: &EffectRow) {
        self.reads.extend(other.reads.iter().copied());
        self.writes.extend(other.writes.iter().copied());
        self.calls.extend(other.calls.iter().cloned());
        self.opaque |= other.opaque;
        self.emits |= other.emits;
        self.tags |= other.tags;
        self.faults |= other.faults;
        self.faults_refined |= other.faults_refined;
    }

    /// Whether `self` conservatively covers (⊒) `other`: every atom `other`
    /// admits, `self` also admits. An opaque `self` covers anything; a
    /// non-opaque `self` can never cover an opaque `other`. This is the
    /// no-under-report relation the conservative-total property tests assert
    /// (spec §3): a def's inferred row must cover its own body atoms and every
    /// callee's row.
    #[must_use]
    pub fn covers(&self, other: &EffectRow) -> bool {
        if self.is_pessimal() {
            return true;
        }
        if other.is_pessimal() {
            return false;
        }
        other.reads.is_subset(&self.reads)
            && other.writes.is_subset(&self.writes)
            && other.calls.is_subset(&self.calls)
            && (self.emits || !other.emits)
            && (self.tags || !other.tags)
            && (self.faults || !other.faults)
    }
}

/// The raw per-definition atoms harvested from one body walk — the inputs the
/// [`solve_scc_effects`] fixpoint closes over. `reads`/`writes`/`calls` are the
/// direct atoms this body emits; `direct_calls` are the inferable
/// (knot/stitch) callees whose rows must be joined in transitively;
/// `creates_fn_values` are the targets this body creates fn values for
/// (Fork A, issue #1726 — structural, fed into the call graph alongside
/// `direct_calls`); `opaque` records that the body performed a call through a
/// function value whose reaching values were not all created in-project (or
/// another effects-opaque construct), forcing the pessimal floor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors EffectRow's independent dimension flags"
)]
pub struct EffectAtoms {
    pub reads: BTreeSet<DefinitionId>,
    pub writes: BTreeSet<DefinitionId>,
    /// Directly-called `EXTERNAL` binding names (call-kind atoms).
    pub calls: BTreeSet<String>,
    /// Inferable (knot/stitch) call targets — the edges the fixpoint follows.
    /// A superset shape of FG-2.1's `call_edges`, harvested from the same walk.
    pub direct_calls: BTreeSet<DefinitionId>,
    /// Fork A (`docs/decision-log.md` 2026-07-28 "Fork A — fn-value
    /// call-graph edges are harvested STRUCTURALLY", issue #1726): the
    /// inferable targets whose **fn values this body creates** — every
    /// `#fn(target, …)` literal in the body, whether or not the value is ever
    /// called here.
    ///
    /// **Structural, never row-derived.** The target of a `#fn` literal is a
    /// syntactic name, so deciding membership never consults an inferred row
    /// or signature — which is exactly what keeps `call_graph_query →
    /// scc_membership_query → solve_scc_query → call_graph_query` acyclic
    /// (§6.1 fixes every fn value's row at its creation site, and creation
    /// sites are syntactic). `bind(f, …)` adds nothing of its own: it copies
    /// an existing value rather than naming a new target.
    ///
    /// A **subset of [`Self::direct_calls`]** by construction — the same walk
    /// records a `#fn` target as a call-graph edge too, which is how these
    /// edges reach the SCC batching and [`solve_scc_effects`] with no change
    /// to either. Kept as its own set because "creates a value for `g`" and
    /// "calls `g`" are different facts: spec §7's token table and §8 rung 1's
    /// reachability slicing both need the creation sites specifically.
    ///
    /// **Lambda literals are still out of scope**: a lambda's
    /// `DefinitionId` is now minted at HIR time (`hir::stamp_container_ids`,
    /// issue #1727 — LIR lowering only reads it, no longer mints it), but a
    /// lambda literal has no index symbol / `DefKey` of its own, so there is
    /// still nothing here to record it against. Joining it into the SCC
    /// solve is #1770's job, not this one's.
    pub creates_fn_values: BTreeSet<DefinitionId>,
    /// This body calls through a function value whose reaching values were
    /// not all created in-project (or otherwise escapes the static call
    /// graph) — its row is pessimal (spec §3/§4). Fork A (issue #1726)
    /// collapsed this to a real row for the in-project case: a call through a
    /// local whose *every* write traced to a `#fn`/`bind` creation site
    /// narrows to the join over those targets instead. It stays pessimal for
    /// genuinely unknown sources — host callbacks (§6.2) and values loaded
    /// from the heap (§6.3).
    pub opaque: bool,
    /// NS-A2: this body directly contains a content-producing construct
    /// (see [`EffectRow::emits`]).
    pub emits: bool,
    /// NS-A2: this body directly touches the tag channel (see
    /// [`EffectRow::tags`]).
    pub tags: bool,
    /// NS-A2: this body directly contains a construct that can raise a
    /// turn-terminating fault (see [`EffectRow::faults`]).
    pub faults: bool,
    /// NS-A4 / F29(a): the refined faults bit (see
    /// [`EffectRow::faults_refined`]) — the same charge sites with local
    /// type-evidence discharges applied. `faults_refined → faults`.
    pub faults_refined: bool,
    /// §6.1 (issue #1680): the declaration indices of this body's own
    /// `fn`-typed params that it **calls through** — the row variables its
    /// row is parametric in. Becomes [`EffectRow::holes`] via
    /// [`Self::base_row`].
    ///
    /// **Structural, never row-derived**, exactly like
    /// [`Self::creates_fn_values`]: membership is decided by "the callee of
    /// this call site resolves to param #*i* of the enclosing definition",
    /// a syntactic fact, so the call graph stays row-independent (Fork A,
    /// §6.1a).
    ///
    /// A param only qualifies when the body cannot have changed what it
    /// holds: `ref` params are excluded outright (the callee's own caller
    /// aliases the slot), and so is any param the body assigns to or hands
    /// to a `ref` slot — for those the call site keeps [`Self::opaque`],
    /// the pre-#1680 behavior.
    pub param_holes: BTreeSet<u32>,
    /// §6.1 (issue #1680): the caller half of a row variable — for each
    /// `(callee, param index)` this body calls with a **traceable fn-value
    /// argument**, what that argument can hold. [`solve_scc_effects`] reads
    /// this to fill the callee row's [`EffectRow::holes`].
    ///
    /// Joined over *every* call site to that callee in this body (the walk is
    /// flow-insensitive), so two sites passing two different `#fn` targets
    /// yield both targets and the fill joins both — conservative, per Fork A's
    /// join-over-writes rule. A position with no entry, or an entry whose
    /// [`FnArgOrigins::untraced`] is set, cannot fill: the hole takes the
    /// pessimal floor instead.
    ///
    /// Recorded only for **inferable** (knot/stitch) callees — the only
    /// definitions that have a row with holes to fill.
    pub call_fn_args: BTreeMap<(DefinitionId, u32), FnArgOrigins>,
}

/// §6.1 (issue #1680): what one call site's argument in a given position can
/// hold, summarized structurally over every call to that callee in one body —
/// the caller-side material [`solve_scc_effects`] instantiates a callee's
/// [`EffectRow::holes`] from.
///
/// The same shape (and the same soundness rule) as `infer::body`'s per-local
/// write summary: a fill is legal only when *every* contributing argument
/// traced to an in-project creation target. One untraced argument and the
/// position is unusable — the value could have been created anywhere,
/// including outside the project (§6.2's host callbacks).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FnArgOrigins {
    /// The `#fn`/`bind`-chain creation targets this position's arguments
    /// traced to, sorted.
    pub targets: BTreeSet<DefinitionId>,
    /// At least one argument in this position did not trace to a creation
    /// site — the position cannot fill a hole.
    pub untraced: bool,
}

impl FnArgOrigins {
    /// Whether this position can fill a row variable: every contributing
    /// argument traced, and at least one target actually recorded.
    #[must_use]
    pub fn is_fillable(&self) -> bool {
        !self.untraced && !self.targets.is_empty()
    }
}

impl EffectAtoms {
    /// The base row before transitive closure — just this body's own atoms,
    /// excluding the `direct_calls`/`creates_fn_values` edges (which the
    /// fixpoint resolves to their callees' rows).
    #[must_use]
    pub fn base_row(&self) -> EffectRow {
        EffectRow {
            reads: self.reads.clone(),
            writes: self.writes.clone(),
            calls: self.calls.clone(),
            opaque: self.opaque,
            emits: self.emits,
            tags: self.tags,
            faults: self.faults,
            faults_refined: self.faults_refined,
            holes: self.param_holes.clone(),
        }
    }
}

/// Solve one SCC batch's effect-row fixpoint (spec §4 — the same per-SCC join
/// TM-1's type solver runs, lifted to the effect lattice). Lifts
/// `infer::solve_one_batch`'s shape: `known_rows` must already carry the
/// finalized row of every def *outside* `batch` that a member calls (every
/// condensation-predecessor SCC's rows); `atoms` carries every batch member's
/// harvested [`EffectAtoms`].
///
/// Each member's row is `base_row ⊔ (join of every direct callee's row)`,
/// re-evaluated until nothing changes. A callee inside `batch` reads the
/// current in-round estimate (that is the mutual-recursion fixpoint); a callee
/// in `known_rows` reads its finalized row; a callee found in neither is an
/// unknown target → pessimal (defensive — `direct_calls` only ever holds
/// inferable ids, so every callee is normally resolvable, but soundness never
/// depends on that).
///
/// Terminates: `join` is monotone over the finite cells + kinds lattice, so
/// each round either grows some row (bounded by the universe) or stabilizes;
/// the round cap is a house-rule guard against unbounded growth, never
/// load-bearing for a well-formed batch.
#[must_use]
pub fn solve_scc_effects(
    batch: &BTreeSet<DefinitionId>,
    atoms: &BTreeMap<DefinitionId, EffectAtoms>,
    known_rows: &BTreeMap<DefinitionId, EffectRow>,
) -> BTreeMap<DefinitionId, EffectRow> {
    // Seed each member with its own base atoms.
    let mut rows: BTreeMap<DefinitionId, EffectRow> = batch
        .iter()
        .map(|&id| {
            let base = atoms
                .get(&id)
                .map(EffectAtoms::base_row)
                .unwrap_or_default();
            (id, base)
        })
        .collect();

    // Information flows one call-hop per round; an SCC's diameter is at most
    // its member count, so `batch.len()` rounds suffice for convergence within
    // the component. `+ 1` leaves headroom; the `changed` break exits earlier
    // in every real case.
    let cap = batch.len().saturating_add(1);
    for _round in 0..cap {
        let mut changed = false;
        for &id in batch {
            let Some(member_atoms) = atoms.get(&id) else {
                continue;
            };
            let mut next = member_atoms.base_row();
            for &callee in &member_atoms.direct_calls {
                // In-batch member → current fixpoint estimate; otherwise a
                // finalized predecessor-SCC row.
                let Some(row) = rows.get(&callee).or_else(|| known_rows.get(&callee)) else {
                    // Unknown callee — no row to read → pessimal (spec §4).
                    next.opaque = true;
                    continue;
                };
                // `join_atoms`, not `join`: the callee's own §6.1 holes index
                // *its* param list, so they are discharged here rather than
                // carried up into this caller's row.
                next.join_atoms(row);
                for &hole in &row.holes {
                    instantiate_hole(
                        &mut next,
                        member_atoms.call_fn_args.get(&(callee, hole)),
                        &rows,
                        known_rows,
                    );
                }
            }
            if rows.get(&id) != Some(&next) {
                changed = true;
                rows.insert(id, next);
            }
        }
        if !changed {
            break;
        }
    }

    rows
}

/// Discharge one of a callee's §6.1 row variables into the caller's row
/// under construction (issue #1680).
///
/// `origins` is what the caller passed in that param position, as summarized
/// structurally by the body walk ([`EffectAtoms::call_fn_args`]). The hole is
/// **filled** — the caller absorbs each traced target's row instead of the
/// callee's floor — only when every one of these hold:
///
/// - the position is [fillable](FnArgOrigins::is_fillable): recorded, and
///   every contributing argument traced to an in-project creation site;
/// - every traced target has a row here (in-batch estimate or finalized
///   predecessor);
/// - that row carries no holes of its own — §6.1's shallow-polymorphism
///   ruling ("every value's row is fixed at its creation site") means a fill
///   is a *ground* row; chaining one hole into another is deliberately not
///   attempted.
///
/// Any other case takes the pessimal floor, which is exactly the pre-#1680
/// behavior for a call through a fn-typed param. Every branch either narrows
/// or degrades to `opaque` — never silently drops the callee's effects.
///
/// `pub(crate)`: also reused by `infer::mod`'s
/// `conservative_total_no_under_report_over_mutual_recursion` property test,
/// which must instantiate a holed callee's row the same way this fixpoint
/// does before comparing it against a caller's row with `covers` — the raw,
/// still-parametric callee row is never itself a coverable target.
pub(crate) fn instantiate_hole(
    next: &mut EffectRow,
    origins: Option<&FnArgOrigins>,
    rows: &BTreeMap<DefinitionId, EffectRow>,
    known_rows: &BTreeMap<DefinitionId, EffectRow>,
) {
    let Some(origins) = origins.filter(|o| o.is_fillable()) else {
        next.opaque = true;
        return;
    };
    for target in &origins.targets {
        match rows.get(target).or_else(|| known_rows.get(target)) {
            Some(row) if row.holes.is_empty() => next.join_atoms(row),
            // A hole-carrying (still parametric) target, or one with no row
            // at all: no ground answer to substitute → floor.
            Some(row) => {
                next.join_atoms(row);
                next.opaque = true;
            }
            None => next.opaque = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::{DefinitionId, DefinitionTag};

    fn cell(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::GlobalVar, n)
    }

    #[test]
    fn join_is_set_union_and_opaque_is_sticky() {
        let mut a = EffectRow {
            reads: [cell(1)].into_iter().collect(),
            calls: ["Play".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let b = EffectRow {
            reads: [cell(2)].into_iter().collect(),
            writes: [cell(3)].into_iter().collect(),
            opaque: true,
            ..Default::default()
        };
        a.join(&b);
        assert_eq!(a.reads, [cell(1), cell(2)].into_iter().collect());
        assert_eq!(a.writes, [cell(3)].into_iter().collect());
        assert_eq!(a.calls, ["Play".to_string()].into_iter().collect());
        assert!(a.opaque, "opaque must be sticky under join");
    }

    #[test]
    fn covers_is_superset_and_opaque_tops_the_lattice() {
        let big = EffectRow {
            reads: [cell(1), cell(2)].into_iter().collect(),
            ..Default::default()
        };
        let small = EffectRow {
            reads: [cell(1)].into_iter().collect(),
            ..Default::default()
        };
        assert!(big.covers(&small));
        assert!(!small.covers(&big));

        let pess = EffectRow::pessimal();
        assert!(pess.covers(&big), "pessimal covers everything");
        assert!(!big.covers(&pess), "no concrete row covers pessimal");
        assert!(pess.covers(&pess));
    }

    #[test]
    fn solve_scc_effects_propagates_a_callee_row_to_its_caller() {
        // up(1) -> leaf(2); leaf reads cell(10), calls "Play".
        let up = cell(1);
        let leaf = cell(2);
        let batch: BTreeSet<DefinitionId> = [up].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            up,
            EffectAtoms {
                writes: [cell(20)].into_iter().collect(),
                direct_calls: [leaf].into_iter().collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let known_rows: BTreeMap<DefinitionId, EffectRow> = [(
            leaf,
            EffectRow {
                reads: [cell(10)].into_iter().collect(),
                calls: ["Play".to_string()].into_iter().collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();

        let rows = solve_scc_effects(&batch, &atoms, &known_rows);
        let row = &rows[&up];
        assert_eq!(row.reads, [cell(10)].into_iter().collect());
        assert_eq!(row.writes, [cell(20)].into_iter().collect());
        assert_eq!(row.calls, ["Play".to_string()].into_iter().collect());
        assert!(!row.opaque);
    }

    #[test]
    fn solve_scc_effects_reaches_a_mutual_recursion_fixpoint() {
        // a(1) <-> b(2) in one SCC. a reads cell(10), b writes cell(20); each
        // calls the other. The fixpoint must give both the union of both atoms.
        let a = cell(1);
        let b = cell(2);
        let batch: BTreeSet<DefinitionId> = [a, b].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [
            (
                a,
                EffectAtoms {
                    reads: [cell(10)].into_iter().collect(),
                    direct_calls: [b].into_iter().collect(),
                    ..Default::default()
                },
            ),
            (
                b,
                EffectAtoms {
                    writes: [cell(20)].into_iter().collect(),
                    direct_calls: [a].into_iter().collect(),
                    ..Default::default()
                },
            ),
        ]
        .into_iter()
        .collect();

        let rows = solve_scc_effects(&batch, &atoms, &BTreeMap::new());
        for id in [a, b] {
            let row = &rows[&id];
            assert_eq!(
                row.reads,
                [cell(10)].into_iter().collect(),
                "both SCC members see a's read"
            );
            assert_eq!(
                row.writes,
                [cell(20)].into_iter().collect(),
                "both SCC members see b's write"
            );
        }
    }

    #[test]
    fn join_carries_emits_tags_faults_stickily() {
        let mut a = EffectRow::default();
        let b = EffectRow {
            emits: true,
            ..Default::default()
        };
        let c = EffectRow {
            tags: true,
            faults: true,
            ..Default::default()
        };
        a.join(&b);
        a.join(&c);
        assert!(a.emits && a.tags && a.faults);
        // Joining an empty row afterwards never clears them (sticky).
        a.join(&EffectRow::default());
        assert!(a.emits && a.tags && a.faults);
    }

    #[test]
    fn covers_is_per_dimension_for_emits_tags_faults() {
        let silent = EffectRow::default();
        let emitting = EffectRow {
            emits: true,
            ..Default::default()
        };
        let tagging = EffectRow {
            tags: true,
            ..Default::default()
        };
        let faulting = EffectRow {
            faults: true,
            ..Default::default()
        };
        assert!(!silent.covers(&emitting));
        assert!(!silent.covers(&tagging));
        assert!(!silent.covers(&faulting));
        assert!(
            emitting.covers(&silent),
            "asserting less than reality is legal"
        );
        // The dimensions are independent: emits does not cover tags/faults.
        assert!(!emitting.covers(&tagging));
        assert!(!emitting.covers(&faulting));
        assert!(!tagging.covers(&emitting));
        // Opaque tops all three new dimensions too.
        let pess = EffectRow::pessimal();
        assert!(pess.covers(&emitting));
        assert!(pess.covers(&tagging));
        assert!(pess.covers(&faulting));
        assert!(!faulting.covers(&pess));
    }

    #[test]
    fn solve_scc_effects_propagates_emitter_tagger_faulter_status_transitively() {
        // up(1) -> leaf(2); leaf emits + tags + faults, up is glue-only.
        let up = cell(1);
        let leaf = cell(2);
        let batch: BTreeSet<DefinitionId> = [up].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            up,
            EffectAtoms {
                direct_calls: [leaf].into_iter().collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let known_rows: BTreeMap<DefinitionId, EffectRow> = [(
            leaf,
            EffectRow {
                emits: true,
                tags: true,
                faults: true,
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();

        let rows = solve_scc_effects(&batch, &atoms, &known_rows);
        let row = &rows[&up];
        assert!(row.emits, "glue-only caller of an emitter still emits");
        assert!(row.tags);
        assert!(row.faults);
        assert!(!row.opaque);
    }

    #[test]
    fn an_opaque_atom_makes_the_whole_row_pessimal() {
        let a = cell(1);
        let batch: BTreeSet<DefinitionId> = [a].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            a,
            EffectAtoms {
                reads: [cell(10)].into_iter().collect(),
                opaque: true,
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let rows = solve_scc_effects(&batch, &atoms, &BTreeMap::new());
        assert!(rows[&a].opaque);
    }

    /// §6.1 (issue #1680) at the lattice level: an unfilled row variable is
    /// as unbounded as intrinsic opacity, and `covers` must treat it that way
    /// or the no-under-report invariant breaks the moment a holed row is
    /// compared with an assertion.
    #[test]
    fn a_hole_tops_the_lattice_exactly_like_opaque() {
        let holed = EffectRow {
            holes: [0].into_iter().collect(),
            ..Default::default()
        };
        let concrete = EffectRow {
            reads: [cell(1)].into_iter().collect(),
            ..Default::default()
        };
        assert!(holed.is_pessimal());
        assert!(!holed.opaque, "the hole is not the intrinsic opaque bit");
        assert!(!holed.is_empty(), "a parametric row is never 'empty'");
        assert!(holed.covers(&concrete));
        assert!(!concrete.covers(&holed));
    }

    /// A callee's holes index the *callee's* param list, so folding its row
    /// into a caller must not carry them up — that is `join_atoms`, and it is
    /// what the solver uses for every callee edge.
    #[test]
    fn join_atoms_leaves_the_callees_holes_behind() {
        let mut up = EffectRow::default();
        let down = EffectRow {
            reads: [cell(1)].into_iter().collect(),
            holes: [2].into_iter().collect(),
            ..Default::default()
        };
        up.join_atoms(&down);
        assert_eq!(up.reads, [cell(1)].into_iter().collect());
        assert!(up.holes.is_empty());

        // `join` (same-definition estimates) does union them.
        let mut same_def = EffectRow::default();
        same_def.join(&down);
        assert_eq!(same_def.holes, [2].into_iter().collect::<BTreeSet<u32>>());
    }

    #[test]
    fn a_traced_argument_instantiates_the_callees_row_variable() {
        // caller(1) -> higher_order(2), whose row holes at param 0; the call
        // site passes a fn value for target(3), which writes cell(20).
        let caller = cell(1);
        let higher_order = cell(2);
        let target = cell(3);
        let batch: BTreeSet<DefinitionId> = [caller].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            caller,
            EffectAtoms {
                direct_calls: [higher_order, target].into_iter().collect(),
                call_fn_args: [(
                    (higher_order, 0),
                    FnArgOrigins {
                        targets: [target].into_iter().collect(),
                        untraced: false,
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let known_rows: BTreeMap<DefinitionId, EffectRow> = [
            (
                higher_order,
                EffectRow {
                    holes: [0].into_iter().collect(),
                    ..Default::default()
                },
            ),
            (
                target,
                EffectRow {
                    writes: [cell(20)].into_iter().collect(),
                    ..Default::default()
                },
            ),
        ]
        .into_iter()
        .collect();

        let rows = solve_scc_effects(&batch, &atoms, &known_rows);
        let row = &rows[&caller];
        assert!(!row.is_pessimal(), "a filled hole is not a floor");
        assert!(row.holes.is_empty(), "the callee's hole is not inherited");
        assert!(
            row.writes.contains(&cell(20)),
            "the instantiated row carries the argument target's own writes"
        );
    }

    #[test]
    fn an_untraced_or_missing_argument_leaves_the_hole_pessimal() {
        let caller = cell(1);
        let higher_order = cell(2);
        let target = cell(3);
        let holed: BTreeMap<DefinitionId, EffectRow> = [(
            higher_order,
            EffectRow {
                holes: [0].into_iter().collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let batch: BTreeSet<DefinitionId> = [caller].into_iter().collect();

        for (label, origins) in [
            ("no entry at all", None),
            (
                "an untraced write in the position",
                Some(FnArgOrigins {
                    targets: [target].into_iter().collect(),
                    untraced: true,
                }),
            ),
            ("an entry with no targets", Some(FnArgOrigins::default())),
        ] {
            let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
                caller,
                EffectAtoms {
                    direct_calls: [higher_order].into_iter().collect(),
                    call_fn_args: origins
                        .into_iter()
                        .map(|o| ((higher_order, 0), o))
                        .collect(),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect();
            let rows = solve_scc_effects(&batch, &atoms, &holed);
            assert!(rows[&caller].opaque, "{label} must keep the pessimal floor");
        }
    }

    /// §6.1 is shallow: a fill target whose own row is still parametric has
    /// no ground answer to substitute, so the caller takes the floor rather
    /// than chaining one hole into another.
    #[test]
    fn a_still_parametric_fill_target_keeps_the_floor() {
        let caller = cell(1);
        let higher_order = cell(2);
        let target = cell(3);
        let batch: BTreeSet<DefinitionId> = [caller].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            caller,
            EffectAtoms {
                direct_calls: [higher_order].into_iter().collect(),
                call_fn_args: [(
                    (higher_order, 0),
                    FnArgOrigins {
                        targets: [target].into_iter().collect(),
                        untraced: false,
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let known_rows: BTreeMap<DefinitionId, EffectRow> = [
            (
                higher_order,
                EffectRow {
                    holes: [0].into_iter().collect(),
                    ..Default::default()
                },
            ),
            (
                target,
                EffectRow {
                    writes: [cell(20)].into_iter().collect(),
                    holes: [0].into_iter().collect(),
                    ..Default::default()
                },
            ),
        ]
        .into_iter()
        .collect();

        let rows = solve_scc_effects(&batch, &atoms, &known_rows);
        assert!(rows[&caller].opaque);
        assert!(
            rows[&caller].writes.contains(&cell(20)),
            "degrading to the floor still absorbs everything the target listed"
        );
    }

    /// A fill target with no row anywhere (a torn edge) must degrade, never
    /// silently drop the callback's effects.
    #[test]
    fn a_fill_target_with_no_row_forces_pessimal() {
        let caller = cell(1);
        let higher_order = cell(2);
        let ghost = cell(99);
        let batch: BTreeSet<DefinitionId> = [caller].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            caller,
            EffectAtoms {
                direct_calls: [higher_order].into_iter().collect(),
                call_fn_args: [(
                    (higher_order, 0),
                    FnArgOrigins {
                        targets: [ghost].into_iter().collect(),
                        untraced: false,
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let known_rows: BTreeMap<DefinitionId, EffectRow> = [(
            higher_order,
            EffectRow {
                holes: [0].into_iter().collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let rows = solve_scc_effects(&batch, &atoms, &known_rows);
        assert!(rows[&caller].opaque);
    }

    #[test]
    fn an_unknown_callee_forces_pessimal() {
        // caller's direct_call target is absent from both batch and known_rows
        // (a torn/unknown edge) — the row must degrade to pessimal, never
        // silently under-report.
        let a = cell(1);
        let ghost = cell(99);
        let batch: BTreeSet<DefinitionId> = [a].into_iter().collect();
        let atoms: BTreeMap<DefinitionId, EffectAtoms> = [(
            a,
            EffectAtoms {
                direct_calls: [ghost].into_iter().collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let rows = solve_scc_effects(&batch, &atoms, &BTreeMap::new());
        assert!(rows[&a].opaque);
    }
}
