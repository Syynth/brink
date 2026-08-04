//! Native project-extent partitioning (issue #1580, RULED 2026-08-03).
//!
//! The LSP used to hold exactly one [`brink_db::ProjectDb`], with exactly
//! one native source root (`Backend::register_native_root`, applying
//! `brink_driver::native_source_root` once to `roots.first()`). A repo with
//! more than one governing `brink.toml` — story sources plus a fixture or
//! example project elsewhere in the tree — got one editor project the real
//! build never forms: a `.brink` file outside the single recognized root
//! kept its raw absolute-path-embedding identity (`root_relative_key`
//! leaves an out-of-root path unchanged) instead of the clean,
//! root-relative, compile-identical name a real build of *its own*
//! `brink.toml` would mint.
//!
//! [`NativeProjects`] fixes the *cardinality*, not the rule: every
//! discovered `brink.toml` gets its own, fully independent
//! [`brink_db::ProjectDb`] — its own native root, its own symbol index, its
//! own `DefinitionId` space — rather than one shared db trying to be
//! multi-root-aware. Two projects' same-relative-path files are *expected*
//! to mint the identical `DefinitionId` (module identity is a pure function
//! of the root-relative path, #1576) — that is only safe because they never
//! share a lookup table to collide in. Namespacing the keys instead (one
//! shared db, tagged identities) was considered and rejected: it would
//! diverge from what a real compile of either project alone mints, which is
//! the exact divergence class the ruling calls out as unacceptable.
//!
//! ## Project extent policy
//!
//! - **The legacy default project** (`NativeProjectKey::Default`): computed
//!   exactly as before (`native_source_root`, walking up from the first
//!   workspace root) — unchanged so the overwhelmingly common single-project
//!   workspace never re-partitions. Every `.ink` file always lives here too
//!   (ink's own project extent is INCLUDE-reachability, unaffected by this
//!   issue).
//! - **Every other discovered `brink.toml`** (`NativeProjectKey::Root`):
//!   found by walking every workspace folder *downward* (siblings, not just
//!   ancestors — `native_source_root`'s walk-up alone can never find a
//!   sibling directory's config). Each becomes its own project, rooted at
//!   its own directory.
//! - **A `.brink` file under no `brink.toml` at all** — the OWED, not
//!   formally ruled, question the #1580 ruling flags. Leaning on
//!   decision-log's NF-3 ("a rootless `.brink` file is a named single-file
//!   project whose root is its own directory") **only when some other part
//!   of the workspace has a real `brink.toml`** (`NativeProjectKey::Orphan`)
//!   — i.e. exactly the issue's own "fixture/example files elsewhere in the
//!   tree" scenario. When the *whole* workspace has no `brink.toml`
//!   anywhere, every native file still falls back to the single legacy
//!   default project: that preserves the pre-#1580 "open a folder of
//!   `.brink` files with no config yet" workflow byte-for-byte (see
//!   `crates/brink-lsp/tests/integration.rs`'s "Native `.brink` workspaces"
//!   tests, which exercise exactly this with zero `brink.toml` anywhere) —
//!   NF-3's "single-file project" mode read literally would silently turn
//!   every such workspace into N unrelated islands with no cross-file
//!   navigation at all, which is not this issue's concern to force.
//!
//! ## Known limitation
//!
//! A `brink.toml` that appears, moves, or disappears mid-session re-syncs
//! every *already-discovered* project's own root (mirroring pre-#1580
//! `register_native_root`), but does not retroactively move an
//! already-admitted file to a *newly* discovered sibling project — that
//! file keeps whatever project it was first classified into until it is
//! closed and reopened (or the session restarts). Only the steady-state
//! extent (what a fresh workspace load computes) is guaranteed correct.
//!
//! ## Known non-compile-identical carve-out
//!
//! The wholly-unconfigured-workspace branch above is a **knowing exception**
//! to "editor extent equals compile extent", not an instance of it. A real
//! standalone compile of a rootless `.brink` file roots it at *that file's
//! own directory* (`brink_driver::native_source_root`'s entry-relative
//! fallback, `native_source_root_inner`) — a per-entry root, same as NF-3's
//! single-file-project mode. The legacy default project this branch falls
//! back to is rooted at the *first workspace folder* instead, which only
//! agrees with the real per-entry root when the file happens to sit at that
//! folder's top level. The divergence is deliberately accepted to preserve
//! the pre-#1580 "open a folder of `.brink` files with no config yet"
//! workflow (see the policy bullet above), not because it is actually
//! compile-identical — a workspace-wide claim to that effect must carry this
//! qualifier.
//!
//! It is also, by the same token, the one case where an unrelated
//! `brink.toml` appearing *elsewhere* in the workspace changes a file's
//! module identity without anything changing above that file's own
//! directory — `classify`'s `None` arm routes to
//! [`NativeProjectKey::Default`] only while `other_roots` is empty and
//! `Orphan` (a different, per-entry root) the moment any other governing
//! `brink.toml` exists. `docs/decision-log.md`'s NF-3 ruling (2026-07-22,
//! lines ~1836-1842) calls exactly this shape of hazard out by name ("the
//! same file resolved to `story::market::barter` or `story::barter`
//! depending on whether a config happened to sit above it") and requires it
//! be a *named, documented* mode rather than a silent fallback — which is
//! what this paragraph and the policy bullet above are for. It remains an
//! open question, not resolved by this issue, whether the wholly-unconfigured
//! fallback should instead be `Orphan` unconditionally; that would restore
//! full compile-identity but re-fragment the "no config yet" workflow this
//! branch exists to preserve, which needs its own sign-off before changing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use brink_analyzer::AnalysisOptions;
use brink_ir::{Diagnostic, FileId, HirFile, SymbolManifest, suppressions::Suppressions};
use brink_syntax::Parse;

