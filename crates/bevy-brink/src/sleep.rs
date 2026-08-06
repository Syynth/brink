//! BH-4: `FlowSleep` and the reactive-wake contract (`docs/effects-spec.md`
//! §13.1; decision-log 2026-07-18; tracking #897, this slice #973).
//!
//! Reactive sleep is **host-driven** — there is no ink-level `await` construct
//! (that is a recorded future direction). The game sets a flow's **standing
//! wake policy** by attaching a [`FlowSleep`] component; ink authors write
//! ordinary knots. The precise contract, as ruled:
//!
//! 1. [`FlowSleep`] does **not** park a flow — flows park at their own natural
//!    yield points (turn end, `-> DONE`). The policy governs *waking*: a parked
//!    flow under a policy is **skipped by Collect** in both drivers
//!    ([`advance_batch`] and
//!    [`advance_batch_parallel`](crate::batch::parallel::advance_batch_parallel)
//!    each filter it out — [`FlowSleep::wants_collect`] is the shared
//!    predicate), so a parked flow costs **zero** per turn no matter which
//!    driver a host uses.
//! 2. A dependency changing triggers **re-evaluation, not waking**: the
//!    condition (a pure ink fn — purity is provable from its effect row) is
//!    re-evaluated only when a dependency moved, and the flow wakes **only when
//!    the condition is true** ("re-evaluate, don't wake"). Re-evaluation runs
//!    in the **owning flow's context** (shared World ⊕ that flow's own locals,
//!    never a bare world) via [`call_ink_function`](crate::call_ink_function).
//! 3. A woken flow runs a normal turn; the condition has no mid-turn influence.
//! 4. Policies are **persistent by default** (re-arm when the flow re-parks);
//!    [`WakeArming::Once`] covers one-shots; [`WakeArming::Latch`] (issue
//!    #1081) covers the reversible boolean-latch shape (wake on a
//!    transition, then go quiet until the opposite transition — a door that
//!    re-locks); the host may clear (remove the component) or replace a
//!    policy anytime, and [`FlowSleep::cancel`] resolves a policy to a
//!    permanent **false** (the flow is never woken by it again).
//! 5. The policy applies to **turn-boundary parks only**
//!    ([`StoryStatus::Done`]). Choice-blocked ([`WaitingForChoice`]) and
//!    external-blocked flows keep their own resume paths (`choose`,
//!    `resolve_external`); an `-> END` flow ([`Ended`]) is dead and the policy
//!    is inert (the component is dropped).
//! 6. A flow spawned with [`FlowSleep::dormant`] is **dormant**: parked at
//!    entry, its first turn runs on the first condition-true.
//!
//! ## The Detect phase (`#913`)
//!
//! Point 2's "only when a dependency moved" is where BH-1's `detect` bits are
//! consumed. A capability's per-container `detect` bit
//! ([`ContainerAccess::detect`](crate::ContainerAccess), **AND-merged** across
//! the container's reads — `#913`) says whether reads of it are backed by
//! bevy's own change ticks (`true`) or must be polled (`false`). A
//! [`FlowSleep`]'s [`DetectSummary`] folds the condition's dependency bits into
//! a single verdict:
//!
//! - **no external-capability dependency** ([`DetectSummary::bits`] empty,
//!   vacuously [`DetectSummary::all_detect_capable`] `true`): the condition
//!   reads only ink World state, so it is re-evaluated only when the shared
//!   World actually changed — the cheap path. Since issue #1146 "changed"
//!   here is **row-directed**: a batch turn's Apply records *which* cells it
//!   wrote ([`BrinkWorldDelta`]), and a policy is re-evaluated only when that
//!   changeset intersects the reads its condition's effect row declares. A
//!   turn's bookkeeping writes (visit counts, turn index) are therefore inert
//!   for a condition that reads a global, which is what stopped #1101's
//!   spurious re-wake. Where the ledger cannot account for the whole window
//!   (a serial-mode driver, a host writing [`BrinkGlobals::inner`] directly)
//!   or the condition's row is missing/opaque, this degrades to plain bevy
//!   change detection on [`BrinkGlobals`] — the pre-#1146 behavior.
//! - **any must-poll dependency** (`#913` AND-merge folded a bit to `false`,
//!   so `all_detect_capable` is `false`): re-evaluated every wake pass. That
//!   capability's reads are not change-detection-backed, so there is no cheap
//!   signal to gate on.
//! - **every dependency change-detection-capable** (`bits` non-empty, `#913`
//!   verdict all-`true`): the §12.5 cheap path (#996). Each dependency
//!   capability's concrete component is tracked by a
//!   [`detect_capability_changes`](crate::capability::detect_capability_changes)
//!   system (wired per component by `register_capability`), which records —
//!   through bevy's own `Changed<C>` window — whether that component moved
//!   this frame. [`mark_wake_dirty`] re-evaluates such a condition only when
//!   the shared World changed **or** one of its watched components' change
//!   ticks advanced — not every frame. A capability the wake layer cannot
//!   observe (unregistered, or with no verdict recorded yet) folds to a
//!   conservative must-poll: it cannot prove the component is unchanged, and a
//!   missed wake is the engine-race bug class.
//!
//! Re-evaluation is **always sound** regardless of the verdict: the detect bits
//! only tune the *cadence*. `#913`'s AND-merge must land before this cheap
//! path — a last-write-wins `true` on a capability that is really must-poll
//! would gate re-evaluation on a component-tick signal that its non-detectable
//! read never fires, reintroducing the missed wake §12.5 is careful to avoid.
//!
//! ## Systems (auto-registered by [`BrinkPlugin`](crate::BrinkPlugin))
//!
//! - [`mark_wake_dirty`] (ordinary system): consults each parked policy's
//!   [`DetectSummary`], [`BrinkGlobals`] change detection, and the
//!   per-capability component-tick verdict (§12.5, #996) and flags which parked
//!   flows need a re-evaluation this frame.
//! - [`run_flow_sleep`] (exclusive system, gated on
//!   `any_with_component::<FlowSleep<M>>`): re-evaluates the flagged conditions
//!   in each flow's own context, wakes on true, and re-arms/removes policies at
//!   turn boundaries. Order-independent w.r.t. [`advance_batch`]: waking takes
//!   effect the following frame either way. Before admitting a flagged policy
//!   for evaluation it also runs the attach-time purity gate
//!   ([`check_named_condition_purity`], issue #995, §13.1 point 2): a
//!   condition whose effect row shows writes — including writes performed
//!   transitively through a host-registered `EXTERNAL` binding the
//!   [`CapabilityManifest`] declares `writes` for (issue #1040, the #995
//!   follow-up; `docs/effects-spec.md` §9/§13), or through a
//!   [`bind_brink_command`](crate::bindings::BrinkBindingsAppExt::bind_brink_command)-bound
//!   `EXTERNAL` regardless of manifest presence (issue #1609, an #1096
//!   follow-up) — is rejected loudly ([`WakeConditionPurityError`]) and never
//!   called, not even once. A dynamically-resolved fn-value condition
//!   ([`FlowSleep::with_condition_value`], issue #1078) runs the same gate
//!   via [`check_value_condition_purity`] instead.
//!
//! [`WaitingForChoice`]: brink_runtime::StoryStatus::WaitingForChoice
//! [`Ended`]: brink_runtime::StoryStatus::Ended
//! [`StoryStatus::Done`]: brink_runtime::StoryStatus::Done
//! [`advance_batch`]: crate::advance_batch

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use bevy_asset::Assets;
use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::QueryState;
use bevy_ecs::reflect::ReflectComponent;
use bevy_ecs::system::{Local, Query, Res, ResMut};
use bevy_ecs::world::World as EcsWorld;
use bevy_log::warn;
use bevy_reflect::Reflect;
use brink_format::{DefinitionId, DirectEffects, EffectRowEntry, Value};
use brink_runtime::{Program, StoryStatus};
use thiserror::Error;

use crate::asset::{BrinkProgram, ProgramAsset};
use crate::bindings::{BrinkBindings, call_ink_function, call_ink_function_value};
use crate::capability::{
    CapabilityChanges, CapabilityManifest, CapabilityRegistry, ContainerAccess,
};
use crate::flow::BrinkFlow;
use crate::globals::BrinkGlobals;
use crate::wake_delta::{BrinkWorldDelta, WorldDelta};

