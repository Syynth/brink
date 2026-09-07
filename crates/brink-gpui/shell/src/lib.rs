//! The GPUI studio shell — tier 2 of `docs/gpui-studio-spec.md` §2.
//!
//! Regions, rails, docks and layout. It depends on the model and MUST NOT
//! depend on the feature crate: features implement its contracts, and the
//! concrete wiring happens once, at the top.

pub mod commands;
pub mod editor_view;
pub mod palette;
pub mod rail;
pub mod region;
pub mod settings;
pub mod settings_appearance;
pub mod settings_keymap;
pub mod settings_modal;
pub mod settings_player;
mod skin;
pub mod theme;
pub mod tool_window;
pub mod workspace;
