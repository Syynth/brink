use std::collections::HashMap;

use brink_ir::suppressions::{Suppressions, parse_suppressions};
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, SymbolManifest, lower, lower_single_knot,
    lower_top_level,
};
use brink_syntax::ast::AstNode as _;
use brink_syntax::{Parse, parse_with_cache};
use rowan::{GreenNode, NodeCache};
use tracing::debug;

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
                let offset = knot_ast.syntax().text_range().start();
                let (knot, manifest, diagnostics) = lower_single_knot(file_id, &knot_ast);
                KnotEntry {
                    green,
                    offset,
                    knot,
                    manifest,
                    diagnostics,
                }
            })
            .collect();

        // Top-level lowering
        let top_level = Self::lower_top_level_entry(file_id, &tree);

        // Assemble full HirFile and SymbolManifest
        let (hir, manifest, mut diagnostics) =
            Self::assemble(file_id, &knot_entries, &top_level, &tree);
        // Surface parser/syntax errors as compile diagnostics alongside lowering.
        diagnostics.extend(Self::syntax_diagnostics(file_id, &parse));

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

            let new_offset = knot_ast.syntax().text_range().start();
            let reuse_entry = old_state
                .and_then(|s| s.knot_entries.get(i))
                .filter(|old| old.green == new_green && old.offset == new_offset);

            if let Some(old_entry) = reuse_entry {
                knot_entries.push(KnotEntry {
                    green: new_green,
                    offset: new_offset,
                    knot: old_entry.knot.clone(),
                    manifest: old_entry.manifest.clone(),
                    diagnostics: old_entry.diagnostics.clone(),
                });
                reused += 1;
            } else {
                let (knot, manifest, diagnostics) = lower_single_knot(file_id, knot_ast);
                knot_entries.push(KnotEntry {
                    green: new_green,
                    offset: new_offset,
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

        let (hir, manifest, mut diagnostics) =
            Self::assemble(file_id, &knot_entries, &top_level, &tree);
        diagnostics.extend(Self::syntax_diagnostics(file_id, &parse));

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

        file_id
    }

    /// Remove a file from the database.
    pub fn remove_file(&mut self, path: &str) {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
            self.files.remove(&id);
            self.include_graph.remove(id);
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

    /// Convert parser/syntax errors into compile diagnostics (`E037`), so
    /// malformed source fails the compile instead of being silently ignored.
    fn syntax_diagnostics(file_id: FileId, parse: &Parse) -> Vec<Diagnostic> {
        parse
            .errors()
            .iter()
            .map(|e| Diagnostic {
                file: file_id,
                range: e.range,
                message: e.message.clone(),
                code: DiagnosticCode::E037,
            })
            .collect()
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
    dst.docs
        .extend(src.docs.iter().map(|(k, v)| (k.clone(), v.clone())));
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