/// When a woken flow re-parks, does its policy re-arm or retire?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum WakeArming {
    /// Re-arm every time the flow re-parks at a turn boundary — a standing
    /// subscription. The default (`docs/effects-spec.md` §13.1 point 4).
    #[default]
    Persistent,
    /// Fire exactly once: after the first wake runs its turn, the policy is
    /// removed and the flow reverts to ordinary per-turn advancement.
    Once,
    /// A reversible boolean latch (issue #1081, `docs/effects-spec.md` §13.1's
    /// wake contract conventions): wakes on a rising edge (the condition
    /// transitioning to the value this policy is currently watching for),
    /// then re-arms watching for the **opposite** value — so the next wake
    /// only fires on the falling edge, and so on indefinitely. Never retires.
    ///
    /// This expresses "wake on a transition, then go quiet until the
    /// opposite transition" — the natural shape for a boolean-latch reactive
    /// entity (a door: wake+open on switch-on, wake+re-lock on switch-off)
    /// — **without** requiring the condition itself to track any state: the
    /// condition stays an ordinary level predicate (e.g. "is the switch
    /// on?"), and this policy does the edge detection by comparing each
    /// reading against [`FlowSleep::latch_waiting_for`] rather than against
    /// a fixed `true`.
    Latch,
}

/// The lifecycle state of a [`FlowSleep`] policy — inspector-visible, and the
/// single field [`FlowSleep::wants_collect`] reads to tell Collect whether the
/// flow steps this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum SleepState {
    /// Parked: the flow is asleep under this policy and is **skipped by
    /// Collect**. The default for a freshly attached policy.
    Parked,
    /// Woken: the condition evaluated true; Collect steps the flow this turn.
    /// On reaching its next turn boundary the policy re-arms
    /// ([`WakeArming::Persistent`], [`WakeArming::Latch`]) or is removed
    /// ([`WakeArming::Once`]).
    Woken,
    /// Cancelled by the host ([`FlowSleep::cancel`]): the condition is
    /// permanently **false**. The flow stays parked and is never re-evaluated
    /// or woken by this policy again (the host must remove or replace it).
    Cancelled,
    /// The condition evaluation faulted (a runtime error, a missing function,
    /// an async external on the exclusive path). Logged once; the flow stays
    /// parked and is not re-evaluated — never silently retried into a spin,
    /// never mistaken for a false condition.
    Faulted,
}

/// The distilled `detect`-bit verdict for a policy's condition dependency set
/// (`#913`, ruled 2026-07-18). Built from the per-container AND-merged
/// [`ContainerAccess::detect`](crate::ContainerAccess) map, or supplied
/// directly by a host that knows its condition's dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub struct DetectSummary {
    /// The per-capability merged bits the summary was built from (kept for
    /// inspector visibility / debugging). Empty means the condition has no
    /// external-capability dependencies (it reads only ink World state).
    pub bits: BTreeMap<String, bool>,
    /// `true` iff **every** dependency capability is change-detection-backed
    /// (the AND of all `bits`, vacuously `true` when `bits` is empty). Drives
    /// the re-evaluation cadence: `true` → re-evaluate only on a World change;
    /// `false` → poll every wake pass.
    pub all_detect_capable: bool,
}

impl Default for DetectSummary {
    /// The no-external-dependency case: an empty `bits` map is vacuously
    /// all-detect-capable (see [`DetectSummary::from_bits`]) — a condition
    /// that reads only ink World state is always change-detection-backed via
    /// `BrinkGlobals`. A derived `Default` would instead default
    /// `all_detect_capable` to `false`, silently forcing every policy built
    /// without an explicit [`FlowSleep::with_detect`] onto the must-poll path
    /// (the exact defect this hand-written impl exists to prevent).
    fn default() -> Self {
        Self::from_bits(BTreeMap::new())
    }
}

impl DetectSummary {
    /// Build a summary from a per-capability `detect` bit map — typically the
    /// AND-merged [`ContainerAccess::detect`](crate::ContainerAccess) of the
    /// condition's container.
    #[must_use]
    pub fn from_bits(bits: BTreeMap<String, bool>) -> Self {
        let all_detect_capable = bits.values().all(|&b| b);
        Self {
            bits,
            all_detect_capable,
        }
    }

    /// Build a summary from a joined [`ContainerAccess`] — reads its
    /// AND-merged `detect` map directly. Use when the wake condition's
    /// dependency container has an entry in a loaded
    /// [`CapabilityTable`](crate::CapabilityTable).
    #[must_use]
    pub fn from_container_access(access: &ContainerAccess) -> Self {
        Self::from_bits(access.detect.clone())
    }
}

