//! The compilation **environment as a deterministic input** (#1306).
//!
//! brink already compiles as a pure query over salsa inputs, so a determinism
//! boundary *exists* — but the inputs were only ever pushed imperatively
//! (`set_file`/`set_entry`/`set_analysis_options`) with no nameable value to
//! hold, hash, serialize, cache on, or diff. This crate reifies that boundary:
//!
//! - [`Environment`] is the **pure input value** — a serializable,
//!   content-addressed reification of "the sources being compiled + the
//!   resolved policy + the (reserved) resolved dependency set + the entry."
//!   It is the whole compilation universe as a single hashable artifact.
//! - [`Project::load`] is the **effectful producer** — mount-specific, where
//!   all ambient reads (a filesystem walk, a drained `AssetReader`, an LSP
//!   store) and future dependency resolution live. It walks a
//!   [`SourceTree`](brink_source_tree::SourceTree), reads + hashes the
//!   sources, discovers + parses `brink.toml` over the *same* tree, applies
//!   override precedence, and freezes an [`Environment`].
//! - [`compile`] is the **pure function over the input** — it seeds a fresh
//!   salsa `ProjectDb` from an [`Environment`] and pulls the memoized
//!   `story_data` query. No ambient reads, no walk-up, no I/O: everything it
//!   needs is already in the frozen value.
//!
//! ```text
//! [mount-specific resolution]   →   Environment          →   compile(&Environment)
//!  RealFs walk / drained            reified, content-        PURE, deterministic
//!  AssetReader / LSP store          addressed input value    (no ambient reads)
//!  = Project::load (effectful)      {sources+hashes,         = salsa query pull
//!                                    config, deps, entry}
//! ```
//!
//! The `Environment` is serialized/reified **now**, not deferred to when
//! external libraries arrive (ruled 2026-07-23): the boundary's whole value is
//! its explicitness — a reproducible, hashable ([`Environment::content_hash`])
//! input artifact enabling build caching, reproducible builds, and input
//! diffing from day one. The [reserved `resolved_deps`](Environment::resolved_deps)
//! slot (#1093) is where external module artifacts will land, mounted at
//! compile time with the same identity/linking machinery load-time DLC/UGC
//! modules use — designed-for, not built.
//!
//! [`Project::load`] also mounts the **stdlib** into the manifest (#2080,
//! ruled 2026-08-03) — built-in preset/convention `.brink` source
//! (`std/conventions/screenplay.brink`), embedded via `include_str!` so it
//! mounts identically on hosts with no filesystem (wasm). The stdlib is
//! *source*, not a [`ResolvedDep`] (that slot stays reserved for #1093's
//! compiled per-module artifacts): it joins the manifest exactly like any
//! project file, under the same string-key convention, so it needs no
//! parallel identity or resolution mechanism.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use brink_compiler::{CompileError, CompileOutput, ResolvedDiagnostic};
use brink_driver::{AnalysisOptions, Dialect, Driver, LintLevel, TypePolicy};
use brink_ir::Diagnostic;
use brink_project_config::{ConfigError, discover_from_entry_in_tree, parse_str_at};
use brink_source_tree::SourceTree;

// ── Content addressing ───────────────────────────────────────────────

/// FNV-1a 64-bit — a small, dependency-free, fully deterministic hash. Chosen
/// over `std`'s `DefaultHasher` (whose output is not guaranteed stable across
/// Rust versions) and over pulling in a crypto crate: a content-addressed
/// store keyed within one project's source set does not need cryptographic
/// collision resistance, only a stable, platform-independent digest. The v2
/// `ContentStore` swap (a persistent variant) is the natural point to revisit
/// the digest if one is ever needed.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// A content hash: the deterministic digest of one source file's text. The
/// [`Environment::manifest`] keys files by their (root-relative) path and maps
/// each to a `ContentHash`; the [`ContentStore`] maps a `ContentHash` back to
/// its text. Two files with byte-identical content share one hash (and one
/// stored copy) — the store is content-addressed, so it deduplicates.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ContentHash(u64);

impl ContentHash {
    /// The content hash of `text`.
    #[must_use]
    pub fn of(text: &str) -> Self {
        Self(fnv1a_64(text.as_bytes()))
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// The hash of a whole [`Environment`] — over its manifest, entry, resolved
/// options, and resolved deps. This is the reproducible-build / build-cache
/// key the "serialize now" ruling exists to enable: two environments with the
/// same `EnvHash` compile to the same `StoryData`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct EnvHash(u64);

impl std::fmt::Display for EnvHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Where an [`Environment`]'s source text actually lives — the seam that makes
/// the future content-cache an impl swap, not an API break.
///
/// v1 is [`Inline`](ContentStore::Inline): the text is bundled directly into
/// the value, so a serialized `Environment` is fully self-contained (portable,
/// diffable, cacheable as one blob). A later v2 can add a `Persistent` variant
/// backed by an on-disk content-addressed store; consumers are unaffected
/// because they only ever read through [`Environment::source_text`], never a
/// concrete store field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContentStore {
    /// Self-contained: every hash maps to its text, bundled in the value.
    Inline(BTreeMap<ContentHash, String>),
}

impl ContentStore {
    /// The text for `hash`, if this store holds it.
    fn get(&self, hash: ContentHash) -> Option<&str> {
        match self {
            Self::Inline(map) => map.get(&hash).map(String::as_str),
        }
    }
}

/// A resolved external module artifact (a library) — **reserved** (#1093).
///
/// Empty in v1: external libraries are not on the roadmap. The slot exists so
/// that when they arrive, dependency *resolution* (ambient + pinned, on the
/// producer side, à la `Cargo.lock`) freezes its *resolved set* into the
/// `Environment`, keeping compilation pure. `the-tree-is-the-universe`
/// generalizes to `the-environment-is-the-universe`:
/// `{ local module tree } + { resolved external module set }`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedDep {
    /// The stable `(module, name)`-style identity of the resolved artifact.
    /// A placeholder field so the reserved struct is nameable and
    /// round-trippable; its shape is defined when #1093 is built.
    pub module: String,
}

// ── The pure input value ─────────────────────────────────────────────

