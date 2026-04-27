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
mod request;
#[cfg(feature = "dev")]
mod source_loader;
mod system;

pub use asset::{
    BrinkProgram, BrinkStoryAsset, InitError, InitialGlobalsAsset, InkLoaderSettings, InkbLoader,
    InkbLoaderError, LineTablesAsset, ProgramAsset,
};
pub use event::BrinkLineMessage;
pub use flow::BrinkFlow;
pub use globals::BrinkGlobals;
pub use line_tables::BrinkLineTables;
pub use plugin::{BrinkAssetsPlugin, BrinkPlugin};
pub use request::{BrinkFlowRequest, FlowStart, fulfill_flow_requests};
#[cfg(feature = "dev")]
pub use source_loader::{InkLoader, InkLoaderError};
pub use system::advance_flows;