/// A standing reactive-wake policy on a flow entity (`docs/effects-spec.md`
/// §13.1). Attach it to a fulfilled flow entity; the plugin's wake systems do
/// the rest. See the module docs for the full contract.
///
/// Construct via [`FlowSleep::persistent`] / [`FlowSleep::once`], optionally
/// chaining [`with_args`](Self::with_args), [`with_detect`](Self::with_detect),
/// and [`dormant`](Self::dormant).
#[derive(Component, Reflect)]
#[reflect(Component)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent lifecycle/cadence flag with its \
              own doc comment, not a state machine in disguise (`dormant`, \
              `needs_eval`, and `evaluated_once` are orthogonal cadence \
              signals; `waiting_for` (issue #1081) is Latch-only edge state) \
              — see the ChoiceFlags precedent in brink-format::opcode"
)]
pub struct FlowSleep<M: Send + Sync + 'static = ()> {
    /// The ink function name whose (pure) return value is the wake condition.
    /// A diagnostic label only when [`condition_value`](Self::condition_value)
    /// is `Some` — see [`with_condition_value`](Self::with_condition_value).
    condition: String,
    /// A dynamically-resolved fn-value token (`Value::FnRef`/`Closure`) that
    /// overrides `condition`'s by-name resolution — see
    /// [`with_condition_value`](Self::with_condition_value).
    #[reflect(ignore)]
    condition_value: Option<Value>,
    /// Arguments passed to the condition function, in declaration order.
    #[reflect(ignore)]
    args: Vec<Value>,
    /// The `detect`-bit verdict for the condition's dependencies (`#913`).
    detect: DetectSummary,
    /// Re-arm vs one-shot.
    arming: WakeArming,
    /// Current lifecycle state.
    state: SleepState,
    /// `true` until the flow has run at least once under this policy: a
    /// dormant-spawned flow is eligible for its first wake evaluation even
    /// though it has never reached a turn boundary.
    dormant: bool,
    /// Set by [`mark_wake_dirty`] when a dependency may have moved; consumed
    /// (and cleared) by [`run_flow_sleep`] when it re-evaluates the condition.
    needs_eval: bool,
    /// Whether the condition has been evaluated at least once — so a dormant,
    /// all-detect-capable policy still gets its initial evaluation even on a
    /// frame the World didn't change.
    evaluated_once: bool,
    /// [`WakeArming::Latch`] only: the boolean value the condition must next
    /// equal to fire (flipped every time it does). Starts `true` — a fresh
    /// latch watches for the condition to become true first. Inert (never
    /// read or flipped) for `Persistent`/`Once`.
    waiting_for: bool,
    /// Issue #1146: does this condition read story **bookkeeping** (visit
    /// counts, turn counts, the turn index, RNG state)? Host-declared via
    /// [`reads_bookkeeping`](Self::reads_bookkeeping) — effect rows model
    /// only global cells, so the row cannot answer this. Only consulted on
    /// the row-directed cheap path; a condition whose row is unknown or
    /// opaque re-evaluates on any change regardless.
    reads_bookkeeping: bool,
    #[reflect(ignore)]
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> FlowSleep<M> {
    /// A **persistent** policy: `condition` is an ink function name returning a
    /// truthy value when the flow should wake. Re-arms every time the flow
    /// re-parks (§13.1 point 4).
    #[must_use]
    pub fn persistent(condition: impl Into<String>) -> Self {
        Self::new(condition.into(), WakeArming::Persistent)
    }

    /// A **one-shot** policy: fires once on the first condition-true, then the
    /// component is removed and the flow reverts to ordinary advancement.
    #[must_use]
    pub fn once(condition: impl Into<String>) -> Self {
        Self::new(condition.into(), WakeArming::Once)
    }

    /// A **reversible latch** policy (issue #1081): wakes on a transition to
    /// `true`, then re-arms watching for the transition back to `false`, and
    /// so on indefinitely. See [`WakeArming::Latch`] for the full contract.
    #[must_use]
    pub fn latch(condition: impl Into<String>) -> Self {
        Self::new(condition.into(), WakeArming::Latch)
    }

    fn new(condition: String, arming: WakeArming) -> Self {
        Self {
            condition,
            condition_value: None,
            args: Vec::new(),
            detect: DetectSummary::default(),
            arming,
            // A non-dormant policy starts **eligible to run** (`Woken`): a flow
            // parks at its natural yield points (§13.1 point 1), so it must be
            // allowed to run to its first turn boundary before the policy
            // engages. `run_flow_sleep` re-parks it (→ `Parked`) once it
            // reaches that boundary, and the condition governs waking from
            // then on. `dormant()` overrides this to `Parked` (parked at entry).
            state: SleepState::Woken,
            dormant: false,
            needs_eval: false,
            evaluated_once: false,
            waiting_for: true,
            reads_bookkeeping: false,
            _marker: PhantomData,
        }
    }

    /// Pass arguments to the condition function (declaration order). Builder.
    #[must_use]
    pub fn with_args(mut self, args: Vec<Value>) -> Self {
        self.args = args;
        self
    }

    /// Attach a dynamically-resolved fn-value token (`Value::FnRef`/
    /// `Closure`) as the wake condition — for a host that obtained the
    /// condition dynamically (a global's current value, a returned callback
    /// token, a `bind_brink_query` result) rather than naming it statically
    /// via [`persistent`](Self::persistent)/[`once`](Self::once).
    ///
    /// When set, this takes over **both** halves of condition resolution:
    /// the attach-time purity gate checks `value`'s target row
    /// ([`check_value_condition_purity`] instead of
    /// [`check_named_condition_purity`]), and evaluation invokes the token
    /// directly (`call_ink_function_value` instead of resolving `condition`
    /// by path). `condition`'s string (from [`persistent`](Self::persistent)/
    /// [`once`](Self::once)) remains only a diagnostic label at that point —
    /// it is never resolved by path while a `condition_value` is set. Builder.
    #[must_use]
    pub fn with_condition_value(mut self, value: Value) -> Self {
        self.condition_value = Some(value);
        self
    }

    /// Attach the condition's dependency [`DetectSummary`] (`#913`), tuning the
    /// re-evaluation cadence. Without this the summary is empty — treated as
    /// all-detect-capable (re-evaluate only on a World change), the right
    /// default for a condition reading only ink World state. A summary whose
    /// [`bits`](DetectSummary::bits) names external (component) capabilities
    /// that are **all** change-detection-capable (`#913` AND-merge verdict
    /// all-`true`) gets the §12.5 cheap path (#996): re-evaluated only when one
    /// of those components changed — provided each is registered via
    /// `register_capability` (an unregistered capability the wake layer cannot
    /// observe must-polls conservatively). A summary with any must-poll bit
    /// re-evaluates every pass. Builder.
    #[must_use]
    pub fn with_detect(mut self, detect: DetectSummary) -> Self {
        self.detect = detect;
        self
    }

    /// Declare that this condition reads story **bookkeeping** — a visit
    /// count (`{knot}`, `TURNS_SINCE(-> knot)`), a turn count, the turn index
    /// (`TURNS()`, `CHOICE_COUNT()`), or RNG state (issue #1146). Builder.
    ///
    /// The row-directed wake-dirtying path re-evaluates a condition only when
    /// a cell it reads was actually written, and a condition's read set comes
    /// from its **effect row** — which models global cells *only*
    /// (`brink_format::DirectEffects::reads`). Bookkeeping reads are invisible
    /// to it, so a condition that depends on one must say so here, or it will
    /// sit parked through the turns that move it. Nothing else needs this: a
    /// condition reading ink globals is covered by its row automatically, and
    /// a condition whose row is missing/opaque already re-evaluates on any
    /// change.
    ///
    /// Graduating a `reads`-bookkeeping row dimension (so this is inferred
    /// rather than declared) is tracked as the follow-up to #1146.
    ///
    /// **Known tradeoff, not a bug:** once a condition declaring this is
    /// evaluated even once, it stays perpetually flagged for re-evaluation
    /// thereafter, even if nothing it actually depends on ever changes again.
    /// Every Evaluate pass notes an unconditional bookkeeping touch in the
    /// changed-cell ledger (the unavoidable `&mut BrinkGlobals` residue
    /// building a condition's context takes), and that residue is itself
    /// indistinguishable, to the row-directed path, from a real bookkeeping
    /// write — so a `reads_bookkeeping()` reader's own prior evaluation
    /// re-triggers the next one. This is deliberately on the over-report side
    /// of the ledger's "never under-report" law (module docs,
    /// `crate::wake_delta`): the cost is a self-sustaining re-evaluation,
    /// never a missed wake.
    #[must_use]
    pub fn reads_bookkeeping(mut self) -> Self {
        self.reads_bookkeeping = true;
        self
    }

    /// Mark this policy **dormant** (`docs/effects-spec.md` §13.1 point 6): the
    /// flow is parked at entry and its first turn runs on the first
    /// condition-true. Attach to a freshly fulfilled flow before it has stepped.
    /// Builder.
    #[must_use]
    pub fn dormant(mut self) -> Self {
        self.dormant = true;
        // Dormant means parked at entry — override the non-dormant "run the
        // first turn" default so Collect skips it until the first condition-true.
        self.state = SleepState::Parked;
        self
    }

    /// Cancel the policy: its condition is henceforth a permanent **false**
    /// (§13.1 — "cancellation → false"). The flow stays parked and is never
    /// re-evaluated or woken by this policy again. To fully detach, remove the
    /// component; to change the wake condition, replace it with a new one.
    pub fn cancel(&mut self) {
        self.state = SleepState::Cancelled;
        self.needs_eval = false;
    }

    /// The ink function name of the wake condition — a diagnostic label only
    /// when [`condition_value`](Self::condition_value) is `Some`.
    #[must_use]
    pub fn condition(&self) -> &str {
        &self.condition
    }

    /// The dynamically-resolved fn-value token, if
    /// [`with_condition_value`](Self::with_condition_value) set one.
    #[must_use]
    pub fn condition_value(&self) -> Option<&Value> {
        self.condition_value.as_ref()
    }

    /// The current [`SleepState`].
    #[must_use]
    pub fn state(&self) -> SleepState {
        self.state
    }

    /// The re-arm policy.
    #[must_use]
    pub fn arming(&self) -> WakeArming {
        self.arming
    }

    /// [`WakeArming::Latch`] only: the boolean value the condition must next
    /// equal to fire. This doubles as an outside observer's read of which
    /// side of the latch the policy currently sits on — e.g. for a door
    /// whose condition is "is the switch on?": `true` means the policy is
    /// waiting for the switch to turn on (the door is currently locked);
    /// `false` means it is waiting for the switch to turn off (the door is
    /// currently open). Always `true` (and unused) for `Persistent`/`Once`.
    #[must_use]
    pub fn latch_waiting_for(&self) -> bool {
        self.waiting_for
    }

    /// Whether every dependency capability is change-detection-backed (`#913`
    /// AND-merge verdict). `false` means the condition polls every pass. `true`
    /// enables the cheap path: for an empty [`bits`](DetectSummary::bits) map
    /// (no external dependency) that is re-evaluate-on-`BrinkGlobals`-change;
    /// for a non-empty one it is re-evaluate-on-watched-component-change (§12.5,
    /// #996), provided each named capability is registered via
    /// `register_capability` so [`mark_wake_dirty`] can observe its ticks — an
    /// unregistered capability the wake layer cannot observe still must-polls.
    #[must_use]
    pub fn dependencies_all_detect_capable(&self) -> bool {
        self.detect.all_detect_capable
    }

    /// The dependency [`DetectSummary`] this policy was built with.
    #[must_use]
    pub fn detect_summary(&self) -> &DetectSummary {
        &self.detect
    }

    /// Whether the host declared this condition a reader of story
    /// bookkeeping — see [`reads_bookkeeping`](Self::reads_bookkeeping).
    #[must_use]
    pub fn declares_bookkeeping_reads(&self) -> bool {
        self.reads_bookkeeping
    }

    /// Whether Collect should step this flow this turn — the predicate
    /// [`advance_batch`](crate::advance_batch)'s Collect phase applies. Only a
    /// [`Woken`](SleepState::Woken) policy admits the flow; a parked, cancelled,
    /// or faulted policy costs zero (skipped).
    #[must_use]
    pub fn wants_collect(&self) -> bool {
        matches!(self.state, SleepState::Woken)
    }
}

