//! `brink.toml` — the project settings file for dialect + type policy
//! (#1005).
//!
//! `dialect` and `types` are mount-time-only inputs to `AnalysisOptions`
//! (docs/t1b-surface-spec.md §1, docs/typed-mode-spec.md §1): never embedded
//! in `.inkb`, never delivered to the runtime. Before this crate, every
//! surface that compiles the same project — the CLI, `brink ide`, the wasm
//! editor session — picked its own default (a CLI flag here, a hardcoded
//! `setLanguageDialect` call there), so two mounts compiling the same
//! project could silently disagree about which syntax/typing surface it's
//! written in.
//!
//! This crate is the one place that:
//!
//! - **discovers** the config file — walking up from the entry `.ink` file's
//!   directory to the nearest ancestor containing [`CONFIG_FILE_NAME`]
//!   ([`discover_from_entry`], [`find_config`]);
//! - **parses** it, tolerating unknown keys as warnings rather than errors
//!   (forward compat — an older `brink` binary shouldn't choke on a
//!   `brink.toml` written for a newer schema) ([`parse_str`]);
//! - **applies** it to an [`AnalysisOptions`], honoring the precedence rule
//!   every mount must follow: **an explicit API call / CLI flag always wins
//!   over the file.** The file supplies the *default*; code wins
//!   ([`apply_to_options`]).
//!
//! A missing `brink.toml` is not an error anywhere in this crate — it means
//! "use `AnalysisOptions::default()` (or whatever the caller already had)",
//! byte-identical to pre-#1005 behavior.
//!
//! ## Schema
//!
//! ```toml
//! [project]
//! dialect = "brink"      # "brink" | "strict-ink" (default: strict-ink)
//! types   = "gradual"    # "gradual" | "strict"   (default: gradual)
//! ```
//!
//! Both keys are optional; an empty or absent `[project]` table is valid and
//! contributes nothing (`ProjectConfig::default()`).

use std::fmt;
use std::path::{Path, PathBuf};

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use thiserror::Error;
use toml::Value;

/// The config filename every mount discovers, beside the root `.ink` entry
/// file (or in an ancestor directory — see [`find_config`]).
pub const CONFIG_FILE_NAME: &str = "brink.toml";

/// The `[project]` table's recognized keys, parsed out of `brink.toml`.
/// Each field is `None` when the file doesn't set it — callers fall back to
/// `AnalysisOptions::default()` (or an explicit override), never to a
/// default invented by this crate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    /// `[project] dialect`, if set.
    pub dialect: Option<Dialect>,
    /// `[project] types`, if set.
    pub types: Option<TypePolicy>,
}

impl ProjectConfig {
    /// True if the file set neither key (an all-default/empty `[project]`
    /// table, or no `[project]` table at all).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dialect.is_none() && self.types.is_none()
    }
}

/// A recognized-but-not-understood corner of `brink.toml`: an unknown
/// top-level key, or an unknown key inside `[project]`. Never fatal —
/// forward compat (#1005): an older `brink` binary reading a `brink.toml`
/// written for a newer schema warns instead of refusing to compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning(pub String);

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A `brink.toml` that couldn't be read or parsed. Unlike [`ConfigWarning`],
/// these are genuine failures: malformed TOML syntax, or a *recognized* key
/// holding a value outside its enum (`dialect = "sideways"`) — never an
/// unrecognized key, which is always a warning.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file exists but couldn't be read (permissions, race, …).
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Malformed TOML syntax.
    #[error("invalid TOML syntax: {0}")]
    Toml(#[from] toml::de::Error),
    /// The document's root, or a table where one is expected, isn't a table.
    #[error("`{key}` must be a table, found {found}")]
    NotATable { key: String, found: &'static str },
    /// A recognized key's value has the wrong TOML type (e.g. `dialect = 1`).
    #[error("`{key}` must be a string, found {found}")]
    WrongType { key: String, found: &'static str },
    /// A recognized key's value is a string, but not one of its allowed
    /// variants (e.g. `dialect = "sideways"`).
    #[error("`{key}` must be one of {expected:?}, found {found:?}")]
    InvalidValue {
        key: String,
        expected: &'static [&'static str],
        found: String,
    },
}

/// A successfully discovered + parsed `brink.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    /// The path the config was read from (for diagnostics/logging).
    pub path: PathBuf,
    /// The parsed `[project]` table.
    pub config: ProjectConfig,
    /// Unknown-key warnings (never errors — see [`ConfigWarning`]).
    pub warnings: Vec<ConfigWarning>,
}

