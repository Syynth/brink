use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use brink_analyzer::{
    AnalysisOptions, AnalysisResult, BodyTypes, InferenceResult, InferredSig, Sig, SymbolMeta,
};
use brink_format::DefinitionId;
use brink_ir::suppressions::Suppressions;
use brink_ir::{Diagnostic, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolManifest};
use brink_syntax::Parse;
use salsa::Setter as _;
use tracing::debug;

use crate::queries::{
    BrinkDatabase, CompileProduct, DefKey, LirProduct, ProjectInput, ResolvedProject, SourceFile,
    analysis_query, call_site_diagnostics_query, call_site_metas_query, diagnostics_query,
    include_graph_query, infer_body_query, inferred_signature_query, lir_query, lowered_query,
    parse_query, per_file_diagnostics_query, resolutions_index_query, resolve_query,
    signature_query, story_data_query, suppressions_query, symbol_index_query,
    type_diagnostics_query, type_inference_query, value_meta_query,
};

/// Stateful incremental project database.
///
/// A thin, path-keyed shell around a [salsa](https://github.com/salsa-rs/salsa)
/// database: file texts are salsa inputs, and every derived artifact (parse
/// tree, HIR, symbol index, resolutions, LIR, `StoryData`) is a memoized
/// tracked query with real dependency tracking and early cutoff. Both the
/// compiler (one-shot) and LSP/IDE (long-lived) use this as their project
/// model; editor overlays are plain input writes.
pub struct ProjectDb {
    salsa: BrinkDatabase,
    project: ProjectInput,
    /// Live files only — every public accessor reads through this map, so
    /// tombstoned inputs (see `retired`) are invisible to consumers.
    files: HashMap<FileId, SourceFile>,
    path_to_id: HashMap<String, FileId>,
    id_to_path: HashMap<FileId, String>,
    /// Tombstoned salsa inputs from removed files, keyed by path — the
    /// durable path→`FileId` identity store (#536). Salsa never forgets an
    /// input, so [`remove_file`](Self::remove_file) parks the `SourceFile`
    /// here (text cleared) instead of dropping the handle; re-adding the
    /// same path reinstates it, reusing its original `FileId` so the old
    /// per-file memos are overwritten in place rather than leaking as
    /// permanently unreachable dead entries (rust-analyzer precedent).
    retired: HashMap<String, SourceFile>,
    next_id: u32,
}

impl ProjectDb {
    /// Create an empty project database.
    pub fn new() -> Self {
        let salsa = BrinkDatabase::default();
        let project = ProjectInput::new(&salsa, Vec::new(), None, AnalysisOptions::default());
        Self {
            salsa,
            project,
            files: HashMap::new(),
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            retired: HashMap::new(),
            next_id: 0,
        }
    }

    /// Add or replace a file. An existing file's text is overwritten in
    /// place (an input write); derived queries recompute lazily on next read.
    ///
    /// Path→`FileId` identity is durable (#536): re-adding a path that was
    /// previously [`remove_file`](Self::remove_file)d reinstates its original
    /// `FileId` and salsa input, so per-file memos are overwritten in place
    /// instead of accumulating under freshly-minted dead ids.
    pub fn set_file(&mut self, path: &str, source: String) -> FileId {
        if let Some(&id) = self.path_to_id.get(path) {
            if let Some(&file) = self.files.get(&id) {
                file.set_text(&mut self.salsa).to(source);
            }
            debug!(path, id = id.0, "set_file complete");
            return id;
        }

        // Reinstate a tombstoned input if this path existed before,
        // otherwise mint a fresh id + input.
        let file = if let Some(file) = self.retired.remove(path) {
            file.set_text(&mut self.salsa).to(source);
            file
        } else {
            let id = FileId(self.next_id);
            self.next_id += 1;
            SourceFile::new(&self.salsa, id, path.to_string(), source)
        };
        let id = file.file_id(&self.salsa);
        self.path_to_id.insert(path.to_string(), id);
        self.id_to_path.insert(id, path.to_string());
        self.files.insert(id, file);

        // A reinstated `FileId` can be smaller than later-minted ids, so a
        // plain push would break the list's `FileId` ordering — insert at
        // the sorted position instead.
        let mut list = self.project.files(&self.salsa).clone();
        let pos = list.partition_point(|f| f.file_id(&self.salsa).0 < id.0);
        list.insert(pos, file);
        self.project.set_files(&mut self.salsa).to(list);

        debug!(path, id = id.0, "set_file complete");
        id
    }

    /// Incrementally update a file. Identical to [`set_file`](Self::set_file):
    /// salsa's dependency tracking decides what recomputes.
    pub fn update_file(&mut self, path: &str, source: String) -> FileId {
        self.set_file(path, source)
    }