// ── Wake-condition purity (issue #995, BH-4 follow-up) ──────────────────────
//
// `docs/effects-spec.md` §13.1 point 2 requires a `FlowSleep` condition to be
// a **pure** fn: it is re-evaluated whenever a dependency moves, and the
// re-evaluate contract can never tolerate that re-evaluation observing (or
// causing) a mutation. Before this slice, nothing checked that — a condition
// naming a knot/function that writes a global would be called anyway, every
// wake pass. `check_named_condition_purity` (a `&str` condition, the shape
// every `FlowSleep` uses today) and `check_value_condition_purity` (a `Value`
// fn-value token — `FnRef`/`Closure` — for a host that resolves its condition
// dynamically) both resolve to a `DefinitionId` and inspect its `EffectRows`
// row (T2-3, `docs/effects-spec.md` §11): any global-cell write in the row's
// direct part, or in a dispatch's static fallback (v1 does no runtime
// narrowing — §7 — so a dispatch's conservative fallback always applies), or
// an opaque row (the §3 pessimal top: effects inference couldn't summarize a
// call it makes) makes the condition impure and rejects loudly.
//
// **Closed follow-up** (issue #1040, tracked from #995/#897): the check also
// walks every `EXTERNAL` call a row (or a dispatch's static fallback) makes
// and consults the [`CapabilityManifest`]'s declared `effects.writes` for it
// (`docs/effects-spec.md` §9's "the manifest IS the external's row", §13.2's
// grammar) — a manifest entry declaring one or more `writes` capabilities
// rejects the condition exactly like an ink-level global write does. Unlike
// the BH-1 access join (`compute_container_access`), this needs no
// `CapabilityRegistry`/`ComponentId` resolution: the yes/no purity verdict
// only needs the capability *names* a manifest entry lists, not what
// `ComponentId` they resolve to.
//
// A manifest entry with no `writes` (reads-only, or no `effects` key at all —
// §13.2's opt-in default) is accepted: it does not touch a write-capable ECS
// capability, and purity is exactly "no writes". An `EXTERNAL` name with **no
// manifest entry at all** is likewise accepted (unless it is a
// `bind_brink_command` binding — see the #1609 paragraph below), deliberately
// matching the BH-1 access join's posture (`resolve_call_atom`,
// `crate::capability`): "a call whose `NameId` doesn't resolve, or that has
// no manifest entry at all, contributes no access — silently, since not
// every `EXTERNAL` touches ECS state (§13.2's `effects` key is opt-in)".
// Rejecting an unregistered external here would fault the flow permanently —
// the same missed-wake bug class the #913 detect-merge ruling treats as the
// worse failure mode (`docs/decision-log.md` 2026-07-18, "a missed wake is
// the engine-race bug class") — for a binding (e.g. a `bind_brink_fn`
// helper) that legitimately never touches ECS state and was accepted before
// this check existed. Only a manifest entry that affirmatively declares
// `writes` is rejected; the manifest is an honesty contract, not a security
// boundary (the host is the TCB — `docs/effects-spec.md` §9).
//
// A story whose `EffectRows` table is empty entirely (a converter-built
// program, or a program that never went through the compiler's effects
// emission) is outside the guarantee this checks: "compiler rows guarantee
// purity only for ink-authored conditions reaching codegen" (issue #995) — so
// an empty table skips the check rather than rejecting every condition a
// story like that could ever declare.
//
// `run_flow_sleep` calls this at the moment a parked policy is first admitted
// for evaluation (dormant policies: immediately; persistent ones: their first
// park) — before the condition is ever called, so an impure condition is
// never evaluated even once. Faulted the same way a runtime eval error is
// (never silently retried into a spin), but with its own distinct, named
// error so the two classes of failure are never confused in a log.
//
// **Closed follow-up** (issue #1078, tracked from #1062): `check_row_purity`
// (via `check_value_condition_purity`) has always covered a dynamically-
// resolved fn-value token (`Value::FnRef`/`Closure`), but nothing in
// `FlowSleep`/`run_flow_sleep` could ever produce one to check — the named
// path (`FlowSleep::persistent`/`once`) was the only wake-condition shape
// `run_flow_sleep` resolved. `FlowSleep::with_condition_value` adds the
// missing shape: a host that obtains its condition dynamically (a global's
// current value, a returned callback token, a `bind_brink_query` result)
// attaches the token directly, and `run_flow_sleep`'s gather/evaluate phases
// branch on it exactly like the named path — `check_value_condition_purity`
// gates admission, `call_ink_function_value` (`crate::bindings`) evaluates.
//
// **Closed** (issue #1609, a #1096 follow-up): a `bind_brink_command`-bound
// `EXTERNAL` with no [`CapabilityManifest`] entry used to pass
// `check_external_calls_purity` above — "no manifest entry at all accepts"
// is deliberate (a `bind_brink_fn` helper that never touches ECS state
// shouldn't need one), but it meant a wake condition naming a
// `call_ink_function`/`call_ink_function_value` path that reaches a
// `bind_brink_command` binding was accepted as pure even though, since
// #1096's fix, that path fires a real Bevy event on every re-evaluation
// pass — where before #1096 it was inert (silently ran the in-story
// fallback instead). `check_named_condition_purity`/
// `check_value_condition_purity` (and the `check_row_purity`/
// `check_external_calls_purity` helpers they share) now take an optional
// [`BrinkBindings<M>`] reference and reject any call whose target name is
// present in [`BrinkBindings::is_command`] — bevy-brink has that
// binding-kind information locally, so this needs no manifest entry at all
// to answer, unlike the #1040 manifest-`writes` check above. `run_flow_sleep`
// fetches the app's `BrinkBindings<M>` resource (absent entirely if the host
// never registered any binding) alongside the `CapabilityManifest` and
// threads it through. This is scoped to `bind_brink_command` only: a pure
// (`bind_brink_fn`) binding is not rejected by this check. A world-query
// (`bind_brink_query`) binding is *also* not rejected here, but not because
// its World access is out of reach — `call_ink_function`/
// `call_ink_function_value` (`crate::bindings`, the same drivers
// `run_flow_sleep` uses to evaluate a condition) resolve a query binding
// **inline**, synchronously, mid-evaluation (`docs/bevy-brink.md`'s binding
// table: "flow pauses (`Pending`); a driver runs it via `run_system_with`
// between suspensions, then resumes" — the pause/resume is internal to the
// single call, not a cross-frame park a wake condition's re-evaluation could
// dodge). Widening this gate to cover write-capable query bindings is a
// design question left open (whether/how to distinguish a read-only query
// system from a writing one), not something this fix's scope covers.
//
// `resolve_brink_calls` (`crate::call`, the deferred
// `commands.brink_call(...)` path) also drives `call_ink_function` under the
// hood and so can also fire a command event — but it is an explicit,
// engine-initiated call, not a `FlowSleep` re-evaluation, so it is outside
// this gate's contract (`docs/effects-spec.md` §13.1 point 2 only binds
// wake-condition purity) and unaffected by this fix.

