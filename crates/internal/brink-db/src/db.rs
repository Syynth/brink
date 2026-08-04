use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use brink_analyzer::{
    AnalysisOptions, AnalysisResult, BodyTypes, EffectRow, HarvestIndex, InferenceResult,
    InferredSig, Sig, SymbolMeta,
};
use brink_format::DefinitionId;
use brink_ir::suppressions::Suppressions;
use brink_ir::{Diagnostic, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolManifest};
use brink_syntax::Parse;
use brink_syntax_native::Parse as NativeParse;
use salsa::Setter as _;
use tracing::debug;

use crate::determinism::LookupMap;
use crate::queries::{
    BrinkDatabase, CompileProduct, DefKey, KnotChunkKey, LirProduct, ProjectInput, ResolvedProject,
    SourceFile, analysis_query, call_site_diagnostics_query, call_site_metas_query,
    conventions_projection_query, diagnostics_query, effects_query, harvest_index_query,
    has_errors_query, include_graph_query, infer_body_query, inferred_signature_query,
    lir_knot_chunk_query, lir_prelude_decls_query, lir_query, local_signature_query, lowered_query,
    module_map_query, parse_native_query, parse_query, per_file_diagnostics_query,
    resolutions_index_query, resolve_query, signature_query, story_data_query, suppressions_query,
    symbol_index_query, type_diagnostics_query, type_inference_query, ufcs_resolution_query,
    value_meta_query,
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
    files: LookupMap<FileId, SourceFile>,
    path_to_id: LookupMap<String, FileId>,
    id_to_path: LookupMap<FileId, String>,
    /// Tombstoned salsa inputs from removed files, keyed by path — the
    /// durable path→`FileId` identity store (#536). Salsa never forgets an
    /// input, so [`remove_file`](Self::remove_file) parks the `SourceFile`
    /// here (text cleared) instead of dropping the handle; re-adding the
    /// same path reinstates it, reusing its original `FileId` so the old
    /// per-file memos are overwritten in place rather than leaking as
    /// permanently unreachable dead entries (rust-analyzer precedent).
    retired: LookupMap<String, SourceFile>,
    next_id: u32,
}

impl ProjectDb {
    /// Create an empty project database.
    pub fn new() -> Self {
        Self::with_id_base(0)
    }