/// Parse `brink.toml` source text (already read, by whatever means the
/// caller has — a native `std::fs::read_to_string`, a wasm embedder's own
/// host filesystem API, …). This is the sandbox-agnostic half of the crate:
/// no filesystem access, so it's also what the wasm editor mount uses
/// (the browser sandbox has no `walk up the directory tree` of its own).
///
/// Unknown top-level keys and unknown `[project]` keys become
/// [`ConfigWarning`]s. Only malformed TOML syntax or a recognized key with
/// an invalid value is a [`ConfigError`].
pub fn parse_str(text: &str) -> Result<(ProjectConfig, Vec<ConfigWarning>), ConfigError> {
    let doc: Value = toml::from_str(text)?;
    let root = match doc {
        Value::Table(t) => t,
        other => {
            return Err(ConfigError::NotATable {
                key: "<root>".to_owned(),
                found: value_type_name(&other),
            });
        }
    };

    let mut config = ProjectConfig::default();
    let mut warnings = Vec::new();

    for (key, value) in &root {
        if key == "project" {
            let project = match value {
                Value::Table(t) => t,
                other => {
                    return Err(ConfigError::NotATable {
                        key: "project".to_owned(),
                        found: value_type_name(other),
                    });
                }
            };
            for (pkey, pvalue) in project {
                match pkey.as_str() {
                    "dialect" => config.dialect = Some(parse_dialect(pkey, pvalue)?),
                    "types" => config.types = Some(parse_types(pkey, pvalue)?),
                    _ => warnings.push(ConfigWarning(format!(
                        "unknown key `project.{pkey}` in {CONFIG_FILE_NAME} (ignored)"
                    ))),
                }
            }
        } else {
            warnings.push(ConfigWarning(format!(
                "unknown top-level key `{key}` in {CONFIG_FILE_NAME} (ignored)"
            )));
        }
    }

    Ok((config, warnings))
}

fn parse_dialect(key: &str, value: &Value) -> Result<Dialect, ConfigError> {
    let s = value.as_str().ok_or_else(|| ConfigError::WrongType {
        key: format!("project.{key}"),
        found: value_type_name(value),
    })?;
    match s {
        "brink" => Ok(Dialect::Brink),
        "strict-ink" => Ok(Dialect::StrictInk),
        other => Err(ConfigError::InvalidValue {
            key: format!("project.{key}"),
            expected: &["brink", "strict-ink"],
            found: other.to_owned(),
        }),
    }
}

fn parse_types(key: &str, value: &Value) -> Result<TypePolicy, ConfigError> {
    let s = value.as_str().ok_or_else(|| ConfigError::WrongType {
        key: format!("project.{key}"),
        found: value_type_name(value),
    })?;
    match s {
        "gradual" => Ok(TypePolicy::Gradual),
        "strict" => Ok(TypePolicy::Strict),
        other => Err(ConfigError::InvalidValue {
            key: format!("project.{key}"),
            expected: &["gradual", "strict"],
            found: other.to_owned(),
        }),
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "boolean",
        Value::Datetime(_) => "datetime",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
    }
}