/// A wake condition failed the attach-time purity check. See the module
/// section above for the contract this enforces.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WakeConditionPurityError {
    /// The condition name/path didn't resolve to any definition in this
    /// story.
    #[error(
        "wake condition `{condition}` does not resolve to a known definition in this story \
         (check the name/path is correct for the loaded story)"
    )]
    UnknownCondition {
        /// The condition name/path (or a `divert_target_path` label for a
        /// value-resolved condition) that failed to resolve.
        condition: String,
    },
    /// The condition value wasn't a function value (`FnRef`/`Closure`) at
    /// all — [`check_value_condition_purity`] only has a target definition to
    /// inspect for an actual fn-value token.
    #[error(
        "wake condition value is not a function value (FnRef/Closure) — no target definition to \
         check for purity"
    )]
    NotAFunctionValue,
    /// The condition resolved to a definition, but this story's (non-empty)
    /// `EffectRows` table has no row for it — an internal invariant
    /// violation (every knot/stitch ships one once the table is populated at
    /// all), never a panic: conservatively treated as impure.
    #[error(
        "wake condition `{condition}` resolved to a definition with no EffectRows entry — an \
         internal invariant expects one once a story's EffectRows table is populated at all; \
         treating conservatively as impure"
    )]
    MissingEffectRow {
        /// The condition name/path/label this row lookup failed for.
        condition: String,
    },
    /// The condition (or a dispatch fallback its row folds in) writes one or
    /// more global cells.
    #[error(
        "wake condition `{condition}` is not pure: it writes global(s) {writes:?} — a FlowSleep \
         condition is re-evaluated whenever a dependency moves (docs/effects-spec.md §13.1 point \
         2), and a writing condition would let that re-evaluation observe or cause a mutation"
    )]
    Writes {
        /// The condition name/path/label.
        condition: String,
        /// The written globals' names, sorted and deduplicated.
        writes: Vec<String>,
    },
    /// The condition's row (or a dispatch fallback) is opaque — effects
    /// inference couldn't summarize a call it makes (§3's pessimal top).
    /// Purity can't be proven, so it's conservatively rejected.
    #[error(
        "wake condition `{condition}`'s effect row is opaque (a call it makes couldn't be \
         summarized by effects inference) — purity can't be proven, so it is conservatively \
         rejected"
    )]
    Opaque {
        /// The condition name/path/label.
        condition: String,
    },
    /// The condition (or a dispatch fallback its row folds in) calls a
    /// host-registered `EXTERNAL` binding whose [`CapabilityManifest`] entry
    /// declares one or more `writes` capabilities (issue #1040, the #995
    /// follow-up; `docs/effects-spec.md` §9/§13).
    #[error(
        "wake condition `{condition}` is not pure: it calls EXTERNAL `{external}`, whose \
         capability manifest declares writes {writes:?} — a FlowSleep condition is \
         re-evaluated whenever a dependency moves (docs/effects-spec.md §13.1 point 2), and a \
         writing binding would let that re-evaluation observe or cause a mutation \
         (docs/effects-spec.md §9/§13, issue #1040)"
    )]
    ExternalWrites {
        /// The condition name/path/label.
        condition: String,
        /// The `EXTERNAL` binding's name.
        external: String,
        /// The written capability names the manifest declares, sorted and
        /// deduplicated.
        writes: Vec<String>,
    },
    /// The condition (or a dispatch fallback its row folds in) calls a
    /// [`bind_brink_command`](crate::bindings::BrinkBindingsAppExt::bind_brink_command)-bound
    /// `EXTERNAL` (issue #1609, an #1096 follow-up). Rejected regardless of
    /// [`CapabilityManifest`] presence: `bevy-brink` knows the binding kind
    /// locally, and a command binding mutates the World when its parsed
    /// event is triggered, so a wake condition reaching it would let
    /// re-evaluation fire that mutation repeatedly (§13.1 point 2).
    #[error(
        "wake condition `{condition}` is not pure: it calls `{external}`, a bind_brink_command \
         binding — a command binding mutates the World when triggered, and a FlowSleep \
         condition is re-evaluated whenever a dependency moves (docs/effects-spec.md §13.1 \
         point 2), so a command-bound wake condition is rejected regardless of \
         CapabilityManifest presence (issue #1609)"
    )]
    CommandBinding {
        /// The condition name/path/label.
        condition: String,
        /// The command-bound external's name.
        external: String,
    },
}

/// Find the `EffectRows` entry for `def`, if this story's table carries one.
fn effect_row_for(effect_rows: &[EffectRowEntry], def: DefinitionId) -> Option<&EffectRowEntry> {
    effect_rows.iter().find(|row| row.def == def)
}

/// Resolve global-cell `DefinitionId`s to their variable names for a
/// human-readable error — falling back to the id's own debug form rather than
/// panicking if the name table doesn't carry it (shouldn't happen for a
/// well-formed row, but this is a diagnostic path, not a hot one).
fn write_names(program: &Program, ids: &[DefinitionId]) -> Vec<String> {
    ids.iter()
        .map(|id| {
            program
                .global_var_name(*id)
                .map_or_else(|| format!("<{id}>"), str::to_owned)
        })
        .collect()
}

/// The shared purity check once a condition token has resolved to a `row`
/// (see the module section above for exactly what this inspects).
///
/// `bindings` is `None` when the host never registered any
/// [`BrinkBindings<M>`] (no `bind_brink_*` call at all — the resource is
/// inserted lazily) — in that case there is no `commands` bucket to consult,
/// so the #1609 command-binding check below is trivially skipped, same as an
/// empty registry would be.
fn check_row_purity<M: Send + Sync + 'static>(
    program: &Program,
    row: &EffectRowEntry,
    manifest: &CapabilityManifest,
    bindings: Option<&BrinkBindings<M>>,
    condition_label: &str,
) -> Result<(), WakeConditionPurityError> {
    if row.direct.opaque
        || row
            .dispatches
            .iter()
            .any(|dispatch| dispatch.fallback.opaque)
    {
        return Err(WakeConditionPurityError::Opaque {
            condition: condition_label.to_owned(),
        });
    }

    check_external_calls_purity(program, &row.direct, manifest, bindings, condition_label)?;
    for dispatch in &row.dispatches {
        check_external_calls_purity(
            program,
            &dispatch.fallback,
            manifest,
            bindings,
            condition_label,
        )?;
    }

    let mut writes = write_names(program, &row.direct.writes);
    for dispatch in &row.dispatches {
        writes.extend(write_names(program, &dispatch.fallback.writes));
    }
    if writes.is_empty() {
        Ok(())
    } else {
        writes.sort();
        writes.dedup();
        Err(WakeConditionPurityError::Writes {
            condition: condition_label.to_owned(),
            writes,
        })
    }
}

/// Issue #1040 (the #995 follow-up) + issue #1609: check every `EXTERNAL`
/// call atom in `direct` (a row's direct part, or a dispatch's static
/// fallback) against, in order: `bindings`'s `commands` registry (#1609 — a
/// `bind_brink_command`-bound name rejects **unconditionally**, no manifest
/// entry needed), then `manifest`'s declared `effects.writes` (#1040). See
/// the module section above for the full contract: a command-bound name, or
/// a manifest entry declaring `writes`, rejects; a reads-only entry, a
/// no-`effects`-key entry, or no manifest entry at all for a non-command
/// name (the same opt-in posture `crate::capability::resolve_call_atom`
/// applies) all accept.
fn check_external_calls_purity<M: Send + Sync + 'static>(
    program: &Program,
    direct: &DirectEffects,
    manifest: &CapabilityManifest,
    bindings: Option<&BrinkBindings<M>>,
    condition_label: &str,
) -> Result<(), WakeConditionPurityError> {
    for call in &direct.calls {
        let external_name = program
            .name_checked(call.name)
            .map_or_else(|| format!("<{:?}>", call.name), str::to_owned);
        // #1609: a command-bound name rejects unconditionally — bevy-brink
        // knows this binding kind locally, so no manifest entry is needed
        // (or consulted) to reject it.
        if bindings.is_some_and(|b| b.is_command(&external_name)) {
            return Err(WakeConditionPurityError::CommandBinding {
                condition: condition_label.to_owned(),
                external: external_name,
            });
        }
        // No manifest entry at all accepts, same as a reads-only/no-`effects`
        // entry — see the doc comment above.
        if let Some(external) = manifest.external(&external_name)
            && !external.effects.writes.is_empty()
        {
            let mut writes = external.effects.writes.clone();
            writes.sort();
            writes.dedup();
            return Err(WakeConditionPurityError::ExternalWrites {
                condition: condition_label.to_owned(),
                external: external_name,
                writes,
            });
        }
    }
    Ok(())
}

