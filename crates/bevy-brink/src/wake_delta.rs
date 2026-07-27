//! Row-directed wake dirtying (issue #1146, the #1101 fix): *which* shared
//! `World` cells a batch turn actually wrote, so
//! [`mark_wake_dirty`](crate::mark_wake_dirty) can re-evaluate only the wake
//! conditions whose **effect read row** intersects that set.
//!
//! Before this, the only dependency signal the wake layer had was bevy's
//! resource-level change detection on [`BrinkGlobals`](crate::BrinkGlobals):
//! *any* mutation re-checked *every* parked all-detect-capable policy. Every
//! real batch turn writes bookkeeping (visit counts, turn index), so that
//! signal fires on every frame a flow steps — enough, for a persistent
//! policy whose condition stays true, to re-wake a flow no dependency of
//! which moved (#1101's spurious wake; measured at ~1-in-12 sleep-suite runs
//! before this fix).
//!
//! The ledger closes that: Apply records the cells it wrote
//! ([`WorldDelta`]), and the wake pass intersects them with each condition's
//! inferred read row (`docs/effects-spec.md` §11 — the A2/A6 row machinery
//! consumed for scheduler precision). A `gate`-reading condition is
//! automatically inert to a turn that only bumped a visit count.
//!
//! ## Attribution — why [`BrinkWorldDelta::drain`] returns an `Option`
//!
//! The ledger is only usable when it is a **complete** account of every
//! change since the last drain. [`BrinkGlobals::inner`](crate::BrinkGlobals)
//! is a public field: a host system (or a test) can write an ink global
//! directly, and the serial driver ([`advance_flow`](crate::advance_flow))
//! writes it without going through batch Apply at all. Neither is recorded
//! here. Three facts together decide it, and between them every unrecorded
//! write lands on one side or the other of the driver's own Apply:
//!
//! - **recorded** — a batch Apply positively recorded a turn since the last
//!   drain. Without one (a serial-mode host, an idle frame), the ledger
//!   explains nothing at all.
//! - **foreign** — a batch driver observed [`BrinkGlobals`] already changed
//!   when it *started* its turn, i.e. somebody else wrote between that
//!   driver's previous run and this one. Sticky until the next drain. This
//!   catches every unrecorded write that lands **before** an Apply.
//! - **the recorded change tick** — the [`BrinkGlobals`] change tick as of
//!   the last Apply. If the resource's live tick has moved past it, somebody
//!   wrote **after** that Apply, which is the other side of the same window
//!   (and the side the batch driver cannot see at all, since the wake pass
//!   is ordered before it).
//!
//! [`drain`](BrinkWorldDelta::drain) hands back `Some(delta)` only when all
//! three agree; otherwise `None`, and the wake pass falls back to the
//! pre-#1146 coarse behavior (any change re-checks every parked policy).
//! Over-report, never under-report — a missed wake is the engine-race bug
//! class (`docs/decision-log.md` 2026-07-18).

use std::collections::BTreeSet;
use std::marker::PhantomData;

use bevy_ecs::change_detection::Tick;
use bevy_ecs::resource::Resource;

/// The set of shared-`World` cells written over one accounting window — the
/// changed-set [`mark_wake_dirty`](crate::mark_wake_dirty) intersects each
/// wake condition's read row against.
///
/// Two granularities, because that is exactly what the row can express
/// (`brink_format::DirectEffects::reads` is "global cells this row may
/// read"):
///
/// - [`globals`](Self::globals) — per-cell, by global **slot index** (the
///   `Program::global_index` numbering `World::set_global` takes), so a
///   condition reading `gate` is unaffected by a write to `mood`.
/// - [`bookkeeping`](Self::touched_bookkeeping) — one coarse bit covering
///   visit counts, turn counts, the turn index, and RNG state. Effect rows
///   model **no** read of these (see [`FlowSleep::reads_bookkeeping`]), so
///   there is nothing finer to intersect against.
///
/// [`FlowSleep::reads_bookkeeping`]: crate::FlowSleep::reads_bookkeeping
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldDelta {
    globals: BTreeSet<u32>,
    bookkeeping: bool,
}

impl WorldDelta {
    /// The global slot indices written over this window.
    #[must_use]
    pub fn globals(&self) -> &BTreeSet<u32> {
        &self.globals
    }

    /// Whether any bookkeeping cell (visit count, turn count, turn index, RNG
    /// state) was written over this window.
    #[must_use]
    pub fn touched_bookkeeping(&self) -> bool {
        self.bookkeeping
    }