use super::ConfigLoadOutcome;

/// `FileId` numeric stride reserved per project (issue #1580): each
/// [`brink_db::ProjectDb`] mints its own `FileId`s starting at `0`
/// internally, so without disjoint ranges two projects' first-registered
/// files would both be `FileId(0)` — and `Backend`'s cross-project
/// `ProjectAnalyses` (keyed bare by `FileId`) would silently conflate them.
/// A million files in one native project is already an absurd amount of
/// source; `u32::MAX / STRIDE` (~4294) projects is far more than any real
/// workspace will ever discover.
const ID_STRIDE: u32 = 1_000_000;

/// The most projects [`ID_STRIDE`] can address before two projects' `FileId`
/// ranges would overlap (issue #1580 review finding) — see
/// [`NativeProjects::ensure_index`]'s guard.
const MAX_PROJECTS: usize = (u32::MAX / ID_STRIDE) as usize;

/// Which native project a `.brink` file belongs to — see the module doc for
/// the full policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum NativeProjectKey {
    /// The legacy single project every `.ink` file lives in too, and every
    /// native file falls back to.
    Default,
    /// Governed by a discovered `brink.toml` at this directory.
    Root(PathBuf),
    /// No governing `brink.toml` anywhere for this specific file, in a
    /// workspace where at least one exists elsewhere (NF-3's rootless
    /// single-file-project mode) — keyed by the file's own path so two
    /// orphan siblings never merge into one project.
    Orphan(PathBuf),
}

impl NativeProjectKey {
    /// The directory this project's `native_root`/`ink_root` salsa input
    /// should be set to, so its module identity is root-relative instead of
    /// the absolute-path-embedding fallback `root_relative_key` uses for a
    /// path outside any registered root.
    fn root_dir(&self, default_root: Option<&Path>) -> Option<PathBuf> {
        match self {
            NativeProjectKey::Default => default_root.map(Path::to_path_buf),
            NativeProjectKey::Root(dir) => Some(dir.clone()),
            // NF-3: a rootless file's root is its own *directory* (not the
            // file itself) — root-relative to that, the file's own name is
            // its whole relative key, exactly like a real single-file
            // compile of that directory would mint (`story::<stem>`)
            // instead of an absolute-path leak.
            NativeProjectKey::Orphan(file) => file.parent().map(Path::to_path_buf),
        }
    }
}

/// Everything native-project-extent classification needs, recomputed
/// whenever `brink.toml` discovery might have changed (workspace load, or a
/// watched-file `brink.toml` add/remove/edit) — see
/// [`NativeProjects::compute_roots_context`] and
/// [`NativeProjects::apply_roots_context`].
#[derive(Debug, Clone, Default)]
pub(crate) struct NativeRootsContext {
    /// `native_source_root`'s own result, unchanged.
    default_root: Option<PathBuf>,
    /// Whether `default_root` came from an actually-discovered `brink.toml`
    /// (vs. `native_source_root`'s bare `roots.first()` fallback) — gates
    /// the legacy whole-workspace fallback for a genuinely unconfigured
    /// workspace (see [`NativeRootsContext::classify`]).
    default_is_configured: bool,
    /// Every OTHER governing `brink.toml`'s directory, distinct from
    /// `default_root`.
    other_roots: Vec<PathBuf>,
}

