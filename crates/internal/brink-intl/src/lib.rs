//! Internationalization tooling for brink stories.
//!
//! Provides line table export for localization workflows and
//! locale overlay compilation.

pub mod align;

/// XLIFF 2.0 Metadata module support.
///
/// Reserved for future use: per decision-log-2026-07-26, localized display names
/// (roster-level data such as speaker and channel names) will be stored via XLIFF v2
/// metadata groups rather than per-line translation. See docs/decision-log.md entry
/// "XLIFF v1 excludes element data" for the rationale.
pub mod metadata {
    pub use xliff2::modules::metadata::*;
}
mod compile;
mod error;
mod export;
mod json_model;
pub mod plural;
mod regenerate;
mod xliff_convert;
mod xliff_ops;

pub use compile::compile_locale;
pub use error::IntlError;
pub use export::export_lines;
pub use json_model::{ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson};
pub use plural::{DefaultPluralResolver, IcuPluralResolver};
pub use regenerate::regenerate_lines;
pub use xliff_convert::{BRINK_NS, lines_json_to_xliff, migrate_unit_ids, xliff_to_lines_json};
pub use xliff_ops::{compile_locale_xliff, generate_locale, regenerate_locale};