/// Walk up from `start_dir` (inclusive) through every ancestor directory,
/// returning the path to the first [`CONFIG_FILE_NAME`] found. This is the
/// "walk up from the entry file to the nearest `brink.toml`" discovery rule
/// (#1005) — a project's entry `.ink` file doesn't have to sit directly
/// beside the config for every mount to find the same one.
#[must_use]
pub fn find_config(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = Some(start_dir);
    while let Some(d) = dir {
        let candidate = d.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

/// [`find_config`], starting from an entry `.ink` file's directory rather
/// than a directory directly. The common case: `brink compile story.ink`
/// discovers `brink.toml` starting from `story.ink`'s parent.
#[must_use]
pub fn discover_from_entry(entry_file: &Path) -> Option<PathBuf> {
    let start = entry_file.parent().unwrap_or_else(|| Path::new("."));
    find_config(start)
}

/// Discover (via [`discover_from_entry`]) and parse (via [`parse_str`]) the
/// `brink.toml` governing `entry_file`'s project, if one exists.
///
/// Returns `Ok(None)` — never an error — when no `brink.toml` is found
/// anywhere from `entry_file`'s directory up to the filesystem root: missing
/// file is current behavior exactly, no regression.
pub fn load_from_entry(entry_file: &Path) -> Result<Option<LoadedConfig>, ConfigError> {
    let Some(path) = discover_from_entry(entry_file) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;
    let (config, warnings) = parse_str(&text)?;
    Ok(Some(LoadedConfig {
        path,
        config,
        warnings,
    }))
}

/// Apply a parsed [`ProjectConfig`] onto an [`AnalysisOptions`], honoring
/// the #1005 precedence rule: **explicit API calls / CLI flags override the
/// file.** `dialect_overridden`/`types_overridden` tell this function
/// whether the caller already has an explicit value for that field (a CLI
/// flag the user actually passed, an editor session's own
/// `set_language_dialect`/`set_type_policy` call, …) — when true, that
/// field is left untouched regardless of what the file says. The file only
/// ever supplies a *default*.
///
/// Fields the file doesn't set are also left untouched (so `options`
/// should already carry whatever it would have without a config file —
/// typically `AnalysisOptions::default()`).
pub fn apply_to_options(
    options: &mut AnalysisOptions,
    config: &ProjectConfig,
    dialect_overridden: bool,
    types_overridden: bool,
) {
    if !dialect_overridden && let Some(dialect) = config.dialect {
        options.dialect = dialect;
    }
    if !types_overridden && let Some(types) = config.types {
        options.types = types;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_str ────────────────────────────────────────────────────

    #[test]
    fn empty_document_is_empty_config_no_warnings() {
        let (config, warnings) = parse_str("").unwrap();
        assert_eq!(config, ProjectConfig::default());
        assert!(config.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_dialect_and_types() {
        let (config, warnings) = parse_str(
            r#"
            [project]
            dialect = "brink"
            types = "strict"
            "#,
        )
        .unwrap();
        assert_eq!(config.dialect, Some(Dialect::Brink));
        assert_eq!(config.types, Some(TypePolicy::Strict));
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_strict_ink_and_gradual() {
        let (config, warnings) = parse_str(
            r#"
            [project]
            dialect = "strict-ink"
            types = "gradual"
            "#,
        )
        .unwrap();
        assert_eq!(config.dialect, Some(Dialect::StrictInk));
        assert_eq!(config.types, Some(TypePolicy::Gradual));
        assert!(warnings.is_empty());
    }

    #[test]
    fn partial_project_table_leaves_other_field_none() {
        let (config, _) = parse_str("[project]\ndialect = \"brink\"\n").unwrap();
        assert_eq!(config.dialect, Some(Dialect::Brink));
        assert_eq!(config.types, None);
    }

    #[test]
    fn unknown_top_level_key_warns_not_errors() {
        let (config, warnings) = parse_str("future_section = 1\n").unwrap();
        assert!(config.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("future_section"));
    }

    #[test]
    fn unknown_project_key_warns_not_errors() {
        let (config, warnings) =
            parse_str("[project]\ndialect = \"brink\"\nfuture_key = \"x\"\n").unwrap();
        assert_eq!(config.dialect, Some(Dialect::Brink));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("project.future_key"));
    }

    #[test]
    fn invalid_dialect_value_is_an_error() {
        let err = parse_str("[project]\ndialect = \"sideways\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    #[test]
    fn invalid_types_value_is_an_error() {
        let err = parse_str("[project]\ntypes = \"loose\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    #[test]
    fn wrong_type_value_is_an_error() {
        let err = parse_str("[project]\ndialect = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    #[test]
    fn malformed_toml_is_an_error() {
        let err = parse_str("this is not [ toml").unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    #[test]
    fn non_table_root_is_an_error() {
        let err = parse_str("\"just a string\"").unwrap_err();
        assert!(matches!(
            err,
            ConfigError::NotATable { .. } | ConfigError::Toml(_)
        ));
    }

    // ── apply_to_options ─────────────────────────────────────────────

    #[test]
    fn apply_sets_unset_fields_from_config() {
        let mut options = AnalysisOptions::default();
        let config = ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Strict),
        };
        apply_to_options(&mut options, &config, false, false);
        assert_eq!(options.dialect, Dialect::Brink);
        assert_eq!(options.types, TypePolicy::Strict);
    }

    #[test]
    fn apply_leaves_overridden_fields_alone() {
        let mut options = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: TypePolicy::Gradual,
            ..AnalysisOptions::default()
        };
        let config = ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Strict),
        };
        // Both overridden: explicit calls win, file is ignored entirely.
        apply_to_options(&mut options, &config, true, true);
        assert_eq!(options.dialect, Dialect::StrictInk);
        assert_eq!(options.types, TypePolicy::Gradual);
    }

    #[test]
    fn apply_mixed_override_only_touches_non_overridden_field() {
        let mut options = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: TypePolicy::Gradual,
            ..AnalysisOptions::default()
        };
        let config = ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Strict),
        };
        // dialect explicitly overridden (stays StrictInk); types is not
        // (file wins, becomes Strict).
        apply_to_options(&mut options, &config, true, false);
        assert_eq!(options.dialect, Dialect::StrictInk);
        assert_eq!(options.types, TypePolicy::Strict);
    }

    #[test]
    fn apply_with_no_config_values_leaves_options_untouched() {
        let mut options = AnalysisOptions {
            dialect: Dialect::Brink,
            types: TypePolicy::Strict,
            ..AnalysisOptions::default()
        };
        apply_to_options(&mut options, &ProjectConfig::default(), false, false);
        assert_eq!(options.dialect, Dialect::Brink);
        assert_eq!(options.types, TypePolicy::Strict);
    }

    // ── discovery ─────────────────────────────────────────────────────

    fn unique_tmp_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "brink-project-config-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        dir
    }

    #[test]
    fn find_config_walks_up_from_start_dir() {
        let root = unique_tmp_dir("walk-up");
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        let found = find_config(&nested).expect("should find brink.toml in an ancestor");
        assert_eq!(found, root.join(CONFIG_FILE_NAME));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_config_returns_none_when_absent() {
        let root = unique_tmp_dir("absent");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(find_config(&root), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn discover_from_entry_starts_at_entry_parent() {
        let root = unique_tmp_dir("entry-parent");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ntypes = \"strict\"\n",
        )
        .unwrap();
        let entry = root.join("story.ink");
        std::fs::write(&entry, "content").unwrap();

        let found = discover_from_entry(&entry).expect("should find brink.toml beside entry");
        assert_eq!(found, root.join(CONFIG_FILE_NAME));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_from_entry_none_when_no_config() {
        let root = unique_tmp_dir("load-none");
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("story.ink");
        std::fs::write(&entry, "content").unwrap();

        assert!(load_from_entry(&entry).unwrap().is_none());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_from_entry_reads_and_parses() {
        let root = unique_tmp_dir("load-some");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\ntypes = \"strict\"\n",
        )
        .unwrap();
        let entry = root.join("story.ink");
        std::fs::write(&entry, "content").unwrap();

        let loaded = load_from_entry(&entry).unwrap().expect("config found");
        assert_eq!(loaded.path, root.join(CONFIG_FILE_NAME));
        assert_eq!(loaded.config.dialect, Some(Dialect::Brink));
        assert_eq!(loaded.config.types, Some(TypePolicy::Strict));
        assert!(loaded.warnings.is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