/// Check purity for a **named** wake condition (`FlowSleep::condition`'s
/// shape) — resolves `condition` to a `DefinitionId` via
/// [`Program::definition_id_for_path`], then inspects its `EffectRows` row.
///
/// `Ok(())` when `effect_rows` is empty entirely: a story that never shipped
/// an `EffectRows` table (converter-built, or otherwise never ran the
/// compiler's effects emission) is outside the guarantee this checks — see
/// the module section above.
///
/// `manifest` threads the host's [`CapabilityManifest`] (issue #1040) so a
/// condition calling a host-registered `EXTERNAL` binding is checked against
/// that binding's declared `effects.writes`, not just the row's own
/// ink-level writes. `bindings` threads the host's [`BrinkBindings<M>`]
/// registry (issue #1609) so a condition calling a `bind_brink_command`
/// binding is rejected outright, regardless of manifest presence — pass
/// `None` if the host never registered any binding for marker `M`.
///
/// # Errors
/// See [`WakeConditionPurityError`].
pub fn check_named_condition_purity<M: Send + Sync + 'static>(
    program: &Program,
    effect_rows: &[EffectRowEntry],
    manifest: &CapabilityManifest,
    bindings: Option<&BrinkBindings<M>>,
    condition: &str,
) -> Result<(), WakeConditionPurityError> {
    if effect_rows.is_empty() {
        return Ok(());
    }
    let def = program.definition_id_for_path(condition).ok_or_else(|| {
        WakeConditionPurityError::UnknownCondition {
            condition: condition.to_owned(),
        }
    })?;
    let row = effect_row_for(effect_rows, def).ok_or_else(|| {
        WakeConditionPurityError::MissingEffectRow {
            condition: condition.to_owned(),
        }
    })?;
    check_row_purity(program, row, manifest, bindings, condition)
}

/// Check purity for a **dynamic fn-value** wake condition — a `Value`
/// (`FnRef`/`Closure`) resolved token rather than a static name, e.g. one a
/// host obtained from a global or a `bind_brink_query` result. Resolves the
/// value's target via [`Value::fn_target`], then inspects the same
/// `EffectRows` row [`check_named_condition_purity`] does.
///
/// Same empty-`effect_rows` bypass as [`check_named_condition_purity`]. Same
/// `manifest`/`bindings` threading (issues #1040/#1609) as
/// [`check_named_condition_purity`].
///
/// # Errors
/// See [`WakeConditionPurityError`]. [`WakeConditionPurityError::NotAFunctionValue`]
/// if `value` isn't a function value at all.
pub fn check_value_condition_purity<M: Send + Sync + 'static>(
    program: &Program,
    effect_rows: &[EffectRowEntry],
    manifest: &CapabilityManifest,
    bindings: Option<&BrinkBindings<M>>,
    value: &Value,
) -> Result<(), WakeConditionPurityError> {
    if effect_rows.is_empty() {
        return Ok(());
    }
    let def = value
        .fn_target()
        .ok_or(WakeConditionPurityError::NotAFunctionValue)?;
    let label = program
        .divert_target_path(def)
        .unwrap_or_else(|| format!("<{def}>"));
    let row = effect_row_for(effect_rows, def).ok_or_else(|| {
        WakeConditionPurityError::MissingEffectRow {
            condition: label.clone(),
        }
    })?;
    check_row_purity(program, row, manifest, bindings, &label)
}

// ── Row-directed wake dirtying (issue #1146, the #1101 fix) ─────────────────
//
// A wake condition's **read row** is the dependency set the scheduler needs:
// re-evaluate a parked policy only when a cell the condition actually reads
// was written (`docs/effects-spec.md` §11's rows, consumed for scheduler
// precision). The changed-cell side is `crate::wake_delta`; this side turns a
// condition into the read set to intersect it with.

/// What a wake condition's effect row says it may read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConditionReads {
    /// The row could not be consulted, or does not bound the reads: no
    /// `ProgramAsset` loaded, a story with no `EffectRows` table at all
    /// (converter-built — the same bypass the purity gate takes), a condition
    /// that doesn't resolve, a missing row, an **opaque** row (§3's pessimal
    /// top — "touches every cell"), or a listed read whose global cell can't
    /// be resolved to a slot. Any change re-evaluates: over-report, never
    /// under-report.
    Unknown,
    /// The condition reads exactly these global slot indices (possibly none —
    /// a condition that reads no cell can only be moved by something the row
    /// does model as a dependency, e.g. a capability component).
    Globals(BTreeSet<u32>),
}

/// Resolve `sleep`'s condition to the global slots its effect row says it may
/// read. See [`ConditionReads::Unknown`] for every case that degrades to the
/// conservative "assume it reads everything" answer.
///
/// Global cells are returned as **slot indices** (not `DefinitionId`s) so the
/// caller can intersect directly against the [`WorldDelta`] a batch Apply
/// records, which is keyed by the same `World::set_global` numbering.
fn condition_reads<M: Send + Sync + 'static>(
    asset: Option<&ProgramAsset>,
    sleep: &FlowSleep<M>,
) -> ConditionReads {
    let Some(asset) = asset else {
        return ConditionReads::Unknown;
    };
    if asset.effect_rows.is_empty() {
        return ConditionReads::Unknown;
    }
    let def = if let Some(value) = &sleep.condition_value {
        value.fn_target()
    } else {
        asset.program.definition_id_for_path(&sleep.condition)
    };
    let Some(def) = def else {
        return ConditionReads::Unknown;
    };
    let Some(row) = effect_row_for(&asset.effect_rows, def) else {
        return ConditionReads::Unknown;
    };
    if row.direct.opaque
        || row
            .dispatches
            .iter()
            .any(|dispatch| dispatch.fallback.opaque)
    {
        return ConditionReads::Unknown;
    }

    let mut slots = BTreeSet::new();
    // v1 emits no dispatch entries (call-through-value folds into the direct
    // part), but a populated dispatch list round-trips — fold each static
    // fallback's reads in exactly as the purity gate folds its writes.
    let reads = row
        .direct
        .reads
        .iter()
        .chain(row.dispatches.iter().flat_map(|d| d.fallback.reads.iter()));
    for id in reads {
        // `DefinitionId` → slot index via the program's own global table. A
        // read the loaded program doesn't declare (a stale row, a `VAR` a
        // story patch removed) can't be proven unchanged, so it degrades the
        // whole row rather than being silently dropped.
        let Some(slot) = asset.program.global_slot(*id) else {
            return ConditionReads::Unknown;
        };
        slots.insert(slot);
    }
    ConditionReads::Globals(slots)
}

/// Does `delta` (a complete account of the shared world's changes this
/// window) touch anything `sleep`'s condition reads?
///
/// The bookkeeping bit is matched against the host's
/// [`FlowSleep::reads_bookkeeping`] declaration, not the row: effect rows
/// model global cells only, so a visit-count/turn-index read is invisible to
/// them (see that builder's docs). An [`ConditionReads::Unknown`] row skips
/// the question entirely and re-evaluates on any change at all.
fn delta_touches_condition<M: Send + Sync + 'static>(
    delta: &WorldDelta,
    reads: &ConditionReads,
    sleep: &FlowSleep<M>,
) -> bool {
    match reads {
        ConditionReads::Unknown => !delta.is_empty(),
        ConditionReads::Globals(slots) => {
            (delta.touched_bookkeeping() && sleep.reads_bookkeeping)
                || delta.globals().iter().any(|slot| slots.contains(slot))
        }
    }
}

/// Ink truthiness for a wake condition's return value: a `Bool(true)`, a
/// nonzero `Int`, or a nonzero `Float` wakes the flow. Every other value
/// (including `Null` and non-numeric types) is treated as **false** —
/// conservative: a malformed condition parks rather than spuriously wakes.
fn is_condition_true(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        _ => false,
    }
}

