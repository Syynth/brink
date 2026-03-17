use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use brink_analyzer::AnalysisResult;
use brink_syntax::ast::AstNode;
use tokio::sync::{Notify, watch};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CodeLens, CodeLensOptions, CodeLensParams, CompletionItem,
    CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentFormattingParams,
    DocumentRangeFormattingParams, DocumentSymbolParams, DocumentSymbolResponse, FileChangeType,
    FileSystemWatcher, FoldingRange, FoldingRangeKind, FoldingRangeParams,
    FoldingRangeProviderCapability, GlobPattern, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, InlayHint as LspInlayHint, InlayHintLabel, InlayHintParams, Location,
    MarkupContent, MarkupKind, OneOf, ParameterInformation, ParameterLabel, Position,
    PrepareRenameResponse, Range, ReferenceParams, Registration, RenameOptions, RenameParams,
    SaveOptions, SemanticTokens, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    SignatureHelp, SignatureHelpOptions, SignatureHelpParams, SignatureInformation,
    SymbolInformation, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url,
    WorkDoneProgressOptions, WorkspaceEdit, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer};

use brink_ide::{
    CompletionContext, cursor_scope, detect_completion_context, is_visible_in_context,
};

use crate::convert::{self, LineIndex};
use crate::semantic_tokens;

/// Per-project analysis results, keyed by project root.
pub(crate) struct ProjectAnalyses {
    /// Per-project analysis, keyed by root `FileId`.
    by_root: HashMap<brink_ir::FileId, Arc<AnalysisResult>>,
    /// Reverse: file → all project roots that contain it (sorted).
    file_to_roots: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>>,
    /// Project membership: root → member file IDs.
    project_members: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>>,
}

impl ProjectAnalyses {
    /// Primary project for navigation (first/lowest root).
    fn for_file(&self, file: brink_ir::FileId) -> Option<&Arc<AnalysisResult>> {
        let roots = self.file_to_roots.get(&file)?;
        let root = roots.first()?;
        self.by_root.get(root)
    }

    /// All projects containing this file (for diagnostic union).
    fn all_for_file(&self, file: brink_ir::FileId) -> Vec<&Arc<AnalysisResult>> {
        self.file_to_roots
            .get(&file)
            .map(|roots| roots.iter().filter_map(|r| self.by_root.get(r)).collect())
            .unwrap_or_default()
    }

    /// Project members for the primary project of a file.
    fn project_files_for(&self, file: brink_ir::FileId) -> Option<&[brink_ir::FileId]> {
        let roots = self.file_to_roots.get(&file)?;
        let root = roots.first()?;
        self.project_members.get(root).map(Vec::as_slice)
    }
}

pub struct Backend {
    client: Client,
    db: Arc<Mutex<brink_db::ProjectDb>>,
    analysis_rx: watch::Receiver<Option<Arc<ProjectAnalyses>>>,
    analysis_trigger: Arc<Notify>,
    generation: Arc<AtomicU64>,
    last_published: Arc<Mutex<HashMap<brink_ir::FileId, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    workspace_roots: Arc<Mutex<Vec<PathBuf>>>,
}

