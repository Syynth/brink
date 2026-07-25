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
//! - defines the two policy enums the file can set ([`Dialect`],
//!   [`TypePolicy`]); applying them to an `AnalysisOptions` lives in
//!   `brink-analyzer` (`AnalysisOptions::apply_project_config`), honoring
//!   the precedence rule
//!   every mount must follow: **an explicit API call / CLI flag always wins
//!   over the file.** The file supplies the *default*; code wins
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
//! types   = "strict"     # "gradual" | "strict"   (default: dialect-keyed —
//!                        # brink → strict, strict-ink → gradual; issue
//!                        # #1127, ruled 2026-07-19)
//!
//! [lints]
//! deny-warnings = true   # promote every Warning-severity diagnostic to
//!                        # Error (the `-D warnings` equivalent; issue #1160)
//! E014 = "deny"          # per-code severity override: "allow" | "warn" | "deny"
//! ```
//!
//! (`E014` — a plainly `Warning`-by-default code — is used here rather than
//! `E063`: `E063`'s own *base* severity is `types`-policy-dependent (`Error`
//! under `types = strict`, see `brink_analyzer::effective_severity`'s doc
//! comment), so it makes a confusing flagship example — under `types =
//! strict` a `[lints]` entry for it is never even consulted.)
//!
//! Every key is optional; an empty or absent `[project]`/`[lints]` table is
//! valid and contributes nothing (`ProjectConfig::default()`).
//!
//! `[lints]` is shaped like Rust's own `[lints]` table (issue #1160) but is
//! **not** a drop-in semantic match: each key other than the reserved
//! `deny-warnings` is taken as a diagnostic code (`"E014"`) mapped to a
//! [`LintLevel`], and `Deny`/`Warn` behave as their Rust namesakes suggest —
//! but `Allow` does not *remove* the diagnostic the way Rust's `allow`
//! does. `LintLevel::Allow` only buys immunity from `deny-warnings`; the
//! diagnostic still resolves to `Severity::Warning` and is still reported
//! (`brink_analyzer::effective_severity`'s doc comment, step 3). An author
//! who wants a code gone entirely wants `brink_ir::suppressions`
//! (`//brink-disable`), a different, per-site mechanism — not `[lints]`.
//!
//! This crate does not know the closed set of real `DiagnosticCode`s
//! (keeping it dependency-free, #1234), so it accepts any key here without
//! validation — resolving a key against the real code set, and deciding
//! which codes are actually overridable (a `Warning`-base-severity code
//! only — see `effective_severity`'s hard-error exemption), is
//! `AnalysisOptions::apply_project_config`'s job in `brink-analyzer` (which
//! owns `DiagnosticCode`): an unknown or non-overridable key is never
//! merged into the resolved policy, and is surfaced back to the caller as a
//! [`ConfigWarning`]-shaped string through that function's return value —
//! the same "warn, never silently drop" channel this crate's own unknown-key
//! warnings use.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use brink_source_tree::SourceTree;

/// Compiler dialect: gates T1b brink-extension syntax. Default `StrictInk` —
/// divergence from the oracle-anchored ink subset is a visible, one-time,
/// per-project choice (docs/t1b-surface-spec.md §1).
///
/// Defined here rather than in `brink-analyzer` because it is a
/// **project-policy** type: the analyzer consumes it, this crate parses it,
/// and owning it here is what keeps this crate free of workspace
/// dependencies (#1234). `brink-analyzer` re-exports it, so
/// `brink_analyzer::Dialect` remains the canonical path for consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Dialect {
    #[default]
    StrictInk,
    Brink,
}

/// `types` project policy (docs/typed-mode-spec.md §1). `Gradual` is the
/// pre-flip behavior — `Unknown` unifies with anything, annotations are
/// optional seasoning, and the strict checks do not run. `Strict` requires
/// `dialect = brink`.
///
/// The *default* is dialect-keyed since the 2026-07-19 "Typing posture
/// ruled" decision (issue #1127) — see `brink_analyzer::resolve_type_policy`.
/// The derived `Default` (`Gradual`) exists only so pre-resolution containers
/// can derive theirs; policy defaulting must never read it directly.
///
/// Defined here for the same reason as [`Dialect`], and re-exported by
/// `brink-analyzer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TypePolicy {
    #[default]
    Gradual,
    Strict,
}

