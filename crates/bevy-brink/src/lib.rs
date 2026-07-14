//! Bevy integration for brink ink stories.
//!
//! This crate exposes the brink runtime as a Bevy plugin: story programs
//! are loaded as `Asset`s, flow state lives on `Component`s, story-wide
//! globals live in `Resource`s. All types are parameterized over a ZST
//! marker so multiple independent story instances can coexist in one app.
//!
//! Most games will use the default `()` marker and just do:
//!
//! ```ignore
//! app.add_plugins(BrinkPlugin::default());
//! ```
//!
//! Games with multiple concurrent story instances declare marker types
//! and register a plugin per marker:
//!
//! ```ignore
//! struct MainStory;
//! struct DreamSequence;
//!
//! app.add_plugins((
//!     BrinkPlugin::<MainStory>::default(),
//!     BrinkPlugin::<DreamSequence>::default(),
//! ));
//! ```

// Lets `#[derive(BrinkCommand)]`-generated code reference `::bevy_brink`
// from within this crate itself (the same trick serde/bevy use for their
// own derives).
extern crate self as bevy_brink;

mod asset;
mod async_bind;
mod bindings;
mod brkt;
mod call;
mod event;
mod flow;
mod globals;
mod input;
mod line_tables;
mod locale;
mod plugin;
#[cfg(feature = "dev")]
mod replay;
mod request;
#[cfg(feature = "dev")]
mod source_loader;
#[cfg(test)]
mod test_support;
mod transcript;

pub use asset::{
    BrinkProgram, BrinkStory, BrinkStoryAsset, InkbLoader, InkbLoaderError, LineTablesAsset,
    ProgramAsset,
};
pub use async_bind::{
    BrinkAwaiting, BrinkExternalAwaited, BrinkPendingTask, BrinkResolveExternalExt,
    poll_brink_tasks,
};
/// `#[derive(BrinkCommand)]` — generates [`BrinkCommand::from_ink_args`].
/// Shares its name with the trait (macro vs. type namespace), so a single
/// `use bevy_brink::BrinkCommand;` brings both into scope.
pub use bevy_brink_derive::BrinkCommand;
pub use bindings::{
    BrinkArgError, BrinkBindings, BrinkBindingsAppExt, BrinkCallError, BrinkCommand, BrinkHandler,
    BrinkQueryInput, advance_flow, any_flow_awaiting_external, call_ink_function,
    call_ink_function_value, resolve_pending_externals,
};
/// Re-exported so `#[derive(BrinkCommand)]`-generated code (and binding
/// authors) can name the ink runtime value type without depending on
/// `brink-format` directly.
pub use brink_format::Value;
/// The whole runtime crate, re-exported as an escape hatch: any `brink_runtime`
/// type that surfaces in a `bevy-brink` API but isn't individually re-exported
/// below is reachable as `bevy_brink::runtime::…` — so a consumer never needs a
/// direct `brink-runtime` Cargo dependency to name one.
pub use brink_runtime as runtime;
/// Re-exported so consumers can choose `Overlay`/`Strict` application without
/// a direct `brink-runtime` dependency.
pub use brink_runtime::LocaleMode;
/// `brink_runtime::World` — the single story-state layer shared by every
/// flow under a marker, carried by [`BrinkGlobals`]. Aliased to avoid the
/// glob-import collision with `bevy::prelude::World` (the ECS world): `use
/// bevy::prelude::*; use bevy_brink::*;` would make a bare `World`
/// ambiguous. Name story state `BrinkWorld` and the ECS world `World`.
pub use brink_runtime::World as BrinkWorld;
/// Re-exported so consumers can name the decoded-transcript type and its
/// error without depending on `brink-runtime` directly.
pub use brink_runtime::transcript::{TranscriptData, TranscriptError};
/// The runtime types that appear in `bevy-brink`'s own public signatures,
/// re-exported so consumers can name them without depending on `brink-runtime`:
/// [`FlowInstance`](BrinkFlow::inner), [`Program`](crate::ProgramAsset::program),
/// [`Choice`](crate::BrinkChoicesPresented::choices), [`Line`](advance_flow)'s
/// return, [`RuntimeError`](BrinkFlow::choose)'s error,
/// [`FallbackHandler`] for the "no bindings" advance path, the scoped
/// story-state types a host needs to build a policy and a per-step routing
/// view (see `docs/scoped-flow-state-spec.md`): [`WorldPolicy`], [`Scope`],
/// [`PolicyError`], and [`ContextView`] (usually built via
/// [`flow_context_view`] instead of by hand) — and the per-entity durability
/// types produced/consumed by [`BrinkGlobals::save_state`]/`load_state` and
/// [`save_flow_state`]/[`load_flow_state`] (F6.3, see the `globals` module's
/// "Save/load" docs): [`SaveState`] and [`LoadReport`].
///
/// `World` is deliberately absent here — it collides with `bevy::prelude::World`
/// under a glob import, so it is re-exported under the alias [`BrinkWorld`].
pub use brink_runtime::{
    Choice, ContextView, FallbackHandler, FlowInstance, FlowLocal, Line, LoadReport, PolicyError,
    Program, RuntimeError, SaveState, Scope, WorldPolicy,
};
pub use brkt::{
    BrktLoader, BrktLoaderError, TranscriptAsset, capture_transcript, render_transcript_asset,
};
pub use call::{
    BrinkCallCommandsExt, BrinkCallFailed, BrinkCallRequest, BrinkCallResolved, IntoBrinkArgs,
    resolve_brink_calls,
};
#[cfg(feature = "dev")]
pub use event::BrinkFlowReset;
pub use event::{BrinkChoicesPresented, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};
pub use flow::{Advance, BrinkFlow};
pub use globals::{
    BrinkContext, BrinkGlobals, BrinkWorldPolicy, flow_context_view, load_flow_state,
    save_flow_state,
};
pub use input::digit_key_to_choice_index;
pub use line_tables::BrinkLocale;
pub use locale::{
    BrinkBaseLocale, BrinkCurrentLocale, BrinkLocaleChanged, BrinkLocaleOverride, InklLoader,
    InklLoaderError, LocaleAsset, LocalizedTablesCache, SetBrinkLocale, apply_locale_overlay,
    catch_up_loaded_locales, on_locale_changed,
};
pub use plugin::{BrinkAssetsPlugin, BrinkPlugin};
#[cfg(feature = "dev")]
pub use replay::{BrinkReplayConfig, BrinkReplayLog, ReplayQueryModeOverride, replay_on_reload};
pub use request::{BrinkFlowRequest, FlowStart, fulfill_flow_requests};
#[cfg(feature = "dev")]
pub use source_loader::{InkLoader, InkLoaderError};
pub use transcript::{BrinkTranscript, refresh_transcripts};
