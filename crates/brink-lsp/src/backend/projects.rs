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

use std::collections::HashMap;
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
/// [`NativeProjects::resync_roots`].
#[derive(Debug, Clone, Default)]
struct NativeRootsContext {
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
}

impl NativeProjects {
    pub(crate) fn new() -> Self {
        let mut this = Self {
            ctx: NativeRootsContext::default(),
            entries: Vec::new(),
            key_index: HashMap::new(),
            owner: HashMap::new(),
        };
        this.ensure_index(NativeProjectKey::Default);
        this
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
        let id_base = u32::try_from(self.entries.len())
            .unwrap_or(u32::MAX / ID_STRIDE)
            .saturating_mul(ID_STRIDE);
        let mut db = brink_db::ProjectDb::with_id_base(id_base);
        let root = key
            .root_dir(self.ctx.default_root.as_deref())
            .map(|p| p.to_string_lossy().into_owned());
        db.set_native_root(root.clone());
        if key == NativeProjectKey::Default {
            db.set_ink_root(root);
        }
        let idx = self.entries.len();
        self.entries.push(ProjectEntry {
            key: key.clone(),
            db,
        });
        self.key_index.insert(key, idx);
        idx
    }

    /// Re-run `brink.toml` discovery (issue #1580) and re-sync every
    /// already-known project's own root. See the module doc's "Known
    /// limitation" for what this does *not* do (retroactively move an
    /// already-admitted file to a newly discovered sibling project).
    pub(super) fn resync_roots(&mut self, roots: &[PathBuf], outcome: &ConfigLoadOutcome) {
        let default_root = super::native_source_root(roots, outcome);
        let default_is_configured = outcome.path.is_some();
        let other_roots = discover_other_native_roots(roots, default_root.as_deref());
        self.ctx = NativeRootsContext {
            default_root,
            default_is_configured,
            other_roots,
        };
        for entry in &mut self.entries {
            let root = entry
                .key
                .root_dir(self.ctx.default_root.as_deref())
                .map(|p| p.to_string_lossy().into_owned());
            entry.db.set_native_root(root.clone());
            if entry.key == NativeProjectKey::Default {
                entry.db.set_ink_root(root);
            }
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
    /// workspace-wide view `workspace/symbol` needs.
    pub(crate) fn all_file_metadata(&self) -> Vec<(FileId, String, String)> {
        self.entries
            .iter()
            .flat_map(|e| e.db.file_metadata())
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
            let meta = db.file_metadata();
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
    use super::{NativeProjectKey, NativeRootsContext};
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
}