impl NativeRootsContext {
    /// Classify a native (`.brink`) file's project by nearest-ancestor
    /// match among every known governing root (deepest directory wins, so a
    /// `brink.toml` nested *inside* the default root's own tree still gets
    /// its own project rather than being swallowed by the outer one).
    fn classify(&self, path: &Path) -> NativeProjectKey {
        let mut best: Option<(&Path, bool)> = None;
        if let Some(default_root) = self.default_root.as_deref()
            && path.starts_with(default_root)
        {
            best = Some((default_root, true));
        }
        for other in &self.other_roots {
            if !path.starts_with(other) {
                continue;
            }
            let better = match best {
                Some((cur, _)) => other.as_os_str().len() > cur.as_os_str().len(),
                None => true,
            };
            if better {
                best = Some((other.as_path(), false));
            }
        }
        match best {
            Some((_, true)) => NativeProjectKey::Default,
            Some((dir, false)) => NativeProjectKey::Root(dir.to_path_buf()),
            // No governing root at all for this file. A wholly unconfigured
            // workspace (no `brink.toml` anywhere) keeps the pre-#1580
            // single-shared-project fallback; a workspace that has a real
            // `brink.toml` *somewhere*, just not over this file, is exactly
            // the issue's "fixture/example file elsewhere" scenario.
            None if self.other_roots.is_empty() && !self.default_is_configured => {
                NativeProjectKey::Default
            }
            None => NativeProjectKey::Orphan(path.to_path_buf()),
        }
    }
}

/// Whether `path` names a native (`.brink`) source file — the only
/// extension this module's root-discovery/classification applies to; ink
/// files always resolve to [`NativeProjectKey::Default`] (ink's own project
/// extent is INCLUDE-reachability, out of this issue's scope).
fn is_native_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext == "brink")
}

