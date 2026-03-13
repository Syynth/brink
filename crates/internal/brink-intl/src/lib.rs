//! Internationalization tooling for brink stories.
//!
//! Provides line table export for localization workflows.

mod error;
mod export;
mod json_model;

pub use error::IntlError;
pub use export::export_lines;
pub use json_model::{ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson};