    /// Remove a file from the database.
    ///
    /// The salsa input is tombstoned, not forgotten (#536): salsa can never
    /// reclaim an input or the memos keyed on it, so the `SourceFile` is
    /// parked in `retired` with its text cleared (releasing the source and
    /// invalidating stale derived memos) while dropping out of the project
    /// file list and every path/id map. From a consumer's view the file is
    /// gone — enumeration, lookups, and INCLUDE resolution behave exactly as
    /// if it never existed; re-adding the path reuses its original `FileId`.
    pub fn remove_file(&mut self, path: &str) {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
            if let Some(file) = self.files.remove(&id) {
                file.set_text(&mut self.salsa).to(String::new());
                self.retired.insert(path.to_string(), file);
                let list: Vec<SourceFile> = self
                    .project
                    .files(&self.salsa)
                    .iter()
                    .copied()
                    .filter(|f| f.file_id(&self.salsa) != id)
                    .collect();
                self.project.set_files(&mut self.salsa).to(list);
            }
            if self.project.entry(&self.salsa) == Some(id) {
                self.project.set_entry(&mut self.salsa).to(None);
            }
        }
    }

    /// Set the compile entry point (for the [`lir_product`](Self::lir_product)
    /// and [`story_data`](Self::story_data) queries). The file must already
    /// be in the database.
    pub fn set_entry(&mut self, path: &str) -> Option<FileId> {
        let id = self.file_id(path)?;
        if self.project.entry(&self.salsa) != Some(id) {
            self.project.set_entry(&mut self.salsa).to(Some(id));
        }
        Some(id)
    }

    /// The current compile entry point, if any.
    pub fn entry(&self) -> Option<FileId> {
        self.project.entry(&self.salsa)
    }

    /// Set the analysis options (host manifest + external-check severity)
    /// used by the [`analysis`](Self::analysis) and downstream queries.
    pub fn set_analysis_options(&mut self, options: AnalysisOptions) {
        self.project
            .set_analysis_options(&mut self.salsa)
            .to(options);
    }

    /// The analysis options currently registered with the database.
    pub fn analysis_options(&self) -> &AnalysisOptions {
        self.project.analysis_options(&self.salsa)
    }

    /// Look up a file's ID by path.
    pub fn file_id(&self, path: &str) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }

    /// Look up a file's path by ID.
    pub fn file_path(&self, id: FileId) -> Option<&str> {
        self.id_to_path.get(&id).map(String::as_str)
    }

    /// Iterate over all registered file IDs.
    pub fn file_ids(&self) -> impl Iterator<Item = FileId> + '_ {
        let mut ids: Vec<_> = self.files.keys().copied().collect();
        ids.sort_by_key(|id| id.0);
        ids.into_iter()
    }

    /// Return file IDs in topological include order (included files before
    /// the files that include them), matching ink's `INCLUDE` paste semantics.
    pub fn file_ids_topo(&self, entry: FileId) -> Vec<FileId> {
        let all: Vec<_> = self.file_ids().collect();
        self.include_graph().topological_order(entry, &all)
    }

    /// Get the parse tree for a file.
    pub fn parse(&self, id: FileId) -> Option<&Parse> {
        let file = self.files.get(&id)?;
        Some(parse_query(&self.salsa, *file))
    }

    /// Get the HIR for a file.
    pub fn hir(&self, id: FileId) -> Option<&HirFile> {
        let file = self.files.get(&id)?;
        Some(&lowered_query(&self.salsa, *file).hir)
    }

    /// Get the symbol manifest for a file.
    pub fn manifest(&self, id: FileId) -> Option<&SymbolManifest> {
        let file = self.files.get(&id)?;
        Some(&lowered_query(&self.salsa, *file).manifest)
    }

    /// Get the source text for a file.
    pub fn source(&self, id: FileId) -> Option<&str> {
        let file = self.files.get(&id)?;
        Some(file.text(&self.salsa).as_str())
    }

    /// Get per-file diagnostics (parse + lowering).
    pub fn file_diagnostics(&self, id: FileId) -> Option<&[Diagnostic]> {
        let file = self.files.get(&id)?;
        Some(lowered_query(&self.salsa, *file).diagnostics.as_slice())
    }

    /// Get parsed suppression directives for a file.
    pub fn suppressions(&self, id: FileId) -> Option<&Suppressions> {
        let file = self.files.get(&id)?;
        Some(suppressions_query(&self.salsa, *file))
    }

    /// Rebuild include graph edges for all files.
    ///
    /// No-op since the salsa migration: the include graph is a tracked query
    /// over the full file set and is always complete. Kept so batch-loading
    /// call sites need no change.
    pub fn rebuild_include_graph(&mut self) {}

    /// Detect cycles in the include graph.
    ///
    /// Returns the first cycle found as an ordered path of file IDs.
    pub fn find_cycle(&self) -> Option<Vec<FileId>> {
        self.include_graph().find_cycle()
    }

    /// Compute independent projects from include relationships.
    ///
    /// Returns `(root, members)` pairs sorted by root `FileId`.
    pub fn compute_projects(&self) -> Vec<(FileId, Vec<FileId>)> {
        let all: Vec<_> = self.file_ids().collect();
        self.include_graph().compute_projects(&all)
    }

    /// All files reachable from `entry` via the forward `INCLUDE` graph,
    /// `entry` included.
    ///
    /// A forward DFS over `INCLUDE` edges (transitive). The result is a
    /// [`BTreeSet`], so iteration order is deterministic regardless of graph
    /// internals — callers that compare or render the set get stable output.
    pub fn reachable_from(&self, entry: FileId) -> BTreeSet<FileId> {
        self.include_graph().reachable_from(entry)
    }

    /// Snapshot analysis inputs for a subset of files.
    ///
    /// Like `analysis_inputs()` but filtered to the given set.
    pub fn analysis_inputs_for(
        &self,
        file_ids: &[FileId],
    ) -> Vec<(FileId, HirFile, SymbolManifest)> {
        let mut inputs: Vec<_> = file_ids
            .iter()
            .filter_map(|&id| {
                let file = self.files.get(&id)?;
                let lowered = lowered_query(&self.salsa, *file);
                Some((id, lowered.hir.clone(), lowered.manifest.clone()))
            })
            .collect();
        inputs.sort_by_key(|(id, _, _)| id.0);
        inputs
    }

    /// Snapshot all analysis inputs for background analysis.
    ///
    /// Returns `(FileId, HirFile, SymbolManifest)` tuples cloned out of the db,
    /// so the caller can run `brink_analyzer::analyze()` without holding the lock.
    pub fn analysis_inputs(&self) -> Vec<(FileId, HirFile, SymbolManifest)> {
        let ids: Vec<_> = self.file_ids().collect();
        self.analysis_inputs_for(&ids)
    }

    /// Snapshot file metadata for diagnostic publishing.
    ///
    /// Returns `(FileId, path, source)` tuples for all files in the db.
    pub fn file_metadata(&self) -> Vec<(FileId, String, String)> {
        let mut meta: Vec<_> = self
            .files
            .iter()
            .filter_map(|(&id, file)| {
                let path = self.id_to_path.get(&id)?.clone();
                Some((id, path, file.text(&self.salsa).clone()))
            })
            .collect();
        meta.sort_by_key(|(id, _, _)| id.0);
        meta
    }

    // ── Query surface (scripting-substrate spec §4) ──────────────────

    /// The merged project-wide symbol index (layer 2, `symbol_index()`).
    pub fn symbol_index(&self) -> Arc<SymbolIndex> {
        Arc::clone(&symbol_index_query(&self.salsa, self.project).0)
    }

    /// Indexing diagnostics (duplicate definitions, built-in shadowing)
    /// produced alongside [`symbol_index`](Self::symbol_index).
    pub fn symbol_index_diagnostics(&self) -> &[Diagnostic] {
        &symbol_index_query(&self.salsa, self.project).1
    }

    /// One file's resolved references + resolution diagnostics (layer 2,
    /// `resolve(FileId)`).
    pub fn resolve(&self, id: FileId) -> Option<(Arc<ResolutionMap>, &[Diagnostic])> {
        let file = self.files.get(&id)?;
        let (map, diags) = resolve_query(&self.salsa, self.project, *file);
        Some((Arc::clone(map), diags.as_slice()))
    }

    /// Per-declaration signature stub (layer 2, `signature(def)`). `None`
    /// for an unknown definition id.
    pub fn signature(&self, def: DefinitionId) -> Option<Arc<Sig>> {
        signature_query(&self.salsa, self.project, DefKey::new(&self.salsa, def))
    }

    /// Full cross-file analysis over all files, honoring the registered
    /// [`AnalysisOptions`]. Memoized; identical to
    /// `brink_analyzer::analyze_with_options` over
    /// [`analysis_inputs`](Self::analysis_inputs) by construction.
    pub fn analysis(&self) -> &AnalysisResult {
        analysis_query(&self.salsa, self.project)
    }

    /// Index + resolutions, no diagnostics (issue #632 / FG-3 — the
    /// RESOLUTIONS/INDEX half of [`analysis`](Self::analysis), split off
    /// from the diagnostics half so a diagnostics-only `AnalysisOptions`
    /// edit leaves this `Arc`'s pointer identity untouched).
    pub fn resolutions_index(&self) -> Arc<ResolvedProject> {
        resolutions_index_query(&self.salsa, self.project)
    }

    /// One file's per-file diagnostic contributors — structural validation,
    /// the dialect gate, and (brink dialect only) annotation-content checks
    /// (issue #632 / FG-3). `None` for an unknown file id. A body edit in a
    /// *different* file leaves this `Arc`'s pointer identity untouched.
    pub fn per_file_diagnostics(&self, id: FileId) -> Option<Arc<Vec<Diagnostic>>> {
        let file = *self.files.get(&id)?;
        Some(per_file_diagnostics_query(&self.salsa, self.project, file))
    }

    /// One file's VAR/CONST/LIST initializer/doc enrichment (issue #750 /
    /// FG-3 completion) — purely presentational `symbol_meta` entries, no
    /// diagnostics. `None` for an unknown file id. A body edit in a
    /// *different* file leaves this `Arc`'s pointer identity untouched.
    pub fn file_value_meta(&self, id: FileId) -> Option<Arc<BTreeMap<DefinitionId, SymbolMeta>>> {
        let file = *self.files.get(&id)?;
        Some(value_meta_query(&self.salsa, self.project, file))
    }

    /// One file's external call-site literal checks (`E041`/`E042`, issue
    /// #750 / FG-3 completion). `None` for an unknown file id; empty when
    /// the `external_check` severity is `Off`. A body edit in a *different*
    /// file leaves this `Arc`'s pointer identity untouched.
    pub fn file_call_site_diagnostics(&self, id: FileId) -> Option<Arc<Vec<Diagnostic>>> {
        let file = *self.files.get(&id)?;
        Some(call_site_diagnostics_query(&self.salsa, self.project, file))
    }

    /// The range-free, name-keyed external metas feeding the per-file
    /// call-site checks (issue #750 / FG-3 completion) — the cutoff seam
    /// between the (often re-executed, full-ranged-index-reading)
    /// enrichment pass and every file's call-site memo. Exposed for the
    /// dependency-edge tests; pointer identity across an edit proves the
    /// seam backdated.
    pub fn call_site_metas(&self) -> Arc<BTreeMap<String, SymbolMeta>> {
        call_site_metas_query(&self.salsa, self.project)
    }

    /// Per-file diagnostics including this file's share of analysis
    /// diagnostics (layer 3, `diagnostics(FileId)`). Raw — no suppression
    /// filtering.
    pub fn diagnostics(&self, id: FileId) -> Option<&[Diagnostic]> {
        let file = self.files.get(&id)?;
        Some(diagnostics_query(&self.salsa, self.project, *file).as_slice())
    }

    /// Whole-project type inference (TM-1, typed-mode-spec §2/§9 step 1).
    /// Advisory-only substrate: `infer_body`/`type_diagnostics` are thin
    /// per-def/per-file views over this. Lazy — nothing in `story_data`,
    /// `lir_product`, or `diagnostics` reads it, so calling this (directly
    /// or via `infer_body`/`type_diagnostics`) is the only thing that
    /// triggers the underlying computation.
    pub fn type_inference(&self) -> &InferenceResult {
        type_inference_query(&self.salsa, self.project)
    }

    /// Per-def inferred body types (`infer_body(def)`). `None` for a def
    /// with no inferable body (not a knot/stitch, or an unknown id).
    pub fn infer_body(&self, def: DefinitionId) -> Option<Arc<BodyTypes>> {
        infer_body_query(&self.salsa, self.project, DefKey::new(&self.salsa, def))
    }

    /// Per-def inferred signature (`inferred_signature(def)`, FG-2 issue
    /// #631) — the firewall-facing per-def view: params + return type only,
    /// no locals, no ranges. This is the boundary TM-2's annotation-override
    /// consumer reads. `None` for a def with no inferable body (not a
    /// knot/stitch, or an unknown id) — same `None` contract as
    /// [`signature`](Self::signature)/[`infer_body`](Self::infer_body).
    pub fn inferred_signature(&self, def: DefinitionId) -> Option<Arc<InferredSig>> {
        inferred_signature_query(&self.salsa, self.project, DefKey::new(&self.salsa, def))
    }

    /// Per-file type diagnostics (`type_diagnostics(FileId)`). Advisory-only
    /// in this slice — always empty (see `type_diagnostics_query`'s docs).
    pub fn type_diagnostics(&self, id: FileId) -> Option<&[Diagnostic]> {
        let file = self.files.get(&id)?;
        Some(type_diagnostics_query(&self.salsa, self.project, *file).as_slice())
    }

    /// Whole-project LIR lowering (layer 3). `None` until an entry point is
    /// set via [`set_entry`](Self::set_entry).
    pub fn lir_product(&self) -> Option<&LirProduct> {
        self.project.entry(&self.salsa)?;
        Some(lir_query(&self.salsa, self.project))
    }

    /// Whole-project compile to [`brink_format::StoryData`] (layer 3,
    /// `story_data()`). `None` until an entry point is set via
    /// [`set_entry`](Self::set_entry).
    pub fn story_data(&self) -> Option<&CompileProduct> {
        self.project.entry(&self.salsa)?;
        Some(story_data_query(&self.salsa, self.project))
    }

    /// Snapshot salsa's memo-table memory usage — one row per salsa
    /// ingredient (input/tracked struct or memoized query function), sorted
    /// for deterministic output. Behind the `memory-introspection` feature
    /// (issue #529); see `brink-test-harness`'s `editor_session_bench`.
    #[cfg(feature = "memory-introspection")]
    pub fn memory_snapshot(&self) -> Vec<crate::memory::IngredientMemory> {
        crate::memory::snapshot(&self.salsa)
    }

    // ── Internal helpers ──────────────────────────────────────────────

    fn include_graph(&self) -> &crate::include_graph::IncludeGraph {
        include_graph_query(&self.salsa, self.project)
    }
}

