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
//!    flow under a policy is **skipped by Collect** ([`advance_batch`] filters
//!    it out — [`FlowSleep::wants_collect`] is the predicate), so a parked flow
//!    costs **zero** per turn.
//! 2. A dependency changing triggers **re-evaluation, not waking**: the
//!    condition (a pure ink fn — purity is provable from its effect row) is
//!    re-evaluated only when a dependency moved, and the flow wakes **only when
//!    the condition is true** ("re-evaluate, don't wake"). Re-evaluation runs
//!    in the **owning flow's context** (shared World ⊕ that flow's own locals,
//!    never a bare world) via [`call_ink_function`](crate::call_ink_function).
//! 3. A woken flow runs a normal turn; the condition has no mid-turn influence.
//! 4. Policies are **persistent by default** (re-arm when the flow re-parks);
//!    [`WakeArming::Once`] covers one-shots; the host may clear (remove the
//!    component) or replace a policy anytime, and [`FlowSleep::cancel`] resolves
//!    a policy to a permanent **false** (the flow is never woken by it again).
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
//! - **all detect-capable** ([`DetectSummary::all_detect_capable`] `true`,
//!   including the no-external-dependency case): the condition is re-evaluated
//!   only when the shared World actually changed (bevy change detection on
//!   [`BrinkGlobals`]) — the cheap path.
//! - **any must-poll** (a single `false` bit — exactly what the `#913`
//!   AND-merge guarantees a conflicting-bits capability folds to): the
//!   condition **polls** — re-evaluated every wake pass. Conservative: a wasted
//!   re-evaluation of a still-false condition just leaves the flow parked
//!   (§3 soundness direction — over-report, never miss a wake).
//!
//! Re-evaluation is **always sound** regardless of the verdict: the detect bits
//! only tune the *cadence*. That is why `#913`'s fold must land first — a
//! last-write-wins `true` on a capability that is really must-poll would gate
//! re-evaluation on a change signal that never fires, and the flow would miss
//! its wake (the engine-race class).
//!
//! ## Systems (auto-registered by [`BrinkPlugin`](crate::BrinkPlugin))
//!
//! - [`mark_wake_dirty`] (ordinary system): consults each parked policy's
//!   [`DetectSummary`] + [`BrinkGlobals`] change detection and flags which
//!   parked flows need a re-evaluation this frame.
//! - [`run_flow_sleep`] (exclusive system, gated on
//!   `any_with_component::<FlowSleep<M>>`): re-evaluates the flagged conditions
//!   in each flow's own context, wakes on true, and re-arms/removes policies at
//!   turn boundaries. Order-independent w.r.t. [`advance_batch`]: waking takes
//!   effect the following frame either way.
//!
//! [`WaitingForChoice`]: brink_runtime::StoryStatus::WaitingForChoice
//! [`Ended`]: brink_runtime::StoryStatus::Ended
//! [`StoryStatus::Done`]: brink_runtime::StoryStatus::Done
//! [`advance_batch`]: crate::advance_batch

use std::collections::BTreeMap;
use std::marker::PhantomData;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Query, Res};
use bevy_ecs::world::World as EcsWorld;
use bevy_log::warn;
use brink_format::Value;
use brink_runtime::StoryStatus;

use crate::bindings::call_ink_function;
use crate::capability::ContainerAccess;
use crate::flow::BrinkFlow;
use crate::globals::BrinkGlobals;

/// When a woken flow re-parks, does its policy re-arm or retire?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WakeArming {
    /// Re-arm every time the flow re-parks at a turn boundary — a standing
    /// subscription. The default (`docs/effects-spec.md` §13.1 point 4).
    #[default]
    Persistent,
    /// Fire exactly once: after the first wake runs its turn, the policy is
    /// removed and the flow reverts to ordinary per-turn advancement.
    Once,
}