/// Every `brink.toml` reachable by walking `roots` **downward** (siblings,
/// not just ancestors), excluding `default_root` itself — the LSP's
/// counterpart of `collect_source_files`'s pruned walk, hunting the config
/// file name instead of source extensions. `native_source_root`'s walk-*up*
/// finds a config *above* the first workspace root; this is what finds one
/// in a sibling subdirectory, which a walk-up from a single starting point
/// structurally cannot do.
fn discover_other_native_roots(roots: &[PathBuf], default_root: Option<&Path>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        for entry in brink_source_tree::Walk::new(root).flatten() {
            if entry.is_dir() {
                continue;
            }
            if entry.file_name().to_str() != Some(brink_project_config::CONFIG_FILE_NAME) {
                continue;
            }
            let path = entry.into_path();
            let Some(dir) = path.parent() else { continue };
            if Some(dir) == default_root {
                continue;
            }
            found.push(dir.to_path_buf());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Mount the stdlib source set (#2080's mount) into a freshly created
/// project `db` — the LSP counterpart of `brink_environment::Project::load`'s
/// own `mount_stdlib` call (issue #2198). Before this, `brink-lsp` built its
/// `ProjectDb`s through an entirely separate path that never mounted
/// anything, so a mounted std symbol was invisible to the editor even though
/// a real compile through `Environment::load` already saw it.
///
/// Keyed by `root_dir.join(key)` — an **absolute** path, matching this
/// module's own `set_native_root`/`set_ink_root` convention (see
/// `discover_native`'s `absolute_keys_plus_native_root_mint_compile_identical_
/// identity` test): `db`'s registered native/ink root strips that prefix back
/// down to the same root-relative key (`std/conventions/screenplay.brink`)
/// `Environment::load`'s manifest uses, so module identity mints identically
/// either way. `root_dir` is `None` only for the rare rootless-and-wholly-
/// unconfigured-workspace fallback (see the module doc's "Known
/// non-compile-identical carve-out") — there is no real root to key an
/// absolute path against there, so mounting is skipped rather than minting
/// an ambiguous identity; a real compile of that same rootless tree has no
/// stable root either. Pulls the identical `(key, text)` pairs
/// `Environment::load` mounts from `brink_environment::stdlib_sources` (the
/// shared source of truth, #2198), never a second copy of the stdlib. A
/// project file already registered at the same key wins over the mounted
/// copy, mirroring `mount_stdlib`'s own precedence — `db.set_file` is called
/// before any project file has been admitted into a freshly created entry,
/// so this only ever matters for a re-mount of an already-created project,
/// which never happens (`ensure_index` only builds a `db` once per key).
///
/// Returns every mounted key's absolute path (whether newly inserted into
/// `db` or already present) — the caller must still route each one through
/// [`NativeProjects::owner`], since `db.set_file` alone only makes the key
/// resolvable *inside this one db*; `NativeProjects::file_id`/
/// `project_for_path` look the path up in `owner` first (issue #2198 review
/// finding: without this, a mounted file was reachable from inside its own
/// `ProjectDb` but invisible through every `NativeProjects`-level query, the
/// only surface any `brink-lsp` handler actually calls).
fn mount_stdlib(db: &mut brink_db::ProjectDb, root_dir: Option<&Path>) -> Vec<String> {
    let Some(root_dir) = root_dir else {
        return Vec::new();
    };
    let mut mounted = Vec::with_capacity(brink_environment::stdlib_sources().len());
    for (key, text) in brink_environment::stdlib_sources() {
        let abs = root_dir.join(key).to_string_lossy().into_owned();
        if db.file_id(&abs).is_none() {
            db.set_file(&abs, (*text).to_string());
        }
        mounted.push(abs);
    }
    mounted
}

/// One project entry: its classification key plus its own, fully
/// independent database.
struct ProjectEntry {
    key: NativeProjectKey,
    db: brink_db::ProjectDb,
}

/// One project's per-file analysis inputs (`root`, its members' `(FileId,
/// HirFile, SymbolManifest)` triples, and whether it's a native project) —
/// the same shape `db.compute_projects()` + `db.analysis_inputs_for()` +
/// `db.is_native()` produced for one db, now one entry per project.
pub(crate) type ProjectAnalysisInputs = (FileId, Vec<(FileId, HirFile, SymbolManifest)>, bool);

/// The per-project analysis inputs [`analysis_loop`](super::analysis_loop)
/// needs, snapshotted under one lock across every project (issue #1580 —
/// generalizes what used to be a single-db read into a multi-project one).
/// Merging is safe because every project's `FileId`s are disjoint
/// ([`ID_STRIDE`]).
pub(crate) struct AnalysisSnapshot {
    pub projects: Vec<ProjectAnalysisInputs>,
    pub modules: brink_analyzer::ModuleMap,
    pub module_diags: Vec<Diagnostic>,
    pub file_meta: Vec<(FileId, String, String)>,
    pub per_file_diags: Vec<(FileId, Vec<Diagnostic>)>,
    pub file_suppressions: HashMap<FileId, Suppressions>,
    pub manifests: HashMap<FileId, SymbolManifest>,
}

/// Multiple independent native projects, one [`brink_db::ProjectDb`] per
/// governing `brink.toml` (plus the legacy default and any orphan
/// single-file projects) — see the module doc.
///
/// Holds exactly the same place `Backend`'s single `Arc<Mutex<ProjectDb>>`
/// used to: one lock guards every project, matching the previous
/// single-db locking granularity (LSP requests are not so contended that
/// per-project locks would ever pay for their own complexity, and the
/// overwhelmingly common case is exactly one project anyway).
pub(crate) struct NativeProjects {
    ctx: NativeRootsContext,
    /// Creation order fixes each project's `FileId` range
    /// (`index * ID_STRIDE`) — never reordered or removed, even once a
    /// project's last file is closed, so existing `FileId`s never point at
    /// the wrong project after a later project is created.
    entries: Vec<ProjectEntry>,
    key_index: HashMap<NativeProjectKey, usize>,
    /// path -> owning project index, so routing is O(1) instead of scanning
    /// every project on every request.
    owner: HashMap<String, usize>,
    /// Every `FileId` that is a mounted stdlib file, not a real project file
    /// (issue #2198) — [`Self::snapshot_for_analysis`] excludes these from
    /// `file_meta` (and so from the `$/brink/backgroundAnalysisComplete`
    /// `file_count`, `publish_all_diagnostics`, and
    /// [`Self::all_file_metadata`]'s `workspace/symbol` listing), so the
    /// mount is observable to real analysis/resolution (it still joins
    /// `db.compute_projects()`/`analysis_inputs_for()`, matching a real
    /// compile's symbol universe) without perturbing the client-facing
    /// "how many of *my* files have been analyzed" protocol every
    /// `file_count`-synchronized test (and, in principle, a real client)
    /// relies on — a mount happening to exist is not a file the user
    /// opened or the workspace scan found on disk.
    mounted_std_ids: HashSet<FileId>,
}

impl NativeProjects {
    pub(crate) fn new() -> Self {
        let mut this = Self {
            ctx: NativeRootsContext::default(),
            entries: Vec::new(),
            key_index: HashMap::new(),
            owner: HashMap::new(),
            mounted_std_ids: HashSet::new(),
        };
        this.ensure_index(NativeProjectKey::Default);
        this
    }

    /// The [`NativeProjectKey::Default`] project's index — always present
    /// ([`Self::new`] creates it eagerly), looked up rather than hard-coded
    /// to `0` so this stays correct even if creation order ever changes.
    fn default_index(&self) -> usize {
        self.key_index
            .get(&NativeProjectKey::Default)
            .copied()
            .unwrap_or(0)
    }

    /// Get (creating if necessary) the project db for `key`, freshly
    /// registering its own native/ink root at creation time so the very
    /// first file admitted into it already mints compile-identical identity
    /// (mirrors `register_native_root`'s pre-#1580 "declare the root before
    /// any file is loaded" ordering, now per project).
    fn ensure_index(&mut self, key: NativeProjectKey) -> usize {
        if let Some(&idx) = self.key_index.get(&key) {
            return idx;
        }
        if self.entries.len() >= MAX_PROJECTS {
            // One more project would saturate `id_base` into (or past) an
            // earlier project's own `ID_STRIDE`-wide `FileId` range:
            // `project_for_file`'s `id.0 / ID_STRIDE` would then misroute the
            // new project's files onto whichever project already owns that
            // range, and `ProjectDb::set_file`'s `next_id += 1` would overflow
            // on the range's second file. `entries` is never pruned and
            // `NativeProjectKey::Orphan` mints one project per unrooted file,
            // so a pathological workspace can really reach this. Route into
            // the default project instead — degraded (shared identity space
            // with whatever else lives there) but never silently wrong.
            tracing::warn!(
                requested = ?key,
                project_count = self.entries.len(),
                "native project count reached ID_STRIDE capacity ({MAX_PROJECTS}); \
                 routing into the default project instead of minting an \
                 overlapping FileId range",
            );
            return self.default_index();
        }
        let id_base = u32::try_from(self.entries.len())
            .unwrap_or(u32::MAX)
            .saturating_mul(ID_STRIDE);
        let mut db = brink_db::ProjectDb::with_id_base(id_base);
        let root_dir = key.root_dir(self.ctx.default_root.as_deref());
        let root = root_dir
            .as_deref()
            .map(|p| p.to_string_lossy().into_owned());
        db.set_native_root(root.clone());
        if key == NativeProjectKey::Default {
            db.set_ink_root(root);
        }
        let mounted = mount_stdlib(&mut db, root_dir.as_deref());
        let mounted_ids: Vec<FileId> = mounted.iter().filter_map(|key| db.file_id(key)).collect();
        let idx = self.entries.len();
        self.entries.push(ProjectEntry {
            key: key.clone(),
            db,
        });
        self.key_index.insert(key, idx);
        for mounted_key in mounted {
            self.owner.insert(mounted_key, idx);
        }
        self.mounted_std_ids.extend(mounted_ids);
        idx
    }

    /// Run `brink.toml` discovery (issue #1580) and return the resulting
    /// context, WITHOUT applying it to any project.
    ///
    /// This performs a full recursive filesystem walk
    /// (`discover_other_native_roots`) and must be called *before* taking
    /// the `NativeProjects` lock — see the review finding on
    /// `Backend::register_native_root`, which calls this first and only
    /// takes the lock to hand the already-computed result to
    /// [`apply_roots_context`](Self::apply_roots_context). Doing the walk
    /// under the lock would block every other LSP request (`goto_definition`,
    /// `hover`, …) on a full workspace walk for as long as it takes the
    /// filesystem to answer — and a batch of `brink.toml` watched-file
    /// events calls this once per changed config, multiplying the stall.
    pub(super) fn compute_roots_context(
        roots: &[PathBuf],
        outcome: &ConfigLoadOutcome,
    ) -> NativeRootsContext {
        let default_root = super::native_source_root(roots, outcome);
        let default_is_configured = outcome.path.is_some();
        let other_roots = discover_other_native_roots(roots, default_root.as_deref());
        NativeRootsContext {
            default_root,
            default_is_configured,
            other_roots,
        }
    }

    /// Apply an already-computed [`NativeRootsContext`] (see
    /// [`compute_roots_context`](Self::compute_roots_context)) and re-sync
    /// every already-known project's own root. Touches no filesystem, so
    /// it's cheap to run under the `NativeProjects` lock. See the module
    /// doc's "Known limitation" for what this does *not* do (retroactively
    /// move an already-admitted file to a newly discovered sibling project).
    ///
    /// Also (re-)mounts the stdlib (issue #2198) into every entry: the
    /// eagerly-created [`NativeProjectKey::Default`] project
    /// ([`Self::new`]) is built *before* any workspace root is known, so
    /// [`ensure_index`](Self::ensure_index)'s own mount attempt at creation
    /// time sees `root_dir == None` and mounts nothing — this is the first
    /// point a real root exists for it. [`mount_stdlib`]'s own
    /// already-present guard makes calling it again for every other,
    /// already-mounted entry a no-op.
    pub(super) fn apply_roots_context(&mut self, ctx: NativeRootsContext) {
        self.ctx = ctx;
        let mut newly_mounted: Vec<(usize, String)> = Vec::new();
        for (idx, entry) in self.entries.iter_mut().enumerate() {
            let root_dir = entry.key.root_dir(self.ctx.default_root.as_deref());
            let root = root_dir
                .as_deref()
                .map(|p| p.to_string_lossy().into_owned());
            entry.db.set_native_root(root.clone());
            if entry.key == NativeProjectKey::Default {
                entry.db.set_ink_root(root);
            }
            for mounted_key in mount_stdlib(&mut entry.db, root_dir.as_deref()) {
                newly_mounted.push((idx, mounted_key));
            }
        }
        for (idx, mounted_key) in newly_mounted {
            if let Some(id) = self.entries[idx].db.file_id(&mounted_key) {
                self.mounted_std_ids.insert(id);
            }
            self.owner.insert(mounted_key, idx);
        }
    }

    /// Add or update a file, routing it to the correct project. An
    /// already-tracked path keeps whatever project it was first classified
    /// into (see the module doc's "Known limitation"); a new path is
    /// classified fresh against the current roots context.
    pub(crate) fn set_file(&mut self, path: &str, source: String) -> FileId {
        let idx = if let Some(&idx) = self.owner.get(path) {
            idx
        } else {
            let key = if is_native_path(path) {
                self.ctx.classify(Path::new(path))
            } else {
                NativeProjectKey::Default
            };
            self.ensure_index(key)
        };
        let id = self.entries[idx].db.set_file(path, source);
        self.owner.insert(path.to_owned(), idx);
        id
    }

    /// Identical to [`set_file`](Self::set_file) — `ProjectDb::update_file`
    /// is itself a plain alias of `set_file`.
    pub(crate) fn update_file(&mut self, path: &str, source: String) -> FileId {
        self.set_file(path, source)
    }

    pub(crate) fn remove_file(&mut self, path: &str) {
        if let Some(idx) = self.owner.remove(path) {
            self.entries[idx].db.remove_file(path);
        }
    }

    pub(crate) fn file_id(&self, path: &str) -> Option<FileId> {
        let &idx = self.owner.get(path)?;
        self.entries[idx].db.file_id(path)
    }

    /// The project db that owns `path` (already admitted), if any — for a
    /// caller that needs to hand a concrete `&brink_db::ProjectDb` to
    /// `brink-ide` (navigation, hover, rename, code actions all take one
    /// directly, not a project-agnostic view).
    pub(crate) fn project_for_path(&self, path: &str) -> Option<&brink_db::ProjectDb> {
        let &idx = self.owner.get(path)?;
        Some(&self.entries[idx].db)
    }

    /// The project db that owns an already-minted `FileId` (disjoint id
    /// ranges — see [`ID_STRIDE`]).
    fn project_for_file(&self, id: FileId) -> Option<&brink_db::ProjectDb> {
        let idx = (id.0 / ID_STRIDE) as usize;
        self.entries.get(idx).map(|e| &e.db)
    }

    pub(crate) fn source(&self, id: FileId) -> Option<&str> {
        self.project_for_file(id)?.source(id)
    }

    pub(crate) fn file_path(&self, id: FileId) -> Option<&str> {
        self.project_for_file(id)?.file_path(id)
    }

    pub(crate) fn hir(&self, id: FileId) -> Option<&HirFile> {
        self.project_for_file(id)?.hir(id)
    }

    pub(crate) fn manifest(&self, id: FileId) -> Option<&SymbolManifest> {
        self.project_for_file(id)?.manifest(id)
    }

    pub(crate) fn parse(&self, id: FileId) -> Option<&Parse> {
        self.project_for_file(id)?.parse(id)
    }

    pub(crate) fn suppressions(&self, id: FileId) -> Option<&Suppressions> {
        self.project_for_file(id)?.suppressions(id)
    }

    pub(crate) fn file_diagnostics(&self, id: FileId) -> Option<&[Diagnostic]> {
        self.project_for_file(id)?.file_diagnostics(id)
    }

    pub(crate) fn analysis_options_for(&self, id: FileId) -> Option<&AnalysisOptions> {
        Some(self.project_for_file(id)?.analysis_options())
    }

    /// `(FileId, path, source)` for every file in every project — the
    /// workspace-wide view `workspace/symbol` needs. Excludes mounted stdlib
    /// files (issue #2198, [`mounted_std_ids`](Self::mounted_std_ids)) — a
    /// mount is not a file the workspace scan found or the user opened, so
    /// it stays out of the client-facing file listing exactly as it stays
    /// out of [`snapshot_for_analysis`]'s `file_meta`.
    pub(crate) fn all_file_metadata(&self) -> Vec<(FileId, String, String)> {
        self.entries
            .iter()
            .flat_map(|e| e.db.file_metadata())
            .filter(|(id, _, _)| !self.mounted_std_ids.contains(id))
            .collect()
    }

    /// No-op today (`ProjectDb::rebuild_include_graph` is itself a no-op —
    /// salsa recomputes the include graph lazily), kept so
    /// `load_workspace_files`'s "rebuild after every file is loaded" intent
    /// stays documented per project rather than silently dropped.
    pub(crate) fn rebuild_include_graphs(&mut self) {
        for entry in &mut self.entries {
            entry.db.rebuild_include_graph();
        }
    }

    /// Snapshot every project's analysis inputs under one lock (see
    /// [`AnalysisSnapshot`]).
    pub(crate) fn snapshot_for_analysis(&mut self, opts: &AnalysisOptions) -> AnalysisSnapshot {
        let mut projects = Vec::new();
        let mut modules = brink_analyzer::ModuleMap::new();
        let mut module_diags = Vec::new();
        let mut file_meta = Vec::new();
        let mut per_file_diags = Vec::new();
        let mut file_suppressions = HashMap::new();
        let mut manifests = HashMap::new();

        for entry in &mut self.entries {
            let db = &mut entry.db;
            if db.analysis_options() != opts {
                db.set_analysis_options(opts.clone());
            }
            let project_defs = db.compute_projects();
            for (root, members) in &project_defs {
                projects.push((*root, db.analysis_inputs_for(members), db.is_native(*root)));
            }
            modules.extend(db.module_map().iter().map(|(k, v)| (*k, v.clone())));
            module_diags.extend(db.module_map_diagnostics().iter().cloned());
            // Excludes mounted stdlib files (issue #2198,
            // `mounted_std_ids`): a mount still fully participates in
            // `compute_projects`/`analysis_inputs_for` above (so real
            // analysis/resolution sees it, matching a real compile's
            // symbol universe), but it is not a file the client's own
            // workspace scan or `didOpen` produced, so it stays out of
            // `file_meta` — the client-facing `file_count`/per-file
            // diagnostic-publish/`workspace-symbol` surface.
            let meta: Vec<_> = db
                .file_metadata()
                .into_iter()
                .filter(|(fid, _, _)| !self.mounted_std_ids.contains(fid))
                .collect();
            for (fid, _, _) in &meta {
                if let Some(d) = db.file_diagnostics(*fid) {
                    per_file_diags.push((*fid, d.to_vec()));
                }
                if let Some(s) = db.suppressions(*fid) {
                    file_suppressions.insert(*fid, s.clone());
                }
                if let Some(m) = db.manifest(*fid) {
                    manifests.insert(*fid, m.clone());
                }
            }
            file_meta.extend(meta);
        }

        AnalysisSnapshot {
            projects,
            modules,
            module_diags,
            file_meta,
            per_file_diags,
            file_suppressions,
            manifests,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigLoadOutcome, NativeProjectKey, NativeProjects, NativeRootsContext};
    use std::path::{Path, PathBuf};

    fn ctx(
        default_root: Option<&str>,
        configured: bool,
        other_roots: &[&str],
    ) -> NativeRootsContext {
        NativeRootsContext {
            default_root: default_root.map(PathBuf::from),
            default_is_configured: configured,
            other_roots: other_roots.iter().map(PathBuf::from).collect(),
        }
    }

    /// A file under the default project's own root classifies as `Default`
    /// — the common single-project case must never re-partition.
    #[test]
    fn classify_under_default_root_is_default() {
        let c = ctx(Some("/ws/game"), true, &[]);
        assert_eq!(
            c.classify(Path::new("/ws/game/main.brink")),
            NativeProjectKey::Default
        );
    }

    /// A file under a sibling `brink.toml` — the issue's own two-root
    /// scenario — gets its own `Root` project, not the default's.
    #[test]
    fn classify_under_sibling_root_is_that_root() {
        let c = ctx(Some("/ws/game"), true, &["/ws/demo"]);
        assert_eq!(
            c.classify(Path::new("/ws/demo/main.brink")),
            NativeProjectKey::Root(PathBuf::from("/ws/demo"))
        );
    }

    /// A `brink.toml` nested *inside* the default root's own tree governs
    /// its own subtree — nearest-ancestor wins over the outer default root
    /// even though both technically contain the file.
    #[test]
    fn classify_nested_root_wins_over_default() {
        let c = ctx(Some("/ws"), true, &["/ws/fixtures/sub"]);
        assert_eq!(
            c.classify(Path::new("/ws/fixtures/sub/main.brink")),
            NativeProjectKey::Root(PathBuf::from("/ws/fixtures/sub"))
        );
        assert_eq!(
            c.classify(Path::new("/ws/other/main.brink")),
            NativeProjectKey::Default
        );
    }

    /// A file under no governing root at all, when the workspace has at
    /// least one `brink.toml` elsewhere, is its own orphan single-file
    /// project (NF-3, OWED lean) — not silently merged into the default.
    #[test]
    fn classify_orphan_when_some_root_exists_elsewhere() {
        let c = ctx(Some("/ws/game"), true, &[]);
        assert_eq!(
            c.classify(Path::new("/ws/scratch/stray.brink")),
            NativeProjectKey::Orphan(PathBuf::from("/ws/scratch/stray.brink"))
        );
    }

    /// A wholly unconfigured workspace (`default_root` is only
    /// `native_source_root`'s bare fallback, no `brink.toml` anywhere) keeps
    /// the pre-#1580 single-shared-project behavior — the common "no config
    /// yet" workflow must not fragment into unrelated single-file islands.
    #[test]
    fn classify_falls_back_to_default_when_wholly_unconfigured() {
        let c = ctx(Some("/ws"), false, &[]);
        assert_eq!(
            c.classify(Path::new("/ws/elsewhere/stray.brink")),
            NativeProjectKey::Default
        );
    }

    /// Issue #2198: `brink-lsp` builds its own `NativeProjects`/`ProjectDb`
    /// universe, entirely separate from `brink_environment::Project::load`
    /// (the one real production compile path that mounts the stdlib, #2080/
    /// #2190) — before this fix, `NativeProjects` never mounted anything, so
    /// the editor disagreed with a real compile the moment a project could
    /// see into `std/`. Runs the SAME two-step sequence
    /// `Backend::register_native_root` uses in production
    /// (`compute_roots_context` then `apply_roots_context`) against a real
    /// temp workspace, then proves a symbol the mounted std file declares is
    /// actually reachable through `NativeProjects`' own query surface — not
    /// merely that a mount call was added somewhere. Rule 20a verified: with
    /// both `mount_stdlib` call sites in this file reverted, this test fails
    /// (`file_id(&std_key)` is `None`, so the `assert!` panics before the
    /// symbol-index check is ever reached).
    #[test]
    fn native_projects_mounts_stdlib_and_a_std_symbol_is_visible() {
        let dir = temp_dir("stdlib-mount");
        std::fs::write(dir.join("brink.toml"), "[project]\ndialect = \"brink\"\n")
            .expect("write brink.toml");
        let main_source = "flow main() {\n  Hi. -> END\n}\n";
        std::fs::write(dir.join("main.brink"), main_source).expect("write main.brink");

        let mut projects = NativeProjects::new();
        let outcome = ConfigLoadOutcome {
            path: Some(dir.join("brink.toml")),
            diagnostic: None,
        };
        let roots_ctx = NativeProjects::compute_roots_context(std::slice::from_ref(&dir), &outcome);
        projects.apply_roots_context(roots_ctx);

        let main_path = dir.join("main.brink").to_string_lossy().into_owned();
        projects.set_file(&main_path, main_source.to_string());

        let std_key = dir
            .join("std/conventions/screenplay.brink")
            .to_string_lossy()
            .into_owned();
        assert!(
            projects.file_id(&std_key).is_some(),
            "the mounted std file must be a registered project file, got no \
             FileId for {std_key}"
        );

        let opts = brink_analyzer::AnalysisOptions {
            dialect: brink_analyzer::Dialect::Brink,
            ..brink_analyzer::AnalysisOptions::default()
        };
        let _ = projects.snapshot_for_analysis(&opts);

        let db = projects
            .project_for_path(&main_path)
            .expect("main.brink's owning project db");
        let index = db.symbol_index();
        assert!(
            index.by_name.contains_key("scene_entered"),
            "a symbol declared in the mounted std file must be visible in \
             the project's own symbol index, got names: {:?}",
            index.by_name.keys().collect::<Vec<_>>()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A fresh, empty temp directory, unique per call.
    fn temp_dir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "brink-lsp-projects-test-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