impl Default for ProjectDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve an INCLUDE path relative to the including file's directory.
///
/// Uses string-based path manipulation (`rfind('/')`) rather than
/// `std::path::Path` to avoid platform-specific separator issues and
/// to work in WASM contexts. The joined path is normalized so `.`/`..`
/// segments collapse to a clean project-relative key (e.g.
/// `a/b/../d.ink` → `a/d.ink`) — consistent across the compiler, runtime,
/// and IDE so upward-relative includes resolve to real files.
pub fn resolve_include_path(from_file: &str, include_path: &str) -> String {
    let joined = match from_file.rfind('/') {
        Some(i) => format!("{}/{include_path}", &from_file[..i]),
        None => include_path.to_string(),
    };
    normalize_path(&joined)
}

/// Collapse `.` and `..` segments in a `/`-separated path. A `..` pops the
/// previous real segment; a `..` with nothing to pop (or above an existing
/// `..`) is kept literally rather than escaping the root. A leading `/`
/// (absolute path) is preserved — the test harness and disk-backed compiles
/// resolve against absolute filesystem paths.
fn normalize_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." if matches!(out.last(), Some(&s) if s != "..") => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// Compute the relative INCLUDE target to reach `to_file` from `from_file`'s
/// directory — the inverse of [`resolve_include_path`]:
/// `normalize(resolve_include_path(from_file, compute_relative_path(from_file, to_file))) == to_file`
/// for both forward and `..`-traversing layouts. Used when a file is
/// renamed/moved to rewrite every `INCLUDE` that points at it (and the moved
/// file's own includes).
pub fn compute_relative_path(from_file: &str, to_file: &str) -> String {
    let mut from_dirs: Vec<&str> = from_file.split('/').collect();
    from_dirs.pop(); // drop the including file's own name
    let to_all: Vec<&str> = to_file.split('/').collect();
    let Some((to_name, to_dirs)) = to_all.split_last() else {
        return to_file.to_owned();
    };

    // Longest common directory prefix.
    let mut k = 0;
    while k < from_dirs.len() && k < to_dirs.len() && from_dirs[k] == to_dirs[k] {
        k += 1;
    }

    let mut parts: Vec<&str> = Vec::new();
    parts.extend(std::iter::repeat_n("..", from_dirs.len() - k));
    parts.extend_from_slice(&to_dirs[k..]);
    parts.push(to_name);
    parts.join("/")
}

