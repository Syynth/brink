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
//!   directory to the nearest ancestor containing [`CONFIG_FILE_NAME`],
//!   bounded at a workspace/git boundary (#1425) so the walk can never
//!   escape the project and pick up an unrelated `brink.toml` far above it,
//!   and **also** bounded by a fixed ancestor-depth cap
//!   ([`MAX_ANCESTOR_DEPTH`]) so a VCS-less project — no `.git` boundary to
//!   stop at — still can't climb all the way to the filesystem root (#1435)
//!   ([`discover_from_entry`], [`find_config`]). A `brink.toml` the bounded
//!   walk steps over is never silently dropped: [`find_config_with_warnings`]
//!   reports it back as a [`ConfigWarning`] instead;
//! - **parses** it, tolerating unknown keys as warnings rather than errors
//!   (forward compat — an older `brink` binary shouldn't choke on a
//!   `brink.toml` written for a newer schema) ([`parse_str`],
//!   [`parse_str_at`] — the latter threads the discovered path into every
//!   [`ConfigError`] it raises, #1384);
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
//! E014 = "deny"          # per-code severity override:
//!                        # "allow" | "warn" | "deny" | "info" | "hint"
//!                        # ("info"/"hint" down-level to the advisory tiers
//!                        # below Warning, issue #1162)
//! ```
//!
//! ```toml
//! [project]
//! unprune-dirs = ["node_modules"]  # directory names discovery must NOT
//!                                  # prune, on top of the standing
//!                                  # `target`/`.git`/`node_modules` policy
//!                                  # (issue #1407's escape hatch — see
//!                                  # `brink_source_tree::Walk::allow`). A
//!                                  # name that isn't one of those three is
//!                                  # a no-op (there was nothing to
//!                                  # un-prune) and warns.
//! ```
//!
//! ```toml
//! [project]
//! conventions = "conventions.brink"  # docs/prose-dialect-spec.md §3.4: a
//!                                    # built-in preset name ("screenplay")
//!                                    # or a project-relative path to a
//!                                    # `.brink` conventions module. Names
//!                                    # the ONE file a pattern-claiming
//!                                    # `@[convention(claims = "…", order =
//!                                    # N)]` handler may be declared in
//!                                    # (issue #1844's confinement rule,
//!                                    # `E169` elsewhere) — unset means no
//!                                    # conventions module is configured, so
//!                                    # nothing is enforced yet.
//!                                    #
//!                                    # `elements` is a DEPRECATED alias for
//!                                    # this key (issue #2180: the key
//!                                    # predates the split of `@[element]`
//!                                    # from `@[convention]` and now names a
//!                                    # module of the latter, not the
//!                                    # former). Setting `elements` still
//!                                    # works but warns; setting both keys
//!                                    # prefers `conventions` and warns
//!                                    # about the conflict. The alias will
//!                                    # be removed in a future release —
//!                                    # migrate to `conventions`.
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

use brink_source_tree::{IGNORED_DIR_NAMES, SourceTree};

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
    /// Down-level to `Severity::Info` (issue #1162) — an advisory tier below
    /// `Warning`, immune to `deny-warnings` like `Allow` (escalating an
    /// author's deliberate downgrade back up would defeat the point of it).
    Info,
    /// Down-level to `Severity::Hint` (issue #1162) — the quietest tier,
    /// immune to `deny-warnings` for the same reason as `Info`. The IDE-
    /// convention use case this exists for (e.g. unused-symbol dimming) is
    /// exactly the case where even an `Info` squiggle is too loud.
    Hint,
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
    /// `[project] unprune-dirs`, if set: directory names discovery must not
    /// prune, layered on top of the standing
    /// [`brink_source_tree::IGNORED_DIR_NAMES`] policy (issue #1407's escape
    /// hatch). Empty (the default) means "the standing policy applies with
    /// no override" — same "unset means untouched" convention as `lints`.
    /// Raw strings as written in the file; a name outside
    /// [`brink_source_tree::IGNORED_DIR_NAMES`] parses fine (this crate
    /// stays dependency-free of anything beyond `brink_source_tree`, and
    /// there is nothing wrong in principle with naming a directory that
    /// isn't pruned in the first place) but is a no-op, so [`parse_str_at`]
    /// warns about it rather than silently accepting a likely typo (e.g.
    /// `"node-modules"` instead of `"node_modules"`).
    pub unprune_dirs: Vec<String>,
    /// `[project] conventions`, if set (docs/prose-dialect-spec.md §3.4's
    /// pointer mechanism): either a built-in preset name (`"screenplay"`)
    /// or a project-relative path to a `.brink` conventions module
    /// (`"conventions.brink"`, `"scenes/conventions.brink"`). This crate
    /// only carries the raw string — it doesn't know the closed preset-name
    /// set or validate the path exists, for the same dependency-free
    /// reason `lints` doesn't validate codes (#1234); resolving it (and, if
    /// it names a project path, checking that pattern-claiming handlers
    /// only live in that one file, issue #1844's confinement rule) is
    /// `brink-analyzer`/`brink-db`'s job.
    ///
    /// Renamed from `elements` by issue #2180 (the key predates the split
    /// of `@[element]` from `@[convention]`, docs/decision-log.md's
    /// 2026-08-03 ruling, and now names a module of the latter, not the
    /// former). [`parse_str_at`] still accepts the old `[project] elements`
    /// spelling as a deprecated alias — see its own doc comment for the
    /// precedence/warning rules — but every in-memory representation past
    /// parsing uses only this field; there is no separate `elements` field
    /// to keep in sync.
    pub conventions: Option<String>,
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
            && self.unprune_dirs.is_empty()
            && self.conventions.is_none()
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
///
/// Every variant carries `path` — the file this error came from (#1384: the
/// path/span threading [`parse_str`]'s doc comment describes below). Before
/// #1384 only [`ConfigError::Io`] carried one; a caller with a discovered
/// path in scope (every one of them, in practice — see [`parse_str_at`]) had
/// to re-derive and hand-format the "which file" prefix itself for every
/// other variant, a duplicated, easy-to-forget convention that is exactly
/// how #1369 happened in the first place (`LoadError::Config` lost its path
/// for a release when that hand-formatting was dropped). Structural fields
/// mean a new caller gets it for free.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file exists but couldn't be read (permissions, race, …).
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Malformed TOML syntax. `source` (`toml::de::Error`) carries its own
    /// byte span into the document — see [`ConfigError::span`] — and its
    /// `Display` already renders a `line X, column Y` location plus a
    /// caret-annotated snippet on its own, independent of `path` (`toml`'s
    /// own error type does this regardless of whether a path is threaded
    /// in). What `path` adds here is the file-name attribution this variant
    /// lacked before #1384; the line/column were always there.
    #[error("invalid TOML syntax in {path}: {source}")]
    Toml {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    /// The document's root, or a table where one is expected, isn't a table.
    #[error("`{key}` must be a table, found {found} (in {path})")]
    NotATable {
        path: String,
        key: String,
        found: &'static str,
    },
    /// A recognized key's value has the wrong TOML type (e.g. `dialect = 1`).
    #[error("`{key}` must be a string, found {found} (in {path})")]
    WrongType {
        path: String,
        key: String,
        found: &'static str,
    },
    /// A recognized key's value is a string, but not one of its allowed
    /// variants (e.g. `dialect = "sideways"`). No span: this fires *after*
    /// the document parsed successfully — a syntactically valid string in an
    /// out-of-range value — so the `toml` crate never attaches a byte range
    /// to it the way it does for [`ConfigError::Toml`]; `path` is the most
    /// precise location available (#1384).
    #[error("`{key}` must be one of {expected:?}, found {found:?} (in {path})")]
    InvalidValue {
        path: String,
        key: String,
        expected: &'static [&'static str],
        found: String,
    },
}

