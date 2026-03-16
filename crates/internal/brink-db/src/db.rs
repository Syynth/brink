use std::collections::HashMap;
use std::io;

use brink_analyzer::AnalysisResult;
use brink_ir::suppressions::{Suppressions, parse_suppressions};
use brink_ir::{Diagnostic, FileId, HirFile, SymbolManifest, lower, lower_knot, lower_top_level};
use brink_syntax::ast::AstNode as _;
use brink_syntax::{Parse, parse_with_cache};
use rowan::{GreenNode, NodeCache};
use tracing::{debug, info};

use crate::file_state::{FileState, TopLevelEntry};
use crate::include_graph::IncludeGraph;
use crate::knot_cache::KnotEntry;

/// Stateful incremental project database.
///
/// Caches parsed trees and lowered HIR per file, enabling efficient re-analysis
/// when individual files change. Both the compiler (one-shot) and LSP
/// (long-lived) use this as their project model.
pub struct ProjectDb {
    files: HashMap<FileId, FileState>,
    path_to_id: HashMap<String, FileId>,
    id_to_path: HashMap<FileId, String>,
    next_id: u32,
    include_graph: IncludeGraph,
    analysis: Option<AnalysisResult>,
    node_cache: NodeCache,
}

impl ProjectDb {
    /// Create an empty project database.
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            next_id: 0,
            include_graph: IncludeGraph::new(),
            analysis: None,
            node_cache: NodeCache::default(),
        }
    }

    /// Add or replace a file. Performs full parse + lower + cache.
    pub fn set_file(&mut self, path: &str, source: String) -> FileId {
        let file_id = self.get_or_create_id(path);

        let parse = parse_with_cache(&source, &mut self.node_cache);
        let tree = parse.tree();

        // Per-knot lowering
        let knot_entries: Vec<KnotEntry> = tree
            .knots()
            .map(|knot_ast| {
                let green = knot_ast.syntax().green().into();
                let (knot, manifest, diagnostics) = lower_knot(file_id, &knot_ast);
                KnotEntry {
                    green,
                    knot,
                    manifest,
                    diagnostics,
                }
            })
            .collect();

        // Top-level lowering
        let top_level = Self::lower_top_level_entry(file_id, &tree);

        // Assemble full HirFile and SymbolManifest
        let (hir, manifest, diagnostics) =
            Self::assemble(file_id, &knot_entries, &top_level, &tree);

        let suppressions = parse_suppressions(&source);

        let state = FileState {
            source,
            parse,
            knot_entries,
            top_level,
            hir,
            manifest,
            diagnostics,
            suppressions,
        };

        // Update include graph
        let include_ids: Vec<FileId> = state
            .hir
            .includes
            .iter()
            .filter_map(|inc| {
                let resolved = resolve_include_path(path, &inc.file_path);
                self.path_to_id.get(&resolved).copied()
            })
            .collect();
        self.include_graph.update(file_id, include_ids);

        self.files.insert(file_id, state);
        self.analysis = None;

        debug!(path, id = file_id.0, "set_file complete");
        file_id
    }

    /// Incrementally update a file. Re-parses, diffs knots by green-node
    /// identity, and only re-lowers changed knots.
    pub fn update_file(&mut self, path: &str, source: String) -> FileId {
        let file_id = self.get_or_create_id(path);

        // If the file doesn't exist yet, fall through to set_file
        if !self.files.contains_key(&file_id) {
            return self.set_file(path, source);
        }

        let parse = parse_with_cache(&source, &mut self.node_cache);
        let tree = parse.tree();

        // Top-level is always re-lowered (cheap relative to knots)
        let top_level = Self::lower_top_level_entry(file_id, &tree);

        // Diff knots by green-node identity
        let new_knot_asts: Vec<_> = tree.knots().collect();
        let old_state = self.files.get(&file_id);

        let mut knot_entries = Vec::with_capacity(new_knot_asts.len());
        let mut reused = 0u32;

        for (i, knot_ast) in new_knot_asts.iter().enumerate() {
            let new_green: GreenNode = knot_ast.syntax().green().into();

            let reuse_entry = old_state
                .and_then(|s| s.knot_entries.get(i))
                .filter(|old| old.green == new_green);

            if let Some(old_entry) = reuse_entry {
                knot_entries.push(KnotEntry {
                    green: new_green,
                    knot: old_entry.knot.clone(),
                    manifest: old_entry.manifest.clone(),
                    diagnostics: old_entry.diagnostics.clone(),
                });
                reused += 1;
            } else {
                let (knot, manifest, diagnostics) = lower_knot(file_id, knot_ast);
                knot_entries.push(KnotEntry {
                    green: new_green,
                    knot,
                    manifest,
                    diagnostics,
                });
            }
        }

        debug!(
            path,
            total = new_knot_asts.len(),
            reused,
            "knot diff complete"
        );

        let (hir, manifest, diagnostics) =
            Self::assemble(file_id, &knot_entries, &top_level, &tree);

        let suppressions = parse_suppressions(&source);

        let state = FileState {
            source,
            parse,
            knot_entries,
            top_level,
            hir,
            manifest,
            diagnostics,
            suppressions,
        };

        // Update include graph
        let include_ids: Vec<FileId> = state
            .hir
            .includes
            .iter()
            .filter_map(|inc| {
                let resolved = resolve_include_path(path, &inc.file_path);
                self.path_to_id.get(&resolved).copied()
            })
            .collect();
        self.include_graph.update(file_id, include_ids);

        self.files.insert(file_id, state);
        self.analysis = None;
        file_id
    }

    /// Remove a file from the database.
    pub fn remove_file(&mut self, path: &str) {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
            self.files.remove(&id);
            self.include_graph.remove(id);
            self.analysis = None;
        }
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
        let all: Vec<_> = self.files.keys().copied().collect();
        self.include_graph.topological_order(entry, &all)
    }

    /// Get the cached parse tree for a file.
    pub fn parse(&self, id: FileId) -> Option<&Parse> {
        self.files.get(&id).map(|s| &s.parse)
    }

    /// Get the cached HIR for a file.
    pub fn hir(&self, id: FileId) -> Option<&HirFile> {
        self.files.get(&id).map(|s| &s.hir)
    }

    /// Get the cached symbol manifest for a file.
    pub fn manifest(&self, id: FileId) -> Option<&SymbolManifest> {
        self.files.get(&id).map(|s| &s.manifest)
    }

    /// Get the source text for a file.
    pub fn source(&self, id: FileId) -> Option<&str> {
        self.files.get(&id).map(|s| s.source.as_str())
    }

    /// Get per-file diagnostics (parse + lowering).
    pub fn file_diagnostics(&self, id: FileId) -> Option<&[Diagnostic]> {
        self.files.get(&id).map(|s| s.diagnostics.as_slice())
    }

    /// Get parsed suppression directives for a file.
    pub fn suppressions(&self, id: FileId) -> Option<&Suppressions> {
        self.files.get(&id).map(|s| &s.suppressions)
    }

    /// Rebuild include graph edges for all files.
    ///
    /// Must be called after batch-loading files (e.g. workspace discovery)
    /// because `set_file` can only create edges to files already in the db.
    /// Files loaded before their include targets will have missing edges.
    pub fn rebuild_include_graph(&mut self) {
        let file_list: Vec<(FileId, String)> = self
            .files
            .keys()
            .filter_map(|&id| self.id_to_path.get(&id).map(|p| (id, p.clone())))
            .collect();

        for (file_id, file_path) in &file_list {
            if let Some(state) = self.files.get(file_id) {
                let include_ids: Vec<FileId> = state
                    .hir
                    .includes
                    .iter()
                    .filter_map(|inc| {
                        let resolved = resolve_include_path(file_path, &inc.file_path);
                        self.path_to_id.get(&resolved).copied()
                    })
                    .collect();
                self.include_graph.update(*file_id, include_ids);
            }
        }
        self.analysis = None;
    }

    /// Detect cycles in the include graph.
    ///
    /// Returns the first cycle found as an ordered path of file IDs.
    pub fn find_cycle(&self) -> Option<Vec<FileId>> {
        self.include_graph.find_cycle()
    }

    /// Compute independent projects from include relationships.
    ///
    /// Returns `(root, members)` pairs sorted by root `FileId`.
    pub fn compute_projects(&self) -> Vec<(FileId, Vec<FileId>)> {
        let all: Vec<_> = self.files.keys().copied().collect();
        self.include_graph.compute_projects(&all)
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
                let state = self.files.get(&id)?;
                Some((id, state.hir.clone(), state.manifest.clone()))
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
        let mut inputs: Vec<_> = self
            .files
            .iter()
            .map(|(&id, state)| (id, state.hir.clone(), state.manifest.clone()))
            .collect();
        inputs.sort_by_key(|(id, _, _)| id.0);
        inputs
    }

    /// Snapshot file metadata for diagnostic publishing.
    ///
    /// Returns `(FileId, path, source)` tuples for all files in the db.
    pub fn file_metadata(&self) -> Vec<(FileId, String, String)> {
        let mut meta: Vec<_> = self
            .files
            .keys()
            .filter_map(|&id| {
                let path = self.id_to_path.get(&id)?.clone();
                let source = self.files.get(&id)?.source.clone();
                Some((id, path, source))
            })
            .collect();
        meta.sort_by_key(|(id, _, _)| id.0);
        meta
    }

    /// Run cross-file analysis (or return cached result).
    #[expect(
        clippy::expect_used,
        reason = "analysis is always Some after the if-block above"
    )]
    pub fn analyze(&mut self) -> &AnalysisResult {
        if self.analysis.is_none() {
            let files: Vec<_> = self
                .files
                .iter()
                .map(|(&id, state)| (id, &state.hir, &state.manifest))
                .collect();

            info!(files = files.len(), "running cross-file analysis");
            self.analysis = Some(brink_analyzer::analyze(&files));
        }
        self.analysis.as_ref().expect("just set above")
    }

    /// BFS discovery of all files reachable via INCLUDEs from the entry point.
    pub fn discover<F>(
        &mut self,
        entry: &str,
        read_file: &mut F,
    ) -> Result<(), crate::DiscoverError>
    where
        F: FnMut(&str) -> Result<String, io::Error>,
    {
        let mut queue: Vec<String> = vec![entry.to_string()];
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }

            let source = read_file(&path)?;
            let file_id = self.set_file(&path, source);

            // Discover INCLUDEs
            if let Some(state) = self.files.get(&file_id) {
                for include in &state.hir.includes {
                    let resolved = resolve_include_path(&path, &include.file_path);
                    if !seen.contains(&resolved) {
                        debug!(from = path, include = resolved, "discovered INCLUDE");
                        queue.push(resolved);
                    }
                }
            }
        }

        // Rebuild include graph now that all files are loaded
        let file_list: Vec<(FileId, String)> = self
            .files
            .keys()
            .filter_map(|&id| self.id_to_path.get(&id).map(|p| (id, p.clone())))
            .collect();

        for (file_id, file_path) in &file_list {
            if let Some(state) = self.files.get(file_id) {
                let include_ids: Vec<FileId> = state
                    .hir
                    .includes
                    .iter()
                    .filter_map(|inc| {
                        let resolved = resolve_include_path(file_path, &inc.file_path);
                        self.path_to_id.get(&resolved).copied()
                    })
                    .collect();
                self.include_graph.update(*file_id, include_ids);
            }
        }

        // Detect circular includes
        if let Some(cycle) = self.include_graph.find_cycle() {
            let names: Vec<_> = cycle
                .iter()
                .filter_map(|id| self.id_to_path.get(id).map(String::as_str))
                .collect();
            return Err(crate::DiscoverError::CircularInclude(names.join(" -> ")));
        }

        info!(files = seen.len(), "discovery complete");
        Ok(())
    }

    // ── Internal helpers ──────────────────────────────────────────────

    fn get_or_create_id(&mut self, path: &str) -> FileId {
        if let Some(&id) = self.path_to_id.get(path) {
            return id;
        }
        let id = FileId(self.next_id);
        self.next_id += 1;
        self.path_to_id.insert(path.to_string(), id);
        self.id_to_path.insert(id, path.to_string());
        id
    }

    fn lower_top_level_entry(
        file_id: FileId,
        tree: &brink_syntax::ast::SourceFile,
    ) -> TopLevelEntry {
        let green_children = Self::collect_top_level_green(tree);
        let (root_content, top_level_knots, manifest, diagnostics) = lower_top_level(file_id, tree);
        TopLevelEntry {
            green_children,
            root_content,
            top_level_knots,
            manifest,
            diagnostics,
        }
    }

    /// Collect green nodes of non-knot direct children for diffing.
    fn collect_top_level_green(tree: &brink_syntax::ast::SourceFile) -> Vec<GreenNode> {
        use brink_syntax::SyntaxKind;

        tree.syntax()
            .children()
            .filter(|child| child.kind() != SyntaxKind::KNOT_DEF)
            .map(|child| child.green().into())
            .collect()
    }

    /// Assemble a complete `HirFile` and `SymbolManifest` from cached pieces.
    fn assemble(
        file_id: FileId,
        knot_entries: &[KnotEntry],
        top_level: &TopLevelEntry,
        tree: &brink_syntax::ast::SourceFile,
    ) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
        // We need declarations from the full lower to build HirFile.
        // lower_top_level only returns (Block, SymbolManifest, diagnostics).
        // For the declarations (variables, constants, lists, externals, includes),
        // we need to call `lower()` or extract them from the AST.
        //
        // Approach: use `lower()` to get the full HirFile, then replace knots
        // with our cached versions. This means top-level lowering happens twice
        // on change, but it's simple and correct.
        let (mut full_hir, _full_manifest, _full_diag) = lower(file_id, tree);

        // Replace knots with our cached (possibly reused) versions,
        // plus any top-level stitches promoted to knots.
        full_hir.knots = knot_entries.iter().filter_map(|e| e.knot.clone()).collect();
        full_hir.knots.extend(top_level.top_level_knots.clone());
        full_hir.root_content = top_level.root_content.clone();

        // Merge manifests: top-level + all knots
        let mut manifest = top_level.manifest.clone();
        for entry in knot_entries {
            merge_manifest_into(&mut manifest, &entry.manifest);
        }

        // Merge diagnostics
        let mut diagnostics = top_level.diagnostics.clone();
        for entry in knot_entries {
            diagnostics.extend(entry.diagnostics.iter().cloned());
        }

        (full_hir, manifest, diagnostics)
    }
}

impl Default for ProjectDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge `src` manifest fields into `dst`.
fn merge_manifest_into(dst: &mut SymbolManifest, src: &SymbolManifest) {
    dst.knots.extend(src.knots.iter().cloned());
    dst.stitches.extend(src.stitches.iter().cloned());
    dst.variables.extend(src.variables.iter().cloned());
    dst.constants.extend(src.constants.iter().cloned());
    dst.lists.extend(src.lists.iter().cloned());
    dst.externals.extend(src.externals.iter().cloned());
    dst.labels.extend(src.labels.iter().cloned());
    dst.list_items.extend(src.list_items.iter().cloned());
    dst.locals.extend(src.locals.iter().cloned());
    dst.unresolved.extend(src.unresolved.iter().cloned());
}

/// Resolve an INCLUDE path relative to the including file's directory.
///
/// Uses string-based path manipulation (`rfind('/')`) rather than
/// `std::path::Path` to avoid platform-specific separator issues and
/// to work in WASM contexts.
pub fn resolve_include_path(from_file: &str, include_path: &str) -> String {
    match from_file.rfind('/') {
        Some(i) => format!("{}/{include_path}", &from_file[..i]),
        None => include_path.to_string(),
    }
}