/// The reified, content-addressed compilation input — the determinism
/// boundary (#1306).
///
/// Everything [`compile`] needs is frozen here: the source set (as a
/// path→hash [`manifest`](Self::manifest) plus a hash→text
/// [`content`](Self::content) store), the designated [`entry`](Self::entry),
/// the fully [resolved `options`](Self::options), and the reserved
/// [`resolved_deps`](Self::resolved_deps). Because it is a plain serializable
/// value, it can be hashed ([`content_hash`](Self::content_hash)), cached on,
/// diffed, and round-tripped.
///
/// Consumers read sources **only** through [`source_keys`](Self::source_keys)
/// and [`source_text`](Self::source_text) — never a public sources field.
/// That is the whole point of the hash-addressed shape: the inline store can
/// later become a persistent one with no breaking migration.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Environment {
    /// key (root-relative, forward-slash) → content hash. Deterministic
    /// (`BTreeMap`); native module identity derives from these keys.
    /// Hash-addressed from day one.
    manifest: BTreeMap<String, ContentHash>,
    /// The text backing store. v1 = [`ContentStore::Inline`]: text bundled,
    /// self-contained. A v2 `Persistent` variant swaps in later; consumers,
    /// reading only through [`source_text`](Self::source_text), are unchanged.
    content: ContentStore,
    /// Designated entry key — its top-level content is the start flow
    /// (compilation universe != execution entry, #1296).
    pub entry: String,
    /// The **resolved** effective policy — the producer already applied
    /// override precedence (CLI/API > `brink.toml` > default), so [`compile`]
    /// does no further resolution.
    pub options: AnalysisOptions,
    /// Reserved (#1093): resolved external module artifacts (libraries).
    /// Empty in v1.
    pub resolved_deps: Vec<ResolvedDep>,
}

impl Environment {
    /// Every source key (root-relative path), in deterministic sorted order.
    pub fn source_keys(&self) -> impl Iterator<Item = &str> {
        self.manifest.keys().map(String::as_str)
    }

    /// The source text for `key`, resolved key → hash → store.
    ///
    /// Returns [`Cow`] (not `&str`) so the accessor signature is identical for
    /// the inline store (borrows) and a future persistent store (which would
    /// own the read-back text). `None` if `key` is not in the manifest.
    pub fn source_text(&self, key: &str) -> Option<Cow<'_, str>> {
        let hash = *self.manifest.get(key)?;
        self.content.get(hash).map(Cow::Borrowed)
    }

    /// The hash of the whole environment — over manifest + entry + options +
    /// deps. A reproducible-build / cache key (see [`EnvHash`]).
    pub fn content_hash(&self) -> EnvHash {
        let mut buf: Vec<u8> = Vec::new();
        for (key, hash) in &self.manifest {
            buf.extend_from_slice(key.as_bytes());
            buf.push(0);
            buf.extend_from_slice(&hash.0.to_le_bytes());
        }
        buf.push(0xff);
        buf.extend_from_slice(self.entry.as_bytes());
        buf.push(0xff);
        // serde_json is deterministic for these value shapes (BTreeMap is
        // sorted; the option/dep structs are field-ordered), so this is a
        // stable digest of the resolved policy + reserved deps.
        buf.extend_from_slice(&serde_json::to_vec(&self.options).unwrap_or_default());
        buf.push(0xff);
        buf.extend_from_slice(&serde_json::to_vec(&self.resolved_deps).unwrap_or_default());
        EnvHash(fnv1a_64(&buf))
    }
}

// ── Producer-side policy overrides ───────────────────────────────────

/// Explicit policy a mount supplies that **wins over `brink.toml`** — the
/// `CLI/API > file > default` precedence rule (#1005). A field left `None`
/// (or, for `lints`, absent from the map) means "the caller has no explicit
/// value," so the discovered `brink.toml` (or the default) governs that
/// field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionOverrides {
    /// An explicit dialect (e.g. the CLI `--dialect` flag), if the caller set
    /// one.
    pub dialect: Option<Dialect>,
    /// An explicit type policy (e.g. the CLI `--types` flag), if set.
    pub types: Option<TypePolicy>,
    /// Explicit per-code lint-level overrides (e.g. the CLI's repeatable
    /// `--deny`/`--warn`/`--allow <CODE>` flags, issue #1373), keyed by the
    /// diagnostic code's string form. Always wins over the same code in a
    /// discovered `brink.toml`'s `[lints]` table — folded in by
    /// [`AnalysisOptions::apply_lint_overrides`], which validates each code
    /// the same way the file's table is validated (#1160: only codes whose
    /// *default* severity is `Warning` are overridable).
    pub lints: BTreeMap<String, LintLevel>,
    /// An explicit `deny-warnings` (e.g. the CLI's `-D warnings` flag), if
    /// set. `None` means "the caller has no explicit value," same as
    /// `dialect`/`types`.
    pub deny_warnings: Option<bool>,
}

// ── The effectful producer ───────────────────────────────────────────

/// The producer namespace: `Project::load` turns a mount (a
/// [`SourceTree`](brink_source_tree::SourceTree)) into an [`Environment`].
///
/// `Project` is where every ambient/effectful concern lives (filesystem
/// walks, an `AssetReader` drain, an LSP store, and — later — dependency
/// resolution), quarantined off the pure [`Environment`] it produces.
pub struct Project;

impl Project {
    /// Walk `tree`, read + hash its sources, discover + parse `brink.toml`
    /// over the same tree, apply override precedence, and freeze an
    /// [`Environment`].
    ///
    /// The tree is treated as rooted at `.` with root-relative keys (the
    /// #1312 `SourceTree` config-discovery convention): `entry` is a
    /// root-relative key, and any `brink.toml` is looked up by a direct
    /// `{ancestor}/brink.toml` probe walking up from `entry` (#1370 —
    /// discovery no longer calls `list` at all, so a mount's `list` is not
    /// required to surface `brink.toml`). The mount is responsible for
    /// rooting its tree (the CLI drains its project root into an in-memory
    /// tree; web / LSP push their own root-relative store).
    ///
    /// **Sync** — the only asynchrony a mount has (e.g. a bevy `AssetReader`)
    /// is quarantined in *building the tree*, before this runs, matching the
    /// `InkLoader` drain-then-compile pattern.
    ///
    /// Dispatches on `entry`'s extension: a `.brink` entry's universe is the
    /// whole native source tree (enumerate every `.brink` key); a `.ink` entry
    /// follows its `INCLUDE` graph from the entry (a BFS over the tree's
    /// reads).
    ///
    /// ## Repeat compiles are deterministic
    ///
    /// Each call resolves `AnalysisOptions` from a brand-new
    /// `AnalysisOptions::default()` (see `resolve_options` below) — never one
    /// left over from a previous call — so two sequential `load` calls for
    /// unrelated projects never leak `dialect`/`types` between them, even
    /// though [`AnalysisOptions::apply_project_config`]'s "unset means
    /// untouched" rule for those two fields would otherwise let a stale
    /// value silently survive (see that method's own "must be fresh"
    /// invariant doc). This matters for any caller that calls `load`
    /// repeatedly against different mounts in the same process — notably
    /// `bevy-brink`'s `InkLoader`, invoked once per `.ink` asset (re)load —
    /// where a leaked `dialect` would make the *n*-th load's outcome depend
    /// on what the (*n*-1)-th load happened to resolve. Pinned by
    /// `repeat_compiles_do_not_leak_options_across_project_load_calls`
    /// below.
    pub fn load(
        tree: &dyn SourceTree,
        entry: &str,
        overrides: &OptionOverrides,
    ) -> Result<Environment, LoadError> {
        let sources = collect_sources(tree, entry)?;

        let mut manifest = BTreeMap::new();
        let mut inline = BTreeMap::new();
        for (key, text) in sources {
            let hash = ContentHash::of(&text);
            inline.insert(hash, text);
            manifest.insert(key, hash);
        }

        mount_stdlib(&mut manifest, &mut inline);

        let options = resolve_options(tree, entry, overrides)?;

        Ok(Environment {
            manifest,
            content: ContentStore::Inline(inline),
            entry: entry.to_string(),
            options,
            resolved_deps: Vec::new(),
        })
    }
}