#[cfg(test)]
mod path_tests {
    use super::{compute_relative_path, resolve_include_path};

    #[test]
    fn resolve_forward_includes() {
        assert_eq!(
            resolve_include_path("src/main.ink", "utils.ink"),
            "src/utils.ink"
        );
        assert_eq!(resolve_include_path("story.ink", "other.ink"), "other.ink");
        assert_eq!(resolve_include_path("a/b/c.ink", "d/e.ink"), "a/b/d/e.ink");
    }

    #[test]
    fn resolve_normalizes_dot_and_dotdot() {
        assert_eq!(resolve_include_path("a/b/c.ink", "../d.ink"), "a/d.ink");
        assert_eq!(resolve_include_path("a/b/c.ink", "./d.ink"), "a/b/d.ink");
        assert_eq!(resolve_include_path("a/b/c.ink", "../../d.ink"), "d.ink");
        assert_eq!(
            resolve_include_path("a/b/c.ink", "../x/../d.ink"),
            "a/d.ink"
        );
    }

    #[test]
    fn compute_relative_is_inverse_of_resolve() {
        // (from including file, target file) round-trips through resolve.
        let cases = [
            ("main.ink", "scenes/intro.ink"), // move into a subdir
            ("a/b/c.ink", "a/d.ink"),         // sibling dir (needs ..)
            ("a/b/c.ink", "a/b/renamed.ink"), // rename in place
            ("scenes/intro.ink", "lib.ink"),  // up to root (needs ..)
            ("a/b/c.ink", "x/y/z.ink"),       // fully divergent
            ("main.ink", "other.ink"),        // both at root
        ];
        for (from, to) in cases {
            let rel = compute_relative_path(from, to);
            assert_eq!(
                resolve_include_path(from, &rel),
                to,
                "round-trip failed for from={from} to={to} rel={rel}",
            );
        }
    }