    /// Create an empty project database whose `FileId`s start counting from
    /// `id_base` instead of `0` (issue #1580).
    ///
    /// A long-lived host that keeps *multiple* independent `ProjectDb`
    /// instances alive at once — `brink-lsp`'s per-native-project extent
    /// partitioning, one db per governing `brink.toml` — needs every
    /// instance's `FileId`s to be mutually disjoint: each db mints its own
    /// `FileId`s starting at `0` internally, so two dbs each holding a
    /// first-registered file would otherwise both mint `FileId(0)`, and a
    /// caller merging per-project data into one `FileId`-keyed map (as
    /// `brink-lsp`'s cross-project `ProjectAnalyses` does) would silently
    /// conflate two unrelated files. Callers are responsible for choosing
    /// non-overlapping `id_base` ranges (e.g. a fixed stride per project
    /// index) — this constructor only seeds the counter, it does not police
    /// collisions across instances it knows nothing about.
    pub fn with_id_base(id_base: u32) -> Self {
        let salsa = BrinkDatabase::default();
        let project = ProjectInput::new(
            &salsa,
            Vec::new(),
            None,
            AnalysisOptions::default(),
            None,
            None,
        );
        Self {
            salsa,
            project,
            files: LookupMap::new(),
            path_to_id: LookupMap::new(),
            id_to_path: LookupMap::new(),
            retired: LookupMap::new(),
            next_id: id_base,
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

    /// Register the directory native `.brink` file keys are root-relative
    /// *to* (issue #1572).
    ///
    /// A native file's module — and therefore every `DefinitionId` it
    /// qualifies — is a pure function of its **root-relative** key
    /// (decision-log 2026-07-22 "Native module identity"). `brink-driver`'s
    /// `discover_native` already registers such keys, so a compile leaves
    /// this `None` and nothing changes. A consumer that must key by some
    /// other prefix — the LSP keys by absolute OS path, because every path it
    /// holds round-trips through a `file://` URI — declares that prefix here,
    /// and the identity it mints then matches a real compile of the same
    /// tree byte for byte instead of embedding the machine's directory
    /// layout. Paths not under `root` are unaffected.
    ///
    /// Ink (`.ink`) files never consult this: their module is their file
    /// *stem*, which no path prefix can change.
    pub fn set_native_root(&mut self, root: Option<String>) {
        if self.project.native_root(&self.salsa) != &root {
            self.project.set_native_root(&mut self.salsa).to(root);
        }
    }

    /// The registered native source root, if any — see
    /// [`set_native_root`](Self::set_native_root).
    pub fn native_root(&self) -> Option<&str> {
        self.project.native_root(&self.salsa).as_deref()
    }

    /// Register the directory `.ink` file keys are root-relative *to*
    /// (issue #1696) — ink's sibling of [`set_native_root`](Self::set_native_root),
    /// consulted by [`hir::root_content_scope_path`](brink_ir::hir::root_content_scope_path)'s
    /// qualifier rather than by module identity.
    ///
    /// `brink-compiler/src/driver.rs`'s `prepare_driver` registers this for
    /// every ink compile, using `brink_driver::native_source_root` (the same
    /// root-discovery rule native compiles already use) fed the entry path.
    /// `None` — no caller has registered a root — is byte-identical to the
    /// pre-#1696 world: the qualifier stays the file's raw registered path.
    pub fn set_ink_root(&mut self, root: Option<String>) {
        if self.project.ink_root(&self.salsa) != &root {
            self.project.set_ink_root(&mut self.salsa).to(root);
        }
    }

    /// The registered ink source root, if any — see
    /// [`set_ink_root`](Self::set_ink_root).
    pub fn ink_root(&self) -> Option<&str> {
        self.project.ink_root(&self.salsa).as_deref()
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
    /// the files that include them), matching ink's `INCLUDE` paste
    /// semantics. Only `entry` and files it transitively `INCLUDE`s are
    /// returned — see [`IncludeGraph::topological_order`] (issue #815).
    pub fn file_ids_topo(&self, entry: FileId) -> Vec<FileId> {
        self.include_graph().topological_order(entry)
    }

    /// Get the parse tree for a file.
    pub fn parse(&self, id: FileId) -> Option<&Parse> {
        let file = self.files.get(&id)?;
        Some(parse_query(&self.salsa, *file))
    }

    /// Get the native (`.brink`) parse tree for a file (B0.10a, the native
    /// compile seam, issue #1106). The native-frontend sibling of
    /// [`parse`](Self::parse) — a distinct nominal `Parse` type. This runs the
    /// native parser regardless of the file's extension; the extension-based
    /// frontend dispatch that decides which parser *lowering* uses lives in
    /// `lowered_query`, so `parse()` stays ink-typed and untouched for the
    /// LSP/IDE ink path.
    pub fn parse_native(&self, id: FileId) -> Option<&NativeParse> {
        let file = self.files.get(&id)?;
        Some(parse_native_query(&self.salsa, *file))
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

    /// Get the B0.3 HIR admission validator's output for a file
    /// (docs/hir-admission-contract.md §4.2, issue #1172) — kept separate
    /// from [`Self::file_diagnostics`] because it is non-suppressible
    /// (never routed through `apply_suppressions`).
    pub fn admission_diagnostics(&self, id: FileId) -> Option<&[Diagnostic]> {
        let file = self.files.get(&id)?;
        Some(lowered_query(&self.salsa, *file).admission.as_slice())
    }

    /// Get suppression directives for a file — the text-scanned
    /// `brink-disable`/`brink-expect` comments merged with the file's
    /// HIR-derived `@[allow(…)]` scopes (issue #1161), i.e. parsed ∪
    /// HIR-derived, not parsed alone.
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

    /// Compute independent projects — the unit every editor surface scopes
    /// itself to (the LSP analyzes one project at a time, and navigation only
    /// ever sees the files of the project the cursor's file belongs to).
    ///
    /// Returns `(root, members)` pairs sorted by root `FileId`; each
    /// project's members are sorted by `FileId`.
    ///
    /// One rule per frontend:
    ///
    /// - **Ink** groups by `INCLUDE` reachability — a root file plus its
    ///   transitive `INCLUDE` closure (see
    ///   [`IncludeGraph::compute_projects`](crate::include_graph::IncludeGraph::compute_projects)).
    ///   Unchanged.
    /// - **Native `.brink`** files are *one* project, all of them. Issue
    ///   #1562: `.brink` has no `INCLUDE` (the module system replaced it), so
    ///   running them through the ink rule made every native file its own
    ///   single-file project and broke go-to-definition, find-references,
    ///   completion, and diagnostics across every real native workspace. The
    ///   rule here is the one
    ///   [`compilation_closure_files`](crate::queries::compilation_closure_files)
    ///   already applies to codegen (decision-log *"Native multi-file
    ///   linking"*, 2026-07-23): the discovered module set **is** the
    ///   compilation unit, so it is also the editor's scope. No second
    ///   discovery mechanism is involved — this partitions the files the db
    ///   already holds.
    ///
    /// The two sets are disjoint, so an `INCLUDE` in an ink file that names a
    /// `.brink` target (not expressible in native, and meaningless as ink)
    /// contributes no edge: the native file is in the native project only.
    pub fn compute_projects(&self) -> Vec<(FileId, Vec<FileId>)> {
        let (native, ink): (Vec<FileId>, Vec<FileId>) =
            self.file_ids().partition(|&id| self.is_native(id));

        let mut projects = self.include_graph().compute_projects(&ink);
        if let Some(root) = self.native_project_root(&native) {
            projects.push((root, native));
        }
        projects.sort_by_key(|(root, _)| root.0);
        projects
    }

    /// Whether `id` is a native (`.brink`) module rather than an ink file.
    ///
    /// `pub` (issue #1562 review finding) so a caller running the off-db
    /// `brink_analyzer::analyze_with_modules` pass per project root —
    /// `brink-lsp`'s `analysis_loop`, which needs the same "does this
    /// project's dialect axis even apply" answer
    /// [`crate::queries::project_is_native`] gives the salsa-backed
    /// `symbol_index_query` — can ask it of a project's root `FileId`
    /// without rederiving [`crate::queries::file_language`] itself.
    pub fn is_native(&self, id: FileId) -> bool {
        self.file_path(id).is_some_and(|path| {
            crate::queries::file_language(path) == crate::queries::Language::Native
        })
    }

    /// Whether **every** file this db holds is a native (`.brink`) module —
    /// `false` for an empty db or one holding even a single ink file.
    ///
    /// The whole-db view of [`crate::queries::project_is_all_native`], for a
    /// caller that analyzes this db's entire file set as one unit off-db
    /// (`IdeSession`, whose editor analysis runs
    /// [`brink_analyzer::analyze_with_modules`] over
    /// [`analysis_inputs`](Self::analysis_inputs)). That flag is
    /// whole-project, so it is only correct when the set is *entirely*
    /// native: a mixed set must analyze as ink, or an ink file would get the
    /// native arm of passes that would then mis-judge it.
    ///
    /// Distinct from [`is_native`](Self::is_native), which answers for one
    /// file and is what a per-project caller (`brink-lsp`'s `analysis_loop`,
    /// which analyzes each project root separately) asks of its root.
    pub fn is_all_native(&self) -> bool {
        crate::queries::project_is_all_native(&self.salsa, self.project)
    }

    /// The root of the single native project: the file whose **path** sorts
    /// first (`FileId` breaking a tie that paths cannot actually produce).
    /// `None` when the db holds no native file.
    ///
    /// Keyed on the path rather than on the `FileId` — which is how
    /// [`compilation_closure_files`](crate::queries::compilation_closure_files)
    /// orders the same file set — because a project root is *identity*, not
    /// just order: it keys the published per-project analysis and names the
    /// project in multi-project diagnostics. `FileId`s are minted in
    /// registration order, which for a long-lived LSP session is `didOpen`
    /// order and varies run to run; the path does not.
    fn native_project_root(&self, native: &[FileId]) -> Option<FileId> {
        native.iter().copied().min_by(|&a, &b| {
            self.file_path(a)
                .unwrap_or_default()
                .cmp(self.file_path(b).unwrap_or_default())
                .then(a.0.cmp(&b.0))
        })
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
    /// so the caller can run `brink_analyzer::analyze_with_modules` (with
    /// [`module_map`](Self::module_map), also snapshotted) without holding
    /// the lock. Issue #1526: a bare `brink_analyzer::analyze()` /
    /// `analyze_with_options` over these inputs is module-*blind* and mints
    /// different `DefinitionId`s than this db's own queries for native
    /// `.brink` files — see [`module_map`](Self::module_map)'s doc.
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

    /// The project-wide harvest index (layer 2, issue #2114,
    /// `docs/prose-dialect-spec.md` §5): every `@NAME` cue payload and every
    /// inline-markup span kind/attribute name written anywhere in the
    /// project, upgraded by the registered host manifest's `markup`
    /// vocabulary where one is declared. The compiler-side sibling of
    /// [`symbol_index`](Self::symbol_index) — a completion consumer reads
    /// this the same way it reads that index, and gets the same per-file
    /// [`lowered_query`] early cutoff the symbol index has: an edit
    /// backdates this memo when it backdates the symbol index's
    /// `lowered_query` half, but this index also depends on the registered
    /// host manifest, so a manifest-only edit backdates this memo without
    /// touching the symbol index at all (see
    /// [`harvest_index_query`](crate::queries::harvest_index_query)'s own
    /// doc for the full dependency set).
    pub fn harvest_index(&self) -> Arc<HarvestIndex> {
        Arc::clone(harvest_index_query(&self.salsa, self.project))
    }

    /// The conventions projection (issue #2111, NS-T seam 1/6): every
    /// `@[convention]` handler declared in the project's one configured
    /// conventions module, ascending by `order` — "THE SOLE EDITOR
    /// INTERCHANGE" the design-backport comment on #2111 names
    /// (`docs/decision-log.md` 2026-08-03). Reads the `[project] elements`
    /// pointer, the project module map, the resolved conventions module's
    /// transitive `IMPORT` closure (`import_closure_query`, issue #2111
    /// finding 3), and every file in that closure's own `lowered_query`
    /// output — see `conventions_projection_query`'s doc for the exact
    /// dependency set, and `brink_ir::ConventionsProjection`'s doc for the
    /// one part of #2111 this still does not deliver: it is not yet
    /// serialized into `.inkb`/`StoryData` (the attach schema IS now
    /// resolved to its fields and types, not merely a struct name — that
    /// gap closed in the 2026-08-04 continuation).
    pub fn conventions_projection(&self) -> Arc<brink_ir::ConventionsProjection> {
        Arc::clone(conventions_projection_query(&self.salsa, self.project))
    }

    /// Every file's resolved module (M-1, docs/modules-spec.md §1/§5) — the
    /// map that qualifies `DefinitionId` identity, built here from file
    /// stems, `#@module` declarations, the INCLUDE graph, and (for native
    /// `.brink` files) the path-derived `story::…` module.
    ///
    /// Exposed (issue #1526) for callers that must run
    /// [`brink_analyzer::analyze_with_modules`] *outside* the db — the LSP's
    /// background analysis pass and [`analysis_inputs`](Self::analysis_inputs)
    /// consumers generally — so their `DefinitionId`s match the ones this
    /// db's per-def queries ([`effects`](Self::effects),
    /// [`signature`](Self::signature), [`infer_body`](Self::infer_body)) are
    /// keyed by. Identity is minted here and nowhere else.
    ///
    /// The map's *diagnostics* half is
    /// [`module_map_diagnostics`](Self::module_map_diagnostics) — an
    /// off-db `analyze_with_modules` pass has to fold it back in itself
    /// (issue #1553).
    pub fn module_map(&self) -> &brink_analyzer::ModuleMap {
        &module_map_query(&self.salsa, self.project).0
    }

    /// Stem-collision diagnostics (`E085`) produced alongside
    /// [`module_map`](Self::module_map): a file with no `#@module` whose
    /// stem is some *other* file's declared module name.
    ///
    /// A db-driven compile picks these up through
    /// [`symbol_index_diagnostics`](Self::symbol_index_diagnostics), which
    /// folds them in. A caller that instead runs
    /// [`brink_analyzer::analyze_with_modules`] outside the db (the LSP's
    /// background pass, `IdeSession`'s editor analysis) gets only the
    /// analyzer's own diagnostics, so before issue #1553 the collision was
    /// silently dropped on every editor surface. Such callers must snapshot
    /// this alongside [`module_map`](Self::module_map) and extend their
    /// result with the entries belonging to their file set.
    pub fn module_map_diagnostics(&self) -> &[Diagnostic] {
        &module_map_query(&self.salsa, self.project).1
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

    /// Signature stub for a **local** (`Param`/`Temp`) `def`, declared in
    /// `id` (issue #530): the per-file locals path [`signature`](Self::signature)
    /// itself can't take — see `local_signature_query`'s doc for why a
    /// local's `DefinitionId` needs a caller-supplied file. `None` for an
    /// unknown file id or a `def` not declared as a local in that file
    /// (including a declaration id — those stay [`signature`](Self::signature)'s
    /// job).
    pub fn local_signature(&self, id: FileId, def: DefinitionId) -> Option<Arc<Sig>> {
        let file = *self.files.get(&id)?;
        local_signature_query(
            &self.salsa,
            self.project,
            file,
            DefKey::new(&self.salsa, def),
        )
    }

    /// Full cross-file analysis over all files, honoring the registered
    /// [`AnalysisOptions`]. Memoized; module-aware — identical to
    /// `brink_analyzer::analyze_with_modules` over
    /// [`analysis_inputs`](Self::analysis_inputs) and
    /// [`module_map`](Self::module_map) by construction. For native
    /// `.brink` files this is *not* identical to `analyze_with_options`
    /// (module-blind), which mints different `DefinitionId`s — see
    /// [`module_map`](Self::module_map)'s doc (issue #1526).
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

    /// Per-def effect row (`effects(def)`, T2-1, docs/effects-spec.md §2/§4,
    /// issue #860) — the advisory `{reads, writes, calls}` summary of the
    /// atomic effects `def` (and everything it transitively calls) may
    /// perform, sited beside [`inferred_signature`](Self::inferred_signature).
    /// Conservative-total (spec §3): the row over-reports, never under-reports;
    /// a call through a function value or an unknown callee makes it pessimal
    /// ([`EffectRow::opaque`]). `None` for a def with no inferable body (not a
    /// knot/stitch, or an unknown id) — same contract as
    /// [`inferred_signature`](Self::inferred_signature).
    ///
    /// **Advisory-only**: nothing in `story_data`/`lir_product`/`diagnostics`
    /// reads this, so the row is additive metadata that leaves compiled output
    /// byte-identical. Lazy — calling this is the only thing that triggers the
    /// underlying atom harvest + per-SCC fixpoint.
    pub fn effects(&self, def: DefinitionId) -> Option<Arc<EffectRow>> {
        effects_query(&self.salsa, self.project, DefKey::new(&self.salsa, def))
    }

    /// The B3a UFCS resolution verdict for the call site at `range` in
    /// `file` (issue #1507) — reads the same memoized `ufcs_resolution_query`
    /// (#1506) LIR lowering already shares, rather than re-running the
    /// analyzer's `ufcs` pass a second time for IDE hover/go-to-def. `None`
    /// when the pass recorded no verdict at this exact range: not a
    /// UFCS-shaped call site, an unresolved one (already diagnosed
    /// E140–E143 elsewhere), or the project has no dotted-callee call
    /// anywhere (`ufcs_resolution_query`'s own laziness gate).
    pub fn ufcs_verdict(
        &self,
        file: FileId,
        range: rowan::TextRange,
    ) -> Option<&brink_ir::lir::UfcsVerdict> {
        ufcs_resolution_query(&self.salsa, self.project)
            .table
            .get(file, range)
    }

    /// Every UFCS call site (`recv.verb(args)`) whose verdict desugars to a
    /// free function targeting `target`, project-wide (issue #1539) — reads
    /// the same memoized `ufcs_resolution_query` table
    /// [`ufcs_verdict`](Self::ufcs_verdict) does. The `find_references`/
    /// `rename` counterpart to that single-site lookup: renaming or listing
    /// references to a free function must also reach every UFCS call site
    /// that resolves to it, not just its plain `ResolutionMap` references.
    #[must_use]
    pub fn ufcs_call_sites_for_target(
        &self,
        target: DefinitionId,
    ) -> Vec<(FileId, rowan::TextRange)> {
        ufcs_resolution_query(&self.salsa, self.project)
            .table
            .call_sites_for_target(target)
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

    /// Whether the project has at least one Error-severity diagnostic after
    /// suppression filtering (issue #791 / FG-4a) — the narrow boolean
    /// projection [`lir_product`](Self::lir_product)'s gate reads instead of
    /// the full diagnostics vector. `false` (never `None`) when no entry
    /// point is set, matching `partition_diagnostics`'s empty-`errors`
    /// default in that case. Exposed for the dependency-edge tests; a
    /// diagnostics-content edit that leaves this boolean unchanged proves
    /// the cutoff seam backdated.
    pub fn has_errors(&self) -> bool {
        has_errors_query(&self.salsa, self.project)
    }

    /// FG-4d non-re-execution probe (issue #830): the `Arc<ScopeChunk>` the
    /// per-knot LIR chunk memo stores for the `knot_index`-th knot of `file`.
    /// `Arc::ptr_eq` on the result across an edit proves the memo validated
    /// without re-executing — salsa only hands back the same allocation when
    /// a query's inputs (this file's HIR, the project resolutions, and the
    /// struct-shape projection) are all unchanged. Exposed for the
    /// dependency-edge tests, mirroring [`resolutions_index`](Self::resolutions_index).
    #[doc(hidden)]
    #[must_use]
    pub fn knot_chunk(&self, file: FileId, knot_index: u32) -> Arc<brink_ir::lir::ScopeChunk> {
        let key = KnotChunkKey::new(&self.salsa, file, knot_index);
        lir_knot_chunk_query(&self.salsa, self.project, key).chunk
    }

    /// FG-4e non-re-execution probe (issue #839): the
    /// `Arc<brink_ir::lir::PreludeDecls>` the whole-project prelude-decls
    /// memo stores. `Arc::ptr_eq` on the result across an edit proves the
    /// memo validated without re-executing — salsa only hands back the same
    /// allocation when every entry-reachable file's [decl-only HIR
    /// projection](crate::queries::PreludeDeclsResult) is unchanged, which
    /// holds across a knot-body-only edit. Exposed for the dependency-edge
    /// tests, mirroring [`knot_chunk`](Self::knot_chunk).
    #[doc(hidden)]
    #[must_use]
    pub fn lir_prelude_decls(&self) -> Arc<brink_ir::lir::PreludeDecls> {
        lir_prelude_decls_query(&self.salsa, self.project).decls
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

    /// Test-only escape hatch: hand out the raw salsa handle and project
    /// input so crate-internal `#[cfg(test)]` code elsewhere (e.g.
    /// `queries::tests`) can call `pub(crate)` queries — `call_graph_query`,
    /// `def_effect_atoms_query` — directly instead of only through this
    /// façade's own accessors (issue #1736 finding: a direct edge-set
    /// parity guard needs both raw pieces).
    #[cfg(test)]
    pub(crate) fn salsa_and_project(&self) -> (&BrinkDatabase, ProjectInput) {
        (&self.salsa, self.project)
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
mod native_seam_tests {
    use super::ProjectDb;

    use brink_analyzer::{AnalysisOptions, Dialect};

    /// B0.10a gate (issue #1106): a native `.brink` file, registered by the
    /// plain public db API, must compile all the way through the *real* salsa
    /// pipeline to `StoryData` — proving the frontend seam in `lowered_query`
    /// dispatches on the `.brink` extension and that everything downstream of
    /// lowering is frontend-agnostic. This flow falls off the end (the
    /// `lower_native` implicit `-> DONE`), so no explicit terminator is needed.
    #[test]
    fn native_brink_file_compiles_through_to_story_data() {
        let mut db = ProjectDb::new();
        // Native compiles under the brink dialect (the analysis posture the
        // first-light native harness uses).
        db.set_analysis_options(AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });
        let id = db.set_file(
            "scene.brink",
            "flow main() {\n  Hello, world.\n}\n".to_owned(),
        );
        db.set_entry("scene.brink");

        // No parse/lowering diagnostics, and the non-suppressible admission
        // gate is clean.
        assert_eq!(
            db.file_diagnostics(id),
            Some(&[][..]),
            "native lowering must produce no per-file diagnostics"
        );
        assert_eq!(
            db.admission_diagnostics(id),
            Some(&[][..]),
            "native HIR must pass the B0.3 admission gate"
        );

        // End-to-end: parse_native -> lower_native -> analyze -> LIR ->
        // codegen -> StoryData, all via the public `story_data()` accessor.
        let product = db
            .story_data()
            .expect("entry is set, so story_data is Some");
        assert!(
            product.errors.is_empty(),
            "native compile must be error-free, got: {:?}",
            product.errors
        );
        assert!(
            product.story.is_some(),
            "native compile must yield a StoryData"
        );
    }

    /// SAVE-KEY INVARIANT (decision-log 2026-07-22 "Native module identity"):
    /// a native symbol's `DefinitionId` is hashed from its **path-derived**
    /// module (`native_module_path`) + name, and nothing else. Two properties
    /// follow, both of which keep player saves stable across recompiles:
    ///   1. Identity is qualified by the file's *location* — the same-named
    ///      flow at a different path is a different definition.
    ///   2. Identity is independent of `FileId` (assigned in discovery order)
    ///      — adding an unrelated file cannot change it.
    #[test]
    fn native_definition_id_is_path_qualified_and_fileid_independent() {
        use brink_format::DefinitionId;

        fn hero_id(files: &[(&str, &str)]) -> DefinitionId {
            let mut db = ProjectDb::new();
            db.set_analysis_options(AnalysisOptions {
                dialect: Dialect::Brink,
                ..AnalysisOptions::default()
            });
            for (path, src) in files {
                db.set_file(path, (*src).to_owned());
            }
            db.set_entry(files[0].0);
            let index = db.symbol_index();
            let ids = index.by_name.get("hero").expect("`hero` is defined");
            assert_eq!(ids.len(), 1, "exactly one `hero`");
            ids[0]
        }

        let hero = "flow hero() {\n  hi\n}\n";
        let other = "flow other() {\n  x\n}\n";

        // `flow hero()` in `market/barter.brink`, compiled alone → FileId(0).
        let solo = hero_id(&[("market/barter.brink", hero)]);

        // Same file, but an unrelated sibling is registered FIRST, so
        // `market/barter.brink` is now FileId(1) — its `FileId` shifted. If
        // `FileId` leaked into identity, `hero`'s `DefinitionId` would change.
        let with_sibling = hero_id(&[("aaa/early.brink", other), ("market/barter.brink", hero)]);
        assert_eq!(
            solo, with_sibling,
            "adding a file must not change `market/barter`'s `hero` identity — \
             `FileId` must never enter `DefinitionId`"
        );

        // The SAME `flow hero()` at a DIFFERENT path is a different module, so a
        // distinct identity — the path-derived module qualifies.
        let elsewhere = hero_id(&[("shop/wares.brink", hero)]);
        assert_ne!(
            solo, elsewhere,
            "`story::market::barter::hero` and `story::shop::wares::hero` must be distinct"
        );
    }

    /// NATIVE `@[was]` RENAME MIGRATION (issue #1286, the save-key companion to
    /// path-derived native module identity). A native module's `DefinitionId`
    /// is `hash(native_module_path, name)`, so *moving* the file changes every
    /// id and breaks saves keyed on the old ones. A file-level
    /// `@[was("old::path")]` is the migration record: it must emit an
    /// `AliasEntry { old, new }` mapping each pre-rename id to its current one,
    /// so `brink-runtime`'s miss-path lookup still resolves an old save.
    #[test]
    fn native_was_annotation_produces_pre_rename_alias() {
        use brink_format::DefinitionId;

        fn hero_id(path: &str, src: &str) -> DefinitionId {
            let mut db = ProjectDb::new();
            db.set_analysis_options(AnalysisOptions {
                dialect: Dialect::Brink,
                ..AnalysisOptions::default()
            });
            db.set_file(path, src.to_owned());
            db.set_entry(path);
            let index = db.symbol_index();
            index.by_name.get("hero").expect("`hero` is defined")[0]
        }

        let plain = "flow hero() {\n  hi\n}\n";
        // The OLD identity: `hero` when the module lived at `old/barter.brink`
        // (module `story::old::barter`), before any rename.
        let old_id = hero_id("old/barter.brink", plain);

        // The renamed module: same `hero`, now at `market/barter.brink`
        // (module `story::market::barter`), declaring where it came from.
        let renamed_src = "@[was(\"story::old::barter\")]\nflow hero() {\n  hi\n}\n";
        let mut db = ProjectDb::new();
        db.set_analysis_options(AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });
        db.set_file("market/barter.brink", renamed_src.to_owned());
        db.set_entry("market/barter.brink");

        let index = db.symbol_index();
        let new_id = index.by_name.get("hero").expect("`hero` is defined")[0];
        assert_ne!(
            old_id, new_id,
            "moving the module changes `hero`'s identity — that is the problem \
             `@[was]` migrates"
        );
        assert!(
            index
                .aliases
                .iter()
                .any(|a| a.old == old_id && a.new == new_id),
            "`@[was(\"story::old::barter\")]` must alias the pre-rename id to the \
             current one; aliases: {:?}",
            index.aliases
        );
    }

    /// The end-to-end proof #1286 claimed but issue #1355 found not actually
    /// reachable: the unquoted `::`-path spelling of `@[was(…)]` (issue
    /// #1349's grammar) must migrate a pre-rename `DefinitionId` exactly like
    /// the quoted-string spelling in
    /// [`native_was_annotation_produces_pre_rename_alias`] — same alias, not
    /// just a clean parse.
    #[test]
    fn native_was_annotation_unquoted_path_produces_pre_rename_alias() {
        use brink_format::DefinitionId;

        fn hero_id(path: &str, src: &str) -> DefinitionId {
            let mut db = ProjectDb::new();
            db.set_analysis_options(AnalysisOptions {
                dialect: Dialect::Brink,
                ..AnalysisOptions::default()
            });
            db.set_file(path, src.to_owned());
            db.set_entry(path);
            let index = db.symbol_index();
            index.by_name.get("hero").expect("`hero` is defined")[0]
        }

        let plain = "flow hero() {\n  hi\n}\n";
        // The OLD identity: `hero` when the module lived at `old/barter.brink`
        // (module `story::old::barter`), before any rename.
        let old_id = hero_id("old/barter.brink", plain);

        // The renamed module: same `hero`, now at `market/barter.brink`
        // (module `story::market::barter`), declaring where it came from
        // using the **unquoted** `::`-path spelling.
        let renamed_src = "@[was(story::old::barter)]\nflow hero() {\n  hi\n}\n";
        let mut db = ProjectDb::new();
        db.set_analysis_options(AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });
        db.set_file("market/barter.brink", renamed_src.to_owned());
        db.set_entry("market/barter.brink");

        let index = db.symbol_index();
        let new_id = index.by_name.get("hero").expect("`hero` is defined")[0];
        assert_ne!(
            old_id, new_id,
            "moving the module changes `hero`'s identity — that is the problem \
             `@[was]` migrates"
        );
        assert!(
            index
                .aliases
                .iter()
                .any(|a| a.old == old_id && a.new == new_id),
            "`@[was(story::old::barter)]` (unquoted) must alias the pre-rename \
             id to the current one; aliases: {:?}",
            index.aliases
        );
    }

    /// The negative control for [`native_was_annotation_produces_pre_rename_alias`]:
    /// WITHOUT `@[was]`, the same physical move changes `hero`'s `DefinitionId`
    /// and produces **no** alias — an old save silently fails to resolve. This
    /// is exactly why `@[was]` is required before native is used for real saves.
    #[test]
    fn native_rename_without_was_leaves_no_alias() {
        use brink_format::DefinitionId;

        fn setup(path: &str, src: &str) -> (DefinitionId, Vec<brink_format::AliasEntry>) {
            let mut db = ProjectDb::new();
            db.set_analysis_options(AnalysisOptions {
                dialect: Dialect::Brink,
                ..AnalysisOptions::default()
            });
            db.set_file(path, src.to_owned());
            db.set_entry(path);
            let index = db.symbol_index();
            let id = index.by_name.get("hero").expect("`hero` is defined")[0];
            (id, index.aliases.clone())
        }

        let plain = "flow hero() {\n  hi\n}\n";
        let (old_id, _) = setup("old/barter.brink", plain);
        let (new_id, aliases) = setup("market/barter.brink", plain);
        assert_ne!(old_id, new_id, "the move still changes identity");
        assert!(
            !aliases.iter().any(|a| a.old == old_id),
            "no `@[was]` means no migration path — the old id is unrecoverable"
        );
    }

    /// NATIVE MULTI-FILE LINKING (issue #1296, decision-log 2026-07-23): a
    /// multi-file native project links **every discovered `.brink` module**
    /// into the one `StoryData` — the discovery set is the compilation unit.
    /// Native files carry no `INCLUDE` edges, so before the codegen-closure
    /// fix only the *entry* file reached codegen and the sibling module's
    /// definitions silently vanished from the compiled story. Here the sibling
    /// `helper.brink` is never referenced from `main.brink`, yet its `helper`
    /// flow must still appear as a container in the linked `StoryData`.
    #[test]
    fn native_sibling_module_links_into_one_story_data() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });
        db.set_file("main.brink", "flow main() {\n  Hello.\n}\n".to_owned());
        db.set_file("helper.brink", "flow helper() {\n  Aside.\n}\n".to_owned());
        db.set_entry("main.brink");

        let product = db
            .story_data()
            .expect("entry is set, so story_data is Some");
        assert!(
            product.errors.is_empty(),
            "two clean native modules must compile: {:?}",
            product.errors
        );
        let story = product
            .story
            .as_ref()
            .expect("native multi-file compile must yield a StoryData");

        let index = db.symbol_index();
        let main_id = index.by_name.get("main").expect("`main` is defined")[0];
        let helper_id = index.by_name.get("helper").expect("`helper` is defined")[0];

        assert!(
            story.containers.iter().any(|c| c.id == main_id),
            "entry module's `main` flow must be linked"
        );
        assert!(
            story.containers.iter().any(|c| c.id == helper_id),
            "unreferenced sibling module's `helper` flow must ALSO be linked — \
             the whole discovered `.brink` tree is the compilation unit"
        );
    }

    /// RUST PARITY (issue #1296, decision-log 2026-07-23): a `.brink` file that
    /// fails to compile is an error **even if no other module references it**.
    /// Because the native codegen closure is every discovered module, a broken
    /// unreferenced sibling's Error-severity diagnostic (`E037` for a malformed
    /// flow header) is inside the build gate's closure and must fail the whole
    /// build — the entry file's clean flow does not rescue it.
    #[test]
    fn broken_unreferenced_native_sibling_fails_the_build() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });
        db.set_file("main.brink", "flow main() {\n  Hello.\n}\n".to_owned());
        // Malformed flow header (bad parameter list) → a `ParseSeverity::Error`
        // that surfaces as the non-suppressible `E037` compile diagnostic.
        db.set_file("broken.brink", "flow broken( {\n}\n".to_owned());
        db.set_entry("main.brink");

        let product = db
            .story_data()
            .expect("entry is set, so story_data is Some");
        assert!(
            !product.errors.is_empty(),
            "a broken unreferenced native sibling must fail the build"
        );
        assert!(
            product.story.is_none(),
            "no StoryData may be produced when any discovered native module is broken"
        );
    }

    /// The seam must not leak: an `.ink` file still runs the ink frontend and
    /// compiles as before (the native parser is never invoked for it).
    #[test]
    fn ink_file_still_compiles_via_ink_frontend() {
        let mut db = ProjectDb::new();
        db.set_file("main.ink", "Hello, world.\n-> DONE\n".to_owned());
        db.set_entry("main.ink");
        let product = db.story_data().expect("entry is set");
        assert!(
            product.errors.is_empty(),
            "ink path unchanged: {:?}",
            product.errors
        );
        assert!(product.story.is_some());
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

#[cfg(test)]
mod module_tests {
    //! M-1 modules (docs/modules-spec.md §1/§5): end-to-end reachability of
    //! module-qualified identity through the same public `ProjectDb`
    //! symbol-index surface the compiler (and IDE) use — the path that feeds
    //! codegen and the checked-in `.inkb`.
    use super::ProjectDb;
    use brink_ir::DiagnosticCode;

    fn knot_id(db: &ProjectDb, name: &str) -> u64 {
        db.symbol_index()
            .by_name
            .get(name)
            .and_then(|ids| ids.first())
            .map(|id| id.to_raw())
            .expect("knot indexed")
    }

    #[test]
    fn undeclared_file_keeps_bare_identity() {
        // The identity gate, exercised through the real db pipeline: an
        // undeclared single-file module hashes exactly as a bare-name build.
        // Byte-exact identity of a knot in an undeclared file is pinned in
        // the analyzer's `known_good_bare_definition_ids`; here we prove the
        // db path itself resolves an undeclared file to a *non-qualifying*
        // module — two undeclared files with different stems hash the knot
        // identically (the stem never enters the hash).
        let mut one = ProjectDb::new();
        one.set_file("story.ink", "== start ==\nHi\n-> DONE\n".to_owned());
        let mut other = ProjectDb::new();
        other.set_file("elsewhere.ink", "== start ==\nHi\n-> DONE\n".to_owned());
        assert_eq!(
            knot_id(&one, "start"),
            knot_id(&other, "start"),
            "two undeclared files (different stems) hash the knot identically"
        );
    }

    #[test]
    fn declared_module_qualifies_identity_through_db() {
        let mut bare = ProjectDb::new();
        bare.set_file("story.ink", "== start ==\nHi\n-> DONE\n".to_owned());

        let mut declared = ProjectDb::new();
        declared.set_file(
            "story.ink",
            "#@module(quest)\n== start ==\nHi\n-> DONE\n".to_owned(),
        );

        assert_ne!(
            knot_id(&bare, "start"),
            knot_id(&declared, "start"),
            "declaring a module must qualify (change) the knot's DefinitionId"
        );
    }

    #[test]
    fn included_file_inherits_module_identity() {
        // Standalone `part.ink` (undeclared) vs the same file INCLUDE-glued
        // under a declaring head — the included knot's identity must follow
        // the head's module.
        let mut standalone = ProjectDb::new();
        standalone.set_file("part.ink", "== helper ==\nHi\n-> DONE\n".to_owned());

        let mut glued = ProjectDb::new();
        glued.set_file(
            "head.ink",
            "#@module(quest)\nINCLUDE part.ink\n-> helper\n".to_owned(),
        );
        glued.set_file("part.ink", "== helper ==\nHi\n-> DONE\n".to_owned());
        glued.set_entry("head.ink");

        assert_ne!(
            knot_id(&standalone, "helper"),
            knot_id(&glued, "helper"),
            "an INCLUDE-glued file inherits the includer's declared module"
        );
    }

    #[test]
    fn stem_collision_with_declared_module_is_e085_through_db() {
        let mut db = ProjectDb::new();
        // `a.ink` declares module `shared`; `shared.ink` is an undeclared
        // file whose stem is *also* `shared` — the forbidden footgun.
        db.set_file("a.ink", "#@module(shared)\n== a_knot ==\nHi\n".to_owned());
        db.set_file("shared.ink", "== other ==\nHi\n".to_owned());

        let codes: Vec<_> = db
            .symbol_index_diagnostics()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E085),
            "expected E085 stem collision, got {codes:?}"
        );
    }

    /// End-to-end reachability for M-2 cross-module visibility (§4/§7): a
    /// `#@private` knot in declared module `quest`, diverted to from a
    /// different declared module `town`, surfaces `E087` through the same
    /// production diagnostics path the compiler/studio read.
    #[test]
    fn private_cross_module_reference_is_e087_through_db() {
        let mut db = ProjectDb::new();
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@private\nGotcha!\n-> DONE\n".to_owned(),
        );
        let town = db.set_file(
            "town.ink",
            "#@module(town)\n== square ==\nHi\n-> ambush\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(town)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E087),
            "expected E087 private-cross-module reference, got {codes:?}"
        );
    }

