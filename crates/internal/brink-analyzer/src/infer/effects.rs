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
//! - **Types**: rows conceptually ride `Ty::Fn` (spec §5, the heap answer);
//!   this slice keeps the row *inference* advisory and treats every call
//!   through a function value as **opaque** (the conservative floor — see
//!   [`EffectRow::opaque`]), which is sound; reading a concrete row back off a
//!   stored `Ty::Fn` value is the §8 precision refinement left for a follow-up.
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
}

impl EffectRow {
    /// The pessimal touches-everything row (spec §3) — always sound, the
    /// answer for an `Unknown`/opaque callee.
    #[must_use]
    pub fn pessimal() -> Self {
        Self {
            opaque: true,
            ..Self::default()
        }
    }

    /// Whether this row lists (or subsumes) nothing at all — an empty,
    /// non-opaque row (a genuinely pure definition).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.opaque && self.reads.is_empty() && self.writes.is_empty() && self.calls.is_empty()
    }

    /// Fold `other` into `self` — the lattice join (set union per component,
    /// `opaque` is sticky). Monotone: `self` only ever grows, which is what
    /// makes the [`solve_scc_effects`] fixpoint converge over the finite
    /// cells + kinds universe.
    pub fn join(&mut self, other: &EffectRow) {
        self.reads.extend(other.reads.iter().copied());
        self.writes.extend(other.writes.iter().copied());
        self.calls.extend(other.calls.iter().cloned());
        self.opaque |= other.opaque;
    }

    /// Whether `self` conservatively covers (⊒) `other`: every atom `other`
    /// admits, `self` also admits. An opaque `self` covers anything; a
    /// non-opaque `self` can never cover an opaque `other`. This is the
    /// no-under-report relation the conservative-total property tests assert
    /// (spec §3): a def's inferred row must cover its own body atoms and every
    /// callee's row.
    #[must_use]
    pub fn covers(&self, other: &EffectRow) -> bool {
        if self.opaque {
            return true;
        }
        if other.opaque {
            return false;
        }
        other.reads.is_subset(&self.reads)
            && other.writes.is_subset(&self.writes)
            && other.calls.is_subset(&self.calls)
    }
}

/// The raw per-definition atoms harvested from one body walk — the inputs the
/// [`solve_scc_effects`] fixpoint closes over. `reads`/`writes`/`calls` are the
/// direct atoms this body emits; `direct_calls` are the inferable
/// (knot/stitch) callees whose rows must be joined in transitively;
/// `opaque` records that the body performed a call through a function value
/// (or another effects-opaque construct), forcing the pessimal floor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectAtoms {
    pub reads: BTreeSet<DefinitionId>,
    pub writes: BTreeSet<DefinitionId>,
    /// Directly-called `EXTERNAL` binding names (call-kind atoms).
    pub calls: BTreeSet<String>,
    /// Inferable (knot/stitch) call targets — the edges the fixpoint follows.
    /// A superset shape of FG-2.1's `call_edges`, harvested from the same walk.
    pub direct_calls: BTreeSet<DefinitionId>,
    /// This body calls through a function value (or otherwise escapes the
    /// static call graph) — its row is pessimal (spec §3/§4).
    pub opaque: bool,
}

impl EffectAtoms {
    /// The base row before transitive closure — just this body's own atoms,
    /// excluding the `direct_calls` edges (which the fixpoint resolves to
    /// their callees' rows).
    #[must_use]
    pub fn base_row(&self) -> EffectRow {
        EffectRow {
            reads: self.reads.clone(),
            writes: self.writes.clone(),
            calls: self.calls.clone(),
            opaque: self.opaque,
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
            for callee in &member_atoms.direct_calls {
                if let Some(row) = rows.get(callee) {
                    // In-batch member: current fixpoint estimate.
                    next.join(row);
                } else if let Some(row) = known_rows.get(callee) {
                    // Finalized predecessor-SCC row.
                    next.join(row);
                } else {
                    // Unknown callee — no row to read → pessimal (spec §4).
                    next.opaque = true;
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