/// Decide whether a parked policy needs its condition re-evaluated this pass,
/// given the two change signals `mark_wake_dirty` can observe: the shared ink
/// World ([`BrinkGlobals`], `world_changed`) and — new in #996 — the
/// per-capability component-tick verdict [`CapabilityChanges`] (§12.5).
///
/// The cases, in order:
///
/// - **Never evaluated yet** (`!evaluated_once`): always re-evaluate. A
///   dormant policy must get its first evaluation even on a quiet frame.
/// - **No external-capability dependency** ([`DetectSummary::bits`] empty): the
///   condition reads only ink World state, so `world_changed` is the whole
///   signal — re-evaluate only when it is set. Since #1146 that flag is
///   already **row-directed** where the caller could prove it (see
///   [`mark_wake_dirty`]): it means "a cell this condition reads moved", not
///   merely "something in `BrinkGlobals` moved".
/// - **Any dependency capability is must-poll** (`#913` AND-merge folded a bit
///   to `false`, so `all_detect_capable` is `false`): re-evaluate every pass;
///   that capability's reads are not change-detection-backed.
/// - **Every dependency capability is change-detection-capable** (`#913`
///   verdict all-`true`, `bits` non-empty): the §12.5 cheap path. Re-evaluate
///   only if the shared World changed (the condition may also read ink globals)
///   **or** one of the watched components' change ticks advanced this pass
///   ([`CapabilityChanges`]). A capability the wake layer cannot observe —
///   unregistered (no [`CapabilityRegistry::type_id`]), or tracked but with no
///   verdict recorded yet ([`CapabilityChanges::changed`] returns `None`) —
///   folds to a conservative must-poll: it cannot prove the component is
///   unchanged, and a missed wake is the engine-race bug class (over-report,
///   never under-report — §3 soundness direction).
fn wake_needs_reeval<M: Send + Sync + 'static>(
    sleep: &FlowSleep<M>,
    registry: &CapabilityRegistry<M>,
    changes: &CapabilityChanges<M>,
    world_changed: bool,
) -> bool {
    if !sleep.evaluated_once {
        return true;
    }
    let detect = &sleep.detect;
    if detect.bits.is_empty() {
        // No external-capability dependency (`bits` empty is vacuously
        // all-detect-capable — see `DetectSummary::from_bits`): the shared ink
        // World is the only signal, so re-evaluate exactly when it changed.
        return world_changed;
    }
    if !detect.all_detect_capable {
        return true;
    }
    // All dependency capabilities are change-detection-capable and non-empty:
    // the §12.5 cheap path.
    if world_changed {
        return true;
    }
    detect.bits.keys().any(|name| {
        // Untracked (name unregistered, or no verdict recorded yet) → `true`
        // (conservative must-poll); tracked → the recorded changed bit.
        registry
            .type_id(name)
            .and_then(|ty| changes.changed(ty))
            .unwrap_or(true)
    })
}

/// Ordinary (non-exclusive) system: flag which parked policies need their
/// condition re-evaluated this frame, consuming the `#913` `detect` verdict and
/// the change signals it can observe.
///
/// - The [`BrinkWorldDelta`] changed-cell ledger (issue #1146) covers
///   conditions that read the shared ink World **per cell**: a policy is
///   flagged only when a global its condition's effect row lists as a read was
///   actually written this window, so a turn that only bumped visit counts
///   leaves a `gate`-reading condition alone (the #1101 spurious re-wake).
///   Bookkeeping reads are host-declared ([`FlowSleep::reads_bookkeeping`]) —
///   rows model global cells only. When the ledger cannot account for the
///   whole window (a serial driver, a direct host write into
///   [`BrinkGlobals::inner`]) it degrades to the coarse signal below, and so
///   does a condition whose row is missing or opaque.
/// - [`BrinkGlobals`] change detection is that coarse signal: any change
///   re-checks every parked all-detect-capable policy.
/// - The per-capability component-tick verdict [`CapabilityChanges`] (§12.5,
///   #996) covers component-backed **detect-capable** conditions — e.g. an
///   `is_player_nearby` reading `Transform`, or a door's `should_open` reading
///   a `Switch` — so they re-evaluate only when the watched component actually
///   changed, not every frame. A [`detect_capability_changes`](crate::capability::detect_capability_changes)
///   tracker (wired per registered component by `register_capability`, ordered
///   before this system) supplies that verdict.
///
/// The per-policy decision is [`wake_needs_reeval`]; see it for the exact
/// cadence and the conservative-must-poll fallback for capabilities this layer
/// cannot observe. Only [`Parked`](SleepState::Parked) policies are touched;
/// woken, cancelled, and faulted policies are left alone.
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]
pub fn mark_wake_dirty<M: Send + Sync + 'static>(
    globals: Option<Res<BrinkGlobals<M>>>,
    registry: Res<CapabilityRegistry<M>>,
    changes: Res<CapabilityChanges<M>>,
    wake_delta: Option<ResMut<BrinkWorldDelta<M>>>,
    // Optional so the system still runs in a bare `App` with no `AssetPlugin`
    // (the unit tests below drive it that way); absent, every condition's read
    // row is `Unknown` and the pass stays conservative.
    programs: Option<Res<Assets<ProgramAsset>>>,
    mut sleepers: Query<(&mut FlowSleep<M>, Option<&BrinkProgram<M>>)>,
) {
    let coarse_changed = globals.as_ref().is_some_and(DetectChanges::is_changed);
    let globals_tick = globals.as_ref().map(DetectChanges::last_changed);
    // Issue #1146: drain the changed-cell ledger once per pass, whatever the
    // sleepers turn out to need. `Some` means it is a complete account of
    // every shared-world change since this system last ran, so it *replaces*
    // the coarse resource-level bit rather than refining it; `None` (a serial
    // driver, a direct host write, a frame no batch turn ran) falls back to
    // that bit exactly as before this fix.
    let delta = wake_delta
        .map(ResMut::into_inner)
        .and_then(|ledger| ledger.drain(globals_tick, coarse_changed));
    for (mut sleep, program_ref) in &mut sleepers {
        if sleep.state != SleepState::Parked {
            continue;
        }
        let world_changed = match &delta {
            None => coarse_changed,
            Some(delta) => {
                // Only pay for the row lookup when something was actually
                // written this window.
                !delta.is_empty() && {
                    let asset = program_ref
                        .zip(programs.as_ref())
                        .and_then(|(program_ref, assets)| assets.get(&program_ref.handle));
                    let reads = condition_reads(asset, &sleep);
                    delta_touches_condition(delta, &reads, &sleep)
                }
            }
        };
        if wake_needs_reeval(&sleep, &registry, &changes, world_changed) && !sleep.needs_eval {
            sleep.needs_eval = true;
        }
    }
}

/// One parked flow scheduled for condition re-evaluation this pass.
struct WakeCandidate {
    entity: Entity,
    /// The named condition to resolve by path — ignored (a diagnostic label
    /// only) when `condition_value` is `Some`.
    condition: String,
    /// A dynamically-resolved fn-value token (issue #1078): when present,
    /// evaluation invokes it directly instead of resolving `condition` by
    /// path.
    condition_value: Option<Value>,
    args: Vec<Value>,
}

/// What to do with a woken/dead policy once its turn boundary is reached.
enum ReparkAction {
    /// Persistent policy re-parks: back to [`SleepState::Parked`].
    Rearm,
    /// One-shot policy (or a dead `-> END` flow): remove the component.
    Remove,
}

/// The gather query [`run_flow_sleep`] caches across frames (kept as a
/// `type` alias to satisfy `clippy::type_complexity` on the `Local` param).
type SleepGatherQuery<M> = QueryState<(
    Entity,
    &'static FlowSleep<M>,
    &'static BrinkFlow<M>,
    &'static BrinkProgram<M>,
)>;

