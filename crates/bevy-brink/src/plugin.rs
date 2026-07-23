//! The Bevy plugin for brink ink stories.

use std::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_asset::AssetApp;
use bevy_ecs::schedule::IntoScheduleConfigs as _;
use brink_runtime::{ExecMode, WorldPolicy};

use crate::asset::{BrinkStoryAsset, InkbLoader, LineTablesAsset, ProgramAsset};
use crate::globals::{BrinkExecMode, BrinkWorldPolicy};
use crate::request::fulfill_flow_requests;

/// A Bevy plugin that registers brink story types, messages, and asset
/// loaders for a single story instance identified by the marker type `M`.
///
/// The default `M = ()` suits the common single-story case. Declare your
/// own marker types (any `Send + Sync + 'static` ZST works) when you need
/// multiple concurrent stories in one app — each gets its own
/// `BrinkGlobals<M>` resource and `BrinkFlow<M>`/`BrinkContext<M>`/
/// `BrinkLocale<M>` components, monomorphized to distinct Bevy types
/// with no runtime overhead.
///
/// Adding `BrinkPlugin<M>` also ensures [`BrinkAssetsPlugin`] is added
/// once to the app (for shared asset types that don't depend on `M`).
///
/// **This plugin does not register an auto-advance system.** Most games
/// drive advancement from input or game-state events, not every tick.
/// Apps that want per-tick advancement can register
/// [`advance_flows`](crate::advance_flows) themselves:
///
/// ```ignore
/// app.add_systems(Update, advance_flows::<MyStory>);
/// ```
pub struct BrinkPlugin<M: Send + Sync + 'static = ()> {
    policy: WorldPolicy,
    exec_mode: ExecMode,
    // #1029: out-of-band `ProjectConfig` override, threaded to the dev-mode
    // `InkLoader` (via `BrinkAssetsPlugin`) when this plugin is the one that
    // adds it. `dev`-only: the config only matters to the source-compiling
    // asset loader, which doesn't exist without the `dev` feature.
    #[cfg(feature = "dev")]
    config: Option<brink_project_config::ProjectConfig>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkPlugin<M> {
    fn default() -> Self {
        Self {
            policy: WorldPolicy::default(),
            // F35 (ruled 2026-07-19): profile-keyed default via
            // `BrinkExecMode::default` — `Dev` under debug_assertions, `Prod`
            // in release. Diverges from core `ExecMode::default` (always
            // `Dev`); a host overrides with `with_exec_mode`.
            exec_mode: BrinkExecMode::<M>::default().mode,
            #[cfg(feature = "dev")]
            config: None,
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkPlugin<M> {
    /// Install a host-supplied [`WorldPolicy`] for this marker's shared
    /// [`BrinkGlobals<M>`](crate::BrinkGlobals) `World` — resolved once,
    /// against the first-fulfilled flow's program, when
    /// [`fulfill_flow_requests`](crate::fulfill_flow_requests) creates it.
    ///
    /// Default (if this is never called): `WorldPolicy::default()` — every
    /// unit homed to `World`, byte-identical to plain ink. Per the F6
    /// AMENDMENT (`docs/scoped-flow-state-spec.md`): the plain-ink default
    /// stays `World`; hosts opt a per-entity NPC into private state by
    /// enumerating `Local` overrides on top, not by flipping the default.
    ///
    /// A [`PolicyError`](brink_runtime::PolicyError) (an override names a
    /// variable or knot/stitch the program doesn't declare) surfaces as a
    /// logged fulfillment error on the offending request, not a panic — see
    /// `fulfill_flow_requests`.
    #[must_use]
    pub fn with_policy(mut self, policy: WorldPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Override the [`ExecMode`] every flow of this marker starts in (F35,
    /// ruled 2026-07-19).
    ///
    /// Default (if this is never called): the build-profile-keyed value —
    /// [`ExecMode::Dev`] under `debug_assertions`, [`ExecMode::Prod`] in a
    /// release build (see [`BrinkExecMode`](crate::BrinkExecMode)). Call this
    /// to pin a mode regardless of profile — e.g. `with_exec_mode(ExecMode::Dev)`
    /// to keep the fault-loud posture in a release editor build, or
    /// `with_exec_mode(ExecMode::Prod)` to run keep-moving in a debug build.
    ///
    /// The mode is a host/build knob, never embedded in `.inkb` and never
    /// persisted in saves; a per-flow override is still available at runtime
    /// via [`FlowInstance::set_exec_mode`](brink_runtime::FlowInstance::set_exec_mode).
    #[must_use]
    pub fn with_exec_mode(mut self, mode: ExecMode) -> Self {
        self.exec_mode = mode;
        self
    }

    /// Override the [`ProjectConfig`](brink_project_config::ProjectConfig)
    /// (`dialect`/`types`) the dev-mode [`InkLoader`](crate::InkLoader) uses
    /// for stories compiled under this marker (#1029).
    ///
    /// The **programmatic escape hatch**: it wins over whatever `brink.toml`
    /// the loader's bounded asset walk-up discovers beside the entry story —
    /// for packed/embedded builds where there's no meaningful sibling file,
    /// or for a host that simply prefers configuring dialect/types in game
    /// code. Fields left `None` on the given [`ProjectConfig`] still fall
    /// through to whatever the discovered `brink.toml` (or the built-in
    /// default) supplies — same "only touch what you set" precedence as the
    /// CLI's `--dialect`/`--types` flags
    /// (`AnalysisOptions::apply_project_config`).
    ///
    /// Only takes effect if this is the [`BrinkPlugin<M>`] instance that
    /// ends up adding [`BrinkAssetsPlugin`] (the first one, per marker
    /// registration order) — later markers' overrides are ignored once
    /// `BrinkAssetsPlugin` already exists in the app, same as every other
    /// `BrinkAssetsPlugin`-owned setting. Call
    /// [`BrinkAssetsPlugin::with_config`] directly instead if you're adding
    /// it standalone.
    #[cfg(feature = "dev")]
    #[must_use]
    pub fn with_config(mut self, config: brink_project_config::ProjectConfig) -> Self {
        self.config = Some(config);
        self
    }
}

impl<M: Send + Sync + 'static> Plugin for BrinkPlugin<M> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<BrinkAssetsPlugin>() {
            #[cfg(feature = "dev")]
            let assets_plugin = BrinkAssetsPlugin::default().with_config_option(self.config);
            #[cfg(not(feature = "dev"))]
            let assets_plugin = BrinkAssetsPlugin::default();
            app.add_plugins(assets_plugin);
        }
        app.insert_resource(BrinkWorldPolicy::<M>::new(self.policy.clone()));
        // F35 (ruled 2026-07-19): the host-selected (or profile-defaulted)
        // ExecMode every flow of marker `M` spawns in. Applied to each
        // FlowInstance at creation by `fulfill_flow_requests`.
        app.insert_resource(BrinkExecMode::<M>::new(self.exec_mode));
        app.add_systems(Update, fulfill_flow_requests::<M>);
        // T1d-3 handle integration (docs/t1d-spec.md §4): the type-erased
        // kind index (empty until a host calls `register_handle_kind`) and
        // its diagnostics-only retention metrics, the `is_valid(h)` binding
        // (a standard world-query binding per spec, not a language
        // intrinsic), and registry GC at `-> DONE` quiescent sweeps.
        app.init_resource::<crate::handle::HandleKinds<M>>();
        app.init_resource::<crate::handle::HandleRetentionMetrics<M>>();
        app.init_resource::<crate::handle::HandleEntityRemap>();
        {
            use crate::bindings::BrinkBindingsAppExt as _;
            app.bind_brink_query::<M, _, _>("is_valid", crate::handle::is_valid_system::<M>);
        }
        app.add_observer(crate::handle::gc_on_turn_done::<M>);
        // BH-1 (docs/effects-spec.md §9, §12–§13; #899): the capability
        // registry (name -> ComponentId, empty until `register_capability`
        // is called) and the per-story joined-access table, rebuilt whenever
        // a `ProgramAsset` loads/unloads (§12.5's load-boundary invariant).
        // `CapabilityManifest` inits empty too — a host that never inserts
        // one just never gets ECS-capability access data.
        app.init_resource::<crate::capability::CapabilityManifest>();
        app.init_resource::<crate::capability::CapabilityRegistry<M>>();
        app.init_resource::<crate::capability::CapabilityTable<M>>();
        // BH detect path (docs/effects-spec.md §12.5; #996): the per-frame,
        // per-capability change verdict `mark_wake_dirty` reads. Always present
        // so the wake layer can take it as a plain `Res`; each
        // `register_capability` call also wires the typed tracker that fills it.
        app.init_resource::<crate::capability::CapabilityChanges<M>>();
        app.add_systems(Update, crate::capability::rebuild_capability_table::<M>);
        // BH-2 (docs/effects-spec.md §12.4; #914): the batch-turn report
        // resource, always present so a host that opts into
        // `advance_batch::<M>` (not auto-registered — batch mode is opt-in,
        // like `advance_flows`) gets its per-flow capability/access bookkeeping
        // recorded. The batch driver itself is NOT added here; a host adds
        // `app.add_systems(Update, advance_batch::<M>)` when it wants
        // frame-start-consistent batched stepping.
        app.init_resource::<crate::batch::BrinkBatchReport<M>>();
        // BH-4 (docs/effects-spec.md §13.1; #973): reactive sleep. `FlowSleep`
        // is a standing wake policy on a flow entity; parked flows are skipped
        // by Collect (`advance_batch`). `mark_wake_dirty` consults the `#913`
        // detect verdict + `BrinkGlobals` change detection to flag which parked
        // conditions need re-evaluation; `run_flow_sleep` (exclusive — it
        // re-enters the VM via `call_ink_function`) re-evaluates flagged
        // conditions in each flow's own context and wakes on true. Gated so
        // neither does work until a flow actually sleeps. Ordered
        // dirty-then-eval so a same-frame World change is seen this pass.
        //
        // `.before(advance_batch::<M>)` closes a same-frame race discovered
        // while hardening issue #1081's `WakeArming::Latch` tests: `Collect`
        // (`advance_batch`) steps any flow whose `FlowSleep::wants_collect()`
        // is true (`state == Woken`), and only `run_flow_sleep`'s repark
        // phase clears that back to `Parked` once the woken turn reaches a
        // `Done` boundary. Without an explicit order, a host that also
        // registers `advance_batch::<M>` leaves the two system sets
        // unconstrained relative to each other — Bevy's default multithreaded
        // executor does not guarantee a stable relative order between
        // independently-added systems that don't conflict on data access, so
        // it can (rarely) run `advance_batch` before this chain on one frame
        // and after it on the next. That window lets a flow that woke and
        // was collected on frame N (still `Woken`, not yet reparked because
        // `run_flow_sleep` hadn't run again) get collected a **second** time
        // on frame N+1 if `advance_batch` happens to run before this chain
        // that frame — an extra, spurious turn from a single wake, the exact
        // "over-fire" `wake_fan_out` scenario tests never exercised (they
        // don't assert an exact repeated count across many cycles the way
        // the `Latch` cycling test does). Forcing this chain before
        // `advance_batch` guarantees the repark for a completed wake is
        // always applied in the same frame the wake was collected, before
        // `advance_batch` gets another chance to run — closing the window
        // regardless of scheduler ordering. Inert if the host never adds
        // `advance_batch::<M>` (an ordering constraint against an absent
        // system is a no-op).
        app.add_systems(
            Update,
            (
                crate::sleep::mark_wake_dirty::<M>,
                crate::sleep::run_flow_sleep::<M>,
            )
                .chain()
                .before(crate::batch::advance_batch::<M>)
                .run_if(
                    bevy_ecs::schedule::common_conditions::any_with_component::<
                        crate::sleep::FlowSleep<M>,
                    >,
                ),
        );
        // Auto-render BrinkTranscript<M> for any flow that has it.
        // No-op for flows that don't (the query just yields nothing).
        app.add_systems(Update, crate::transcript::refresh_transcripts::<M>);
        // Resolve deferred engine→ink calls (commands.brink_call). Exclusive
        // (needs &mut World to run query bindings), gated so it only runs
        // when a call is actually pending.
        app.add_systems(
            Update,
            crate::call::resolve_brink_calls::<M>.run_if(
                bevy_ecs::schedule::common_conditions::any_with_component::<
                    crate::call::BrinkCallRequest<M>,
                >,
            ),
        );
        // Service flows that paused on a pending external during normal
        // playback (a non-exclusive step_one yielded AwaitingQuery): resolve
        // world-access queries inline, fire BrinkExternalAwaited for async
        // (event) bindings, spawn tasks for task bindings. Exclusive (needs
        // &mut World), gated so it only runs when a flow is actually awaiting.
        app.add_systems(
            Update,
            crate::bindings::resolve_pending_externals::<M>
                .run_if(crate::bindings::any_flow_awaiting_external::<M>),
        );
        // Poll detached bind_brink_task futures; resolve the flow when one
        // finishes. Gated so it only runs while a task is pending.
        app.add_systems(
            Update,
            crate::async_bind::poll_brink_tasks::<M>.run_if(
                bevy_ecs::schedule::common_conditions::any_with_component::<
                    crate::async_bind::BrinkPendingTask<M>,
                >,
            ),
        );
        // Global, event-driven locale switching: the current-locale resource,
        // an observer that reconciles flows when it changes, and a catch-up
        // system for `.inkl`s that finish loading after a switch.
        app.init_resource::<crate::locale::BrinkCurrentLocale<M>>();
        app.init_resource::<crate::locale::LocalizedTablesCache<M>>();
        app.add_observer(crate::locale::on_locale_changed::<M>);
        app.add_systems(Update, crate::locale::catch_up_loaded_locales::<M>);
        #[cfg(feature = "dev")]
        app.init_resource::<crate::replay::BrinkReplayConfig>();
        #[cfg(feature = "dev")]
        app.add_systems(Update, crate::replay::replay_on_reload::<M>);
        #[cfg(debug_assertions)]
        app.add_systems(Update, crate::request::warn_post_fulfillment_mutations::<M>);
    }
}

/// Registers asset types and loaders that are shared across all markers.
///
/// [`BrinkPlugin::build`] adds this automatically if it's not already
/// present, so you rarely need to add it manually — but you can if you
/// want the asset machinery without any marker-specific plumbing (e.g.
/// for a headless asset-processing binary).
#[derive(Default)]
pub struct BrinkAssetsPlugin {
    // #1029: threaded to the dev-mode `InkLoader` (see `BrinkPlugin::with_config`
    // for the full precedence contract). `dev`-only for the same reason
    // `BrinkPlugin::config` is.
    #[cfg(feature = "dev")]
    config: Option<brink_project_config::ProjectConfig>,
}

impl BrinkAssetsPlugin {
    /// Override the `brink.toml`-sourced [`ProjectConfig`](brink_project_config::ProjectConfig)
    /// the dev-mode [`InkLoader`](crate::InkLoader) uses (#1029) — the
    /// standalone-plugin equivalent of [`BrinkPlugin::with_config`], for
    /// hosts that add `BrinkAssetsPlugin` directly (e.g. a headless
    /// asset-processing binary) without going through `BrinkPlugin<M>`.
    #[cfg(feature = "dev")]
    #[must_use]
    pub fn with_config(mut self, config: brink_project_config::ProjectConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Same as [`Self::with_config`] but takes the already-`Option`al form
    /// `BrinkPlugin::build` holds, so it can thread its own (possibly unset)
    /// override through without an `if let` at the call site.
    #[cfg(feature = "dev")]
    #[must_use]
    fn with_config_option(mut self, config: Option<brink_project_config::ProjectConfig>) -> Self {
        self.config = config;
        self
    }
}

impl Plugin for BrinkAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<BrinkStoryAsset>();
        app.init_asset::<ProgramAsset>();
        app.init_asset::<LineTablesAsset>();
        app.init_asset::<crate::locale::LocaleAsset>();
        app.init_asset::<crate::brkt::TranscriptAsset>();
        app.init_asset_loader::<InkbLoader>();
        app.init_asset_loader::<crate::locale::InklLoader>();
        app.init_asset_loader::<crate::brkt::BrktLoader>();
        #[cfg(feature = "dev")]
        app.register_asset_loader(crate::source_loader::InkLoader {
            override_config: self.config,
        });
    }
}
