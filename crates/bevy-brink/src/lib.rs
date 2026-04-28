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

mod asset;
mod event;
mod flow;
mod globals;
mod line_tables;
mod plugin;
#[cfg(feature = "dev")]
mod replay;
mod request;
#[cfg(feature = "dev")]
mod source_loader;
#[cfg(test)]
mod test_support;

pub use asset::{
    BrinkProgram, BrinkStory, BrinkStoryAsset, InkbLoader, InkbLoaderError, LineTablesAsset,
    ProgramAsset,
};
pub use event::{
    BrinkChoicesPresented, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone,
};
#[cfg(feature = "dev")]
pub use event::BrinkFlowReset;
pub use flow::BrinkFlow;
pub use globals::{BrinkContext, BrinkGlobals};
pub use line_tables::BrinkLocale;
pub use plugin::{BrinkAssetsPlugin, BrinkPlugin};
#[cfg(feature = "dev")]
pub use replay::{BrinkReplayLog, replay_on_reload};
pub use request::{BrinkFlowRequest, FlowStart, fulfill_flow_requests};
#[cfg(feature = "dev")]
pub use source_loader::{InkLoader, InkLoaderError};