/// Exclusive system: re-evaluate flagged wake conditions in each flow's own
/// context, wake on true, and re-arm/remove policies at turn boundaries
/// (`docs/effects-spec.md` §13.1). Gated by the plugin on
/// `any_with_component::<FlowSleep<M>>`, so it does no work when no flow sleeps.
///
/// Order-independent w.r.t. [`advance_batch`](crate::advance_batch): whether it
/// runs before or after the batch driver in a frame, a wake takes effect on the
/// following frame's Collect.
#[expect(
    clippy::too_many_lines,
    reason = "four coherent phases (gather, purity faults, re-park/retire, evaluate) that share \
              locals across the whole pass; splitting would just move the length into extra \
              parameter-passing"
)]
pub fn run_flow_sleep<M: Send + Sync + 'static>(
    world: &mut EcsWorld,
    // Cache the gather query across frames instead of rebuilding a fresh
    // `QueryState` (archetype match + component-id resolution) on every call
    // — the BH-5 prefetch pattern (#937 lineage; #1007 secondary). `iter`
    // still folds in any archetypes added since the last frame, so newly
    // spawned sleeping flows are picked up.
    mut gather: Local<SleepGatherQuery<M>>,
) {
    // ── Gather ── inspect (FlowSleep, BrinkFlow, BrinkProgram) without
    // holding the borrow across the `call_ink_function` re-entries below.
    // Only entities that are fully fulfilled flows carry `BrinkFlow`, so
    // unfulfilled requests are never touched (no spurious NotAFlow faults).
    let mut candidates: Vec<WakeCandidate> = Vec::new();
    let mut reparks: Vec<(Entity, ReparkAction)> = Vec::new();
    // Purity faults (issue #995): a condition rejected before its first
    // evaluation this pass — never called even once. Named separately from
    // `reparks` because it needs the (distinct) `WakeConditionPurityError` to
    // log, not just a re-arm/remove action.
    let mut purity_faults: Vec<(Entity, WakeConditionPurityError)> = Vec::new();
    {
        let programs = world.resource::<Assets<ProgramAsset>>();
        // The manifest is app-global (not per-marker `M` — `CapabilityRegistry`
        // is, but `CapabilityManifest` is the one host-authored table every
        // marker's stories share, per `crate::capability`'s module docs), so a
        // single fetch here covers every candidate this pass gathers.
        let manifest = world.resource::<CapabilityManifest>();
        // The `BrinkBindings<M>` registry (issue #1609): `None` if the host
        // never registered any `bind_brink_*` binding for this marker (the
        // resource is inserted lazily) — `check_named_condition_purity`/
        // `check_value_condition_purity` treat that the same as an empty
        // `commands` bucket.
        let bindings = world.get_resource::<BrinkBindings<M>>();
        for (entity, sleep, flow, program_ref) in gather.iter(world) {
            let status = flow.inner.status();
            // An `-> END` flow is dead; the policy is inert (§13.1 point 5).
            if status == StoryStatus::Ended {
                reparks.push((entity, ReparkAction::Remove));
                continue;
            }
            match sleep.state {
                SleepState::Cancelled | SleepState::Faulted => {}
                SleepState::Woken => {
                    // The woken turn finished (reached a natural yield): re-arm
                    // or retire. If it is still mid-turn / parked on an external
                    // it is left as-is for that resume path.
                    if status == StoryStatus::Done {
                        reparks.push((
                            entity,
                            match sleep.arming {
                                WakeArming::Persistent | WakeArming::Latch => ReparkAction::Rearm,
                                WakeArming::Once => ReparkAction::Remove,
                            },
                        ));
                    }
                }
                SleepState::Parked => {
                    // Policy applies to turn-boundary parks only; a dormant
                    // policy is additionally eligible before its first turn.
                    let eligible = sleep.dormant || status == StoryStatus::Done;
                    if eligible && sleep.needs_eval {
                        // Purity gate (issue #995, §13.1 point 2): admitted
                        // into `candidates` only if the condition's effect row
                        // proves no writes. A missing `ProgramAsset` (the
                        // story unloaded mid-frame) just skips this pass —
                        // `needs_eval` stays set and it's retried once the
                        // asset is back.
                        //
                        // A dynamically-resolved fn-value token (issue #1078:
                        // `FlowSleep::with_condition_value`) is checked via
                        // `check_value_condition_purity` — the same row
                        // inspection as the named path, just resolved through
                        // the value's own target instead of a path lookup.
                        if let Some(asset) = programs.get(&program_ref.handle) {
                            let purity = if let Some(value) = &sleep.condition_value {
                                check_value_condition_purity(
                                    &asset.program,
                                    &asset.effect_rows,
                                    manifest,
                                    bindings,
                                    value,
                                )
                            } else {
                                check_named_condition_purity(
                                    &asset.program,
                                    &asset.effect_rows,
                                    manifest,
                                    bindings,
                                    &sleep.condition,
                                )
                            };
                            match purity {
                                Ok(()) => candidates.push(WakeCandidate {
                                    entity,
                                    condition: sleep.condition.clone(),
                                    condition_value: sleep.condition_value.clone(),
                                    args: sleep.args.clone(),
                                }),
                                Err(err) => purity_faults.push((entity, err)),
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Purity faults ── reject before evaluation so an impure condition is
    // never called, not even once (issue #995). Faulted the same way a
    // runtime eval error is (never silently retried into a spin) — logged
    // with its own distinct, named error so the two failure classes are
    // never confused.
    for (entity, err) in purity_faults {
        if let Some(mut sleep) = world.get_mut::<FlowSleep<M>>(entity)
            && sleep.state == SleepState::Parked
        {
            warn!(
                "brink wake condition `{}` rejected for flow {:?}: {err} — policy parked \
                 (Faulted); a FlowSleep condition must be pure (docs/effects-spec.md §13.1 \
                 point 2)",
                sleep.condition, entity
            );
            sleep.state = SleepState::Faulted;
            sleep.needs_eval = false;
        }
    }

    // ── Re-park / retire ── apply before evaluation so a persistent flow that
    // just finished its woken turn is Parked again and can re-wake this pass.
    for (entity, action) in reparks {
        match action {
            ReparkAction::Rearm => {
                if let Some(mut sleep) = world.get_mut::<FlowSleep<M>>(entity) {
                    sleep.state = SleepState::Parked;
                    sleep.dormant = false;
                    sleep.needs_eval = false;
                }
            }
            ReparkAction::Remove => {
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.remove::<FlowSleep<M>>();
                }
            }
        }
    }

    // ── Evaluate ── each condition runs in its owning flow's context (shared
    // World ⊕ that flow's locals) and cannot advance the visible story.
    //
    // Issue #1146: building that context needs `&mut BrinkGlobals<M>`, which
    // trips bevy's change detection the instant the reference is taken —
    // even though the condition is purity-gated above and provably writes no
    // global cell. Left alone that is a **self-sustaining** wake signal: an
    // evaluation marks the world changed, the change re-flags the same
    // policy next frame, and it evaluates forever (and, worse, poisons the
    // changed-cell ledger's attribution for every *other* sleeper under `M`,
    // since a batch driver cannot tell that write apart from a host's).
    // Snapshot the change tick around the phase and restore it afterwards;
    // the one thing an evaluation can still legitimately move — bookkeeping
    // (a counted container's visit count, an RNG draw), which no effect row
    // models — is recorded in the ledger instead, so a policy that declares
    // it reads bookkeeping still sees it.
    if candidates.is_empty() {
        return;
    }
    let globals_tick = world
        .get_resource_ref::<BrinkGlobals<M>>()
        .map(|globals| globals.last_changed());

    for candidate in candidates {
        // A dynamically-resolved fn-value token (issue #1078) invokes
        // directly; a named condition resolves by path — mirrors the purity
        // gate's own branch above.
        let outcome = if let Some(value) = &candidate.condition_value {
            call_ink_function_value::<M>(world, candidate.entity, value, &candidate.args)
        } else {
            call_ink_function::<M>(
                world,
                candidate.entity,
                &candidate.condition,
                &candidate.args,
            )
        };
        let Some(mut sleep) = world.get_mut::<FlowSleep<M>>(candidate.entity) else {
            continue;
        };
        // A concurrent cancel between gather and evaluate must win.
        if sleep.state != SleepState::Parked {
            continue;
        }
        sleep.needs_eval = false;
        sleep.evaluated_once = true;
        match outcome {
            Ok(value) => {
                let raw = is_condition_true(&value);
                // `Persistent`/`Once` wake on a plain true reading; `Latch`
                // (issue #1081) wakes only on the edge it is currently
                // watching for (`waiting_for`), then flips that target so
                // the next wake requires the opposite edge.
                let fires = match sleep.arming {
                    WakeArming::Persistent | WakeArming::Once => raw,
                    WakeArming::Latch => raw == sleep.waiting_for,
                };
                if fires {
                    // Wake: Collect steps it next turn. `dormant` is cleared
                    // when the woken turn re-parks (or on removal for Once).
                    sleep.state = SleepState::Woken;
                    if sleep.arming == WakeArming::Latch {
                        sleep.waiting_for = !sleep.waiting_for;
                    }
                }
            }
            Err(err) => {
                warn!(
                    "brink wake condition `{}` faulted for flow {:?}: {err} — \
                     policy parked (Faulted); host must clear or replace it",
                    candidate.condition, candidate.entity
                );
                sleep.state = SleepState::Faulted;
            }
        }
    }

    // Restore the pre-evaluation change tick (see the phase comment above)
    // and hand the ledger the conservative bookkeeping touch that replaces
    // it. Both are no-ops when the marker has neither resource.
    if let Some(tick) = globals_tick
        && let Some(mut globals) = world.get_resource_mut::<BrinkGlobals<M>>()
    {
        globals.set_last_changed(tick);
    }
    if let Some(mut ledger) = world.get_resource_mut::<BrinkWorldDelta<M>>() {
        ledger.record_condition_evaluation();
    }
}

#[cfg(test)]
mod tests;