impl ConfigError {
    /// The file this error came from, for every variant (#1384).
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            ConfigError::Io { path, .. } => path.to_str().unwrap_or_default(),
            ConfigError::Toml { path, .. }
            | ConfigError::NotATable { path, .. }
            | ConfigError::WrongType { path, .. }
            | ConfigError::InvalidValue { path, .. } => path,
        }
    }

    /// The byte range into the parsed document where this error occurred,
    /// when the underlying TOML parser reported one (#1384) — only ever
    /// `Some` for [`ConfigError::Toml`] (malformed syntax): every other
    /// variant is raised *after* the document parsed successfully (a
    /// recognized key holding an out-of-range value, or the wrong shape), so
    /// there is no narrower-than-"the whole file" location the `toml` crate
    /// ever attached to it. Centralizes the match `brink-lsp` previously
    /// re-derived itself (`toml_span_to_lsp_range`) so a new caller doesn't
    /// have to.
    #[must_use]
    pub fn span(&self) -> Option<std::ops::Range<usize>> {
        match self {
            ConfigError::Toml { source, .. } => source.span(),
            _ => None,
        }
    }
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
///
/// Every [`ConfigError`] this can raise still needs *some* `path` (#1384);
/// this is [`parse_str_at`] with [`CONFIG_FILE_NAME`] as a fallback label,
/// for the one caller that genuinely has no location of its own — an
/// embedder pushing raw `brink.toml` text it read through its own host API,
/// with no discovered key to give (`EditorSession::apply_project_config` in
/// `brink-web`). A caller that *did* discover the file (walked up to find
/// it, has a `SourceTree` key or filesystem path in hand) should call
/// [`parse_str_at`] directly with that path instead.
pub fn parse_str(text: &str) -> Result<(ProjectConfig, Vec<ConfigWarning>), ConfigError> {
    parse_str_at(CONFIG_FILE_NAME, text)
}

