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
mod bindings;
mod event;
mod flow;
mod globals;
mod input;
mod line_tables;
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
/// `#[derive(BrinkCommand)]` — generates [`BrinkCommand::from_ink_args`].
/// Shares its name with the trait (macro vs. type namespace), so a single
/// `use bevy_brink::BrinkCommand;` brings both into scope.
pub use bevy_brink_derive::BrinkCommand;
pub use bindings::{
    BrinkArgError, BrinkBindings, BrinkBindingsAppExt, BrinkCallError, BrinkCommand, BrinkHandler,
    BrinkQueryInput, call_ink_function,
};
/// Re-exported so `#[derive(BrinkCommand)]`-generated code (and binding
/// authors) can name the ink runtime value type without depending on
/// `brink-format` directly.
pub use brink_format::Value;
#[cfg(feature = "dev")]
pub use event::BrinkFlowReset;
pub use event::{BrinkChoicesPresented, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};
pub use flow::BrinkFlow;
pub use globals::{BrinkContext, BrinkGlobals};
pub use input::digit_key_to_choice_index;
pub use line_tables::BrinkLocale;
pub use plugin::{BrinkAssetsPlugin, BrinkPlugin};
#[cfg(feature = "dev")]
pub use replay::{BrinkReplayLog, replay_on_reload};
pub use request::{BrinkFlowRequest, ContextSeed, FlowStart, fulfill_flow_requests};
#[cfg(feature = "dev")]
pub use source_loader::{InkLoader, InkLoaderError};
pub use transcript::{BrinkTranscript, refresh_transcripts};