impl Backend {
    pub fn new(
        client: Client,
        db: Arc<Mutex<brink_db::ProjectDb>>,
        analysis_rx: watch::Receiver<Option<Arc<ProjectAnalyses>>>,
        analysis_trigger: Arc<Notify>,
        generation: Arc<AtomicU64>,
        last_published: Arc<
            Mutex<HashMap<brink_ir::FileId, Vec<tower_lsp::lsp_types::Diagnostic>>>,
        >,
    ) -> Self {
        Self {
            client,
            db,
            analysis_rx,
            analysis_trigger,
            generation,
            last_published,
            workspace_roots: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn uri_to_path(uri: &Url) -> Option<String> {
        uri.to_file_path()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Publish per-file diagnostics (parse + lowering only, no analysis).
    /// This gives instant syntax error feedback without waiting for background analysis.
    async fn publish_perfile_diagnostics(&self, uri: &Url, path: &str) {
        let lsp_diags = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(path) else {
                return;
            };

            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return;
            };
            let idx = LineIndex::new(&source);

            let raw_diags: Vec<brink_ir::Diagnostic> =
                db.file_diagnostics(file_id).unwrap_or_default().to_vec();
            let suppressions = db.suppressions(file_id).cloned().unwrap_or_default();
            let filtered = brink_ir::suppressions::apply_suppressions(
                file_id,
                &source,
                raw_diags,
                &suppressions,
            );

            filtered
                .iter()
                .map(|d| convert::diagnostic_to_lsp(d, &idx))
                .collect()
        };

        self.client
            .publish_diagnostics(uri.clone(), lsp_diags, None)
            .await;
    }

    /// Bump the generation counter and notify the background analysis task.
    fn trigger_analysis(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.analysis_trigger.notify_one();
    }

    /// Chase INCLUDE directives from a file that's already in the db.
    fn chase_includes(&self, path: &str) {
        let includes = {
            let db = lock_db(&self.db);
            let Some(fid) = db.file_id(path) else {
                return;
            };
            let Some(hir) = db.hir(fid) else { return };
            hir.includes
                .iter()
                .map(|inc| inc.file_path.clone())
                .collect::<Vec<_>>()
        };

        let base_dir = std::path::Path::new(path).parent();
        for inc_path in &includes {
            if let Some(resolved) =
                base_dir.map(|d| d.join(inc_path).to_string_lossy().into_owned())
            {
                self.load_file_from_disk(&resolved);
            }
        }
    }

    /// Load a file from disk into the database if not already present.
    /// Recursively chases INCLUDE directives.
    fn load_file_from_disk(&self, path: &str) {
        // Check if already loaded
        {
            let db = lock_db(&self.db);
            if db.file_id(path).is_some() {
                return;
            }
        }

        let Ok(contents) = std::fs::read_to_string(path) else {
            tracing::warn!(path, "failed to read file from disk");
            return;
        };

        let mut db = lock_db(&self.db);
        // Double-check under lock
        if db.file_id(path).is_some() {
            return;
        }
        db.set_file(path, contents);

        // Collect includes to chase (release the lock first)
        let includes = db
            .file_id(path)
            .and_then(|fid| db.hir(fid))
            .map(|hir| {
                hir.includes
                    .iter()
                    .map(|inc| inc.file_path.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let base_dir = std::path::Path::new(path).parent();
        let resolved: Vec<String> = includes
            .iter()
            .filter_map(|inc_path| {
                base_dir
                    .map(|d| d.join(inc_path))
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .collect();
        drop(db);

        for resolved_path in resolved {
            self.load_file_from_disk(&resolved_path);
        }
    }

    /// Scan workspace directories for .ink files and load them all.
    fn load_workspace_files(&self) {
        let roots = match self.workspace_roots.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };

        for root in &roots {
            self.walk_and_load(root);
        }

        // Rebuild include graph now that all files are loaded — set_file
        // can only create edges to files already in the db, so files loaded
        // before their include targets will have missing edges.
        let mut db = lock_db(&self.db);
        db.rebuild_include_graph();
    }

    /// Recursively walk a directory, loading all .ink files.
    fn walk_and_load(&self, dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.walk_and_load(&path);
            } else if path.extension().is_some_and(|ext| ext == "ink") {
                let path_str = path.to_string_lossy().into_owned();
                self.load_file_from_disk(&path_str);
            }
        }
    }
}

fn lock_db(db: &Arc<Mutex<brink_db::ProjectDb>>) -> std::sync::MutexGuard<'_, brink_db::ProjectDb> {
    match db.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Snapshot of analysis + per-file data needed for navigation handlers.
struct NavigationSnapshot {
    analysis: Arc<AnalysisResult>,
    source: String,
    file_id: brink_ir::FileId,
    /// (`FileId`, path, source) for files in the same project.
    project_files: Vec<(brink_ir::FileId, String, String)>,
}

impl Backend {
    /// Take a consistent snapshot without running analysis.
    /// Reads the latest analysis result from the watch channel, scoped to the
    /// project that contains the given file.
    fn navigation_snapshot(&self, path: &str) -> Option<NavigationSnapshot> {
        let projects = self.analysis_rx.borrow().clone()?;
        let db = lock_db(&self.db);
        let file_id = db.file_id(path)?;
        let analysis = Arc::clone(projects.for_file(file_id)?);
        let source = db.source(file_id)?.to_owned();

        // Only include files from the same project
        let project_files: Vec<_> = projects
            .project_files_for(file_id)
            .unwrap_or(&[])
            .iter()
            .filter_map(|&fid| {
                let p = db.file_path(fid)?.to_owned();
                let s = db.source(fid)?.to_owned();
                Some((fid, p, s))
            })
            .collect();

        Some(NavigationSnapshot {
            analysis,
            source,
            file_id,
            project_files,
        })
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Save workspace roots for use in initialized()
        let mut roots = Vec::new();
        if let Some(folders) = &params.workspace_folders {
            for folder in folders {
                if let Ok(path) = folder.uri.to_file_path() {
                    roots.push(path);
                }
            }
        }
        if roots.is_empty() {
            // Fallback: legacy root_uri
            let legacy_uri = params.root_uri.as_ref();
            if let Some(uri) = legacy_uri
                && let Ok(path) = uri.to_file_path()
            {
                roots.push(path);
            }
        }
        if let Ok(mut ws) = self.workspace_roots.lock() {
            *ws = roots;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // ── Sync ──
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),

                // ── Navigation ──
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),

                // ── Info ──
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    ..Default::default()
                }),

                // ── Completion ──
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["->".into(), ".".into()]),
                    resolve_provider: Some(true),
                    ..Default::default()
                }),

                // ── Symbols ──
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),

                // ── Semantic tokens ──
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_tokens::legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(true),
                            ..Default::default()
                        },
                    ),
                ),

                // ── Refactoring ──
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                            CodeActionKind::SOURCE,
                        ]),
                        resolve_provider: Some(true),
                        ..Default::default()
                    },
                )),

                // ── Formatting ──
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),

                // ── Structure ──
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),

                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "brink-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::debug!("initialized");

        // Register file watcher for **/*.ink (fire-and-forget — some test
        // clients don't respond to server-initiated requests)
        let client = self.client.clone();
        tokio::spawn(async move {
            let watcher = FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.ink".to_owned()),
                kind: None,
            };
            let registration = Registration {
                id: "ink-file-watcher".to_owned(),
                method: "workspace/didChangeWatchedFiles".to_owned(),
                register_options: serde_json::to_value(
                    tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions {
                        watchers: vec![watcher],
                    },
                )
                .ok(),
            };
            if let Err(e) = client.register_capability(vec![registration]).await {
                tracing::warn!("failed to register file watcher: {e}");
            }
        });

        // Scan workspace directories for .ink files
        self.load_workspace_files();
        self.trigger_analysis();
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        tracing::debug!(count = params.changes.len(), "did_change_watched_files");

        let mut changed = false;
        for change in &params.changes {
            let Some(path) = Self::uri_to_path(&change.uri) else {
                continue;
            };

            match change.typ {
                FileChangeType::CREATED | FileChangeType::CHANGED => {
                    let Ok(contents) = tokio::fs::read_to_string(&path).await else {
                        tracing::warn!(path, "failed to read watched file");
                        continue;
                    };
                    let mut db = lock_db(&self.db);
                    if db.file_id(&path).is_some() {
                        db.update_file(&path, contents);
                    } else {
                        db.set_file(&path, contents);
                    }
                    changed = true;
                }
                FileChangeType::DELETED => {
                    let file_id = {
                        let mut db = lock_db(&self.db);
                        let fid = db.file_id(&path);
                        db.remove_file(&path);
                        fid
                    };
                    if let Some(fid) = file_id
                        && let Ok(mut published) = self.last_published.lock()
                    {
                        published.remove(&fid);
                    }
                    self.client
                        .publish_diagnostics(change.uri.clone(), vec![], None)
                        .await;
                    changed = true;
                }
                _ => {}
            }
        }

        if changed {
            self.trigger_analysis();
        }
    }

    // ── Document sync ────────────────────────────────────────────────

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        tracing::debug!(
            uri = %params.text_document.uri,
            language_id = %params.text_document.language_id,
            "did_open",
        );

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        {
            let mut db = lock_db(&self.db);
            db.set_file(&path, params.text_document.text);
        }

        // Chase INCLUDE directives — load referenced files from disk
        self.chase_includes(&path);

        self.publish_perfile_diagnostics(&params.text_document.uri, &path)
            .await;
        self.trigger_analysis();
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        tracing::debug!(
            uri = %params.text_document.uri,
            version = params.text_document.version,
            "did_change",
        );

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        // Full sync — take the last content change (there should be exactly one)
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };

        {
            let mut db = lock_db(&self.db);
            db.update_file(&path, change.text);
        }

        self.publish_perfile_diagnostics(&params.text_document.uri, &path)
            .await;
        self.trigger_analysis();
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::debug!(uri = %params.text_document.uri, "did_save");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        if let Some(text) = params.text {
            let mut db = lock_db(&self.db);
            db.update_file(&path, text);
        }

        self.publish_perfile_diagnostics(&params.text_document.uri, &path)
            .await;
        self.trigger_analysis();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        tracing::debug!(uri = %params.text_document.uri, "did_close");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        let file_id = {
            let mut db = lock_db(&self.db);
            let fid = db.file_id(&path);
            db.remove_file(&path);
            fid
        };

        // Clear from last_published tracking
        if let Some(fid) = file_id
            && let Ok(mut published) = self.last_published.lock()
        {
            published.remove(&fid);
        }

        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
        self.trigger_analysis();
    }

    // ── Navigation ───────────────────────────────────────────────────

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "goto_definition",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position_params.text_document.uri)
        else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position_params.position, &idx);

        let Some(loc) =
            brink_ide::navigation::goto_definition(&snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };

        // Find the target file in our snapshot
        let Some((_, target_path, target_source)) = snap
            .project_files
            .iter()
            .find(|(fid, _, _)| *fid == loc.file)
        else {
            return Ok(None);
        };

        let target_idx = LineIndex::new(target_source);
        let target_range = convert::to_lsp_range(loc.range, &target_idx);
        let Ok(target_uri) = Url::from_file_path(target_path) else {
            return Ok(None);
        };

        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri,
            range: target_range,
        })))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            "references",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position.position, &idx);

        let refs = brink_ide::navigation::find_references(
            &snap.analysis,
            snap.file_id,
            offset,
            params.context.include_declaration,
        );

        if refs.is_empty() {
            return Ok(None);
        }

        let locations: Vec<_> = refs
            .iter()
            .filter_map(|loc| {
                let (_, file_path, file_source) = snap
                    .project_files
                    .iter()
                    .find(|(fid, _, _)| *fid == loc.file)?;
                let file_idx = LineIndex::new(file_source);
                let uri = Url::from_file_path(file_path).ok()?;
                Some(Location {
                    uri,
                    range: convert::to_lsp_range(loc.range, &file_idx),
                })
            })
            .collect();

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    // ── Info ─────────────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "hover",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position_params.text_document.uri)
        else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position_params.position, &idx);

        let Some(info) = brink_ide::hover::hover(
            &snap.analysis,
            snap.file_id,
            &snap.source,
            offset,
            &snap.project_files,
        ) else {
            return Ok(None);
        };

        let hover_range = info.range.map(|r| convert::to_lsp_range(r, &idx));

        Ok(Some(Hover {
            contents: tower_lsp::lsp_types::HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: info.content,
            }),
            range: hover_range,
        }))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "signature_help",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position_params.text_document.uri)
        else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let pos = params.text_document_position_params.position;
        let offset = idx.offset(pos.line, pos.character);
        let byte_offset: usize = offset.into();

        let Some(sig) =
            brink_ide::signature::signature_help(&snap.analysis, &snap.source, byte_offset)
        else {
            return Ok(None);
        };

        let param_infos: Vec<ParameterInformation> = sig
            .parameters
            .iter()
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.label.clone()),
                documentation: None,
            })
            .collect();

        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label: sig.label,
                documentation: sig
                    .documentation
                    .map(tower_lsp::lsp_types::Documentation::String),
                parameters: Some(param_infos),
                active_parameter: Some(sig.active_parameter),
            }],
            active_signature: Some(0),
            active_parameter: Some(sig.active_parameter),
        }))
    }

    // ── Completion ───────────────────────────────────────────────────

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            "completion",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let pos = params.text_document_position.position;
        let idx = LineIndex::new(&snap.source);
        let byte_offset: usize = idx.offset(pos.line, pos.character).into();

        let ctx = detect_completion_context(&snap.source, byte_offset);
        let cursor_scope = cursor_scope(&snap.source, byte_offset);

        let mut items: Vec<CompletionItem> = Vec::new();

        // For dotted paths, show only children of the specified knot.
        if let CompletionContext::DottedPath { ref knot } = ctx {
            let prefix = format!("{knot}.");
            for (name, ids) in &snap.analysis.index.by_name {
                if let Some(suffix) = name.strip_prefix(&*prefix) {
                    for &def_id in ids {
                        let Some(info) = snap.analysis.index.symbols.get(&def_id) else {
                            continue;
                        };
                        if !matches!(
                            info.kind,
                            brink_ir::SymbolKind::Stitch | brink_ir::SymbolKind::Label
                        ) {
                            continue;
                        }
                        items.push(make_completion_item(info, Some(suffix.to_owned())));
                    }
                }
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }

        for info in snap.analysis.index.symbols.values() {
            if !is_visible_in_context(&ctx, info, &cursor_scope) {
                continue;
            }
            items.push(make_completion_item(info, None));
        }

        // Add synthetic DONE/END for divert context.
        if matches!(
            ctx,
            CompletionContext::Divert | CompletionContext::InlineExpr
        ) {
            for label in &["DONE", "END"] {
                items.push(CompletionItem {
                    label: (*label).to_owned(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("built-in".to_owned()),
                    ..Default::default()
                });
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        tracing::debug!(label = %item.label, "completion_resolve");
        Ok(item)
    }

    // ── Symbols ──────────────────────────────────────────────────────

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "document_symbol");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };

        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(&path) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };
        let Some(source) = db.source(file_id) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };
        let Some(hir) = db.hir(file_id) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };
        let Some(manifest) = db.manifest(file_id) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };

        let idx = LineIndex::new(source);
        let domain_symbols = brink_ide::document::document_symbols(hir, manifest);

        let symbols = domain_symbols
            .into_iter()
            .map(|s| domain_symbol_to_lsp(s, &idx))
            .collect();

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        tracing::debug!(query = %params.query, "workspace_symbol");

        let Some(projects) = self.analysis_rx.borrow().clone() else {
            return Ok(Some(vec![]));
        };

        let all_files = {
            let db = lock_db(&self.db);
            db.file_ids()
                .filter_map(|fid| {
                    let p = db.file_path(fid)?.to_owned();
                    let s = db.source(fid)?.to_owned();
                    Some((fid, p, s))
                })
                .collect::<Vec<_>>()
        };

        let domain_symbols = brink_ide::document::workspace_symbols(
            projects.by_root.values().map(std::convert::AsRef::as_ref),
            &params.query,
        );

        let results = domain_symbols
            .into_iter()
            .filter_map(|ws| {
                let (_, file_path, file_source) =
                    all_files.iter().find(|(fid, _, _)| *fid == ws.file)?;
                let uri = Url::from_file_path(file_path).ok()?;
                let idx = LineIndex::new(file_source);

                #[expect(deprecated, reason = "SymbolInformation requires this field")]
                let sym = SymbolInformation {
                    name: ws.name,
                    kind: convert::symbol_kind_to_lsp(ws.kind),
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri,
                        range: convert::to_lsp_range(ws.range, &idx),
                    },
                    container_name: None,
                };
                Some(sym)
            })
            .collect();

        Ok(Some(results))
    }

    // ── Semantic tokens ──────────────────────────────────────────────

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        tracing::debug!(uri = %params.text_document.uri, "semantic_tokens_full");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let (analysis, source, root, file_id) = {
            let projects = self.analysis_rx.borrow().clone();
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            let analysis = projects.and_then(|p| p.for_file(file_id).cloned());
            let Some(analysis) = analysis else {
                return Ok(None);
            };
            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return Ok(None);
            };
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            (analysis, source, root, file_id)
        };

        let data = semantic_tokens::compute_semantic_tokens(&source, &root, &analysis, file_id);

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        tracing::debug!(uri = %params.text_document.uri, "semantic_tokens_range");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let (analysis, source, root, file_id) = {
            let projects = self.analysis_rx.borrow().clone();
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            let analysis = projects.and_then(|p| p.for_file(file_id).cloned());
            let Some(analysis) = analysis else {
                return Ok(None);
            };
            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return Ok(None);
            };
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            (analysis, source, root, file_id)
        };

        let range = params.range;
        let data = semantic_tokens::compute_semantic_tokens_range(
            &source,
            &root,
            &analysis,
            file_id,
            range.start.line,
            range.end.line,
        );

        Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    // ── Refactoring ──────────────────────────────────────────────────

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "prepare_rename");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.position, &idx);

        let Some(range) = brink_ide::rename::prepare_rename(&snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };

        Ok(Some(PrepareRenameResponse::Range(convert::to_lsp_range(
            range, &idx,
        ))))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            new_name = %params.new_name,
            "rename",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position.position, &idx);

        let Some(result) =
            brink_ide::rename::rename(&snap.analysis, snap.file_id, offset, &params.new_name)
        else {
            return Ok(None);
        };

        // Convert domain edits to LSP WorkspaceEdit
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for edit in &result.edits {
            if let Some((_, file_path, file_source)) = snap
                .project_files
                .iter()
                .find(|(fid, _, _)| *fid == edit.file)
                && let Ok(uri) = Url::from_file_path(file_path)
            {
                let file_idx = LineIndex::new(file_source);
                changes.entry(uri).or_default().push(TextEdit {
                    range: convert::to_lsp_range(edit.range, &file_idx),
                    new_text: edit.new_text.clone(),
                });
            }
        }

        if changes.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "code_action");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(Some(vec![]));
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(Some(vec![]));
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(Some(vec![]));
        };

        let idx = LineIndex::new(&source);
        let cursor_offset: usize = idx
            .offset(params.range.start.line, params.range.start.character)
            .into();

        let domain_actions = brink_ide::code_actions::code_actions(&source, cursor_offset);

        let lsp_actions = domain_actions
            .into_iter()
            .map(|a| {
                let kind = match a.kind {
                    brink_ide::code_actions::CodeActionKind::QuickFix => CodeActionKind::QUICKFIX,
                    brink_ide::code_actions::CodeActionKind::Refactor => CodeActionKind::REFACTOR,
                    brink_ide::code_actions::CodeActionKind::Source => CodeActionKind::SOURCE,
                };

                let data = match &a.data {
                    brink_ide::code_actions::CodeActionData::SortKnots => serde_json::json!({
                        "kind": "sort_knots",
                        "uri": params.text_document.uri.as_str(),
                    }),
                    brink_ide::code_actions::CodeActionData::SortStitches { knot } => {
                        serde_json::json!({
                            "kind": "sort_stitches",
                            "uri": params.text_document.uri.as_str(),
                            "knot": knot,
                        })
                    }
                    brink_ide::code_actions::CodeActionData::FormatKnot { knot } => {
                        serde_json::json!({
                            "kind": "format_knot",
                            "uri": params.text_document.uri.as_str(),
                            "knot": knot,
                        })
                    }
                    brink_ide::code_actions::CodeActionData::FormatStitch { knot, stitch } => {
                        serde_json::json!({
                            "kind": "format_stitch",
                            "uri": params.text_document.uri.as_str(),
                            "knot": knot,
                            "stitch": stitch,
                        })
                    }
                };

                tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(CodeAction {
                    title: a.title,
                    kind: Some(kind),
                    data: Some(data),
                    ..Default::default()
                })
            })
            .collect();

        Ok(Some(lsp_actions))
    }

    async fn code_action_resolve(&self, mut action: CodeAction) -> Result<CodeAction> {
        tracing::debug!(title = %action.title, "code_action_resolve");

        let data = match &action.data {
            Some(obj) => obj.clone(),
            None => return Ok(action),
        };

        let kind = data.get("kind").and_then(serde_json::Value::as_str);
        let uri_str = data
            .get("uri")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        let Ok(uri) = Url::parse(uri_str) else {
            return Ok(action);
        };

        let Some(path) = Self::uri_to_path(&uri) else {
            return Ok(action);
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(action);
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(action);
        };

        let knot_name = data
            .get("knot")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        let action_data = match kind {
            Some("sort_knots") => brink_ide::code_actions::CodeActionData::SortKnots,
            Some("sort_stitches") => brink_ide::code_actions::CodeActionData::SortStitches {
                knot: knot_name.to_owned(),
            },
            Some("format_knot") => brink_ide::code_actions::CodeActionData::FormatKnot {
                knot: knot_name.to_owned(),
            },
            Some("format_stitch") => {
                let stitch_name = data
                    .get("stitch")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                brink_ide::code_actions::CodeActionData::FormatStitch {
                    knot: knot_name.to_owned(),
                    stitch: stitch_name.to_owned(),
                }
            }
            _ => return Ok(action),
        };

        let Some(new_source) = brink_ide::code_actions::resolve_code_action(&source, &action_data)
        else {
            return Ok(action);
        };

        let edits = diff_to_lsp_edits(&source, &new_source);
        let mut changes = HashMap::new();
        changes.insert(uri, edits);

        action.edit = Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        });

        Ok(action)
    }

    // ── Formatting ───────────────────────────────────────────────────

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        tracing::debug!(uri = %params.text_document.uri, "formatting");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(None);
        };

        let config = format_config_from_options(&params.options);
        let formatted = brink_fmt::format(&source, &config);

        if formatted == source {
            return Ok(None);
        }

        Ok(Some(diff_to_lsp_edits(&source, &formatted)))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        tracing::debug!(uri = %params.text_document.uri, "range_formatting");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(None);
        };

        let config = format_config_from_options(&params.options);
        let formatted = brink_fmt::format(&source, &config);

        if formatted == source {
            return Ok(None);
        }

        let all_edits = diff_to_lsp_edits(&source, &formatted);
        let range = params.range;

        // Filter edits to those that overlap the requested range.
        let filtered: Vec<TextEdit> = all_edits
            .into_iter()
            .filter(|edit| ranges_overlap(&edit.range, &range))
            .collect();

        if filtered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(filtered))
        }
    }

    // ── Structure ────────────────────────────────────────────────────

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        tracing::debug!(uri = %params.text_document.uri, "folding_range");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(&path) else {
            return Ok(None);
        };
        let Some(source) = db.source(file_id) else {
            return Ok(None);
        };
        let Some(hir) = db.hir(file_id) else {
            return Ok(None);
        };

        let domain_ranges = brink_ide::folding::folding_ranges(hir, source);

        let ranges = domain_ranges
            .into_iter()
            .map(|r| FoldingRange {
                start_line: r.start_line,
                start_character: None,
                end_line: r.end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: r.collapsed_text,
            })
            .collect();

        Ok(Some(ranges))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<LspInlayHint>>> {
        tracing::debug!(uri = %params.text_document.uri, "inlay_hint");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let range_start = convert::to_text_size(params.range.start, &idx);
        let range_end = convert::to_text_size(params.range.end, &idx);
        let request_range = rowan::TextRange::new(range_start, range_end);

        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(&path) else {
            return Ok(None);
        };
        let Some(parse) = db.parse(file_id) else {
            return Ok(None);
        };
        let root = parse.tree();
        drop(db);

        let domain_hints =
            brink_ide::inlay_hints::inlay_hints(root.syntax(), &snap.analysis, request_range);

        if domain_hints.is_empty() {
            return Ok(None);
        }

        let hints = domain_hints
            .into_iter()
            .map(|h| {
                let (line, col) = idx.line_col(h.offset);
                LspInlayHint {
                    position: Position::new(line, col),
                    label: InlayHintLabel::String(h.label),
                    kind: Some(tower_lsp::lsp_types::InlayHintKind::PARAMETER),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right: Some(h.padding_right),
                    data: None,
                }
            })
            .collect();

        Ok(Some(hints))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        tracing::debug!(uri = %params.text_document.uri, "code_lens");
        Ok(None)
    }

    async fn code_lens_resolve(&self, lens: CodeLens) -> Result<CodeLens> {
        tracing::debug!("code_lens_resolve");
        Ok(lens)
    }
}