/// [`parse_str`], attaching `path` to every [`ConfigError`] it raises
/// (#1384) — the discovered file's `SourceTree` key or filesystem path,
/// rendered into each variant's own `Display`. `ConfigError::Toml`'s message
/// already named the line/column on its own, via the wrapped
/// `toml::de::Error`'s own `Display` (see [`ConfigError::span`]) —
/// independent of `path`; what threading `path` in adds is the file-name
/// attribution.
///
/// Every discovery-based caller in the workspace has a path in scope at this
/// point and should call this rather than [`parse_str`]:
/// [`load_from_entry`], `brink-environment::resolve_options`, `brink ide`'s
/// `resolve_analysis_options`, brink-web's `discover_project_config`, and
/// the LSP's `resolve_language_options`.
pub fn parse_str_at(
    path: impl Into<String>,
    text: &str,
) -> Result<(ProjectConfig, Vec<ConfigWarning>), ConfigError> {
    let path = path.into();
    let doc: Value = toml::from_str(text).map_err(|source| ConfigError::Toml {
        path: path.clone(),
        source,
    })?;
    let root = match doc {
        Value::Table(t) => t,
        other => {
            return Err(ConfigError::NotATable {
                path,
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
                        path,
                        key: "project".to_owned(),
                        found: value_type_name(other),
                    });
                }
            };
            parse_project_table(&path, project, &mut config, &mut warnings)?;
        } else if key == "lints" {
            let lints = match value {
                Value::Table(t) => t,
                other => {
                    return Err(ConfigError::NotATable {
                        path,
                        key: "lints".to_owned(),
                        found: value_type_name(other),
                    });
                }
            };
            for (lkey, lvalue) in lints {
                if lkey == "deny-warnings" {
                    config.deny_warnings = Some(parse_deny_warnings(&path, lkey, lvalue)?);
                } else {
                    config
                        .lints
                        .insert(lkey.clone(), parse_lint_level(&path, lkey, lvalue)?);
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

/// Parse the `[project]` table's keys into `config`/`warnings` — the body
/// [`parse_str_at`] used to inline directly before it grew too long
/// (clippy's `too_many_lines`) once `conventions`/`elements` reconciliation
/// (issue #2180) was added.
fn parse_project_table(
    path: &str,
    project: &toml::map::Map<String, Value>,
    config: &mut ProjectConfig,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<(), ConfigError> {
    // `conventions` (issue #2180) and its deprecated `elements` alias are
    // collected separately, rather than writing straight into
    // `config.conventions` inside the match arm below, and reconciled only
    // after the whole `[project]` table has been walked. `toml::Table`'s
    // iteration order is not "as written in the file" in general, so
    // resolving "both keys set" precedence arm-by-arm as each key is
    // visited would make the outcome depend on iteration order —
    // collecting both first and resolving once afterward keeps it
    // deterministic regardless of which key the file happens to list
    // first.
    let mut conventions_value: Option<String> = None;
    let mut elements_value: Option<String> = None;
    for (pkey, pvalue) in project {
        match pkey.as_str() {
            "dialect" => config.dialect = Some(parse_dialect(path, pkey, pvalue)?),
            "types" => config.types = Some(parse_types(path, pkey, pvalue)?),
            "unprune-dirs" => {
                let dirs = parse_string_list(path, pkey, pvalue)?;
                for dir in &dirs {
                    if !IGNORED_DIR_NAMES.contains(&dir.as_str()) {
                        warnings.push(ConfigWarning(format!(
                            "`project.unprune-dirs` entry `{dir}` in {CONFIG_FILE_NAME} is not \
                             one of {IGNORED_DIR_NAMES:?} — it was never pruned, so this has no \
                             effect (check for a typo)"
                        )));
                    }
                }
                config.unprune_dirs = dirs;
            }
            "conventions" => {
                let s = parse_conventions(path, pkey, pvalue)?;
                if s.is_empty() {
                    warnings.push(ConfigWarning(format!(
                        "`project.conventions` in {CONFIG_FILE_NAME} is an empty string \
                         (ignored) — expected a built-in preset name (e.g. \"screenplay\") or a \
                         path to a conventions module (e.g. \"conventions.brink\")"
                    )));
                } else {
                    conventions_value = Some(s);
                }
            }
            "elements" => {
                let s = parse_conventions(path, pkey, pvalue)?;
                if s.is_empty() {
                    warnings.push(ConfigWarning(format!(
                        "`project.elements` in {CONFIG_FILE_NAME} is an empty string (ignored) \
                         — expected a built-in preset name (e.g. \"screenplay\") or a path to a \
                         conventions module (e.g. \"conventions.brink\")"
                    )));
                } else {
                    elements_value = Some(s);
                }
            }
            _ => warnings.push(ConfigWarning(format!(
                "unknown key `project.{pkey}` in {CONFIG_FILE_NAME} (ignored)"
            ))),
        }
    }
    config.conventions = resolve_conventions_key(conventions_value, elements_value, warnings);
    Ok(())
}

fn parse_dialect(path: &str, key: &str, value: &Value) -> Result<Dialect, ConfigError> {
    let s = value.as_str().ok_or_else(|| ConfigError::WrongType {
        path: path.to_owned(),
        key: format!("project.{key}"),
        found: value_type_name(value),
    })?;
    match s {
        "brink" => Ok(Dialect::Brink),
        "strict-ink" => Ok(Dialect::StrictInk),
        other => Err(ConfigError::InvalidValue {
            path: path.to_owned(),
            key: format!("project.{key}"),
            expected: &["brink", "strict-ink"],
            found: other.to_owned(),
        }),
    }
}

/// Parse `[project] conventions` (§3.4's pointer mechanism; also called for
/// its deprecated `elements` alias, issue #2180 — the raw string shape is
/// identical for either key): any non-empty string, since this crate
/// doesn't know the closed set of built-in preset names and can't check a
/// project path exists (kept dependency-free, #1234) — [`parse_str_at`]'s
/// caller flags an empty string as a warning; this only enforces the TOML
/// shape (a string, full stop). Checking a bare (preset-shaped) value
/// against the real closed preset-name set is
/// `brink-analyzer::AnalysisOptions::apply_project_config`'s job (issue
/// #1874), the same "this crate stays dependency-free; the crate that owns
/// the closed set validates" split `[lints]`'s `validate_lint_code` uses.
/// Reconcile `[project] conventions` against its deprecated `elements`
/// alias (issue #2180) into the one value [`ProjectConfig::conventions`]
/// carries, pushing whatever [`ConfigWarning`]s the reconciliation itself
/// warrants onto `warnings`.
///
/// `elements` is `conventions`'s deprecated predecessor (renamed post the
/// `@[element]`/`@[convention]` split, docs/decision-log.md's 2026-08-03
/// ruling) — accepted for a deprecation window rather than hard-broken,
/// since it's a silent-misconfiguration risk otherwise (an existing
/// project's `brink.toml` would stop configuring its conventions module
/// with no error at all, just quietly-disabled `E169` enforcement).
/// `conventions` always wins when both are set.
fn resolve_conventions_key(
    conventions_value: Option<String>,
    elements_value: Option<String>,
    warnings: &mut Vec<ConfigWarning>,
) -> Option<String> {
    match (conventions_value, elements_value) {
        (Some(c), Some(_)) => {
            warnings.push(ConfigWarning(format!(
                "`project.elements` and `project.conventions` are both set in \
                 {CONFIG_FILE_NAME} — `project.elements` is deprecated (renamed to \
                 `project.conventions`, issue #2180) and was ignored in favor of \
                 `project.conventions`"
            )));
            Some(c)
        }
        (Some(c), None) => Some(c),
        (None, Some(e)) => {
            warnings.push(ConfigWarning(format!(
                "`project.elements` in {CONFIG_FILE_NAME} is deprecated — rename to \
                 `project.conventions` (issue #2180: the key now names a module of \
                 `@[convention]` declarations, not `@[element]` ones)"
            )));
            Some(e)
        }
        (None, None) => None,
    }
}

fn parse_conventions(path: &str, key: &str, value: &Value) -> Result<String, ConfigError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ConfigError::WrongType {
            path: path.to_owned(),
            key: format!("project.{key}"),
            found: value_type_name(value),
        })
}

fn parse_types(path: &str, key: &str, value: &Value) -> Result<TypePolicy, ConfigError> {
    let s = value.as_str().ok_or_else(|| ConfigError::WrongType {
        path: path.to_owned(),
        key: format!("project.{key}"),
        found: value_type_name(value),
    })?;
    match s {
        "gradual" => Ok(TypePolicy::Gradual),
        "strict" => Ok(TypePolicy::Strict),
        other => Err(ConfigError::InvalidValue {
            path: path.to_owned(),
            key: format!("project.{key}"),
            expected: &["gradual", "strict"],
            found: other.to_owned(),
        }),
    }
}

fn parse_deny_warnings(path: &str, key: &str, value: &Value) -> Result<bool, ConfigError> {
    value.as_bool().ok_or_else(|| ConfigError::WrongType {
        path: path.to_owned(),
        key: format!("lints.{key}"),
        found: value_type_name(value),
    })
}

fn parse_lint_level(path: &str, key: &str, value: &Value) -> Result<LintLevel, ConfigError> {
    let s = value.as_str().ok_or_else(|| ConfigError::WrongType {
        path: path.to_owned(),
        key: format!("lints.{key}"),
        found: value_type_name(value),
    })?;
    match s {
        "allow" => Ok(LintLevel::Allow),
        "warn" => Ok(LintLevel::Warn),
        "deny" => Ok(LintLevel::Deny),
        "info" => Ok(LintLevel::Info),
        "hint" => Ok(LintLevel::Hint),
        other => Err(ConfigError::InvalidValue {
            path: path.to_owned(),
            key: format!("lints.{key}"),
            expected: &["allow", "warn", "deny", "info", "hint"],
            found: other.to_owned(),
        }),
    }
}

/// Parse a TOML array-of-strings value (e.g. `[project] unprune-dirs`).
/// Every element must itself be a string — a non-string element (`[1, 2]`,
/// `[true]`) is [`ConfigError::WrongType`], matching the treatment every
/// other recognized-but-wrong-shaped value gets.
fn parse_string_list(path: &str, key: &str, value: &Value) -> Result<Vec<String>, ConfigError> {
    let arr = value.as_array().ok_or_else(|| ConfigError::WrongType {
        path: path.to_owned(),
        key: format!("project.{key}"),
        found: value_type_name(value),
    })?;
    arr.iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ConfigError::WrongType {
                    path: path.to_owned(),
                    key: format!("project.{key}"),
                    found: value_type_name(item),
                })
        })
        .collect()
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