    /// Nothing was written at all — no policy can need re-evaluation on this
    /// window's account.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.globals.is_empty() && !self.bookkeeping
    }

    /// Record a write to the global at slot `idx`.
    pub(crate) fn note_global(&mut self, idx: u32) {
        self.globals.insert(idx);
    }

    /// Record a write to any bookkeeping cell.
    pub(crate) fn note_bookkeeping(&mut self) {
        self.bookkeeping = true;
    }

    /// Fold `other`'s cells into this delta (the ledger accumulates across
    /// every turn between two drains — a wake pass may not run every frame).
    pub(crate) fn absorb(&mut self, other: &WorldDelta) {
        self.globals.extend(other.globals.iter().copied());
        self.bookkeeping |= other.bookkeeping;
    }
}

/// The per-marker changed-cell ledger: batch Apply records into it, the wake
/// pass drains it. See the module docs for the attribution contract.
///
/// Inserted by [`BrinkPlugin`](crate::BrinkPlugin), so it is always present
/// for a marker whose plugin is installed. Growth is bounded by the story's
/// global count plus one bool, so an app with no sleeping flows (nothing ever
/// drains it) cannot accumulate without limit.
#[derive(Resource, Debug)]
pub struct BrinkWorldDelta<M: Send + Sync + 'static = ()> {
    delta: WorldDelta,
    recorded: bool,
    foreign: bool,
    /// [`BrinkGlobals`](crate::BrinkGlobals)'s change tick as of the last
    /// recorded Apply — see the module docs' attribution contract.
    recorded_tick: Option<Tick>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkWorldDelta<M> {
    fn default() -> Self {
        Self {
            delta: WorldDelta::default(),
            recorded: false,
            foreign: false,
            recorded_tick: None,
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkWorldDelta<M> {
    /// Record one batch turn's Apply changeset, plus
    /// [`BrinkGlobals`](crate::BrinkGlobals)'s change tick as it stands at
    /// the end of that Apply. Unions into whatever is already pending —
    /// several turns may pass between two drains.
    pub(crate) fn record(&mut self, delta: &WorldDelta, globals_tick: Option<Tick>) {
        self.delta.absorb(delta);
        self.recorded = true;
        self.recorded_tick = globals_tick;
    }

    /// Record that the wake pass evaluated at least one condition this frame
    /// (`run_flow_sleep`'s Evaluate phase, issue #1146).
    ///
    /// A purity-gated condition writes no global cell, but building its
    /// context still takes `&mut` on the shared world, and the evaluation can
    /// legitimately move **bookkeeping** (a counted container's visit count,
    /// an RNG draw). `run_flow_sleep` restores the resource's change tick so
    /// that unavoidable `&mut` stops manufacturing a world-changed signal,
    /// and hands the honest residue here instead — an attributed
    /// bookkeeping-only touch.
    ///
    /// This is **unconditional**: it notes a bookkeeping touch every time an
    /// Evaluate phase runs at all, whether or not that particular pass
    /// actually moved a visit count / turn index / RNG draw. The chosen
    /// consequence — a `FlowSleep::reads_bookkeeping` condition's own prior
    /// evaluation is enough to re-flag it for another one, forever, once it
    /// has evaluated a single time — is documented on
    /// [`reads_bookkeeping`](crate::FlowSleep::reads_bookkeeping) and covered
    /// by a regression test in `crate::sleep::tests`. It is the over-report
    /// side of this module's "never under-report" law, not a missed-wake
    /// risk.
    pub(crate) fn record_condition_evaluation(&mut self) {
        self.delta.note_bookkeeping();
        self.recorded = true;
    }

    /// Note that a writer this ledger cannot account for touched the shared
    /// world (a host system, a serial driver, a direct
    /// `BrinkGlobals::inner` write). Sticky until the next drain: the pending
    /// delta is no longer a complete account, so [`drain`](Self::drain) will
    /// report `None`.
    pub(crate) fn note_foreign(&mut self) {
        self.foreign = true;
    }

    /// Take the pending changeset and reset the ledger.
    ///
    /// `Some(delta)` only when the ledger is a **complete** account of every
    /// shared-world change since the previous drain; `None` means the caller
    /// must fall back to the coarse "anything may have changed" posture.
    /// Either way the ledger is reset, so the next window starts clean.
    ///
    /// `globals_last_changed` / `globals_changed` are
    /// [`BrinkGlobals`](crate::BrinkGlobals)'s live change tick and its
    /// change bit as the *draining* system sees them: a live tick past the
    /// one the last Apply recorded means somebody wrote after that Apply, the
    /// half of the window a batch driver's own entry check cannot see (see
    /// the module docs). A `globals_changed` of `false` means nothing moved
    /// the resource at all since this system last ran, so the recorded tick
    /// has nothing to disagree with.
    pub(crate) fn drain(
        &mut self,
        globals_last_changed: Option<Tick>,
        globals_changed: bool,
    ) -> Option<WorldDelta> {
        let complete = self.recorded
            && !self.foreign
            && (!globals_changed || globals_last_changed == self.recorded_tick);
        let delta = std::mem::take(&mut self.delta);
        self.recorded = false;
        self.foreign = false;
        self.recorded_tick = None;
        complete.then_some(delta)
    }

    /// The changeset pending since the last drain — inspector/debug read.
    /// Says nothing about whether it is a complete account; see
    /// [`drain`](Self::drain).
    #[must_use]
    pub fn pending(&self) -> &WorldDelta {
        &self.delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tick an Apply is pretended to have left `BrinkGlobals` at, and the
    /// matching "the draining system agrees" arguments.
    const APPLY_TICK: Tick = Tick::new(7);

    fn drain_agreeing(ledger: &mut BrinkWorldDelta<()>) -> Option<WorldDelta> {
        ledger.drain(Some(APPLY_TICK), true)
    }

    #[test]
    fn drain_reports_none_until_a_turn_is_recorded() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        assert!(
            drain_agreeing(&mut ledger).is_none(),
            "a ledger no driver has recorded into explains nothing — the caller must stay \
             conservative"
        );
    }

    #[test]
    fn drain_reports_the_recorded_cells_and_resets() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        let mut turn = WorldDelta::default();
        turn.note_global(3);
        turn.note_bookkeeping();
        ledger.record(&turn, Some(APPLY_TICK));

        let drained = drain_agreeing(&mut ledger).expect("a recorded turn is a complete account");
        assert!(drained.globals().contains(&3));
        assert!(drained.touched_bookkeeping());

        assert!(
            drain_agreeing(&mut ledger).is_none(),
            "draining resets the ledger — the next window starts clean and unattributed"
        );
    }

    #[test]
    fn drain_accumulates_every_turn_between_two_drains() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        let mut first = WorldDelta::default();
        first.note_global(1);
        ledger.record(&first, Some(Tick::new(3)));
        let mut second = WorldDelta::default();
        second.note_global(2);
        ledger.record(&second, Some(APPLY_TICK));

        let drained = drain_agreeing(&mut ledger).expect("recorded");
        assert!(
            drained.globals().contains(&1) && drained.globals().contains(&2),
            "a wake pass that skipped a frame must still see the earlier turn's cells"
        );
    }

    #[test]
    fn a_foreign_write_makes_the_window_unattributable() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        ledger.note_foreign();
        let mut turn = WorldDelta::default();
        turn.note_global(1);
        ledger.record(&turn, Some(APPLY_TICK));
        assert!(
            drain_agreeing(&mut ledger).is_none(),
            "a write landing before the Apply (host system, serial driver) must force the \
             conservative path — never under-report"
        );
        assert!(
            drain_agreeing(&mut ledger).is_none(),
            "the foreign flag clears on drain, but the window is then unrecorded again"
        );
    }

    #[test]
    fn a_write_after_the_apply_makes_the_window_unattributable() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        let mut turn = WorldDelta::default();
        turn.note_bookkeeping();
        ledger.record(&turn, Some(APPLY_TICK));
        assert!(
            ledger.drain(Some(Tick::new(9)), true).is_none(),
            "a live change tick past the one the Apply recorded means somebody wrote after it \
             — the half of the window the batch driver's own entry check cannot see"
        );
    }

    #[test]
    fn an_unchanged_resource_has_nothing_for_the_recorded_tick_to_disagree_with() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        ledger.record_condition_evaluation();
        let drained = ledger
            .drain(Some(Tick::new(99)), false)
            .expect("nothing moved the resource, so the recorded tick cannot be stale");
        assert!(
            drained.touched_bookkeeping(),
            "a condition evaluation's bookkeeping residue survives even though it \
             deliberately leaves no change tick behind"
        );
    }

    #[test]
    fn an_empty_delta_is_distinguishable_from_an_unattributed_one() {
        let mut ledger = BrinkWorldDelta::<()>::default();
        ledger.record(&WorldDelta::default(), Some(APPLY_TICK));
        let drained = drain_agreeing(&mut ledger).expect("recorded, even though it wrote nothing");
        assert!(
            drained.is_empty(),
            "a turn that wrote nothing is a complete account of *no* change — not a reason \
             to re-check every parked policy"
        );
    }
}
