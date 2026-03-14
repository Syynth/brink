//! Internationalization tooling for brink stories.
//!
//! Provides line table export for localization workflows and
//! locale overlay compilation.

pub mod align;
mod compile;
mod error;
mod export;
mod json_model;
mod regenerate;

pub use compile::compile_locale;
pub use error::IntlError;
pub use export::export_lines;
pub use json_model::{ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson};
pub use regenerate::regenerate_lines;
