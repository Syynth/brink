//! `brink.toml` applied to a session.
//!
//! Applying a project config is not a wasm concern: every consumer opens a
//! project, discovers a `brink.toml`, and has to resolve the same
//! precedence from it. Keeping the application in the wasm wrapper meant a
//! native host had to re-derive it — which the GPUI spike did, and got a
//! thinner version of (see the decision log, "Both studio consumers sit on
//! the same layer").
//!
//! What stays with the host is the part that is genuinely theirs:
//! **discovery** (which file, read how) and the **CLI/API override tier**
//! that outranks the file (`set_lint_overrides` and friends). This module
//! owns the middle: given a parsed config, resolve it onto the session.

use std::collections::BTreeMap;

use brink_project_config::{FixPolicy, ProjectConfig, ProseDialect};

use crate::session::IdeSession;

/// The settings a `brink.toml` resolved to, as a consumer reads them back.
///
/// Every field is **wholesale-replaced** on each application: a key removed
/// from the file between two calls must stop applying rather than stay stuck
/// at a stale value. That is why these are settings rather than options —
/// there is no "unset means untouched" tier here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectSettings {
    /// `[project] entry` (#2331) — the file compilation starts from.
    pub entry: Option<String>,
    /// `[project] indent` (#3149).
    pub indent: Option<u8>,
    /// `[prose] dialect` (#3211).
    pub prose_dialect: Option<ProseDialect>,
    /// `[prose] enable` (#3211).
    pub prose_enable: Option<bool>,
    /// `[prose] dictionary` (#3211).
    pub prose_dictionary: Vec<String>,
    /// `[fix]` per-code policy (#3419/#3420).
    pub fix: BTreeMap<String, FixPolicy>,
    /// The resolved `[dialogue]` dialect (#3387), or `None` when the file
    /// declares none — in which case nothing is registered, deliberately.
    pub dialogue: Option<brink_ir::DialogueDialect>,
    /// Why `[dialogue]` failed to resolve, if it did. Loud, never silent:
    /// the previous dialect is dropped rather than left stale under a config
    /// the author is actively editing.
    pub dialogue_error: Option<String>,
}

impl IdeSession {
    /// Apply a parsed `brink.toml` to this session, returning its warnings.
    ///
    /// `dialect_explicit`/`types_explicit` carry the host's own precedence
    /// tier (#1005): an explicit `set_language_dialect`/`set_type_policy`
    /// call always wins over the file, so when either is true the registered
    /// value is kept and the file supplies nothing. When false the file
    /// supplies a *default* — its value if it sets one, else whatever was
    /// already registered.
    ///
    /// `config_dir` is the directory the config was discovered in, for the
    /// `[dialogue]` artifact escape hatch (`dialogue = "path.json"`), which
    /// resolves relative to the file that named it.
    ///
    /// The CLI/API lint-override tier is **not** applied here — it outranks
    /// the file, so a host that has one reapplies it after this returns.
    pub fn apply_project_config(
        &mut self,
        config: &ProjectConfig,
        dialect_explicit: bool,
        types_explicit: bool,
        config_dir: Option<&str>,
    ) -> Vec<String> {
        self.apply_project_config_with_reader(
            config,
            dialect_explicit,
            types_explicit,
            config_dir,
            &|_| None,
        )
    }