/// Maximum number of ancestor directories [`find_config`]'s walk will climb
/// above `start_dir`, whether or not a `.git` boundary is ever found (#1435).
///
/// #1425 bounded the walk at a workspace/git boundary, but that boundary
/// only exists for a project under version control — a VCS-less tree has no
/// `.git` anywhere above it, so the walk still climbed all the way to the
/// filesystem root, exactly the unbounded-ancestor-walk shape this
/// codebase's "guard against unbounded growth" rule exists to catch. This
/// cap closes that gap unconditionally: it applies to *every* walk, not just
/// the VCS-less case, so the bound is one rule instead of two.
///
/// A fixed constant, not an environment- or filesystem-derived limit:
/// config discovery is a deterministic-compilation input (#1306), so how far
/// the walk climbs must never vary by machine, `$HOME` depth, or anything
/// else runtime-observable — only by `start_dir` itself. 32 is generously
/// above any real project layout in this workspace (the deepest nested
/// fixture is a handful of levels) while still being nowhere near "walk to
/// the filesystem root."
pub const MAX_ANCESTOR_DEPTH: usize = 32;

/// Walk up from `start_dir` (inclusive) through every ancestor directory,
/// returning the path to the first [`CONFIG_FILE_NAME`] found. This is the
/// "walk up from the entry file to the nearest `brink.toml`" discovery rule
/// (#1005) — a project's entry `.ink` file doesn't have to sit directly
/// beside the config for every mount to find the same one.
///
/// A thin wrapper over [`find_config_with_warnings`] that discards its
/// [`ConfigWarning`]s — for callers with no warning channel of their own to
/// report them through. A caller that *does* have one (the LSP's
/// `tracing::warn!`, [`load_from_entry`]'s returned `Vec<ConfigWarning>` via
/// [`discover_from_entry_with_warnings`]) should call
/// [`find_config_with_warnings`] directly instead, per house rule 9 (silent
/// drops are always bugs until proven otherwise).
#[must_use]
pub fn find_config(start_dir: &Path) -> Option<PathBuf> {
    find_config_inner(start_dir, false).0
}

/// [`find_config`], additionally reporting when the bounded walk stepped
/// over a `brink.toml` an author might reasonably have expected to be
/// discovered (#1435) — never used as the result, only as a
/// [`ConfigWarning`] so the caller can tell them it was ignored instead of
/// silently proceeding as if no config existed anywhere.
///
/// **Bounded two ways**, either of which stops the search phase:
///
/// - **Workspace/git boundary (#1425).** Before checking a directory's
///   parent, this stops if the directory itself contains a `.git` entry —
///   the marker of a repository root, whether it's an ordinary repository
///   (`.git/` is a directory) or a linked worktree (`.git` is a *file*
///   holding a `gitdir:` pointer, e.g. `.claude/worktrees/*` in this very
///   repo — checked with [`Path::exists`], not `is_dir`, so both shapes
///   count; the marker name itself is [`brink_source_tree::GIT_DIR_NAME`],
///   the same constant [`brink_source_tree::IGNORED_DIR_NAMES`] uses, so the
///   two never drift apart, #1435).
/// - **Ancestor depth cap ([`MAX_ANCESTOR_DEPTH`], #1435).** Applies
///   regardless of any `.git` boundary — the VCS-less case #1425 didn't
///   cover.
///
/// `start_dir` and every ancestor up to and including whichever boundary is
/// hit first are still probed for `brink.toml` — only climbing *past* it is
/// refused.
///
/// If neither bound stops the walk before it runs out of ancestors
/// naturally (reaches the filesystem root with nothing found), the search is
/// exhaustive and there is nothing above to warn about. If a bound *does*
/// stop it short, a second, equally bounded probe continues past that point
/// — read-only, purely to check whether a `brink.toml` exists somewhere
/// above (walk-up call sites in this workspace: [`find_config`],
/// `brink-lsp`'s `resolve_language_options`, `brink-driver`'s
/// `native_source_root`) — and if one does, [`ConfigError`]-free but
/// warning-worthy: the returned path is still `None` (it was never a
/// candidate the bound allowed), but a [`ConfigWarning`] names it so the
/// caller can tell the author their file was ignored.
#[must_use]
pub fn find_config_with_warnings(start_dir: &Path) -> (Option<PathBuf>, Vec<ConfigWarning>) {
    find_config_inner(start_dir, true)
}