/// A `[lints]` table entry's severity (issue #1160) — mirrors Rust's own
/// `[lints]` levels. `Warn` is every diagnostic code's implicit level when
/// `[lints]` doesn't mention it, so it doubles as this type's `Default`.
///
/// Defined here for the same reason as [`Dialect`]/[`TypePolicy`]: a
/// project-policy type this crate parses but doesn't interpret, kept
/// dependency-free (#1234) and re-exported by `brink-analyzer`, which owns
/// applying it against the real `DiagnosticCode` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LintLevel {
    /// Never escalate this code past `Warning`, even under `deny-warnings`.
    Allow,
    /// The code's ordinary behavior: `Warning`, promoted to `Error` by
    /// `deny-warnings` like any other unconfigured warning.
    #[default]
    Warn,
    /// Always `Error`, regardless of `deny-warnings`.
    Deny,
}
use thiserror::Error;
use toml::Value;

/// The config filename every mount discovers, beside the root `.ink` entry
/// file (or in an ancestor directory — see [`find_config`]).
pub const CONFIG_FILE_NAME: &str = "brink.toml";

/// The `[project]`/`[lints]` tables' recognized keys, parsed out of
/// `brink.toml`. `dialect`/`types` are `None` when the file doesn't set
/// them — callers fall back to `AnalysisOptions::default()` (or an explicit
/// override), never to a default invented by this crate. `lints`/
/// `deny_warnings` follow the same "unset means untouched" rule: an empty
/// `lints` map and a `None` `deny_warnings` both mean "`[lints]` didn't say,
/// leave whatever the caller already had."
///
/// No longer `Copy` (issue #1160): `lints` is a `BTreeMap`, which isn't
/// `Copy`. Every construction site now needs `.clone()` where it used to
/// rely on an implicit copy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    /// `[project] dialect`, if set.
    pub dialect: Option<Dialect>,
    /// `[project] types`, if set.
    pub types: Option<TypePolicy>,
    /// `[lints]` per-code severity overrides, keyed by the raw code string
    /// as written in the file (e.g. `"E063"`) — this crate doesn't validate
    /// codes against the real `DiagnosticCode` set (#1234 dependency-free
    /// constraint); resolving unknown/non-overridable codes is
    /// `brink-analyzer`'s job. Sorted (`BTreeMap`) for deterministic
    /// iteration.
    pub lints: BTreeMap<String, LintLevel>,
    /// `[lints] deny-warnings`, if set.
    pub deny_warnings: Option<bool>,
}