/// The lifecycle state of a [`FlowSleep`] policy — inspector-visible, and the
/// single field [`FlowSleep::wants_collect`] reads to tell Collect whether the
/// flow steps this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepState {
    /// Parked: the flow is asleep under this policy and is **skipped by
    /// Collect**. The default for a freshly attached policy.
    Parked,
    /// Woken: the condition evaluated true; Collect steps the flow this turn.
    /// On reaching its next turn boundary the policy re-arms
    /// ([`WakeArming::Persistent`]) or is removed ([`WakeArming::Once`]).
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Component)]
pub struct FlowSleep<M: Send + Sync + 'static = ()> {
    /// The ink function name whose (pure) return value is the wake condition.
    condition: String,
    /// Arguments passed to the condition function, in declaration order.
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

    fn new(condition: String, arming: WakeArming) -> Self {
        Self {
            condition,
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
            _marker: PhantomData,
        }
    }

    /// Pass arguments to the condition function (declaration order). Builder.
    #[must_use]
    pub fn with_args(mut self, args: Vec<Value>) -> Self {
        self.args = args;
        self
    }

    /// Attach the condition's dependency [`DetectSummary`] (`#913`), tuning the
    /// re-evaluation cadence. Without this the summary is empty — treated as
    /// all-detect-capable (re-evaluate only on a World change), the right
    /// default for a condition reading only ink World state. Builder.
    #[must_use]
    pub fn with_detect(mut self, detect: DetectSummary) -> Self {
        self.detect = detect;
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

    /// The ink function name of the wake condition.
    #[must_use]
    pub fn condition(&self) -> &str {
        &self.condition
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

    /// Whether every dependency capability is change-detection-backed (`#913`
    /// AND-merge verdict). `false` means the condition polls.
    #[must_use]
    pub fn dependencies_all_detect_capable(&self) -> bool {
        self.detect.all_detect_capable
    }

    /// The dependency [`DetectSummary`] this policy was built with.
    #[must_use]
    pub fn detect_summary(&self) -> &DetectSummary {
        &self.detect
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

/// Ordinary (non-exclusive) system: flag which parked policies need their
/// condition re-evaluated this frame, consuming the `#913` `detect` verdict.
///
/// - A [`must-poll`](DetectSummary::all_detect_capable) policy (any dependency
///   not detect-capable) is flagged every frame.
/// - An all-detect-capable policy is flagged only when [`BrinkGlobals`] changed
///   (bevy change detection) — or if it has never been evaluated (so a dormant
///   policy still gets its first evaluation on a quiet frame).
///
/// Only [`Parked`](SleepState::Parked) policies are touched; woken, cancelled,
/// and faulted policies are left alone.
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]
pub fn mark_wake_dirty<M: Send + Sync + 'static>(
    globals: Option<Res<BrinkGlobals<M>>>,
    mut sleepers: Query<&mut FlowSleep<M>>,
) {
    let world_changed = globals.as_ref().is_some_and(DetectChanges::is_changed);
    for mut sleep in &mut sleepers {
        if sleep.state != SleepState::Parked {
            continue;
        }
        let should_flag =
            !sleep.detect.all_detect_capable || world_changed || !sleep.evaluated_once;
        if should_flag && !sleep.needs_eval {
            sleep.needs_eval = true;
        }
    }
}

/// One parked flow scheduled for condition re-evaluation this pass.
struct WakeCandidate {
    entity: Entity,
    condition: String,
    args: Vec<Value>,
}

/// What to do with a woken/dead policy once its turn boundary is reached.
enum ReparkAction {
    /// Persistent policy re-parks: back to [`SleepState::Parked`].
    Rearm,
    /// One-shot policy (or a dead `-> END` flow): remove the component.
    Remove,
}

/// Exclusive system: re-evaluate flagged wake conditions in each flow's own
/// context, wake on true, and re-arm/remove policies at turn boundaries
/// (`docs/effects-spec.md` §13.1). Gated by the plugin on
/// `any_with_component::<FlowSleep<M>>`, so it does no work when no flow sleeps.
///
/// Order-independent w.r.t. [`advance_batch`](crate::advance_batch): whether it
/// runs before or after the batch driver in a frame, a wake takes effect on the
/// following frame's Collect.
pub fn run_flow_sleep<M: Send + Sync + 'static>(world: &mut EcsWorld) {
    // ── Gather ── inspect (FlowSleep, BrinkFlow) without holding the borrow
    // across the `call_ink_function` re-entries below. Only entities that are
    // fully fulfilled flows carry `BrinkFlow`, so unfulfilled requests are
    // never touched (no spurious NotAFlow faults).
    let mut candidates: Vec<WakeCandidate> = Vec::new();
    let mut reparks: Vec<(Entity, ReparkAction)> = Vec::new();
    {
        let mut query = world.query::<(Entity, &FlowSleep<M>, &BrinkFlow<M>)>();
        for (entity, sleep, flow) in query.iter(world) {
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
                                WakeArming::Persistent => ReparkAction::Rearm,
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
                        candidates.push(WakeCandidate {
                            entity,
                            condition: sleep.condition.clone(),
                            args: sleep.args.clone(),
                        });
                    }
                }
            }
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
    for candidate in candidates {
        let outcome = call_ink_function::<M>(
            world,
            candidate.entity,
            &candidate.condition,
            &candidate.args,
        );
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
                if is_condition_true(&value) {
                    // Wake: Collect steps it next turn. `dormant` is cleared
                    // when the woken turn re-parks (or on removal for Once).
                    sleep.state = SleepState::Woken;
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
}

#[cfg(test)]
mod tests;
