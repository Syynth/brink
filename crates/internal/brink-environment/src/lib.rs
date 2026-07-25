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

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use brink_compiler::{CompileError, CompileOutput, ResolvedDiagnostic};
use brink_driver::{AnalysisOptions, Dialect, Driver, TypePolicy};
use brink_ir::Diagnostic;
use brink_project_config::{ConfigError, discover_from_entry_in_tree, parse_str};
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
/// means "the caller has no explicit value," so the discovered `brink.toml`
/// (or the default) governs that field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionOverrides {
    /// An explicit dialect (e.g. the CLI `--dialect` flag), if the caller set
    /// one.
    pub dialect: Option<Dialect>,
    /// An explicit type policy (e.g. the CLI `--types` flag), if set.
    pub types: Option<TypePolicy>,
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
    /// root-relative key, and the tree's `list("." )` enumerates root-relative
    /// keys including any `brink.toml`. The mount is responsible for rooting
    /// its tree (the CLI drains its project root into an in-memory tree; web /
    /// LSP push their own root-relative store).
    ///
    /// **Sync** — the only asynchrony a mount has (e.g. a bevy `AssetReader`)
    /// is quarantined in *building the tree*, before this runs, matching the
    /// `InkLoader` drain-then-compile pattern.
    ///
    /// Dispatches on `entry`'s extension: a `.brink` entry's universe is the
    /// whole native source tree (enumerate every `.brink` key); a `.ink` entry
    /// follows its `INCLUDE` graph from the entry (a BFS over the tree's
    /// reads).
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
        for key in tree.list(Path::new("."))? {
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
/// then apply the explicit overrides. The one resolution point every mount
/// inherits (#1005 precedence: `CLI/API > file > default`).
fn resolve_options(
    tree: &dyn SourceTree,
    entry: &str,
    overrides: &OptionOverrides,
) -> Result<AnalysisOptions, LoadError> {
    let mut options = AnalysisOptions::default();

    if let Some(config_key) = discover_from_entry_in_tree(tree, Path::new("."), entry)? {
        let text = tree.read(&config_key)?;
        let (config, warnings) = parse_str(&text)?;
        for warning in &warnings {
            // The producer is the effectful side; surfacing unknown-key
            // warnings here (rather than dropping them) preserves the CLI's
            // pre-#1306 "warn, never fail" behavior — a silent drop would be a
            // bug (house rule).
            tracing::warn!("[{config_key}] {warning}");
        }
        options.apply_project_config(
            &config,
            overrides.dialect.is_some(),
            overrides.types.is_some(),
        );
    }

    if let Some(dialect) = overrides.dialect {
        options.dialect = dialect;
    }
    if let Some(types) = overrides.types {
        options.types = Some(types);
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
/// `brink-compiler`'s own resolution, using only the public `ProjectDb` API.
fn resolve_diagnostics(
    db: &brink_driver::ProjectDb,
    diags: Vec<Diagnostic>,
) -> Vec<ResolvedDiagnostic> {
    diags
        .into_iter()
        .map(|d| ResolvedDiagnostic {
            path: db.file_path(d.file).unwrap_or_default().to_string(),
            file: d.file,
            range: d.range,
            message: d.message,
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
    /// never this error.
    #[error("project config error: {0}")]
    Config(#[from] ConfigError),
    /// A native source key is not root-relative (contains a `..` segment) — a
    /// save-key-identity guardrail against a `SourceTree` that violates the
    /// contract.
    #[error("invalid source key `{0}` (must be root-relative, no `..`)")]
    InvalidSourceKey(String),
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // Two manifest keys, one deduplicated stored blob (same content hash).
        assert_eq!(env.source_keys().count(), 2);
        assert_eq!(store.len(), 1);
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
        assert_eq!(keys, vec!["a.brink", "m.brink", "z.brink"]);
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
                types: None,
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
                types: None,
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

    #[test]
    fn malformed_brink_toml_is_a_load_error() {
        let t = tree(&[
            ("brink.toml", "[project]\ndialect = \"sideways\"\n"),
            ("main.brink", "flow main() {}"),
        ]);
        let err = Project::load(&t, "main.brink", &OptionOverrides::default())
            .expect_err("invalid dialect value must fail load");
        assert!(matches!(err, LoadError::Config(_)));
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
        assert_eq!(keys, vec!["lib/util.brink", "main.brink"]);
    }

    #[test]
    fn dotdot_native_key_is_rejected() {
        struct Hostile;
        impl SourceTree for Hostile {
            fn list(&self, _root: &Path) -> io::Result<Vec<String>> {
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
        assert_eq!(keys, vec!["lib.ink", "main.ink"]);
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