    #[test]
    fn resolve_preserves_absolute_paths() {
        // The test harness / disk-backed compiles pass absolute paths — the
        // leading slash must survive normalization.
        assert_eq!(
            resolve_include_path("/proj/tier3/main.ink", "included.ink"),
            "/proj/tier3/included.ink",
        );
        assert_eq!(
            resolve_include_path("/proj/a/b/c.ink", "../d.ink"),
            "/proj/a/d.ink"
        );
    }

    #[test]
    fn compute_relative_rename_in_place_is_bare_name() {
        assert_eq!(
            compute_relative_path("a/b/c.ink", "a/b/renamed.ink"),
            "renamed.ink"
        );
        assert_eq!(
            compute_relative_path("main.ink", "renamed.ink"),
            "renamed.ink"
        );
    }

    #[test]
    fn compute_relative_move_shallower_is_bare_name() {
        // Regression (#318): after a shallower move (chapters/main.ink →
        // main.ink), the outbound-INCLUDE rewrite relativizes the resolved
        // target against the NEW path. From the root, a root-level sibling is a
        // bare name — no stale `chapters/` or `../` prefix.
        assert_eq!(compute_relative_path("main.ink", "host.ink"), "host.ink");
        // And from the OLD subdir location, the same root-level target is
        // `../host.ink` — relative to `chapters/`, reaching the root needs `..`.
        // (This is the value resolve→compute round-trips through; the rewrite
        // uses the NEW path above to drop the prefix.)
        assert_eq!(
            compute_relative_path("chapters/main.ink", "host.ink"),
            "../host.ink"
        );
        assert_eq!(
            resolve_include_path("chapters/main.ink", "../host.ink"),
            "host.ink"
        );
    }
}