/// Shared implementation behind [`find_config`] and
/// [`find_config_with_warnings`]. `want_warnings` gates the second, bounded
/// probe past the stop point: [`find_config`] has nowhere to put a
/// [`ConfigWarning`] it would only immediately discard, so it passes `false`
/// and this function skips the probe's filesystem stats entirely instead of
/// running them and throwing the result away — up to [`MAX_ANCESTOR_DEPTH`]
/// (32) extra `is_file` calls per miss, climbing *past* the very
/// git/depth boundary the bound exists to stay inside, was wasted work every
/// discarding caller paid for unconditionally (review finding on #1435).
fn find_config_inner(
    start_dir: &Path,
    want_warnings: bool,
) -> (Option<PathBuf>, Vec<ConfigWarning>) {
    let mut dir = Some(start_dir);
    let mut depth = 0usize;
    // Where (and why) the primary search stopped short of the filesystem
    // root, if it did — `None` means it ran out of ancestors naturally.
    let mut stopped_at: Option<(PathBuf, &'static str)> = None;

    while let Some(d) = dir {
        let candidate = d.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return (Some(candidate), Vec::new());
        }
        if d.join(brink_source_tree::GIT_DIR_NAME).exists() {
            // Workspace/git boundary: this directory is the repository
            // root (or a linked worktree's root) and had no `brink.toml`
            // of its own — do not climb past it.
            stopped_at = Some((d.to_path_buf(), "workspace/git boundary"));
            break;
        }
        if depth >= MAX_ANCESTOR_DEPTH {
            // Ancestor depth cap: no `.git` boundary was found within
            // MAX_ANCESTOR_DEPTH climbs — do not climb further.
            stopped_at = Some((d.to_path_buf(), "ancestor depth limit"));
            break;
        }
        depth += 1;
        dir = d.parent();
    }

    let Some((stopped_at, reason)) = stopped_at else {
        // The walk exhausted every real ancestor without hitting either
        // bound — there is nothing further up to have missed.
        return (None, Vec::new());
    };

    if !want_warnings {
        // No warning channel to report through — skip the probe rather than
        // running it and discarding the result (#1435 review finding).
        return (None, Vec::new());
    }

    // Bounded peek past the stop point, purely to detect a stray config an
    // author might expect to be picked up — its existence is reported as a
    // warning, but it is never returned as a result. Bounded by the same
    // cap so this detection pass cannot itself become an unbounded climb.
    let mut probe = stopped_at.parent();
    let mut probe_depth = 0usize;
    while let Some(p) = probe {
        let candidate = p.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return (
                None,
                vec![ConfigWarning(format!(
                    "{} exists above the {reason} at {} and was ignored",
                    candidate.display(),
                    stopped_at.display(),
                ))],
            );
        }
        probe_depth += 1;
        if probe_depth >= MAX_ANCESTOR_DEPTH {
            break;
        }
        probe = p.parent();
    }

    (None, Vec::new())
}

/// [`find_config`], starting from an entry `.ink` file's directory rather
/// than a directory directly. The common case: `brink compile story.ink`
/// discovers `brink.toml` starting from `story.ink`'s parent.
#[must_use]
pub fn discover_from_entry(entry_file: &Path) -> Option<PathBuf> {
    let start = entry_file.parent().unwrap_or_else(|| Path::new("."));
    find_config(start)
}