// ─── LSP adapter helpers ────────────────────────────────────────────

/// Convert a domain `DocumentSymbol` to an LSP `DocumentSymbol`.
#[expect(deprecated, reason = "DocumentSymbol requires deprecated fields")]
fn domain_symbol_to_lsp(
    sym: brink_ide::document::DocumentSymbol,
    idx: &LineIndex,
) -> tower_lsp::lsp_types::DocumentSymbol {
    let children: Vec<_> = sym
        .children
        .into_iter()
        .map(|c| domain_symbol_to_lsp(c, idx))
        .collect();

    tower_lsp::lsp_types::DocumentSymbol {
        name: sym.name,
        detail: sym.detail,
        kind: convert::symbol_kind_to_lsp(sym.kind),
        tags: None,
        deprecated: None,
        range: convert::to_lsp_range(sym.range, idx),
        selection_range: convert::to_lsp_range(sym.range, idx),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

/// Build a `CompletionItem` from a `SymbolInfo`.
fn make_completion_item(
    info: &brink_ir::SymbolInfo,
    label_override: Option<String>,
) -> CompletionItem {
    let kind = match info.kind {
        brink_ir::SymbolKind::Knot => CompletionItemKind::MODULE,
        brink_ir::SymbolKind::Stitch | brink_ir::SymbolKind::External => {
            CompletionItemKind::FUNCTION
        }
        brink_ir::SymbolKind::Variable
        | brink_ir::SymbolKind::Constant
        | brink_ir::SymbolKind::Param
        | brink_ir::SymbolKind::Temp => CompletionItemKind::VARIABLE,
        brink_ir::SymbolKind::List => CompletionItemKind::ENUM,
        brink_ir::SymbolKind::ListItem => CompletionItemKind::ENUM_MEMBER,
        brink_ir::SymbolKind::Label => CompletionItemKind::REFERENCE,
    };

    let detail = match info.kind {
        brink_ir::SymbolKind::Knot if info.detail.as_deref() == Some("function") => {
            Some("function knot".to_string())
        }
        _ if !info.params.is_empty() => {
            let params: Vec<_> = info.params.iter().map(|p| p.name.as_str()).collect();
            Some(format!("({})", params.join(", ")))
        }
        _ => None,
    };

    CompletionItem {
        label: label_override.unwrap_or_else(|| info.name.clone()),
        kind: Some(kind),
        detail,
        ..Default::default()
    }
}

fn format_config_from_options(
    _options: &tower_lsp::lsp_types::FormattingOptions,
) -> brink_fmt::FormatConfig {
    brink_fmt::FormatConfig::default()
}

/// Convert `brink_ide::diff_to_edits` output to LSP `TextEdit`s.
fn diff_to_lsp_edits(old: &str, new: &str) -> Vec<TextEdit> {
    let idx = LineIndex::new(old);
    brink_ide::diff_to_edits(old, new)
        .into_iter()
        .map(|(range, new_text)| TextEdit {
            range: convert::to_lsp_range(range, &idx),
            new_text,
        })
        .collect()
}

/// Check whether two LSP ranges overlap.
fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
}

// ── Background analysis loop ────────────────────────────────────────

/// Background task that runs per-project cross-file analysis outside the db lock.
///
/// Woken by `trigger.notify_one()` whenever a file changes. Uses `yield_now()`
/// to coalesce rapid edits, then snapshots analysis inputs under the lock,
/// runs per-project analysis without holding the lock, and publishes diagnostics
/// for all files whose diagnostic set changed.
pub async fn analysis_loop(
    db: Arc<Mutex<brink_db::ProjectDb>>,
    _generation: Arc<AtomicU64>,
    trigger: Arc<Notify>,
    tx: watch::Sender<Option<Arc<ProjectAnalyses>>>,
    client: Client,
    last_published: Arc<Mutex<HashMap<brink_ir::FileId, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
) {
    loop {
        trigger.notified().await;
        // Coalesce rapid edits — yield so any queued notifications collapse
        tokio::task::yield_now().await;

        // Snapshot inputs under lock
        let (projects, file_meta, per_file_diags, file_suppressions) = {
            let db = lock_db(&db);
            let project_defs = db.compute_projects();
            let project_inputs: Vec<_> = project_defs
                .iter()
                .map(|(root, members)| (*root, db.analysis_inputs_for(members)))
                .collect();
            let meta = db.file_metadata();
            let diags: Vec<_> = meta
                .iter()
                .filter_map(|(fid, _, _)| Some((*fid, db.file_diagnostics(*fid)?.to_vec())))
                .collect();
            let suppressions: HashMap<brink_ir::FileId, brink_ir::suppressions::Suppressions> =
                meta.iter()
                    .filter_map(|(fid, _, _)| Some((*fid, db.suppressions(*fid)?.clone())))
                    .collect();
            (project_inputs, meta, diags, suppressions)
        };

        // Run per-project analysis OUTSIDE the lock
        let mut by_root = HashMap::new();
        let mut file_to_roots: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>> = HashMap::new();
        let mut project_members = HashMap::new();

        for (root, inputs) in &projects {
            let file_refs: Vec<_> = inputs
                .iter()
                .map(|(id, hir, manifest)| (*id, hir, manifest))
                .collect();
            let result = brink_analyzer::analyze(&file_refs);
            by_root.insert(*root, Arc::new(result));

            let members: Vec<_> = inputs.iter().map(|(id, _, _)| *id).collect();
            for &member in &members {
                file_to_roots.entry(member).or_default().push(*root);
            }
            project_members.insert(*root, members);
        }

        // Sort the root lists for deterministic primary-project selection
        for roots in file_to_roots.values_mut() {
            roots.sort_by_key(|id| id.0);
        }

        let result = Arc::new(ProjectAnalyses {
            by_root,
            file_to_roots,
            project_members,
        });

        // Publish to watch channel
        let _ = tx.send(Some(Arc::clone(&result)));

        // Publish diagnostics for all affected files
        publish_all_diagnostics(
            &client,
            &result,
            &file_meta,
            &per_file_diags,
            &file_suppressions,
            &last_published,
        )
        .await;
    }
}

/// Build a `DiagnosticRelatedInformation` pointing to a project root file.
fn make_project_annotation(
    root_path: &str,
) -> Option<tower_lsp::lsp_types::DiagnosticRelatedInformation> {
    let root_uri = Url::from_file_path(root_path).ok()?;
    Some(tower_lsp::lsp_types::DiagnosticRelatedInformation {
        location: Location {
            uri: root_uri,
            range: Range::default(),
        },
        message: format!("in project: {root_path}"),
    })
}

/// Collect multi-project analysis diagnostics for a file, deduplicating and
/// annotating with project-root related information.
fn collect_multiproject_diags(
    file_id: brink_ir::FileId,
    analyses: &[&Arc<AnalysisResult>],
    roots: &[brink_ir::FileId],
    file_path_map: &HashMap<brink_ir::FileId, &str>,
    idx: &LineIndex,
    lsp_diags: &mut Vec<tower_lsp::lsp_types::Diagnostic>,
) {
    let mut seen: HashMap<(u32, u32, String, String), usize> = HashMap::new();

    for (analysis, root) in analyses.iter().zip(roots) {
        for d in &analysis.diagnostics {
            if d.file != file_id {
                continue;
            }
            let key = (
                d.range.start().into(),
                d.range.end().into(),
                format!("{:?}", d.code),
                d.message.clone(),
            );
            if let Some(&existing_idx) = seen.get(&key) {
                if let Some(ref mut related) = lsp_diags[existing_idx].related_information
                    && let Some(root_path) = file_path_map.get(root)
                    && let Some(annotation) = make_project_annotation(root_path)
                {
                    related.push(annotation);
                }
            } else {
                let mut lsp_diag = convert::diagnostic_to_lsp(d, idx);
                if let Some(root_path) = file_path_map.get(root)
                    && let Some(annotation) = make_project_annotation(root_path)
                {
                    lsp_diag.related_information = Some(vec![annotation]);
                }
                let diag_idx = lsp_diags.len();
                seen.insert(key, diag_idx);
                lsp_diags.push(lsp_diag);
            }
        }
    }

    // Remove annotations from diagnostics that appear in ALL projects (universal)
    let num_projects = analyses.len();
    for &diag_idx in seen.values() {
        if let Some(ref related) = lsp_diags[diag_idx].related_information
            && related.len() >= num_projects
        {
            lsp_diags[diag_idx].related_information = None;
        }
    }
}

/// Compute full diagnostic set for each file and publish if changed.
///
/// Unions analysis diagnostics from all projects containing a file.
/// Applies suppression directives before publishing.
async fn publish_all_diagnostics(
    client: &Client,
    projects: &ProjectAnalyses,
    file_meta: &[(brink_ir::FileId, String, String)],
    per_file_diags: &[(brink_ir::FileId, Vec<brink_ir::Diagnostic>)],
    file_suppressions: &HashMap<brink_ir::FileId, brink_ir::suppressions::Suppressions>,
    last_published: &Mutex<HashMap<brink_ir::FileId, Vec<tower_lsp::lsp_types::Diagnostic>>>,
) {
    let lowering_diags: HashMap<brink_ir::FileId, &[brink_ir::Diagnostic]> = per_file_diags
        .iter()
        .map(|(fid, diags)| (*fid, diags.as_slice()))
        .collect();

    let file_path_map: HashMap<brink_ir::FileId, &str> = file_meta
        .iter()
        .map(|(fid, path, _)| (*fid, path.as_str()))
        .collect();

    // Build set of files whose project root has disable_all
    let disable_all_files: std::collections::HashSet<brink_ir::FileId> = projects
        .project_members
        .iter()
        .filter(|(root, _)| file_suppressions.get(root).is_some_and(|s| s.disable_all))
        .flat_map(|(_, members)| members.iter().copied())
        .collect();

    for (file_id, path, source) in file_meta {
        let idx = LineIndex::new(source);

        // Collect raw IR diagnostics (lowering + analysis) for this file
        let mut raw_diags: Vec<brink_ir::Diagnostic> = lowering_diags
            .get(file_id)
            .copied()
            .unwrap_or_default()
            .to_vec();

        let analyses = projects.all_for_file(*file_id);
        if !disable_all_files.contains(file_id) {
            if analyses.len() <= 1 {
                if let Some(analysis) = analyses.first() {
                    for d in &analysis.diagnostics {
                        if d.file == *file_id {
                            raw_diags.push(d.clone());
                        }
                    }
                }
            } else {
                // Multi-project: collect analysis diags, then convert to LSP
                // (multi-project annotation needs LSP-level conversion)
                let sup = file_suppressions.get(file_id);
                let filtered_lowering = if let Some(sup) = sup {
                    brink_ir::suppressions::apply_suppressions(*file_id, source, raw_diags, sup)
                } else {
                    raw_diags
                };

                let mut lsp_diags: Vec<tower_lsp::lsp_types::Diagnostic> = filtered_lowering
                    .iter()
                    .map(|d| convert::diagnostic_to_lsp(d, &idx))
                    .collect();

                let roots = projects
                    .file_to_roots
                    .get(file_id)
                    .map_or(&[][..], Vec::as_slice);
                collect_multiproject_diags(
                    *file_id,
                    &analyses,
                    roots,
                    &file_path_map,
                    &idx,
                    &mut lsp_diags,
                );

                publish_if_changed(client, last_published, *file_id, path, lsp_diags).await;
                continue;
            }
        }

        // Apply suppressions to the combined diagnostic list
        let sup = file_suppressions.get(file_id);
        let filtered = if let Some(sup) = sup {
            brink_ir::suppressions::apply_suppressions(*file_id, source, raw_diags, sup)
        } else {
            raw_diags
        };

        let lsp_diags: Vec<tower_lsp::lsp_types::Diagnostic> = filtered
            .iter()
            .map(|d| convert::diagnostic_to_lsp(d, &idx))
            .collect();

        publish_if_changed(client, last_published, *file_id, path, lsp_diags).await;
    }
}

/// Publish diagnostics if they differ from the last published set.
async fn publish_if_changed(
    client: &Client,
    last_published: &Mutex<HashMap<brink_ir::FileId, Vec<tower_lsp::lsp_types::Diagnostic>>>,
    file_id: brink_ir::FileId,
    path: &str,
    lsp_diags: Vec<tower_lsp::lsp_types::Diagnostic>,
) {
    let should_publish = {
        let published = match last_published.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match published.get(&file_id) {
            Some(prev) => *prev != lsp_diags,
            None => !lsp_diags.is_empty(),
        }
    };

    if should_publish {
        if let Ok(uri) = Url::from_file_path(path) {
            client
                .publish_diagnostics(uri, lsp_diags.clone(), None)
                .await;
        }

        let mut published = match last_published.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        published.insert(file_id, lsp_diags);
    }
}