#[cfg(test)]
mod remove_readd_tests {
    use super::ProjectDb;
    use brink_ir::FileId;

    #[test]
    fn readd_reuses_original_file_id() {
        let mut db = ProjectDb::new();
        let a = db.set_file("a.ink", "== ka ==\ntext\n".to_owned());
        let b = db.set_file("b.ink", "== kb ==\ntext\n".to_owned());
        assert_eq!(a, FileId(0));
        assert_eq!(b, FileId(1));

        db.remove_file("a.ink");
        let a2 = db.set_file("a.ink", "== ka2 ==\ntext\n".to_owned());
        assert_eq!(a2, a, "re-added path must reuse its original FileId");

        // A genuinely new path still gets a fresh id — reuse never aliases.
        let c = db.set_file("c.ink", "== kc ==\ntext\n".to_owned());
        assert_eq!(c, FileId(2));

        // The project file list stays sorted by FileId even though the
        // reinstated id (0) is smaller than the ids minted after it.
        let ids: Vec<FileId> = db.file_ids().collect();
        assert_eq!(ids, vec![a, b, c]);
        let meta_ids: Vec<FileId> = db
            .file_metadata()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(meta_ids, vec![a, b, c]);
    }

    #[test]
    fn removed_file_is_invisible_to_every_accessor() {
        let mut db = ProjectDb::new();
        db.set_file("main.ink", "INCLUDE sub.ink\n-> DONE\n".to_owned());
        let sub = db.set_file("sub.ink", "== s ==\ntext\n-> DONE\n".to_owned());

        db.remove_file("sub.ink");

        assert_eq!(db.file_id("sub.ink"), None);
        assert_eq!(db.file_path(sub), None);
        assert!(db.file_ids().all(|id| id != sub));
        assert!(db.file_metadata().iter().all(|(id, _, _)| *id != sub));
        assert!(db.analysis_inputs().iter().all(|(id, _, _)| *id != sub));
        assert!(db.source(sub).is_none());
        assert!(db.parse(sub).is_none());
        assert!(db.hir(sub).is_none());
        assert!(db.manifest(sub).is_none());
        assert!(db.diagnostics(sub).is_none());
        assert!(db.suppressions(sub).is_none());
        assert!(db.resolve(sub).is_none());
    }