// ── Standard library mount (#2080) ───────────────────────────────────

/// The stdlib source set, embedded at compile time. #2080's 2026-08-03
/// ruling: `Environment` (#1306) already generalizes
/// `the-tree-is-the-universe` to `the-environment-is-the-universe`
/// (`{ local module tree } + { resolved external module set }`), so the
/// stdlib needs no bespoke resolution mechanism — it mounts into the same
/// hash-addressed [`Environment::manifest`] every project source lives in,
/// keyed by the same root-relative, forward-slash string-key convention
/// (`std/conventions/screenplay.brink`). Native module identity then mints
/// for it exactly as `brink_db::modules::native_module_path` mints for any
/// project file — no std-specific identity rule exists or is needed.
///
/// `include_str!` (not a runtime filesystem read) because the wasm build
/// (`@brink-lang/web`) has no filesystem — the ruling's explicit
/// instruction: "Embed the source in the binary." Each new stdlib
/// module/preset is added here as it ships; nothing downstream changes.
///
/// Scope (per the ruling): this is the *mount* only. Actually importing a
/// mounted module (`use std::…`) additionally needs #1582's pub marker and
/// #2167's closure-scoped confinement — neither shipped yet, so a mounted
/// module's items are not yet reachable from a project's own `use`.
const STDLIB_SOURCES: &[(&str, &str)] = &[(
    "std/conventions/screenplay.brink",
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/std/conventions/screenplay.brink"
    )),
)];

/// The stdlib source set — `(root-relative key, source text)` pairs — for a
/// caller that builds its own analysis universe *outside* [`Project::load`]
/// (#2198): `brink-lsp` and `brink-cli`'s `ide` subcommand handlers construct
/// their own `Driver`/`ProjectDb` directly rather than going through
/// [`Environment`], so they cannot reach [`mount_stdlib`]'s private
/// manifest/content-store merge. Rather than a second, parallel mount
/// mechanism (the "second road" failure this issue exists to close), those
/// callers pull the identical `(key, text)` pairs from here and fold them
/// into whatever file-registration primitive their own loader already uses
/// (`ProjectDb::set_file`), preserving the same "a project's own file at the
/// same key wins" precedence [`mount_stdlib`] applies. This is the single
/// source of truth both producers read — adding a stdlib module here is
/// still the only place a new one is registered.
#[must_use]
pub fn stdlib_sources() -> &'static [(&'static str, &'static str)] {
    STDLIB_SOURCES
}

/// Merge [`STDLIB_SOURCES`] into a manifest/content pair being assembled by
/// [`Project::load`] — the whole mechanism the ruling describes: the
/// producer adds stdlib entries, the same way it adds any other source key.
/// A project source already present at the same key wins over the embedded
/// copy rather than being silently clobbered (`std/` is a reserved-by-
/// convention path, so a real collision is not expected, but "project data
/// always wins" costs nothing and avoids a surprising override).
fn mount_stdlib(
    manifest: &mut BTreeMap<String, ContentHash>,
    inline: &mut BTreeMap<ContentHash, String>,
) {
    for (key, text) in STDLIB_SOURCES {
        if manifest.contains_key(*key) {
            continue;
        }
        let hash = ContentHash::of(text);
        inline.entry(hash).or_insert_with(|| (*text).to_string());
        manifest.insert((*key).to_string(), hash);
    }
}

/// A native `.brink` key with a `..` segment — `native_module_path` treats
/// `..` literally, so letting one through would mint a bogus module. Mirrors
/// `brink_driver`'s `discover_native` guard (issue #1288 review note (a)).
fn is_dotdot_polluted(key: &str) -> bool {
    key.split('/').any(|segment| segment == "..")
}

/// The `.brink` native source extension.
const NATIVE_EXTENSION: &str = "brink";

/// Collect the compilation universe for `entry` as a key→source map.
///
/// Native (`.brink`): the tree *is* the universe — every `.brink` key it
/// enumerates. Ink (`.ink`): the `INCLUDE`-reachable set from `entry`,
/// discovered by reusing the driver's BFS over the tree's reads (so the ink /
/// native discovery duplication is not re-implemented here).
fn collect_sources(
    tree: &dyn SourceTree,
    entry: &str,
) -> Result<BTreeMap<String, String>, LoadError> {
    if brink_driver::is_native(Path::new(entry)) {
        let mut map = BTreeMap::new();
        for key in tree.list()? {
            if Path::new(&key)
                .extension()
                .is_none_or(|ext| ext != NATIVE_EXTENSION)
            {
                continue;
            }
            if is_dotdot_polluted(&key) {
                return Err(LoadError::InvalidSourceKey(key));
            }
            let text = tree.read(&key)?;
            map.insert(key, text);
        }
        Ok(map)
    } else {
        // Reuse the driver's `INCLUDE` BFS, reading through the tree.
        let mut driver = Driver::new();
        driver.discover(entry, |key| tree.read(key))?;
        let db = driver.db();
        let mut map = BTreeMap::new();
        for id in db.file_ids() {
            if let (Some(path), Some(source)) = (db.file_path(id), db.source(id)) {
                map.insert(path.to_string(), source.to_string());
            }
        }
        Ok(map)
    }
}