/// [`discover_from_entry`], surfacing [`find_config_with_warnings`]'s
/// [`ConfigWarning`]s instead of discarding them. [`load_from_entry`] uses
/// this rather than [`discover_from_entry`] so a config skipped by the
/// bounded walk is never silently dropped (#1435, house rule 9).
#[must_use]
pub fn discover_from_entry_with_warnings(
    entry_file: &Path,
) -> (Option<PathBuf>, Vec<ConfigWarning>) {
    let start = entry_file.parent().unwrap_or_else(|| Path::new("."));
    find_config_with_warnings(start)
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
/// Takes no `root` parameter: every current [`SourceTree`] implementation
/// resolves `read` keys against its own constructor-held root (issue #1371),
/// so there is nothing for a caller to supply here. An earlier version of
/// this function accepted (and ignored) a `root: &Path` for shape-symmetry
/// with [`SourceTree::list`]'s old signature; issue #1395 dropped it once
/// #1371 made the equivalent parameter dead on `list` too, closing the gap
/// left when this function's own dead parameter wasn't swept up at the same
/// time.
///
/// Returns the matching key, not file content — callers read it via
/// [`SourceTree::read`] (mirroring how [`find_config`] returns a path the
/// caller reads via `std::fs`, not file content).
///
/// Already bounded at the tree's own root, so it needed no change for #1425
/// or #1435 (unlike [`find_config`]'s `.git`-directory and
/// [`MAX_ANCESTOR_DEPTH`] bounds): a key's ancestors are string-derived
/// (`rsplit_once('/')`), bottoming out at the empty root key with nothing
/// further to strip — there is no lexical equivalent of `find_config`'s
/// `Path::parent` climb here for a depth cap to even apply to. It can only
/// ever "escape" the project if the `tree` itself is rooted somewhere too
/// wide (a caller concern, not this function's).
pub fn find_config_in_tree(tree: &dyn SourceTree, start_key: &str) -> io::Result<Option<String>> {
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
    entry_key: &str,
) -> io::Result<Option<String>> {
    let start = match entry_key.trim_matches('/').rsplit_once('/') {
        Some((parent, _)) => parent,
        None => "",
    };
    find_config_in_tree(tree, start)
}

/// Discover (via [`discover_from_entry_with_warnings`]) and parse (via
/// [`parse_str`]) the `brink.toml` governing `entry_file`'s project, if one
/// exists.
///
/// Returns `Ok((None, warnings))` — never an error — when no `brink.toml` is
/// found within the bounded walk (see [`find_config_with_warnings`]):
/// `warnings` is empty in the ordinary "genuinely no config anywhere" case
/// (current behavior exactly, no regression), and carries a
/// [`ConfigWarning`] in the `#1435` case — a `brink.toml` existed above the
/// walk's workspace/git or ancestor-depth bound and was skipped. Discovery
/// warnings are returned alongside the result rather than folded into
/// [`LoadedConfig::warnings`] because there is no [`LoadedConfig`] to hold
/// them when nothing was loaded; when a config *is* found, this vec is
/// always empty and [`LoadedConfig::warnings`] (the file's own parse-time
/// warnings) is the vec to read instead.
pub fn load_from_entry(
    entry_file: &Path,
) -> Result<(Option<LoadedConfig>, Vec<ConfigWarning>), ConfigError> {
    let (path, discovery_warnings) = discover_from_entry_with_warnings(entry_file);
    let Some(path) = path else {
        return Ok((None, discovery_warnings));
    };
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;
    let (config, warnings) = parse_str_at(path.display().to_string(), &text)?;
    Ok((
        Some(LoadedConfig {
            path,
            config,
            warnings,
        }),
        Vec::new(),
    ))
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

    // ── unprune-dirs (issue #1407) ──────────────────────────────────────

    #[test]
    fn parses_unprune_dirs() {
        let (config, warnings) = parse_str(
            r#"
            [project]
            unprune-dirs = ["node_modules", "target"]
            "#,
        )
        .unwrap();
        assert_eq!(
            config.unprune_dirs,
            vec!["node_modules".to_string(), "target".to_string()]
        );
        assert!(!config.is_empty());
        assert!(
            warnings.is_empty(),
            "both names are real IGNORED_DIR_NAMES entries, no warning expected: {warnings:?}"
        );
    }

    /// An `unprune-dirs` entry that isn't one of the three actually-pruned
    /// names is a no-op (there was nothing to un-prune) — likely a typo, so
    /// it warns rather than silently doing nothing (house-rule "validate
    /// user-supplied config keys").
    #[test]
    fn unprune_dirs_entry_outside_ignored_dir_names_warns() {
        let (config, warnings) = parse_str(
            r#"
            [project]
            unprune-dirs = ["node-modules"]
            "#,
        )
        .unwrap();
        assert_eq!(config.unprune_dirs, vec!["node-modules".to_string()]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("node-modules"));
        assert!(warnings[0].0.contains("unprune-dirs"));
    }

    #[test]
    fn unprune_dirs_wrong_element_type_is_an_error() {
        let err = parse_str("[project]\nunprune-dirs = [1, 2]\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    #[test]
    fn unprune_dirs_not_an_array_is_an_error() {
        let err = parse_str("[project]\nunprune-dirs = \"node_modules\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    #[test]
    fn empty_unprune_dirs_is_not_a_warning_and_leaves_config_empty_by_itself() {
        let (config, warnings) = parse_str("[project]\nunprune-dirs = []\n").unwrap();
        assert!(config.unprune_dirs.is_empty());
        assert!(warnings.is_empty());
        // An explicit empty array still counts as "set" for `is_empty()`'s
        // purposes only if non-empty — an empty list is indistinguishable
        // from unset here, matching `lints`' own empty-map convention.
        assert!(config.is_empty());
    }

    #[test]
    fn invalid_dialect_value_is_an_error() {
        let err = parse_str("[project]\ndialect = \"sideways\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    // ── conventions (issue #1844, renamed from `elements` by #2180) ──────

    #[test]
    fn parses_conventions_as_a_path() {
        let (config, warnings) = parse_str(
            r#"
            [project]
            conventions = "conventions.brink"
            "#,
        )
        .unwrap();
        assert_eq!(config.conventions.as_deref(), Some("conventions.brink"));
        assert!(!config.is_empty());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn parses_conventions_as_a_preset_name() {
        let (config, _warnings) = parse_str("[project]\nconventions = \"screenplay\"\n").unwrap();
        assert_eq!(config.conventions.as_deref(), Some("screenplay"));
    }

    #[test]
    fn empty_conventions_string_warns_and_is_not_set() {
        let (config, warnings) = parse_str("[project]\nconventions = \"\"\n").unwrap();
        assert_eq!(config.conventions, None);
        assert!(config.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("conventions"));
    }

    #[test]
    fn conventions_wrong_type_is_an_error() {
        let err = parse_str("[project]\nconventions = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    #[test]
    fn unset_conventions_leaves_config_empty_by_itself() {
        let (config, _warnings) = parse_str("[project]\ndialect = \"brink\"\n").unwrap();
        assert_eq!(config.conventions, None);
    }

    // ── `elements` deprecated alias (issue #2180) ────────────────────────

    /// The old key still works — a hard break would silently un-configure
    /// every existing project's conventions module (and its `E169`
    /// enforcement) the moment it upgrades, with no error at all.
    #[test]
    fn elements_alias_still_sets_conventions_but_warns() {
        let (config, warnings) = parse_str("[project]\nelements = \"conventions.brink\"\n")
            .expect("deprecated `elements` key must still parse, not hard-error");
        assert_eq!(config.conventions.as_deref(), Some("conventions.brink"));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].0.contains("project.elements"));
        assert!(warnings[0].0.contains("deprecated"));
        assert!(warnings[0].0.contains("project.conventions"));
    }

    #[test]
    fn empty_elements_alias_string_warns_and_is_not_set() {
        let (config, warnings) = parse_str("[project]\nelements = \"\"\n").unwrap();
        assert_eq!(config.conventions, None);
        assert!(config.is_empty());
        // Only the empty-string warning fires — an empty value never
        // reaches `elements_value`, so there is nothing to also warn as a
        // deprecated-but-set alias.
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].0.contains("elements"));
    }

    #[test]
    fn elements_alias_wrong_type_is_an_error() {
        let err = parse_str("[project]\nelements = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::WrongType { .. }));
    }

    /// `conventions` always wins when both keys are set — and the conflict
    /// itself is warned about, so an author isn't left guessing which value
    /// took effect.
    #[test]
    fn both_conventions_and_elements_set_prefers_conventions_and_warns() {
        let (config, warnings) = parse_str(
            r#"
            [project]
            conventions = "new.brink"
            elements = "old.brink"
            "#,
        )
        .unwrap();
        assert_eq!(config.conventions.as_deref(), Some("new.brink"));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].0.contains("project.elements"));
        assert!(warnings[0].0.contains("project.conventions"));
        assert!(warnings[0].0.contains("both set"));
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
        assert!(matches!(err, ConfigError::Toml { .. }));
    }

    #[test]
    fn non_table_root_is_an_error() {
        let err = parse_str("\"just a string\"").unwrap_err();
        assert!(matches!(
            err,
            ConfigError::NotATable { .. } | ConfigError::Toml { .. }
        ));
    }

    // ── path/span threading (#1384) ─────────────────────────────────────

    /// Every [`ConfigError`] channel names the file it came from — the CLI
    /// message, the LSP diagnostic, and now (#1384) the error's own
    /// `Display`, structurally rather than by convention at each call site.
    #[test]
    fn parse_str_at_names_its_path_on_invalid_value() {
        let err =
            parse_str_at("chapters/brink.toml", "[project]\ndialect = \"sideways\"\n").unwrap_err();
        assert_eq!(err.path(), "chapters/brink.toml");
        assert!(
            err.to_string().contains("chapters/brink.toml"),
            "message must name the file, got: {err}"
        );
        assert!(
            matches!(err, ConfigError::InvalidValue { .. }),
            "expected InvalidValue, got: {err:?}"
        );
    }

    #[test]
    fn parse_str_at_names_its_path_on_malformed_toml() {
        let err = parse_str_at("chapters/brink.toml", "this is not [ toml").unwrap_err();
        assert_eq!(err.path(), "chapters/brink.toml");
        assert!(
            err.to_string().contains("chapters/brink.toml"),
            "message must name the file, got: {err}"
        );
        assert!(
            matches!(err, ConfigError::Toml { .. }),
            "expected Toml, got: {err:?}"
        );
    }

    #[test]
    fn parse_str_at_names_its_path_on_wrong_type() {
        let err = parse_str_at("chapters/brink.toml", "[project]\ndialect = 1\n").unwrap_err();
        assert_eq!(err.path(), "chapters/brink.toml");
        assert!(err.to_string().contains("chapters/brink.toml"));
        assert!(
            matches!(err, ConfigError::WrongType { .. }),
            "expected WrongType, got: {err:?}"
        );
    }

    #[test]
    fn parse_str_at_names_its_path_on_not_a_table() {
        // `project = 1` parses fine as TOML (root table with an integer
        // value), so this exercises `NotATable`, not `Toml` — a bare string
        // like `"just a string"` is invalid TOML *syntax* and would hit the
        // `Toml` arm instead, duplicating the malformed-syntax test above and
        // leaving `NotATable`'s `path` field uncovered.
        let err = parse_str_at("chapters/brink.toml", "project = 1\n").unwrap_err();
        assert_eq!(err.path(), "chapters/brink.toml");
        assert!(err.to_string().contains("chapters/brink.toml"));
        assert!(
            matches!(err, ConfigError::NotATable { .. }),
            "expected NotATable, got: {err:?}"
        );
    }

    /// `parse_str` (the pathless entry point) still falls back to the bare
    /// [`CONFIG_FILE_NAME`] rather than an empty/absent path — a caller with
    /// no discovered location still gets a named, non-empty `path()`.
    #[test]
    fn parse_str_falls_back_to_config_file_name_as_path() {
        let err = parse_str("[project]\ndialect = \"sideways\"\n").unwrap_err();
        assert_eq!(err.path(), CONFIG_FILE_NAME);
    }

    /// Malformed TOML *syntax* carries a byte span from the underlying
    /// `toml` crate — a malformed value's line, not just its file, is
    /// locatable (#1384's "a malformed value cannot be located precisely"
    /// gap, for the syntax-error half of it). The span must point at the
    /// actual offending text, not just be present.
    #[test]
    fn toml_syntax_error_carries_a_span_pointing_at_the_bad_text() {
        let text = "[project]\ndialect = \"brink\" oops\n";
        let err = parse_str_at("brink.toml", text).unwrap_err();
        let span = err.span().expect("malformed TOML syntax must carry a span");
        assert!(span.start > 0, "span must not point at the file start");
        // The reported range must fall on the malformed second line, not the
        // first (well-formed) line.
        let first_line_end = text.find('\n').unwrap();
        assert!(
            span.start > first_line_end,
            "span {span:?} must point past the first line (ends at {first_line_end})"
        );
    }

    /// `InvalidValue` fires *after* the document parses successfully (a
    /// syntactically fine string that just isn't a recognized variant), so
    /// there is no narrower-than-file location available — `span()` must be
    /// `None`, not a stale or zeroed range that looks meaningful but isn't.
    #[test]
    fn invalid_value_error_has_no_span() {
        let err = parse_str_at("brink.toml", "[project]\ndialect = \"sideways\"\n").unwrap_err();
        assert_eq!(err.span(), None);
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

    /// #1162: `[lints]` must be able to down-level a code to either advisory
    /// tier below `Warning`, not just `allow`/`warn`/`deny`.
    #[test]
    fn parses_info_and_hint_lint_levels() {
        let (config, warnings) = parse_str(
            r#"
            [lints]
            E014 = "info"
            E022 = "hint"
            "#,
        )
        .unwrap();
        assert_eq!(config.lints.get("E014"), Some(&LintLevel::Info));
        assert_eq!(config.lints.get("E022"), Some(&LintLevel::Hint));
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

    // ── workspace/git boundary (#1425) ──────────────────────────────────

    /// The walk must not climb past a directory containing a `.git`
    /// subdirectory — an unrelated `brink.toml` sitting further up (outside
    /// the repository) must never be picked up.
    #[test]
    fn find_config_stops_at_git_dir_boundary() {
        let root = unique_tmp_dir("git-boundary-dir");
        let repo = root.join("repo");
        let nested = repo.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        // Stray config *above* the repository root — must never be found.
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        assert_eq!(
            find_config(&nested),
            None,
            "must not climb past the .git-marked repository root to a stray ancestor config"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The boundary check also fires when `.git` is a *file* rather than a
    /// directory — the shape a linked git worktree uses (a `gitdir:` pointer
    /// file, exactly how this repository's own `.claude/worktrees/*` are
    /// laid out), not just an ordinary clone's `.git/` directory.
    #[test]
    fn find_config_stops_at_git_file_boundary_worktree_shape() {
        let root = unique_tmp_dir("git-boundary-file");
        let repo = root.join("repo");
        let nested = repo.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(repo.join(".git"), "gitdir: /elsewhere/.git/worktrees/x\n").unwrap();
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        assert_eq!(
            find_config(&nested),
            None,
            "a `.git` worktree-pointer *file* must bound the walk exactly like a `.git` dir"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The boundary directory itself (the one holding `.git`) is still
    /// checked for `brink.toml` before the walk refuses to climb further —
    /// bounding the walk must not also blind it to a config at the boundary.
    #[test]
    fn find_config_still_finds_config_at_the_git_boundary_dir_itself() {
        let root = unique_tmp_dir("git-boundary-config-at-root");
        let repo = root.join("repo");
        let nested = repo.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            repo.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        let found = find_config(&nested).expect("brink.toml at the repo root must still be found");
        assert_eq!(found, repo.join(CONFIG_FILE_NAME));

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A project with no `.git` anywhere above it (no VCS at all) is
    /// unaffected by the bound as long as the config is within
    /// [`MAX_ANCESTOR_DEPTH`] — a shallow nesting (well inside the cap)
    /// behaves exactly as before #1425/#1435.
    #[test]
    fn find_config_without_any_git_boundary_still_finds_config_within_depth_cap() {
        let root = unique_tmp_dir("no-git-anywhere");
        let nested = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        let found =
            find_config(&nested).expect("should still find brink.toml with no .git anywhere");
        assert_eq!(found, root.join(CONFIG_FILE_NAME));

        std::fs::remove_dir_all(&root).unwrap();
    }

    // ── ancestor depth cap, VCS-less trees (#1435) ──────────────────────

    /// Builds `root/d0/d1/.../d{depth-1}`, creating every intermediate
    /// directory, and returns the deepest one.
    fn nested_chain(root: &Path, depth: usize) -> PathBuf {
        let mut dir = root.to_path_buf();
        for i in 0..depth {
            dir = dir.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The defect #1435 exists to fix: a VCS-less tree (no `.git` anywhere)
    /// nested deeper than [`MAX_ANCESTOR_DEPTH`] must not have its
    /// `brink.toml` discovered — before this fix, `find_config`'s
    /// `Path::parent`-only walk had no stop condition at all here and would
    /// have found it regardless of depth.
    #[test]
    fn find_config_bounds_vcs_less_walk_at_max_ancestor_depth() {
        let root = unique_tmp_dir("vcs-less-too-deep");
        let deepest = nested_chain(&root, MAX_ANCESTOR_DEPTH + 10);
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        assert_eq!(
            find_config(&deepest),
            None,
            "a VCS-less walk must not climb past MAX_ANCESTOR_DEPTH ancestors, even with no \
             .git boundary to stop it otherwise"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A VCS-less tree nested exactly at the cap (not beyond it) still finds
    /// its `brink.toml` — the cap must not be off-by-one in the stricter
    /// direction.
    #[test]
    fn find_config_finds_config_exactly_at_max_ancestor_depth() {
        let root = unique_tmp_dir("vcs-less-at-cap");
        let deepest = nested_chain(&root, MAX_ANCESTOR_DEPTH);
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        let found = find_config(&deepest)
            .expect("a brink.toml exactly MAX_ANCESTOR_DEPTH ancestors up must still be found");
        assert_eq!(found, root.join(CONFIG_FILE_NAME));

        std::fs::remove_dir_all(&root).unwrap();
    }

    // ── silent-drop warnings (#1435) ─────────────────────────────────────

    /// A `brink.toml` sitting above the workspace/git boundary is not just
    /// silently ignored — [`find_config_with_warnings`] reports it via a
    /// [`ConfigWarning`] naming both the skipped file and the boundary.
    #[test]
    fn find_config_with_warnings_reports_config_skipped_above_git_boundary() {
        let root = unique_tmp_dir("warn-git-boundary");
        let repo = root.join("repo");
        let nested = repo.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let stray = root.join(CONFIG_FILE_NAME);
        std::fs::write(&stray, "[project]\ndialect = \"brink\"\n").unwrap();

        let (found, warnings) = find_config_with_warnings(&nested);
        assert_eq!(found, None, "the stray config must still never be returned");
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(
            warnings[0].0.contains(&stray.display().to_string()),
            "warning must name the skipped file, got: {}",
            warnings[0]
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The VCS-less analog: a `brink.toml` sitting beyond
    /// [`MAX_ANCESTOR_DEPTH`] in a tree with no `.git` anywhere is reported
    /// the same way.
    #[test]
    fn find_config_with_warnings_reports_config_skipped_beyond_depth_cap() {
        let root = unique_tmp_dir("warn-depth-cap");
        let deepest = nested_chain(&root, MAX_ANCESTOR_DEPTH + 10);
        let stray = root.join(CONFIG_FILE_NAME);
        std::fs::write(&stray, "[project]\ndialect = \"brink\"\n").unwrap();

        let (found, warnings) = find_config_with_warnings(&deepest);
        assert_eq!(found, None, "the stray config must still never be returned");
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(
            warnings[0].0.contains(&stray.display().to_string()),
            "warning must name the skipped file, got: {}",
            warnings[0]
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// No warning when there is genuinely nothing above either — a bound
    /// firing is not itself warning-worthy, only a bound that actually
    /// skipped a real config.
    #[test]
    fn find_config_with_warnings_is_silent_when_nothing_skipped() {
        let root = unique_tmp_dir("warn-nothing-to-skip");
        let deepest = nested_chain(&root, MAX_ANCESTOR_DEPTH + 10);
        // No brink.toml anywhere in this tree at all.

        let (found, warnings) = find_config_with_warnings(&deepest);
        assert_eq!(found, None);
        assert!(warnings.is_empty(), "got: {warnings:?}");

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `find_config` (the discarding wrapper) must behave identically to
    /// `find_config_with_warnings(...).0` for a stray config skipped at the
    /// git boundary — the shared `find_config_inner(..., want_warnings:
    /// false)` path skips the second probe entirely (review finding on
    /// #1435: the probe cost was paid and thrown away), but the result must
    /// still be `None`, never the stray path.
    #[test]
    fn find_config_skips_the_warning_probe_but_still_returns_none_at_git_boundary() {
        let root = unique_tmp_dir("no-warn-probe-git-boundary");
        let repo = root.join("repo");
        let nested = repo.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        assert_eq!(find_config(&nested), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The depth-cap analog of the above.
    #[test]
    fn find_config_skips_the_warning_probe_but_still_returns_none_beyond_depth_cap() {
        let root = unique_tmp_dir("no-warn-probe-depth-cap");
        let deepest = nested_chain(&root, MAX_ANCESTOR_DEPTH + 10);
        std::fs::write(
            root.join(CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();

        assert_eq!(find_config(&deepest), None);

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

        let (loaded, warnings) = load_from_entry(&entry).unwrap();
        assert!(loaded.is_none());
        assert!(warnings.is_empty(), "got: {warnings:?}");

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// [`load_from_entry`]'s discovery-warning half of #1435: a config
    /// skipped by the bounded walk is surfaced through this function's own
    /// return value, not swallowed by its `Ok(None)` "nothing found" case.
    #[test]
    fn load_from_entry_surfaces_discovery_warning_when_config_skipped() {
        let root = unique_tmp_dir("load-skipped-warning");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let stray = root.join(CONFIG_FILE_NAME);
        std::fs::write(&stray, "[project]\ndialect = \"brink\"\n").unwrap();
        let entry = repo.join("story.ink");
        std::fs::write(&entry, "content").unwrap();

        let (loaded, warnings) = load_from_entry(&entry).unwrap();
        assert!(
            loaded.is_none(),
            "the out-of-repo config must never be loaded"
        );
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(
            warnings[0].0.contains(&stray.display().to_string()),
            "warning must name the skipped file, got: {}",
            warnings[0]
        );

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

        let found = find_config_in_tree(&tree, "a/b")
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

        let found = find_config_in_tree(&tree, "a/b").expect("list succeeds");
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
        fn list(&self) -> io::Result<Vec<String>> {
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

        let found = find_config_in_tree(&tree, "a/b/c/d")
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

        let found = find_config_in_tree(&tree, "a/b/c/d")
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
        fn list(&self) -> io::Result<Vec<String>> {
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
        let found = find_config_in_tree(&ErrorsOnRead, "a/b")
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

        let found = discover_from_entry_in_tree(&tree, "story.ink")
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

        let (loaded, discovery_warnings) = load_from_entry(&entry).unwrap();
        let loaded = loaded.expect("config found");
        assert_eq!(loaded.path, root.join(CONFIG_FILE_NAME));
        assert_eq!(loaded.config.dialect, Some(Dialect::Brink));
        assert_eq!(loaded.config.types, Some(TypePolicy::Strict));
        assert!(loaded.warnings.is_empty());
        assert!(discovery_warnings.is_empty(), "got: {discovery_warnings:?}");

        std::fs::remove_dir_all(&root).unwrap();
    }
}