    #[test]
    fn removed_include_matches_never_added_diagnostics() {
        let source = "INCLUDE sub.ink\n-> s\n";

        // Db where sub.ink existed and was removed.
        let mut removed = ProjectDb::new();
        removed.set_file("main.ink", source.to_owned());
        removed.set_file("sub.ink", "== s ==\ntext\n-> DONE\n".to_owned());
        removed.set_entry("main.ink");
        // Pull through the whole pipeline while sub.ink is live, so the
        // removed-state read below exercises invalidation, not a cold start.
        assert!(removed.story_data().is_some());
        removed.remove_file("sub.ink");

        // Db where sub.ink never existed (main.ink gets FileId(0) in both).
        let mut fresh = ProjectDb::new();
        fresh.set_file("main.ink", source.to_owned());
        fresh.set_entry("main.ink");

        let main_removed = removed.file_id("main.ink").expect("main");
        let main_fresh = fresh.file_id("main.ink").expect("main");
        assert_eq!(main_removed, main_fresh);
        assert_eq!(
            removed.diagnostics(main_removed),
            fresh.diagnostics(main_fresh),
            "a removed INCLUDE target must diagnose exactly like a missing one"
        );
        assert_eq!(
            removed.story_data().map(|p| p.errors.clone()),
            fresh.story_data().map(|p| p.errors.clone()),
        );
    }

    #[test]
    fn readd_recomputes_from_new_content() {
        let mut db = ProjectDb::new();
        let id = db.set_file("a.ink", "== old_knot ==\ntext\n-> DONE\n".to_owned());
        // Materialize memos for the original content.
        assert!(
            db.manifest(id)
                .is_some_and(|m| m.knots.iter().any(|k| k.name == "old_knot"))
        );

        db.remove_file("a.ink");
        let id2 = db.set_file("a.ink", "== new_knot ==\ntext\n-> DONE\n".to_owned());
        assert_eq!(id2, id);

        let manifest = db.manifest(id).expect("manifest after re-add");
        assert!(
            manifest.knots.iter().any(|k| k.name == "new_knot"),
            "re-added content must win over stale memos"
        );
        assert!(
            !manifest.knots.iter().any(|k| k.name == "old_knot"),
            "old content must not survive the tombstone round-trip"
        );
        assert_eq!(db.source(id), Some("== new_knot ==\ntext\n-> DONE\n"));
    }

    #[test]
    fn remove_clears_entry_and_readd_does_not_restore_it() {
        let mut db = ProjectDb::new();
        db.set_file("main.ink", "-> DONE\n".to_owned());
        db.set_entry("main.ink");
        assert!(db.entry().is_some());

        db.remove_file("main.ink");
        assert_eq!(db.entry(), None);

        db.set_file("main.ink", "-> DONE\n".to_owned());
        assert_eq!(db.entry(), None, "re-add must not silently restore entry");
    }
}

#[cfg(test)]
mod reachable_tests {
    use super::ProjectDb;

    /// Load files then read reachability — the include graph is a tracked
    /// query, so no rebuild step is needed regardless of insertion order.
    fn db_with(files: &[(&str, &str)]) -> ProjectDb {
        let mut db = ProjectDb::new();
        for (path, src) in files {
            db.set_file(path, (*src).to_owned());
        }
        db
    }

    #[test]
    fn entry_is_always_reachable_from_itself() {
        let db = db_with(&[("main.ink", "== hub ==\ntext\n")]);
        let main = db.file_id("main.ink").expect("main");
        let reachable = db.reachable_from(main);
        assert_eq!(reachable.into_iter().collect::<Vec<_>>(), vec![main]);
    }

    #[test]
    fn direct_includes_are_reachable() {
        let db = db_with(&[
            ("main.ink", "INCLUDE a.ink\nINCLUDE b.ink\n"),
            ("a.ink", "== a ==\n"),
            ("b.ink", "== b ==\n"),
        ]);
        let main = db.file_id("main.ink").expect("main");
        let a = db.file_id("a.ink").expect("a");
        let b = db.file_id("b.ink").expect("b");
        let reachable: Vec<_> = db.reachable_from(main).into_iter().collect();
        assert!(reachable.contains(&main));
        assert!(reachable.contains(&a));
        assert!(reachable.contains(&b));
        assert_eq!(reachable.len(), 3);
    }