/// Resolve the effective [`AnalysisOptions`] for `entry`: start from the
/// default, apply a discovered `brink.toml` (honoring override precedence),
/// then apply the explicit overrides — including `overrides.lints`/
/// `overrides.deny_warnings` (issue #1373), applied last (via
/// [`AnalysisOptions::apply_lint_overrides`]) so they win over the file
/// regardless of whether a `brink.toml` was even discovered. The one
/// resolution point every mount inherits (#1005 precedence:
/// `CLI/API > file > default`).
fn resolve_options(
    tree: &dyn SourceTree,
    entry: &str,
    overrides: &OptionOverrides,
) -> Result<AnalysisOptions, LoadError> {
    // Fresh on every call (issue #1436) — never hoisted out of this
    // function or reused across calls. `apply_project_config`'s
    // `dialect`/`types` fields are "unset means untouched"; starting from
    // `default()` every time is what stops one `Project::load` call's
    // resolved dialect/types from silently surviving into the next,
    // unrelated one. See `Project::load`'s doc comment and
    // `AnalysisOptions::apply_project_config`'s "must be fresh" invariant.
    let mut options = AnalysisOptions::default();

    if let Some(config_key) = discover_from_entry_in_tree(tree, entry)? {
        let text = tree
            .read(&config_key)
            .map_err(|source| LoadError::ConfigRead {
                path: config_key.clone(),
                source,
            })?;
        let (config, warnings) =
            parse_str_at(config_key.clone(), &text).map_err(|source| LoadError::Config {
                path: config_key.clone(),
                source: Box::new(source),
            })?;
        for warning in &warnings {
            // The producer is the effectful side; surfacing unknown-key
            // warnings here (rather than dropping them) preserves the CLI's
            // pre-#1306 "warn, never fail" behavior — a silent drop would be a
            // bug (house rule).
            tracing::warn!("[{config_key}] {warning}");
        }
        let config_warnings = options.apply_project_config(
            &config,
            overrides.dialect.is_some(),
            overrides.types.is_some(),
        );
        for warning in &config_warnings {
            // Same channel as the unknown-key warnings above: an unknown or
            // non-overridable `[lints]` code, or an unrecognized `[project]
            // elements` preset name (issue #1874), is never silently
            // dropped (house rule).
            tracing::warn!("[{config_key}] {warning}");
        }
    }

    if let Some(dialect) = overrides.dialect {
        options.dialect = dialect;
    }
    if let Some(types) = overrides.types {
        options.types = Some(types);
    }

    // The top of the `CLI/API > file > default` stack (#1373): applied last
    // so an explicit `--deny`/`--warn`/`--allow`/`-D warnings` always wins
    // over whatever the file (or nothing, if no `brink.toml` was found) set
    // for the same code.
    let lint_override_warnings =
        options.apply_lint_overrides(&overrides.lints, overrides.deny_warnings);
    for warning in &lint_override_warnings {
        // Same "warn, never silently drop" channel as the file-sourced
        // warnings above (house rule).
        tracing::warn!("{warning}");
    }

    Ok(options)
}

// ── The pure compile over the input ──────────────────────────────────

/// Compile an [`Environment`] — the **pure** function over the reified input.
///
/// Seeds a fresh salsa `ProjectDb` from the frozen value
/// (`set_analysis_options` + `set_file` per source key, in the manifest's
/// deterministic sorted order, so native `FileId`s/module identity mint
/// exactly as native discovery would + `set_entry`) and pulls the memoized
/// `story_data` query. No ambient reads, no walk-up, no I/O.
pub fn compile(env: &Environment) -> Result<CompileOutput, CompileError> {
    let mut driver = Driver::new();
    driver.set_analysis_options(env.options.clone());

    for key in env.source_keys() {
        if let Some(text) = env.source_text(key) {
            driver.db_mut().set_file(key, text.into_owned());
        }
    }

    if driver.db_mut().set_entry(&env.entry).is_none() {
        return Err(CompileError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry file not in environment: {}", env.entry),
        )));
    }

    let product = driver.db().story_data().cloned().unwrap_or_default();

    let Some(story) = product.story else {
        let mut all = product.errors;
        all.extend(product.warnings);
        return Err(CompileError::Diagnostics(resolve_diagnostics(
            driver.db(),
            all,
        )));
    };

    Ok(CompileOutput {
        data: Arc::unwrap_or_clone(story),
        warnings: resolve_diagnostics(driver.db(), product.warnings),
    })
}

/// Resolve `FileId`-keyed diagnostics to path-carrying [`ResolvedDiagnostic`]s
/// while the db is still alive (it owns the `FileId`→path map). Mirrors
/// `brink-compiler`'s own resolution, using only the public `ProjectDb` API —
/// including resolving `severity` through `brink_driver::effective_severity`
/// against the db's own `AnalysisOptions`, same as the mirrored function
/// (issue #1162), so the two never drift on which severity a diagnostic
/// carries.
fn resolve_diagnostics(
    db: &brink_driver::ProjectDb,
    diags: Vec<Diagnostic>,
) -> Vec<ResolvedDiagnostic> {
    let opts = db.analysis_options();
    let types = opts.type_policy();
    diags
        .into_iter()
        .map(|d| ResolvedDiagnostic {
            path: db.file_path(d.file).unwrap_or_default().to_string(),
            file: d.file,
            range: d.range,
            message: d.message,
            severity: brink_driver::effective_severity(d.code, types, &opts.lints),
            code: d.code,
        })
        .collect()
}

// ── Errors ───────────────────────────────────────────────────────────