    /// [`Self::apply_project_config`] with a second place to find the
    /// `[dialogue]` artifact file: `read_file` is asked, by the path as
    /// written (and prefixed with `config_dir`), for a file the session's
    /// own document tree does not hold. A host whose session holds only
    /// story sources — the native studio's analysis worker — serves
    /// `dialect.json` from the project directory through it; the web
    /// studio's session holds every file and passes nothing.
    pub fn apply_project_config_with_reader(
        &mut self,
        config: &ProjectConfig,
        dialect_explicit: bool,
        types_explicit: bool,
        config_dir: Option<&str>,
        read_file: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<String> {
        // `apply_project_config` replaces `.lints` wholesale from `config`
        // (issue #1397), so a throwaway `AnalysisOptions::default()` is
        // enough here — `dialect`/`types` are resolved separately below, so
        // passing `true` for both touches nothing but `.lints`/`.conventions`.
        let mut lint_options = brink_analyzer::AnalysisOptions::default();
        let lint_warnings = lint_options.apply_project_config(config, true, true);
        self.set_file_lint_policy(lint_options.lints.clone());

        // `type_policy_override()` (not the dialect-keyed *effective*
        // value) is read here so an unset override round-trips as `None`
        // rather than being frozen into an explicit choice.
        let resolved = brink_analyzer::AnalysisOptions {
            dialect: if dialect_explicit {
                self.language_dialect()
            } else {
                config.dialect.unwrap_or_else(|| self.language_dialect())
            },
            types: if types_explicit {
                self.type_policy_override()
            } else {
                config.types.or_else(|| self.type_policy_override())
            },
            // Deliberately the session's own current value, never
            // `lint_options.lints` — this field must not trip
            // `apply_analysis_options`'s change guard. The override tier is
            // the one place that pushes lints into the session.
            lints: self.lint_policy().clone(),
            conventions: lint_options.conventions,
            host_manifest: None,
            external_check: brink_analyzer::ExternalCheckSeverity::default(),
            semantic_type_check: brink_analyzer::SemanticTypeDiagnosticSeverity::default(),
            // D6/#3229: the session's own value, NOT a hardcoded `false`.
            // `apply_analysis_options` ignores this field entirely, so what
            // is written changes nothing — but writing `false` would tell
            // the next reader that re-reading `brink.toml` turns a live
            // debug session's compiles back off.
            emit_debug_info: self.emit_debug_info(),
        };
        self.apply_analysis_options(&resolved);

        self.settings.entry.clone_from(&config.entry);
        self.set_draft_globs(config.drafts.clone());
        self.settings.indent = config.indent;
        self.settings.prose_dialect = config.prose_dialect;
        self.settings.fix.clone_from(&config.fix);
        self.settings.prose_enable = config.prose_enable;
        self.settings
            .prose_dictionary
            .clone_from(&config.prose_dictionary);

        let mut warnings: Vec<String> = Vec::new();
        self.apply_dialogue_config(config, config_dir, read_file, &mut warnings);
        warnings.extend(lint_warnings.into_iter().map(|w| w.0));
        warnings
    }

    /// `[dialogue]` (#3387, RULED): resolve and register — or, absent,
    /// register NOTHING. "The project file wins": a mount-time embedder
    /// option only ever fills in for a project that declares nothing.
    fn apply_dialogue_config(
        &mut self,
        config: &ProjectConfig,
        config_dir: Option<&str>,
        extra: &dyn Fn(&str) -> Option<String>,
        warnings: &mut Vec<String>,
    ) {
        let Some(dialogue) = config.dialogue.as_ref() else {
            self.clear_dialect();
            self.settings.dialogue = None;
            self.settings.dialogue_error = None;
            return;
        };

        let db = self.db();
        let read_file = |path: &str| -> Option<String> {
            let candidates = match config_dir {
                Some("") | None => vec![path.to_owned()],
                Some(dir) => vec![format!("{dir}/{path}"), path.to_owned()],
            };
            candidates
                .iter()
                .find_map(|key| {
                    db.file_ids().find_map(|id| {
                        (db.file_path(id)? == key).then(|| db.source(id).map(str::to_owned))?
                    })
                })
                .or_else(|| candidates.iter().find_map(|key| extra(key)))
        };
        match crate::dialect_config::resolve_dialogue_config(dialogue, &read_file) {
            Ok(dialect) => {
                self.set_dialect_config(dialect.clone());
                self.settings.dialogue = Some(dialect);
                self.settings.dialogue_error = None;
            }
            Err(message) => {
                warnings.push(format!("[dialogue]: {message}"));
                self.clear_dialect();
                self.settings.dialogue = None;
                self.settings.dialogue_error = Some(message);
            }
        }
    }

    /// Forget everything a `brink.toml` had set — for a host whose config
    /// file vanished or moved out of reach.
    ///
    /// A stale `entry` is the reason this is not "leave the defaults alone":
    /// it silently repoints compilation at a file the current tree no longer
    /// names.
    pub fn clear_project_config(&mut self) {
        self.settings = ProjectSettings::default();
        self.set_draft_globs(Vec::new());
        self.clear_dialect();
    }

    /// The settings the last applied `brink.toml` resolved to.
    #[must_use]
    pub fn project_settings(&self) -> &ProjectSettings {
        &self.settings
    }
}
