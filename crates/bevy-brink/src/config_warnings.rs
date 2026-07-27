//! Non-log surface for `with_config`'s rejected `[lints]` codes (#1426).
//!
//! `BrinkPlugin::with_config`/[`BrinkAssetsPlugin::with_config`](crate::BrinkAssetsPlugin::with_config)'s
//! `config.lints` is validated against the real `DiagnosticCode` set the same
//! way a discovered `brink.toml`'s `[lints]` table is (issue #1416): an
//! unknown code, or one whose *default* severity isn't `Warning`, is rejected
//! rather than silently applied. That rejection previously only reached a
//! `tracing::warn!` call inside `brink_environment::Project::load` — fine for
//! a real bevy app with `LogPlugin` installed, but a headless/embedding host
//! that never installs `bevy_log` (or any `tracing` subscriber) got nothing
//! at all, the silent-drop problem one layer out. [`BrinkConfigWarnings`] is
//! the programmatic counterpart: a plain resource a host can `Res`-query
//! regardless of whether anything is listening on `tracing`.
//!
//! [`BrinkAssetsPlugin::build`](crate::BrinkAssetsPlugin) inserts this once,
//! eagerly, at plugin-build time — before any asset ever loads — by running
//! the `with_config` override through the exact same
//! [`AnalysisOptions::apply_lint_overrides`] gate `Project::load` uses later
//! at compile time. The two validations are redundant by design (the
//! `tracing::warn!` channel stays, per #1426's "keep the `bevy_log` warning
//! too") and always agree: `apply_lint_overrides`'s rejections depend only on
//! the override map + `deny_warnings`, not on any prior resolution state, so
//! calling it against a scratch [`AnalysisOptions::default()`] here produces
//! byte-identical messages to the ones `Project::load` will log once a story
//! actually compiles.
//!
//! Scoped to `config.lints`/`config.deny_warnings` — a served `brink.toml`'s
//! own `[lints]` table is still only reachable through the `tracing::warn!`
//! channel, since it's discovered per-asset inside the async `InkLoader`
//! (which has no `World`/resource access to write into).

use bevy_ecs::resource::Resource;
use brink_compiler::AnalysisOptions;
use brink_project_config::ProjectConfig;

/// The formatted rejection messages from validating a `with_config`
/// override's `[lints]` table, if any were rejected.
///
/// Always present once [`BrinkAssetsPlugin`](crate::BrinkAssetsPlugin) is
/// added (even with no `with_config` override, or one with no rejected
/// codes) — an empty `Vec` means "nothing was rejected by this plugin
/// instance's own `with_config`," not necessarily "no `[lints]` override was
/// ever rejected." Message text is byte-identical to what
/// `AnalysisOptions::apply_lint_overrides` (`brink-analyzer`) produces, the
/// same wording the `tracing::warn!` channel logs.
///
/// A later `BrinkPlugin<M>::with_config` whose *entire* `ProjectConfig` is
/// ignored because `BrinkAssetsPlugin` was already added (see
/// [`BrinkPlugin::with_config`](crate::BrinkPlugin::with_config)) also
/// appends here — a distinct, single-sentence message (not the
/// per-lint-code shape above, since none of that config's fields were even
/// evaluated) pushed by `BrinkPlugin::<M>`'s own
/// [`build`](bevy_app::Plugin::build) rather than `Self::from_config`
/// below. Previously this whole class of drop reached neither this
/// resource nor `tracing::warn!` at all (the issue #1382 sweep's finding);
/// both channels now carry it.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct BrinkConfigWarnings(pub Vec<String>);

impl BrinkConfigWarnings {
    /// Validate `config`'s `lints`/`deny_warnings` against the real
    /// diagnostic-code set, the same gate `Project::load` applies at compile
    /// time. `config: None` (no `with_config` override) is the empty/no-op
    /// case — `ProjectConfig::default()`'s `lints` map is empty, so there is
    /// nothing to reject.
    pub(crate) fn from_config(config: Option<&ProjectConfig>) -> Self {
        let config = config.cloned().unwrap_or_default();
        let mut options = AnalysisOptions::default();
        let warnings = options.apply_lint_overrides(&config.lints, config.deny_warnings);
        Self(
            warnings
                .into_iter()
                .map(|warning| warning.to_string())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_config_yields_no_warnings() {
        assert_eq!(
            BrinkConfigWarnings::from_config(None),
            BrinkConfigWarnings::default()
        );
    }

    #[test]
    fn valid_lint_code_yields_no_warnings() {
        let mut lints = std::collections::BTreeMap::new();
        lints.insert("E014".to_owned(), brink_project_config::LintLevel::Deny);
        let config = ProjectConfig {
            lints,
            ..ProjectConfig::default()
        };
        assert_eq!(
            BrinkConfigWarnings::from_config(Some(&config)),
            BrinkConfigWarnings::default()
        );
    }

    #[test]
    fn unknown_code_surfaces_a_message_naming_it() {
        let mut lints = std::collections::BTreeMap::new();
        lints.insert(
            "E9999_TYPO".to_owned(),
            brink_project_config::LintLevel::Deny,
        );
        let config = ProjectConfig {
            lints,
            ..ProjectConfig::default()
        };
        let warnings = BrinkConfigWarnings::from_config(Some(&config));
        assert_eq!(warnings.0.len(), 1);
        assert!(
            warnings.0[0].contains("E9999_TYPO") && warnings.0[0].contains("not a recognized"),
            "unexpected message: {warnings:?}"
        );
    }

    #[test]
    fn non_overridable_code_surfaces_a_message_naming_it() {
        // E001's base severity is `Error`, never overridable (mirrors
        // brink-analyzer's own `apply_lint_overrides_rejects_non_overridable_code`).
        let mut lints = std::collections::BTreeMap::new();
        lints.insert("E001".to_owned(), brink_project_config::LintLevel::Deny);
        let config = ProjectConfig {
            lints,
            ..ProjectConfig::default()
        };
        let warnings = BrinkConfigWarnings::from_config(Some(&config));
        assert_eq!(warnings.0.len(), 1);
        assert!(
            warnings.0[0].contains("E001") && warnings.0[0].contains("not overridable"),
            "unexpected message: {warnings:?}"
        );
    }
}