/// A failure producing an [`Environment`] from a mount.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// An I/O error reading or enumerating the tree.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// `INCLUDE` discovery failed (missing include, circular include).
    #[error("discovery error: {0}")]
    Discover(#[from] brink_driver::DiscoverError),
    /// A discovered `brink.toml` could not be parsed (malformed TOML, or a
    /// recognized key with an out-of-range value). Unknown keys are warnings,
    /// never this error. Carries the discovered `path` so the message names
    /// which file failed — lost when this variant went through a bare
    /// `#[from] ConfigError` in #1306 (#1369 restores it). `source` is
    /// itself now path-carrying too (#1384: `parse_str_at` threads `path`
    /// into every `ConfigError` it raises), so this field is kept for
    /// structural/pattern-matching access (existing callers destructure
    /// `LoadError::Config { path, .. }`) rather than duplicated into the
    /// rendered message — `source`'s own `Display` already names the file.
    /// `source` is boxed: `ConfigError` grew past `clippy::result_large_err`'s
    /// threshold once #1384 threaded `path` into its own variants, and this
    /// is the one `LoadError` variant that stacks a second `path` field on
    /// top of it.
    #[error("{source}")]
    Config {
        /// The root-relative key of the `brink.toml` that failed to parse.
        path: String,
        #[source]
        source: Box<ConfigError>,
    },
    /// A discovered `brink.toml` could not be *read* (permission error,
    /// non-UTF-8 bytes, or any other I/O failure) — the other half of
    /// #1369's regression: `tree.read(&config_key)?` used to fall through
    /// the bare `#[from] io::Error` on [`LoadError::Io`], which carries no
    /// path (`RealFs::read` is `fs::read_to_string`, and std I/O errors
    /// don't carry the path that failed). Carries the discovered `path` so
    /// this half names the file too.
    #[error("failed to read project config {path}: {source}")]
    ConfigRead {
        /// The root-relative key of the `brink.toml` that failed to read.
        path: String,
        #[source]
        source: io::Error,
    },
    /// A native source key is not root-relative (contains a `..` segment) — a
    /// save-key-identity guardrail against a `SourceTree` that violates the
    /// contract.
    #[error("invalid source key `{0}` (must be root-relative, no `..`)")]
    InvalidSourceKey(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_driver::LintLevel;
    use brink_source_tree::InMemory;

    fn tree(files: &[(&str, &str)]) -> InMemory {
        InMemory::new(
            files
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    // ── content addressing ───────────────────────────────────────────

    #[test]
    fn content_hash_is_deterministic_and_content_addressed() {
        assert_eq!(ContentHash::of("hello"), ContentHash::of("hello"));
        assert_ne!(ContentHash::of("hello"), ContentHash::of("world"));
    }

    #[test]
    fn source_text_resolves_through_key_hash_store() {
        let t = tree(&[("main.brink", "flow main() {}")]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");

        assert_eq!(
            env.source_text("main.brink").as_deref(),
            Some("flow main() {}")
        );
        assert_eq!(env.source_text("absent.brink"), None);
    }

    #[test]
    fn identical_content_is_stored_once_but_keyed_twice() {
        let t = tree(&[("a.brink", "flow a() {}"), ("b.brink", "flow a() {}")]);
        let env = Project::load(&t, "a.brink", &OptionOverrides::default()).expect("loads");

        let ContentStore::Inline(store) = &env.content;
        // Two project manifest keys (deduplicated to one stored blob) plus
        // the mounted stdlib entry (#2080) — three keys, two stored blobs.
        assert_eq!(env.source_keys().count(), 3);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn source_keys_are_sorted() {
        let t = tree(&[
            ("z.brink", "flow z() {}"),
            ("a.brink", "flow a() {}"),
            ("m.brink", "flow m() {}"),
        ]);
        let env = Project::load(&t, "a.brink", &OptionOverrides::default()).expect("loads");
        let keys: Vec<_> = env.source_keys().collect();
        // The mounted stdlib key (#2080) sorts between "m.brink" and
        // "z.brink".
        assert_eq!(
            keys,
            vec![
                "a.brink",
                "m.brink",
                "std/conventions/screenplay.brink",
                "z.brink"
            ]
        );
    }

    // ── serialize / round-trip / hash ────────────────────────────────

    #[test]
    fn environment_round_trips_through_json_unchanged() {
        let t = tree(&[("main.brink", "flow main() {}")]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");

        let json = serde_json::to_string(&env).expect("serializes");
        let back: Environment = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(env, back);
        assert_eq!(env.content_hash(), back.content_hash());
    }

    #[test]
    fn content_hash_changes_when_a_source_changes() {
        let a = Project::load(
            &tree(&[("m.brink", "flow m() {}")]),
            "m.brink",
            &OptionOverrides::default(),
        )
        .expect("loads");
        let b = Project::load(
            &tree(&[("m.brink", "flow m() { Hi. }")]),
            "m.brink",
            &OptionOverrides::default(),
        )
        .expect("loads");
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_changes_when_options_change() {
        let base = tree(&[("m.brink", "flow m() {}")]);
        let default = Project::load(&base, "m.brink", &OptionOverrides::default()).expect("loads");
        let overridden = Project::load(
            &base,
            "m.brink",
            &OptionOverrides {
                dialect: Some(Dialect::Brink),
                ..OptionOverrides::default()
            },
        )
        .expect("loads");
        assert_ne!(default.content_hash(), overridden.content_hash());
    }

    // ── config resolution / override precedence ──────────────────────

    #[test]
    fn brink_toml_dialect_is_discovered_over_the_tree() {
        let t = tree(&[
            ("brink.toml", "[project]\ndialect = \"brink\"\n"),
            ("main.brink", "flow main() {}"),
        ]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");
        assert_eq!(env.options.dialect, Dialect::Brink);
    }

    #[test]
    fn brink_toml_is_discovered_by_walking_up_from_the_entry() {
        let t = tree(&[
            ("brink.toml", "[project]\ndialect = \"brink\"\n"),
            ("chapters/main.brink", "flow main() {}"),
        ]);
        let env =
            Project::load(&t, "chapters/main.brink", &OptionOverrides::default()).expect("loads");
        assert_eq!(env.options.dialect, Dialect::Brink);
    }

    #[test]
    fn explicit_override_wins_over_brink_toml() {
        let t = tree(&[
            ("brink.toml", "[project]\ndialect = \"brink\"\n"),
            ("main.brink", "flow main() {}"),
        ]);
        let env = Project::load(
            &t,
            "main.brink",
            &OptionOverrides {
                dialect: Some(Dialect::StrictInk),
                ..OptionOverrides::default()
            },
        )
        .expect("loads");
        assert_eq!(env.options.dialect, Dialect::StrictInk);
    }

    #[test]
    fn no_brink_toml_yields_default_options() {
        let t = tree(&[("main.brink", "flow main() {}")]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");
        assert_eq!(env.options, AnalysisOptions::default());
    }

    /// #1436: pins the "repeat compiles are deterministic" invariant
    /// `Project::load`'s doc comment now names explicitly —
    /// `resolve_options` must resolve every call from a fresh
    /// `AnalysisOptions::default()`, never one mutated by a prior call.
    ///
    /// First compile resolves `dialect = Brink` from its own `brink.toml`.
    /// The second, completely unrelated compile has no `brink.toml` at
    /// all — if `resolve_options` ever stopped constructing `default()`
    /// fresh (e.g. a future change hoisted/cached `AnalysisOptions` across
    /// `load` calls "for efficiency"), `apply_project_config`'s "unset
    /// means untouched" rule for `dialect`/`types` would let the first
    /// call's `Brink` silently survive into the second call's result
    /// instead of resolving to the dialect-less default — exactly the
    /// leak `AnalysisOptions::apply_project_config`'s own "must be fresh"
    /// doc warns every non-editor-session caller against.
    #[test]
    fn repeat_compiles_do_not_leak_options_across_project_load_calls() {
        let brink_tree = tree(&[
            ("brink.toml", "[project]\ndialect = \"brink\"\n"),
            ("main.brink", "flow main() {}"),
        ]);
        let first =
            Project::load(&brink_tree, "main.brink", &OptionOverrides::default()).expect("loads");
        assert_eq!(first.options.dialect, Dialect::Brink);

        let default_tree = tree(&[("main2.brink", "flow main() {}")]);
        let second = Project::load(&default_tree, "main2.brink", &OptionOverrides::default())
            .expect("loads");
        assert_eq!(
            second.options,
            AnalysisOptions::default(),
            "a later, unrelated Project::load call must never observe an \
             earlier call's resolved AnalysisOptions -- each call must \
             resolve from a fresh AnalysisOptions::default(), not a \
             reused/mutated one; got {:?}",
            second.options
        );
    }

    #[test]
    fn malformed_brink_toml_is_a_load_error() {
        let t = tree(&[
            ("brink.toml", "[project]\ndialect = \"sideways\"\n"),
            ("main.brink", "flow main() {}"),
        ]);
        let err = Project::load(&t, "main.brink", &OptionOverrides::default())
            .expect_err("invalid dialect value must fail load");
        assert!(matches!(err, LoadError::Config { .. }));
    }

    /// #1369: the `Config` error must name the discovered `brink.toml`'s
    /// path — lost since #1306 when the variant became a bare
    /// `#[from] ConfigError` with no path carried alongside it.
    #[test]
    fn malformed_brink_toml_error_names_its_path() {
        let t = tree(&[
            ("brink.toml", "[project]\ndialect = \"sideways\"\n"),
            ("main.brink", "flow main() {}"),
        ]);
        let err = Project::load(&t, "main.brink", &OptionOverrides::default())
            .expect_err("invalid dialect value must fail load");
        let LoadError::Config { path, .. } = &err else {
            unreachable!("expected LoadError::Config, got {err:?}");
        };
        assert_eq!(path, "brink.toml");
        assert!(
            err.to_string().contains("brink.toml"),
            "error message must name the malformed file, got: {err}"
        );
    }

    /// Nested discovery (walking up from a subdirectory) must report the
    /// `brink.toml`'s actual discovered *key* — a multi-segment root-relative
    /// path — not just a bare filename. A `brink.toml` at the tree root
    /// (the previous fixture) discovers as the bare `"brink.toml"` key
    /// itself, which the preceding test already proves; this fixture nests
    /// the `brink.toml` too, so `path` only passes if the discovered key is
    /// actually threaded through rather than, say, a hardcoded filename.
    #[test]
    fn malformed_brink_toml_error_names_its_nested_path() {
        let t = tree(&[
            ("chapters/brink.toml", "[project]\ndialect = \"sideways\"\n"),
            ("chapters/deep/main.brink", "flow main() {}"),
        ]);
        let err = Project::load(&t, "chapters/deep/main.brink", &OptionOverrides::default())
            .expect_err("invalid dialect value must fail load");
        let LoadError::Config { path, .. } = &err else {
            unreachable!("expected LoadError::Config, got {err:?}");
        };
        assert_eq!(path, "chapters/brink.toml");
        assert!(
            err.to_string().contains("chapters/brink.toml"),
            "error message must name the nested malformed file, got: {err}"
        );
    }

    /// #1369's other half: a `brink.toml` that is *discovered* but fails to
    /// *read* (non-UTF-8 bytes) must also name its path — the failure mode
    /// that fell through the bare `#[from] io::Error` on `LoadError::Io`
    /// pre-fix, since `RealFs::read` (`fs::read_to_string`) returns a std
    /// I/O error with no path attached. `InMemory` can't represent invalid
    /// UTF-8 (its map is `String`-keyed and -valued), so this exercises the
    /// real filesystem via `RealFs` instead.
    #[test]
    fn unreadable_brink_toml_error_names_its_path() {
        let dir = std::env::temp_dir().join(format!(
            "brink-environment-unreadable-config-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Invalid UTF-8: a lone continuation byte can never start a valid
        // UTF-8 sequence, so `fs::read_to_string` fails with `InvalidData`.
        std::fs::write(dir.join("brink.toml"), [0x80_u8, 0x81, 0x82]).unwrap();
        std::fs::write(dir.join("main.brink"), "flow main() {}").unwrap();

        let t = brink_driver::RealFs::new(&dir);
        let err = Project::load(&t, "main.brink", &OptionOverrides::default())
            .expect_err("non-UTF-8 brink.toml must fail load");
        let LoadError::ConfigRead { path, .. } = &err else {
            unreachable!("expected LoadError::ConfigRead, got {err:?}");
        };
        assert_eq!(path, "brink.toml");
        assert!(
            err.to_string().contains("brink.toml"),
            "error message must name the unreadable file, got: {err}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── [lints] resolution (issue #1160) ──────────────────────────────
    //
    // `Project::load` is the ONE point that folds a discovered `brink.toml`
    // into the resolved `AnalysisOptions` (via
    // `AnalysisOptions::apply_project_config`) — these tests exercise that
    // exact seam, then prove the resolved policy is actually consulted by
    // `compile`'s error gate (not just stored inertly on `Environment`).

    /// A logic line with no effect (`~` alone) — `DiagnosticCode::E014`,
    /// `Warning` by default (`brink_ir::hir::lower::tests::
    /// logic_line_emits_diagnostic_on_malformed`).
    const E014_SOURCE: &str = "Hello.\n~\n-> END\n";

    #[test]
    fn brink_toml_lints_table_resolves_into_options() {
        let t = tree(&[
            ("brink.toml", "[lints]\nE014 = \"deny\"\n"),
            ("main.ink", E014_SOURCE),
        ]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        assert_eq!(
            env.options.lints.overrides.get("E014"),
            Some(&LintLevel::Deny)
        );
    }

    #[test]
    fn brink_toml_deny_warnings_resolves_into_options() {
        let t = tree(&[
            ("brink.toml", "[lints]\ndeny-warnings = true\n"),
            ("main.ink", E014_SOURCE),
        ]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        assert!(env.options.lints.deny_warnings);
    }

    #[test]
    fn absent_lints_table_leaves_options_lints_at_default() {
        let t = tree(&[("main.ink", E014_SOURCE)]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        assert_eq!(env.options.lints, brink_driver::LintPolicy::default());
    }

    #[test]
    fn e014_warning_compiles_cleanly_by_default() {
        // No `[lints]` table: E014 stays a Warning, never blocks compile —
        // "absent table = today's behavior" acceptance criterion.
        let t = tree(&[("main.ink", E014_SOURCE)]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let out = compile(&env).expect("a Warning-only diagnostic must not block compilation");
        assert!(
            out.warnings
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E014),
            "expected E014 among the warnings: {:?}",
            out.warnings
        );
    }

    #[test]
    fn brink_toml_lints_deny_relevels_e014_and_blocks_compile() {
        // The same source as above, but `[lints] E014 = "deny"` re-levels
        // it to Error — this must now fail the same `has_errors`-style
        // gate `compile` reads through `Environment.options`.
        let t = tree(&[
            ("brink.toml", "[lints]\nE014 = \"deny\"\n"),
            ("main.ink", E014_SOURCE),
        ]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let err = compile(&env).expect_err("a denied E014 must block compilation");
        let CompileError::Diagnostics(diags) = err else {
            unreachable!("expected CompileError::Diagnostics, got {err:?}");
        };
        assert!(
            diags
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E014),
            "expected E014 among the surfaced diagnostics: {diags:?}"
        );
    }

    #[test]
    fn brink_toml_deny_warnings_blocks_compile_on_an_unconfigured_warning() {
        // `deny-warnings = true` with no per-code override: E014 (an
        // ordinary, unconfigured Warning) is still promoted to Error.
        let t = tree(&[
            ("brink.toml", "[lints]\ndeny-warnings = true\n"),
            ("main.ink", E014_SOURCE),
        ]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let err = compile(&env).expect_err("deny-warnings must promote E014 to a compile error");
        assert!(matches!(err, CompileError::Diagnostics(_)));
    }

    // ── OptionOverrides.lints / .deny_warnings: CLI/API tier (#1373) ──

    #[test]
    fn override_lints_resolves_into_options() {
        let t = tree(&[("main.ink", E014_SOURCE)]);
        let mut lints = BTreeMap::new();
        lints.insert("E014".to_owned(), LintLevel::Deny);
        let env = Project::load(
            &t,
            "main.ink",
            &OptionOverrides {
                lints,
                ..OptionOverrides::default()
            },
        )
        .expect("loads");
        assert_eq!(
            env.options.lints.overrides.get("E014"),
            Some(&LintLevel::Deny)
        );
    }

    #[test]
    fn override_deny_e014_blocks_compile_with_no_brink_toml() {
        // No `brink.toml` at all — a CLI `--deny E014` alone must still
        // relevel E014 to Error and block compilation, exactly as a file's
        // `[lints] E014 = "deny"` already does.
        let t = tree(&[("main.ink", E014_SOURCE)]);
        let mut lints = BTreeMap::new();
        lints.insert("E014".to_owned(), LintLevel::Deny);
        let env = Project::load(
            &t,
            "main.ink",
            &OptionOverrides {
                lints,
                ..OptionOverrides::default()
            },
        )
        .expect("loads");
        let err = compile(&env).expect_err("CLI --deny E014 must block compilation");
        let CompileError::Diagnostics(diags) = err else {
            unreachable!("expected CompileError::Diagnostics, got {err:?}");
        };
        assert!(
            diags
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E014),
            "expected E014 among the surfaced diagnostics: {diags:?}"
        );
    }

    #[test]
    fn override_deny_warnings_blocks_compile_with_no_brink_toml() {
        let t = tree(&[("main.ink", E014_SOURCE)]);
        let env = Project::load(
            &t,
            "main.ink",
            &OptionOverrides {
                deny_warnings: Some(true),
                ..OptionOverrides::default()
            },
        )
        .expect("loads");
        let err = compile(&env).expect_err("CLI -D warnings must promote E014 to a compile error");
        assert!(matches!(err, CompileError::Diagnostics(_)));
    }

    #[test]
    fn override_lints_wins_over_a_conflicting_brink_toml_entry() {
        // `brink.toml` denies E014; the CLI override allows it — the CLI
        // must win (#1005/#1373's `CLI/API > file > default` precedence),
        // so the same source now compiles cleanly.
        let t = tree(&[
            ("brink.toml", "[lints]\nE014 = \"deny\"\n"),
            ("main.ink", E014_SOURCE),
        ]);
        let mut lints = BTreeMap::new();
        lints.insert("E014".to_owned(), LintLevel::Allow);
        let env = Project::load(
            &t,
            "main.ink",
            &OptionOverrides {
                lints,
                ..OptionOverrides::default()
            },
        )
        .expect("loads");
        assert_eq!(
            env.options.lints.overrides.get("E014"),
            Some(&LintLevel::Allow),
            "the CLI override must replace the file's E014 = deny"
        );
        compile(&env).expect("CLI --allow E014 must win over brink.toml's E014 = deny");
    }

    // ── native universe = whole tree; brink.toml never a source ──────

    #[test]
    fn native_universe_is_the_whole_tree_excluding_config() {
        let t = tree(&[
            ("brink.toml", "[project]\n"),
            ("main.brink", "flow main() {}"),
            ("lib/util.brink", "flow util() {}"),
            ("README.md", "not source"),
        ]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");
        let keys: Vec<_> = env.source_keys().collect();
        // The mounted stdlib key (#2080) sorts between "main.brink" and
        // nothing else here, since 'm' < 's'.
        assert_eq!(
            keys,
            vec![
                "lib/util.brink",
                "main.brink",
                "std/conventions/screenplay.brink"
            ]
        );
    }

    #[test]
    fn dotdot_native_key_is_rejected() {
        struct Hostile;
        impl SourceTree for Hostile {
            fn list(&self) -> io::Result<Vec<String>> {
                Ok(vec!["a.brink".to_string(), "../escape.brink".to_string()])
            }
            fn read(&self, key: &str) -> io::Result<String> {
                Ok(format!("-- {key} --"))
            }
        }
        let err = Project::load(&Hostile, "a.brink", &OptionOverrides::default())
            .expect_err("dotdot key must be rejected");
        assert!(matches!(err, LoadError::InvalidSourceKey(k) if k == "../escape.brink"));
    }

    // ── ink INCLUDE discovery ────────────────────────────────────────

    #[test]
    fn ink_universe_follows_the_include_graph() {
        let t = tree(&[
            ("main.ink", "INCLUDE lib.ink\nHello.\n-> END\n"),
            ("lib.ink", "== helper ==\n-> DONE\n"),
            ("unreferenced.ink", "== orphan ==\n-> DONE\n"),
        ]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let keys: Vec<_> = env.source_keys().collect();
        // The orphan is not INCLUDE-reachable, so it is not in the universe.
        // The stdlib mount (#2080) is unconditional — it joins an ink
        // project's environment too, sorting after "main.ink".
        assert_eq!(
            keys,
            vec!["lib.ink", "main.ink", "std/conventions/screenplay.brink"]
        );
    }

    // ── stdlib mount (#2080) ──────────────────────────────────────────

    #[test]
    fn stdlib_screenplay_preset_is_mounted_into_every_environment() {
        let t = tree(&[("main.brink", "flow main() {}")]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");

        let mounted = env
            .source_text("std/conventions/screenplay.brink")
            .expect("the built-in screenplay preset must be mounted into every Environment");
        assert!(
            mounted.contains("heading"),
            "mounted stdlib text looks wrong (embed path misconfigured?): {mounted}"
        );
    }

    /// Reverting `mount_stdlib` (or not calling it from `Project::load`)
    /// makes this fail: `source_text` for the stdlib key returns `None`.
    #[test]
    fn stdlib_mount_is_present_for_a_native_and_an_ink_entry_alike() {
        let native = Project::load(
            &tree(&[("main.brink", "flow main() {}")]),
            "main.brink",
            &OptionOverrides::default(),
        )
        .expect("loads");
        let ink = Project::load(
            &tree(&[("main.ink", "Hello.\n-> END\n")]),
            "main.ink",
            &OptionOverrides::default(),
        )
        .expect("loads");

        assert!(
            native
                .source_text("std/conventions/screenplay.brink")
                .is_some()
        );
        assert!(
            ink.source_text("std/conventions/screenplay.brink")
                .is_some()
        );
    }

    #[test]
    fn a_project_source_at_the_stdlib_key_wins_over_the_embedded_copy() {
        let t = tree(&[
            ("main.brink", "flow main() {}"),
            (
                "std/conventions/screenplay.brink",
                "// project-authored override\nflow overridden() {}",
            ),
        ]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");
        assert_eq!(
            env.source_text("std/conventions/screenplay.brink")
                .as_deref(),
            Some("// project-authored override\nflow overridden() {}"),
            "a project's own file at the stdlib's key must win, not be silently clobbered \
             by the embedded copy"
        );
    }

    #[test]
    fn mounted_stdlib_compiles_cleanly_alongside_an_ordinary_native_project() {
        // Proves the mount reaches the real, sole production compile path
        // (`brink_environment::compile`, per #2080's ruling) — not just
        // that the manifest holds the text. A plain native project must
        // still compile, and the mounted stdlib module must not itself
        // introduce any diagnostic.
        //
        // `warnings.is_empty()` alone is vacuous here (review finding on
        // #2080): a bare `main.brink` project compiles with zero warnings
        // whether or not `mount_stdlib` ran at all, so it cannot
        // distinguish "the mount reached the compile" from "the mount
        // didn't happen". A native entry's compilation universe is
        // "tree is universe" (every `.brink` key joins), so the mounted
        // screenplay preset's `heading` handler — which declares
        // `extern scene_entered(title, slug)` — must actually land in the
        // compiled `StoryData`'s externals table. Assert that too.
        let t = tree(&[("main.brink", "flow main() { Hello. }")]);
        let env = Project::load(&t, "main.brink", &OptionOverrides::default()).expect("loads");
        let out = compile(&env).expect(
            "a plain native project must compile cleanly with the stdlib mounted alongside it",
        );
        assert!(
            out.warnings.is_empty(),
            "the mounted stdlib module must not itself introduce diagnostics: {:?}",
            out.warnings
        );
        let has_scene_entered_extern = out.data.externals.iter().any(|ext| {
            out.data
                .name_table
                .get(ext.name.0 as usize)
                .is_some_and(|name| name == "scene_entered")
        });
        assert!(
            has_scene_entered_extern,
            "the mounted screenplay preset's `heading` handler declares \
             `extern scene_entered(title, slug)` — its absence from the \
             compiled externs means the mount never reached the compile at \
             all, got externs: {:?}",
            out.data.externals
        );
    }

    #[test]
    fn stdlib_mount_is_manifest_only_for_an_ink_entry() {
        // Distinguishes native-entry reachability (asserted just above)
        // from ink-entry reachability, which review found is NOT the
        // same: `brink-db`'s `compilation_closure_files` walks an ink
        // entry's `INCLUDE` graph (`topological_order`), and the mounted
        // `.brink` key has no `INCLUDE` edge into it, so it is excluded
        // from an ink compile's closure entirely — present in the
        // `Environment`'s manifest, never lowered into LIR, contributing
        // nothing to the compiled story. The PR's original changeset and
        // reachability prose claimed the mount is "compiled as an
        // ordinary native module alongside the project's own files" /
        // "participates in native module resolution exactly like any
        // project file" for every compile — true for a native entry
        // (previous test), false for an ink one (this test), which is
        // `@brink-lang/web`'s ordinary case.
        let t = tree(&[("main.ink", "Hello.\n-> END\n")]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let out = compile(&env).expect(
            "a plain ink project must compile cleanly with the stdlib mounted alongside it",
        );
        assert!(
            out.warnings.is_empty(),
            "the mounted stdlib module must not itself introduce diagnostics \
             for an ink entry either: {:?}",
            out.warnings
        );
        let has_scene_entered_extern = out.data.externals.iter().any(|ext| {
            out.data
                .name_table
                .get(ext.name.0 as usize)
                .is_some_and(|name| name == "scene_entered")
        });
        assert!(
            !has_scene_entered_extern,
            "an ink entry's compilation closure must NOT include the \
             mounted, manifest-only stdlib module (no INCLUDE edge reaches \
             it) — its presence here would mean the mount reaches ink \
             compiles too, contradicting the manifest-only scope fence: \
             {:?}",
            out.data.externals
        );
    }

    #[test]
    fn mounted_stdlib_introduces_no_diagnostics_under_types_strict() {
        // Review finding on #2080: the mounted module now sits inside
        // every native project's compilation closure, but the only
        // existing compile test for it (`mounted_stdlib_compiles_cleanly_
        // alongside_an_ordinary_native_project`, above) runs under
        // `AnalysisOptions::default()`, which resolves `TypePolicy::
        // Gradual`. A real `.brink` project setting `dialect = brink` in
        // `brink.toml` resolves `TypePolicy::Strict` instead
        // (`tier1_native_strict.rs`'s own module doc), and any strict
        // diagnostic the mounted module produced would then fail every
        // strict build. It is clean today — `tier1_native_strict.rs`'s
        // baseline has zero rows for `conventions-screenplay-preset` —
        // which is exactly what makes this guard cheap to add now, before
        // the module grows and a strict finding sneaks in unnoticed.
        let t = tree(&[("main.brink", "flow main() { Hello. }")]);
        let overrides = OptionOverrides {
            types: Some(TypePolicy::Strict),
            ..OptionOverrides::default()
        };
        let env = Project::load(&t, "main.brink", &overrides).expect("loads");
        let out = compile(&env).expect(
            "a plain native project must compile cleanly under types = strict \
             with the stdlib mounted alongside it",
        );
        assert!(
            out.warnings.is_empty(),
            "the mounted stdlib module must not itself introduce diagnostics \
             under types = strict: {:?}",
            out.warnings
        );
    }

    // ── the pure compile over the input ──────────────────────────────

    #[test]
    fn compile_over_environment_produces_story_data() {
        let t = tree(&[("main.ink", "Hello, world.\n-> END\n")]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let out = compile(&env).expect("compiles");
        // A real story compiled: at least one container of instructions.
        assert!(
            !out.data.containers.is_empty(),
            "expected compiled containers"
        );
    }

    #[test]
    fn compile_surfaces_diagnostics_as_a_compile_error() {
        // Extension syntax under the default (strict-ink) dialect is rejected.
        let t = tree(&[("main.ink", "VAR arr = 0\n~ { arr = #[1, 2, 3] }\n-> END\n")]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let err = compile(&env).expect_err("strict-ink must reject extension syntax");
        assert!(matches!(err, CompileError::Diagnostics(_)));
    }

    #[test]
    fn load_then_compile_matches_across_a_serialize_round_trip() {
        let t = tree(&[("main.ink", "Hello.\n-> END\n")]);
        let env = Project::load(&t, "main.ink", &OptionOverrides::default()).expect("loads");
        let json = serde_json::to_string(&env).expect("serializes");
        let back: Environment = serde_json::from_str(&json).expect("deserializes");

        let a = compile(&env).expect("compiles");
        let b = compile(&back).expect("compiles from round-tripped env");
        // Same input value → same compiled bytes.
        let mut buf_a = String::new();
        let mut buf_b = String::new();
        brink_format::write_inkt(&a.data, &mut buf_a).expect("inkt a");
        brink_format::write_inkt(&b.data, &mut buf_b).expect("inkt b");
        assert_eq!(buf_a, buf_b);
    }
}