impl ProjectConfig {
    /// True if the file set nothing at all (an all-default/empty
    /// `[project]`/`[lints]` table, or neither table present).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dialect.is_none()
            && self.types.is_none()
            && self.lints.is_empty()
            && self.deny_warnings.is_none()
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
        } else if key == "lints" {
            let lints = match value {
                Value::Table(t) => t,
                other => {
                    return Err(ConfigError::NotATable {
                        key: "lints".to_owned(),
                        found: value_type_name(other),
                    });
                }
            };
            for (lkey, lvalue) in lints {
                if lkey == "deny-warnings" {
                    config.deny_warnings = Some(parse_deny_warnings(lkey, lvalue)?);
                } else {
                    config
                        .lints
                        .insert(lkey.clone(), parse_lint_level(lkey, lvalue)?);
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

fn parse_deny_warnings(key: &str, value: &Value) -> Result<bool, ConfigError> {
    value.as_bool().ok_or_else(|| ConfigError::WrongType {
        key: format!("lints.{key}"),
        found: value_type_name(value),
    })
}

fn parse_lint_level(key: &str, value: &Value) -> Result<LintLevel, ConfigError> {
    let s = value.as_str().ok_or_else(|| ConfigError::WrongType {
        key: format!("lints.{key}"),
        found: value_type_name(value),
    })?;
    match s {
        "allow" => Ok(LintLevel::Allow),
        "warn" => Ok(LintLevel::Warn),
        "deny" => Ok(LintLevel::Deny),
        other => Err(ConfigError::InvalidValue {
            key: format!("lints.{key}"),
            expected: &["allow", "warn", "deny"],
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

/// [`find_config`], but discovering over a [`SourceTree`] rather than the
/// real filesystem (#1312) — mount-agnostic: the same walk-up rule serves
/// the CLI's `RealFs` mount, a wasm sandbox's `InMemory` mount, a git
/// baseline's `GitRev` mount, or any future host, with no per-mount
/// discovery code duplicated outside the seam.
///
/// `start_key` is a root-relative directory key (`""` for the tree root
/// itself), in the same forward-slash-joined form [`SourceTree::list`]
/// returns. Walks `start_key` and every ancestor, closest first, probing
/// whether `{ancestor}/brink.toml` (bare `brink.toml` at the tree root)
/// exists via a direct [`SourceTree::read`] of each candidate key — the
/// tree-relative analog of [`find_config`]'s `Path::is_file` check at each
/// `Path::parent`.
///
/// This is an O(depth) probe, **not** a tree enumeration: unlike an earlier
/// version of this function, it never calls [`SourceTree::list`] (issue
/// #1370 — a full recursive tree walk, including `target/`/`.git`/
/// `node_modules`, just to test a handful of ancestor candidates was the
/// same waste #1357 removed from the CLI drain, relocated here). A `read`
/// that fails with [`io::ErrorKind::NotFound`] means "no `brink.toml` at
/// this candidate, keep walking up"; any other error kind (permission
/// denied, invalid encoding, ...) means a `brink.toml` *exists* at this
/// candidate but this probe couldn't read it — treated as "found" (returns
/// `Some(candidate)`) rather than propagated, so the caller's own
/// [`SourceTree::read`] of the returned key is what actually surfaces the
/// failure, with the path correctly attributed (see `brink-environment`'s
/// `LoadError::ConfigRead`, #1369). Propagating this probe's own read error
/// instead would report the same failure without a path — issue #1370's
/// fix regressed exactly that for a moment before this doc/behavior was
/// tightened; `tree`'s own [`SourceTree::read`] already resolves keys
/// against whatever root the tree was constructed with, so this function
/// needs no enumeration to know where to look.
///
/// `root` is accepted (and unused) only to keep this function's shape
/// symmetric with [`SourceTree::list`]'s signature and preserve source
/// compatibility for existing callers; every current [`SourceTree`]
/// implementation resolves `read` keys against its own stored root, not
/// against a `root` supplied per call.
///
/// Returns the matching key, not file content — callers read it via
/// [`SourceTree::read`] (mirroring how [`find_config`] returns a path the
/// caller reads via `std::fs`, not file content).
pub fn find_config_in_tree(
    tree: &dyn SourceTree,
    _root: &Path,
    start_key: &str,
) -> io::Result<Option<String>> {
    let mut dir = start_key.trim_matches('/');
    loop {
        let candidate = if dir.is_empty() {
            CONFIG_FILE_NAME.to_owned()
        } else {
            format!("{dir}/{CONFIG_FILE_NAME}")
        };
        match tree.read(&candidate) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            // Found: either the read actually succeeded, or it failed with
            // some other error kind — which, under the `SourceTree::read`
            // contract, implies the candidate exists but this probe read
            // just couldn't consume it. Report it as found either way; the
            // caller's own read of the same key is what turns a probe-read
            // failure into a path-attributed error (`LoadError::ConfigRead`)
            // instead of a bare, pathless one.
            Ok(_) | Err(_) => return Ok(Some(candidate)),
        }
        if dir.is_empty() {
            return Ok(None);
        }
        dir = match dir.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "",
        };
    }
}

/// [`find_config_in_tree`], starting from an entry `.brink`/`.ink` file's
/// root-relative key rather than a directory key directly — the
/// [`SourceTree`] analog of [`discover_from_entry`].
pub fn discover_from_entry_in_tree(
    tree: &dyn SourceTree,
    root: &Path,
    entry_key: &str,
) -> io::Result<Option<String>> {
    let start = match entry_key.trim_matches('/').rsplit_once('/') {
        Some((parent, _)) => parent,
        None => "",
    };
    find_config_in_tree(tree, root, start)
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

    // ── [lints] ──────────────────────────────────────────────────────

    #[test]
    fn parses_per_code_lint_levels() {
        let (config, warnings) = parse_str(
            r#"
            [lints]
            E063 = "deny"
            E014 = "allow"
            E022 = "warn"
            "#,
        )
        .unwrap();
        assert_eq!(config.lints.get("E063"), Some(&LintLevel::Deny));
        assert_eq!(config.lints.get("E014"), Some(&LintLevel::Allow));
        assert_eq!(config.lints.get("E022"), Some(&LintLevel::Warn));
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_deny_warnings_flag() {
        let (config, _) = parse_str("[lints]\ndeny-warnings = true\n").unwrap();
        assert_eq!(config.deny_warnings, Some(true));
    }

    #[test]
    fn deny_warnings_and_codes_coexist() {
        let (config, _) = parse_str(
            r#"
            [lints]
            deny-warnings = true
            E063 = "allow"
            "#,
        )
        .unwrap();
        assert_eq!(config.deny_warnings, Some(true));
        assert_eq!(config.lints.get("E063"), Some(&LintLevel::Allow));
    }

    #[test]
    fn absent_lints_table_is_empty_config() {
        let (config, _) = parse_str("[project]\ndialect = \"brink\"\n").unwrap();
        assert!(config.lints.is_empty());
        assert_eq!(config.deny_warnings, None);
    }

    #[test]
    fn invalid_lint_level_value_is_an_error() {
        let err = parse_str("[lints]\nE063 = \"sideways\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    #[test]
    fn wrong_type_deny_warnings_is_an_error() {
        let err = parse_str("[lints]\ndeny-warnings = \"yes\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    #[test]
    fn wrong_type_lint_level_is_an_error() {
        let err = parse_str("[lints]\nE063 = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    #[test]
    fn non_table_lints_is_an_error() {
        let err = parse_str("lints = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::NotATable { .. }));
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
    fn find_config_in_tree_walks_up_from_start_key() {
        use brink_source_tree::InMemory;
        use std::collections::BTreeMap;

        let mut files = BTreeMap::new();
        files.insert(
            CONFIG_FILE_NAME.to_owned(),
            "[project]\ndialect = \"brink\"\n".to_owned(),
        );
        files.insert("a/b/story.ink".to_owned(), "content".to_owned());
        let tree = InMemory::new(files);

        let found = find_config_in_tree(&tree, Path::new("."), "a/b")
            .expect("list succeeds")
            .expect("should find brink.toml in an ancestor key");
        assert_eq!(found, CONFIG_FILE_NAME);
    }

    #[test]
    fn find_config_in_tree_returns_none_when_absent() {
        use brink_source_tree::InMemory;
        use std::collections::BTreeMap;

        let mut files = BTreeMap::new();
        files.insert("a/b/story.ink".to_owned(), "content".to_owned());
        let tree = InMemory::new(files);

        let found = find_config_in_tree(&tree, Path::new("."), "a/b").expect("list succeeds");
        assert_eq!(found, None);
    }

    /// A `SourceTree` whose `list` errors out — proves `find_config_in_tree`
    /// resolves purely via direct `read` probes of the O(depth) ancestor
    /// candidates and never falls back to enumerating the tree (issue
    /// #1370): if it ever called `list`, that error would propagate and the
    /// test's `.expect(..)` calls below would fail. Seeded with a huge,
    /// irrelevant key set (standing in for `target/`/`.git`/`node_modules`
    /// clutter a real tree walk would have to traverse) that a `list`-based
    /// implementation would have to comb through but a `read`-probing one
    /// never touches.
    struct ErrorsOnList {
        files: BTreeMap<String, String>,
    }

    impl SourceTree for ErrorsOnList {
        fn list(&self, _root: &Path) -> io::Result<Vec<String>> {
            Err(io::Error::other(
                "find_config_in_tree must not enumerate the tree via SourceTree::list (issue #1370)",
            ))
        }

        fn read(&self, key: &str) -> io::Result<String> {
            self.files
                .get(key)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{key}: not found")))
        }
    }

    #[test]
    fn find_config_in_tree_probes_directly_without_enumerating_the_tree() {
        let mut files = BTreeMap::new();
        files.insert(
            CONFIG_FILE_NAME.to_owned(),
            "[project]\ndialect = \"brink\"\n".to_owned(),
        );
        for i in 0..10_000 {
            files.insert(format!("target/build-artifact-{i}.o"), "ignored".to_owned());
        }
        let tree = ErrorsOnList { files };

        let found = find_config_in_tree(&tree, Path::new("."), "a/b/c/d")
            .expect("direct probing succeeds without ever calling list")
            .expect("should find brink.toml at the tree root");
        assert_eq!(found, CONFIG_FILE_NAME);
    }

    #[test]
    fn find_config_in_tree_probes_directly_returns_none_without_enumerating_the_tree() {
        let mut files = BTreeMap::new();
        for i in 0..10_000 {
            files.insert(format!(".git/objects/{i}"), "ignored".to_owned());
        }
        let tree = ErrorsOnList { files };

        let found = find_config_in_tree(&tree, Path::new("."), "a/b/c/d")
            .expect("direct probing succeeds without ever calling list");
        assert_eq!(found, None);
    }

    /// A `SourceTree` whose `brink.toml` candidate exists but errors on
    /// `read` with a non-`NotFound` kind (e.g. invalid encoding, permission
    /// denied) — must be reported as *found* (`Some(candidate)`), not
    /// propagated as an `Err` from `find_config_in_tree` itself. Regression
    /// guard for the #1370/#1369 interaction: `find_config_in_tree`'s probe
    /// read used to propagate this error directly, which — since it carries
    /// no path — surfaced to callers as a bare `LoadError::Io` instead of
    /// the path-attributed `LoadError::ConfigRead` the caller's own `read`
    /// of the returned key is meant to produce.
    struct ErrorsOnRead;

    impl SourceTree for ErrorsOnRead {
        fn list(&self, _root: &Path) -> io::Result<Vec<String>> {
            Ok(vec![CONFIG_FILE_NAME.to_owned()])
        }

        fn read(&self, key: &str) -> io::Result<String> {
            if key == CONFIG_FILE_NAME {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "not valid utf-8",
                ))
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{key}: not found"),
                ))
            }
        }
    }

    #[test]
    fn find_config_in_tree_reports_found_when_the_candidate_read_errors_non_not_found() {
        let found = find_config_in_tree(&ErrorsOnRead, Path::new("."), "a/b")
            .expect("a non-NotFound read error is not propagated")
            .expect("the unreadable brink.toml is still reported as found");
        assert_eq!(found, CONFIG_FILE_NAME);
    }

    #[test]
    fn discover_from_entry_in_tree_starts_at_entry_parent_key() {
        use brink_source_tree::InMemory;
        use std::collections::BTreeMap;

        let mut files = BTreeMap::new();
        files.insert(
            CONFIG_FILE_NAME.to_owned(),
            "[project]\ntypes = \"strict\"\n".to_owned(),
        );
        files.insert("story.ink".to_owned(), "content".to_owned());
        let tree = InMemory::new(files);

        let found = discover_from_entry_in_tree(&tree, Path::new("."), "story.ink")
            .expect("list succeeds")
            .expect("should find brink.toml beside entry key");
        assert_eq!(found, CONFIG_FILE_NAME);
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