    #[test]
    fn transitive_includes_are_reachable() {
        let db = db_with(&[
            ("main.ink", "INCLUDE a.ink\n"),
            ("a.ink", "INCLUDE b.ink\n"),
            ("b.ink", "== b ==\n"),
            ("unrelated.ink", "== x ==\n"),
        ]);
        let main = db.file_id("main.ink").expect("main");
        let a = db.file_id("a.ink").expect("a");
        let b = db.file_id("b.ink").expect("b");
        let unrelated = db.file_id("unrelated.ink").expect("unrelated");
        let reachable = db.reachable_from(main);
        assert!(reachable.contains(&main));
        assert!(reachable.contains(&a));
        assert!(reachable.contains(&b));
        assert!(
            !reachable.contains(&unrelated),
            "unrelated file is not reachable"
        );
    }

    #[test]
    fn reachable_terminates_on_cycles() {
        // a -> b -> a; reachability must not loop forever.
        let db = db_with(&[("a.ink", "INCLUDE b.ink\n"), ("b.ink", "INCLUDE a.ink\n")]);
        let a = db.file_id("a.ink").expect("a");
        let b = db.file_id("b.ink").expect("b");
        let reachable: Vec<_> = db.reachable_from(a).into_iter().collect();
        assert!(reachable.contains(&a));
        assert!(reachable.contains(&b));
        assert_eq!(reachable.len(), 2);
    }
}

#[cfg(test)]
mod type_inference_tests {
    use super::ProjectDb;
    use brink_ir::SymbolKind;

    /// End-to-end reachability proof (TM-1, #617): `infer_body`/
    /// `type_inference` are reachable through the same public `ProjectDb`
    /// surface every other query surfaces through, and return a real
    /// inferred type for a param whose body use pins it — not a stub.
    #[test]
    fn infer_body_is_reachable_through_project_db() {
        let mut db = ProjectDb::new();
        db.set_file(
            "main.ink",
            "=== heal(hp) ===\n~ temp x = hp + 1\n-> DONE\n".to_owned(),
        );
        db.set_entry("main.ink");

        let index = db.symbol_index();
        let heal = index
            .by_name
            .get("heal")
            .and_then(|ids| ids.first())
            .copied()
            .expect("heal knot indexed");
        assert_eq!(
            index.symbols.get(&heal).map(|i| i.kind),
            Some(SymbolKind::Knot)
        );

        let body = db.infer_body(heal).expect("heal has an inferable body");
        assert_eq!(body.params.len(), 1);
        assert_eq!(body.params[0].0, "hp");
        assert_eq!(body.params[0].1.display(), "int");

        // Same view via the whole-project result and via `type_diagnostics`
        // (advisory-only: empty, but reachable and correctly shaped).
        assert!(db.type_inference().signatures.contains_key(&heal));
        let main = db.file_id("main.ink").expect("main");
        assert_eq!(db.type_diagnostics(main), Some(&[][..]));
    }

    #[test]
    fn infer_body_is_none_for_a_non_callable_def() {
        let mut db = ProjectDb::new();
        db.set_file("main.ink", "VAR gold = 10\n-> DONE\n".to_owned());
        let index = db.symbol_index();
        let gold = index
            .by_name
            .get("gold")
            .and_then(|ids| ids.first())
            .copied()
            .expect("gold indexed");
        assert_eq!(db.infer_body(gold), None, "a VAR has no inferable body");
    }

    #[test]
    fn type_inference_is_independent_of_the_compile_path() {
        // Pulling every other query surface (diagnostics, story_data) first
        // — neither reads `type_inference_query` (see its module docs), so
        // this exercises `infer_body` cold, after, and must still return the
        // same correct result: nothing about the compile path's query graph
        // secretly depends on inference having (or not having) run yet.
        let mut db = ProjectDb::new();
        db.set_file(
            "main.ink",
            "=== heal(hp) ===\n~ temp x = hp + 1\n-> DONE\n".to_owned(),
        );
        db.set_entry("main.ink");
        let main = db.file_id("main.ink").expect("main");
        let _ = db.diagnostics(main);
        let _ = db.story_data();

        let index = db.symbol_index();
        let heal = index
            .by_name
            .get("heal")
            .and_then(|ids| ids.first())
            .copied()
            .expect("heal knot indexed");
        let body = db.infer_body(heal).expect("heal has an inferable body");
        assert_eq!(body.params[0].1.display(), "int");
    }
}