    /// An explicitly `#@public` knot in another declared module, diverted to
    /// from a file that **imports** it, resolves cleanly — no E087 (public,
    /// visibility-keyed) and no E025 (the import licenses the crossing, §2).
    #[test]
    fn imported_public_cross_module_reference_is_clean() {
        let mut db = ProjectDb::new();
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n".to_owned(),
        );
        let town = db.set_file(
            "town.ink",
            "#@module(town)\nIMPORT { ambush } FROM quest\n== square ==\nHi\n-> ambush\n"
                .to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(town)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            !codes.contains(&DiagnosticCode::E087),
            "public cross-module reference must not be E087, got {codes:?}"
        );
        assert!(
            !codes.contains(&DiagnosticCode::E025),
            "an imported public cross-module reference must not be E025, got {codes:?}"
        );
    }

    /// M-2c (§2): a *public* knot in another **declared** module, referenced
    /// from a file that did **not** `IMPORT` it, is `E025` — names cross
    /// module boundaries only via import. Bringing the name in (bare import)
    /// clears it (proven by `imported_public_cross_module_reference_is_clean`).
    #[test]
    fn public_cross_module_reference_without_import_is_e025() {
        let mut db = ProjectDb::new();
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n".to_owned(),
        );
        let town = db.set_file(
            "town.ink",
            "#@module(town)\n== square ==\nHi\n-> ambush\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(town)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E025),
            "a non-imported public cross-module reference must be E025, got {codes:?}"
        );
    }

    /// The qualified import form (`IMPORT quest`) also licenses references to
    /// the module's exports — no E025.
    #[test]
    fn qualified_import_licenses_cross_module_reference() {
        let mut db = ProjectDb::new();
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n".to_owned(),
        );
        let town = db.set_file(
            "town.ink",
            "#@module(town)\nIMPORT quest\n== square ==\nHi\n-> ambush\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(town)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            !codes.contains(&DiagnosticCode::E025),
            "a qualified import must license the crossing, got {codes:?}"
        );
    }

    /// The import-required restriction is keyed on the *target's* module being
    /// **declared**: a plain multi-file project with no `#@module` anywhere is
    /// one big default-public module (§3), so a cross-*file* bare reference
    /// keeps resolving with no E025 — the byte-identical legacy guarantee.
    #[test]
    fn cross_file_reference_in_undeclared_project_is_not_e025() {
        let mut db = ProjectDb::new();
        // `main.ink` INCLUDEs `helpers.ink`; neither declares a module.
        db.set_file(
            "helpers.ink",
            "== helper ==\nHelping.\n-> DONE\n".to_owned(),
        );
        let main = db.set_file(
            "main.ink",
            "INCLUDE helpers.ink\n== start ==\nHi\n-> helper\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(main)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            !codes.contains(&DiagnosticCode::E025),
            "an undeclared multi-file project must not trip the import gate, got {codes:?}"
        );
    }

    /// M-2c (§2): a `IMPORT quest` (qualified) whose module name also names a
    /// knot visible bare in the same file makes `quest.y` ambiguous — `E091`.
    #[test]
    fn qualified_import_colliding_with_definition_is_e091() {
        let mut db = ProjectDb::new();
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n".to_owned(),
        );
        // `town` has its own knot named `quest` AND imports module `quest`.
        let town = db.set_file(
            "town.ink",
            "#@module(town)\nIMPORT quest\n== quest ==\nHi\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(town)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E091),
            "expected E091 qualified module-vs-definition ambiguity, got {codes:?}"
        );
    }

    /// The `E092` redundant-override warning is reachable end-to-end: a
    /// `#@private` on a definition in a **declared** module restates that
    /// module's private-by-default (§4), so it is redundant.
    #[test]
    fn redundant_private_in_declared_module_is_e092() {
        let mut db = ProjectDb::new();
        let f = db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@private\nHi\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(f)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E092),
            "expected E092 redundant-override warning, got {codes:?}"
        );
    }

    /// A `#@public` on a definition in an **undeclared** stem-module restates
    /// the public-by-default (§4) — also redundant (`E092`).
    #[test]
    fn redundant_public_in_undeclared_module_is_e092() {
        let mut db = ProjectDb::new();
        let f = db.set_file(
            "story.ink",
            "== ambush ==\n#@public\nHi\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(f)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E092),
            "expected E092 redundant-override warning, got {codes:?}"
        );
    }

    /// A module importing itself surfaces `E090` through the db.
    #[test]
    fn self_import_is_e090_through_db() {
        let mut db = ProjectDb::new();
        let f = db.set_file(
            "quest.ink",
            "#@module(quest)\nIMPORT quest\n== start ==\nHi\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(f)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E090),
            "expected E090 self-import, got {codes:?}"
        );
    }

    /// A bare import naming a definition the (declared) module does not
    /// publicly export surfaces `E088`; a repeated local name surfaces
    /// `E089`.
    #[test]
    fn unresolved_and_duplicate_bare_import_through_db() {
        let mut db = ProjectDb::new();
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== ambush ==\n#@public\nHi\n-> DONE\n".to_owned(),
        );
        // `ambush` twice (duplicate local name) and `nope` (not exported).
        let town = db.set_file(
            "town.ink",
            "#@module(town)\nIMPORT { ambush, ambush, nope } FROM quest\n== square ==\nHi\n-> DONE\n"
                .to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(town)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E089),
            "expected E089 duplicate import, got {codes:?}"
        );
        assert!(
            codes.contains(&DiagnosticCode::E088),
            "expected E088 unresolved import, got {codes:?}"
        );
    }

    /// A single file declaring `#@module(quest)` whose knots reference
    /// sibling definitions bare (issue #795): a self-reference inside the
    /// declared module must never be E087, no matter which of the file's
    /// symbols the index's `HashMap` happens to yield first (locals carry
    /// `module: None` and must not poison the file's module attribution).
    /// The bug was nondeterministic — repeated fresh-db runs (fresh
    /// `HashMap` seeds each time) cover the iteration-order space; the
    /// order-independent analyzer-level regression lives in
    /// `brink-analyzer`'s `modules::tests`.
    #[test]
    fn single_file_declared_module_self_reference_is_not_e087() {
        for _ in 0..16 {
            let mut db = ProjectDb::new();
            let f = db.set_file(
                "main.ink",
                "#@module(quest)\nVAR target = -> ambush\n-> ambush\n== ambush ==\n~ temp x = 1\nGotcha!\n-> reader\n== reader ==\nDone.\n-> DONE\n".to_owned(),
            );

            let codes: Vec<_> = db
                .diagnostics(f)
                .unwrap_or_default()
                .iter()
                .map(|d| d.code)
                .collect();
            assert!(
                !codes.contains(&DiagnosticCode::E087),
                "same-module self-reference must not be E087, got {codes:?}"
            );
        }
    }

    /// A file that belongs to a declared module but declares no top-level
    /// symbols of its own (only root content) must still resolve to that
    /// module — a referrer in the *same* declared module referencing a
    /// `#@private` sibling def must not be wrongly flagged `E087` just
    /// because the referrer's own module couldn't be derived from its
    /// (nonexistent) symbols.
    #[test]
    fn symbol_less_file_in_same_module_is_not_e087() {
        let mut db = ProjectDb::new();
        db.set_file(
            "a.ink",
            "#@module(town)\n== square ==\n#@private\nGotcha!\n-> DONE\n".to_owned(),
        );
        // `b.ink` declares the same module but has only root content — no
        // knot/VAR/CONST/LIST/STRUCT of its own.
        let b = db.set_file("b.ink", "#@module(town)\n-> square\n".to_owned());

        let codes: Vec<_> = db
            .diagnostics(b)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            !codes.contains(&DiagnosticCode::E087),
            "same-module reference from a symbol-less file must not be E087, got {codes:?}"
        );
    }

    /// A symbol-less file (only root content) that imports its own declared
    /// module must still trip `E090` self-import — the same derivation gap
    /// that caused the `E087` false positive above also caused this false
    /// negative (the referrer's own module resolved to `None`).
    #[test]
    fn symbol_less_file_self_import_is_e090() {
        let mut db = ProjectDb::new();
        let f = db.set_file(
            "quest.ink",
            "#@module(quest)\nIMPORT quest\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .diagnostics(f)
            .unwrap_or_default()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E090),
            "expected E090 self-import from a symbol-less file, got {codes:?}"
        );
    }

    // ── M-2c cross-module collisions (issue #784, decision-log
    // "Cross-module name collisions" 2026-07-14) ────────────────────────

    fn brink_opts() -> brink_analyzer::AnalysisOptions {
        brink_analyzer::AnalysisOptions {
            dialect: brink_analyzer::Dialect::Brink,
            ..brink_analyzer::AnalysisOptions::default()
        }
    }

    /// Two **different** declared modules exporting the same public knot
    /// name now **coexist** under `dialect = brink` (M-2d, issue #790 —
    /// the E096 stopgap relaxed): no diagnostic, and both definitions land
    /// in the index, through the same `symbol_index_query` path the
    /// compiler/studio read.
    #[test]
    fn cross_declared_module_duplicate_knot_coexists_under_brink() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(brink_opts());
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== start ==\n#@public\nHi from quest\n-> DONE\n".to_owned(),
        );
        db.set_file(
            "town.ink",
            "#@module(town)\n== start ==\n#@public\nHi from town\n-> DONE\n".to_owned(),
        );

        let diags = db.symbol_index_diagnostics();
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E096),
            "E096 is relaxed — cross-declared-module homonyms must coexist, got {diags:?}"
        );
        // Both public `start` knots survive in the index under the shared
        // bare name — the raw material import-scoped resolution binds
        // per-importer.
        let index = db.symbol_index();
        assert_eq!(
            index.by_name.get("start").map(Vec::len),
            Some(2),
            "both modules' `start` knots must be indexed"
        );
    }

    /// Two files sharing the **same** declared module (a multi-file module)
    /// that both define `start` stay the ordinary within-module warning
    /// (`E022`) — never `E096` — even under `dialect = brink`.
    #[test]
    fn same_declared_module_duplicate_knot_still_warns_e022() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(brink_opts());
        db.set_file(
            "a.ink",
            "#@module(quest)\n== start ==\nHi from a\n-> DONE\n".to_owned(),
        );
        db.set_file(
            "b.ink",
            "#@module(quest)\n== start ==\nHi from b\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .symbol_index_diagnostics()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E022),
            "expected the within-module E022 warning, got {codes:?}"
        );
        assert!(
            !codes.contains(&DiagnosticCode::E096),
            "same declared module must never escalate to E096, got {codes:?}"
        );
    }

    /// Undeclared (legacy/soup) files duplicating a knot name are unchanged
    /// by M-2c: still `E022`, never `E096`, even under `dialect = brink`.
    #[test]
    fn undeclared_duplicate_knot_unchanged_under_brink() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(brink_opts());
        db.set_file("a.ink", "== start ==\nHi from a\n-> DONE\n".to_owned());
        db.set_file("b.ink", "== start ==\nHi from b\n-> DONE\n".to_owned());

        let codes: Vec<_> = db
            .symbol_index_diagnostics()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E022),
            "expected the legacy E022 warning, got {codes:?}"
        );
        assert!(
            !codes.contains(&DiagnosticCode::E096),
            "undeclared legacy soup must never escalate to E096, got {codes:?}"
        );
    }

    /// Under `strict-ink` (the default), a cross-declared-module duplicate
    /// stays the ordinary `E022` warning — the compat corpus is untouched.
    #[test]
    fn cross_declared_module_duplicate_stays_e022_under_strict_ink() {
        let mut db = ProjectDb::new();
        // Default AnalysisOptions -> Dialect::StrictInk; no set_analysis_options call.
        db.set_file(
            "quest.ink",
            "#@module(quest)\n== start ==\n#@public\nHi from quest\n-> DONE\n".to_owned(),
        );
        db.set_file(
            "town.ink",
            "#@module(town)\n== start ==\n#@public\nHi from town\n-> DONE\n".to_owned(),
        );

        let codes: Vec<_> = db
            .symbol_index_diagnostics()
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&DiagnosticCode::E022),
            "expected E022 under strict-ink, got {codes:?}"
        );
        assert!(
            !codes.contains(&DiagnosticCode::E096),
            "strict-ink must never see E096, got {codes:?}"
        );
    }

    /// The M-2d flagship (issue #790): two modules each export a public
    /// `ambush`; two files each bare-import a *different* one. Import-scoped
    /// resolution binds each file's `-> ambush` to the module it imported —
    /// not to the flat duplicate-winner — and the whole project compiles
    /// clean (no E025 import-required, no E096). Driven through the real
    /// `ProjectDb`/`resolve_query` path the compiler reads.
    #[test]
    fn two_modules_export_ambush_each_file_binds_its_own() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(brink_opts());
        db.set_file(
            "quest_a.ink",
            "#@module(quest_a)\n== ambush ==\n#@public\nFrom A\n-> DONE\n".to_owned(),
        );
        db.set_file(
            "quest_b.ink",
            "#@module(quest_b)\n== ambush ==\n#@public\nFrom B\n-> DONE\n".to_owned(),
        );
        let main_a = db.set_file(
            "main_a.ink",
            "IMPORT { ambush } FROM quest_a\n-> ambush\n".to_owned(),
        );
        let main_b = db.set_file(
            "main_b.ink",
            "IMPORT { ambush } FROM quest_b\n-> ambush\n".to_owned(),
        );

        // The two `ambush` knots coexist, module-qualified.
        let index = db.symbol_index();
        let ambush_ids = index.by_name.get("ambush").expect("both ambush knots");
        assert_eq!(ambush_ids.len(), 2, "both modules' `ambush` are indexed");
        let module_of = |target: brink_format::DefinitionId| -> Option<String> {
            index
                .symbols
                .get(&target)
                .and_then(|info| info.module.clone())
        };

        // Each importing file's `-> ambush` binds to the module it imported.
        let targets = |file| -> Vec<brink_format::DefinitionId> {
            let (map, _diags) = db.resolve(file).expect("file resolves");
            map.iter().map(|r| r.target).collect()
        };
        let a_targets = targets(main_a);
        assert!(
            a_targets
                .iter()
                .any(|&t| module_of(t).as_deref() == Some("quest_a")),
            "main_a's `ambush` must bind to module quest_a, got {:?}",
            a_targets.iter().map(|&t| module_of(t)).collect::<Vec<_>>()
        );
        assert!(
            !a_targets
                .iter()
                .any(|&t| module_of(t).as_deref() == Some("quest_b")),
            "main_a must NOT bind quest_b's `ambush`"
        );

        let b_targets = targets(main_b);
        assert!(
            b_targets
                .iter()
                .any(|&t| module_of(t).as_deref() == Some("quest_b")),
            "main_b's `ambush` must bind to module quest_b"
        );
        assert!(
            !b_targets
                .iter()
                .any(|&t| module_of(t).as_deref() == Some("quest_a")),
            "main_b must NOT bind quest_a's `ambush`"
        );

        // The whole project compiles clean: no import-required error, no
        // stopgap collision error, on any file.
        for file in [main_a, main_b] {
            let diags = db.diagnostics(file).expect("diagnostics");
            assert!(
                diags
                    .iter()
                    .all(|d| d.code != DiagnosticCode::E025 && d.code != DiagnosticCode::E096),
                "import-scoped resolution must leave the correctly-imported file clean, got {diags:?}"
            );
        }
    }

    /// `ImportScope` granularity regression (issue #790 review): a bare
    /// `IMPORT { other } FROM quest_a` must not license `quest_a`'s *other*
    /// public exports — only the name actually named. Two modules each
    /// export public `ambush`; the referring file bare-imports an unrelated
    /// name from `quest_a` and bare-imports `ambush` itself only from
    /// `quest_b`. Before the fix, `ImportScope` collapsed every import to
    /// just its module name, so `quest_a` counted as "imported" for *any*
    /// name — `-> ambush` could silently mis-resolve to `quest_a.ambush`
    /// and then draw a spurious `E025` telling the author to import `ambush`
    /// from `quest_a`, on a program that should compile clean. Resolution
    /// and the `E025` checker must agree at (module, name) granularity for
    /// bare imports.
    #[test]
    fn bare_import_is_name_precise_no_spurious_e025() {
        let mut db = ProjectDb::new();
        db.set_analysis_options(brink_opts());
        db.set_file(
            "quest_a.ink",
            "#@module(quest_a)\n== ambush ==\n#@public\nFrom A\n-> DONE\n== other ==\n#@public\nOther A\n-> DONE\n".to_owned(),
        );
        db.set_file(
            "quest_b.ink",
            "#@module(quest_b)\n== ambush ==\n#@public\nFrom B\n-> DONE\n".to_owned(),
        );
        let main = db.set_file(
            "main.ink",
            "IMPORT { other } FROM quest_a\nIMPORT { ambush } FROM quest_b\n-> ambush\n".to_owned(),
        );

        let index = db.symbol_index();
        let module_of = |target: brink_format::DefinitionId| -> Option<String> {
            index
                .symbols
                .get(&target)
                .and_then(|info| info.module.clone())
        };

        let (map, _diags) = db.resolve(main).expect("file resolves");
        let targets: Vec<brink_format::DefinitionId> = map.iter().map(|r| r.target).collect();
        assert!(
            targets
                .iter()
                .any(|&t| module_of(t).as_deref() == Some("quest_b")),
            "bare-importing `ambush` from quest_b must bind it to quest_b, got {:?}",
            targets.iter().map(|&t| module_of(t)).collect::<Vec<_>>()
        );
        assert!(
            !targets
                .iter()
                .any(|&t| module_of(t).as_deref() == Some("quest_a")),
            "bare-importing only `other` from quest_a must NOT license quest_a's `ambush`, got {:?}",
            targets.iter().map(|&t| module_of(t)).collect::<Vec<_>>()
        );

        let diags = db.diagnostics(main).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E025),
            "a correctly bare-imported `ambush` must never draw a spurious E025 \
             pointing at the unrelated module that only imported a different name, got {diags:?}"
        );
    }
}
