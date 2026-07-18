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
//! - **no external-capability dependency** ([`DetectSummary::bits`] empty,
//!   vacuously [`DetectSummary::all_detect_capable`] `true`): the condition
//!   reads only ink World state, so it is re-evaluated only when the shared
//!   World actually changed (bevy change detection on [`BrinkGlobals`]) —
//!   the cheap path.
//! - **any capability dependency** (`bits` non-empty) — **always polls**,
//!   re-evaluated every wake pass, *even if every bit is `true`*. The `#913`
//!   AND-merge verdict promises a capability's reads are change-detection
//!   *capable*, but [`mark_wake_dirty`] has no hook on a component's own
//!   change ticks (`docs/effects-spec.md` §12.5 is not wired here) — only on
//!   [`BrinkGlobals`]. Trusting an all-`true` verdict for a component-backed
//!   condition (e.g. `is_player_nearby` reading `Transform`) would gate
//!   re-evaluation on a signal ([`BrinkGlobals`]) that a pure-component
//!   change never fires, and the flow would miss its wake. Conservative: a
//!   wasted re-evaluation of a still-false condition just leaves the flow
//!   parked (§3 soundness direction — over-report, never miss a wake).
//!
//! Re-evaluation is **always sound** regardless of the verdict: the detect bits
//! only tune the *cadence*, and today that cadence is cheap **only** for the
//! no-capability-dependency case above. That is also why `#913`'s fold must
//! land first — a last-write-wins `true` on a capability that is really
//! must-poll would compound the same class of missed wake once §12.5 lands.
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
//!   effect the following frame either way. Before admitting a flagged policy
//!   for evaluation it also runs the attach-time purity gate
//!   ([`check_named_condition_purity`], issue #995, §13.1 point 2): a
//!   condition whose effect row shows writes is rejected loudly
//!   ([`WakeConditionPurityError`]) and never called, not even once.
//!
//! [`WaitingForChoice`]: brink_runtime::StoryStatus::WaitingForChoice
//! [`Ended`]: brink_runtime::StoryStatus::Ended
//! [`StoryStatus::Done`]: brink_runtime::StoryStatus::Done
//! [`advance_batch`]: crate::advance_batch

use std::collections::BTreeMap;
use std::marker::PhantomData;

use bevy_asset::Assets;
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::QueryState;
use bevy_ecs::system::{Local, Query, Res};
use bevy_ecs::world::World as EcsWorld;
use bevy_log::warn;
use brink_format::{DefinitionId, EffectRowEntry, Value};
use brink_runtime::{Program, StoryStatus};
use thiserror::Error;

use crate::asset::{BrinkProgram, ProgramAsset};
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
    /// default for a condition reading only ink World state. A summary whose
    /// [`bits`](DetectSummary::bits) names any external (component)
    /// capability always must-polls instead, regardless of the AND-merge
    /// verdict — `mark_wake_dirty` has no per-capability component-tick hook
    /// yet (`docs/effects-spec.md` §12.5), only `BrinkGlobals`. Builder.
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
    /// AND-merge verdict). `false` means the condition polls — but note `true`
    /// does **not** by itself guarantee the cheap path: `mark_wake_dirty` only
    /// takes it at face value when [`detect_summary`](Self::detect_summary)'s
    /// [`bits`](DetectSummary::bits) is also empty (no external-capability
    /// dependency); a non-empty `bits` map always polls regardless, pending
    /// §12.5 component-tick wiring.
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
// **Scope note** (recorded on issue #995): this checks the row's own
// ink-level writes, not writes performed transitively through a
// host-registered `EXTERNAL` the condition calls (that axis needs the BH-1
// capability join — `CapabilityManifest`/`CapabilityRegistry`/
// `compute_container_access` — which is its own coupling; flagged as a
// follow-up rather than folded into this fix).
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
fn check_row_purity(
    program: &Program,
    row: &EffectRowEntry,
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

/// Check purity for a **named** wake condition (`FlowSleep::condition`'s
/// shape) — resolves `condition` to a `DefinitionId` via
/// [`Program::definition_id_for_path`], then inspects its `EffectRows` row.
///
/// `Ok(())` when `effect_rows` is empty entirely: a story that never shipped
/// an `EffectRows` table (converter-built, or otherwise never ran the
/// compiler's effects emission) is outside the guarantee this checks — see
/// the module section above.
///
/// # Errors
/// See [`WakeConditionPurityError`].
pub fn check_named_condition_purity(
    program: &Program,
    effect_rows: &[EffectRowEntry],
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
    check_row_purity(program, row, condition)
}

/// Check purity for a **dynamic fn-value** wake condition — a `Value`
/// (`FnRef`/`Closure`) resolved token rather than a static name, e.g. one a
/// host obtained from a global or a `bind_brink_query` result. Resolves the
/// value's target via [`Value::fn_target`], then inspects the same
/// `EffectRows` row [`check_named_condition_purity`] does.
///
/// Same empty-`effect_rows` bypass as [`check_named_condition_purity`].
///
/// # Errors
/// See [`WakeConditionPurityError`]. [`WakeConditionPurityError::NotAFunctionValue`]
/// if `value` isn't a function value at all.
pub fn check_value_condition_purity(
    program: &Program,
    effect_rows: &[EffectRowEntry],
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
    check_row_purity(program, row, &label)
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
/// The only change signal this system can observe is [`BrinkGlobals`] (the
/// ink shared World) — there is no per-capability component-tick wiring yet
/// (`docs/effects-spec.md` §12.5 is not implemented here). So:
///
/// - A policy whose [`DetectSummary::bits`] is **non-empty** — its condition
///   depends on at least one external (component) capability, e.g. an
///   `is_player_nearby` reading `Transform` — is **always** flagged
///   (must-polled), *regardless* of the `#913` AND-merge verdict. A `true`
///   bit only promises the capability's *reads* are change-detection-backed
///   in principle; this system has no hook on those component ticks, so
///   trusting the bit here would silently miss a component-only change (the
///   engine-race class this module exists to avoid).
/// - A policy with an **empty** `bits` map (no external-capability
///   dependency — it reads only ink World state) is the one case
///   [`BrinkGlobals`] change detection actually covers: flagged only when
///   [`BrinkGlobals`] changed, or if it has never been evaluated (so a
///   dormant policy still gets its first evaluation on a quiet frame).
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
        // A non-empty `bits` map names at least one external capability
        // dependency; only `BrinkGlobals` change detection is wired below, so
        // any such policy must must-poll regardless of `all_detect_capable`.
        let has_capability_deps = !sleep.detect.bits.is_empty();
        let should_flag = has_capability_deps
            || !sleep.detect.all_detect_capable
            || world_changed
            || !sleep.evaluated_once;
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
                        // Purity gate (issue #995, §13.1 point 2): admitted
                        // into `candidates` only if the condition's effect row
                        // proves no writes. A missing `ProgramAsset` (the
                        // story unloaded mid-frame) just skips this pass —
                        // `needs_eval` stays set and it's retried once the
                        // asset is back.
                        if let Some(asset) = programs.get(&program_ref.handle) {
                            match check_named_condition_purity(
                                &asset.program,
                                &asset.effect_rows,
                                &sleep.condition,
                            ) {
                                Ok(()) => candidates.push(WakeCandidate {
                                    entity,
                                    condition: sleep.condition.clone(),
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
