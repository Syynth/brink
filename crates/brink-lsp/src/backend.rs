use std::sync::{Arc, Mutex};

use brink_analyzer::AnalysisResult;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CodeLens, CodeLensOptions, CodeLensParams, CompletionItem,
    CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, DocumentRangeFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, FoldingRange, FoldingRangeKind,
    FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InlayHint, InlayHintParams, Location, MarkupContent, MarkupKind, OneOf,
    PrepareRenameResponse, ReferenceParams, RenameOptions, RenameParams, SaveOptions,
    SemanticTokens, SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SymbolInformation, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, Url, WorkDoneProgressOptions, WorkspaceEdit,
    WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer};

use crate::convert::{self, LineIndex};
use crate::semantic_tokens;

pub struct Backend {
    client: Client,
    db: Arc<Mutex<brink_db::ProjectDb>>,
}

impl Backend {
    pub fn new(client: Client, db: Arc<Mutex<brink_db::ProjectDb>>) -> Self {
        Self { client, db }
    }

    fn uri_to_path(uri: &Url) -> Option<String> {
        uri.to_file_path()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    async fn publish_diagnostics_for_file(&self, uri: &Url, path: &str) {
        let lsp_diags = {
            let mut db = lock_db(&self.db);
            let Some(file_id) = db.file_id(path) else {
                return;
            };

            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return;
            };
            let idx = LineIndex::new(&source);

            // Per-file diagnostics (parse + lowering)
            let mut diags: Vec<_> = db
                .file_diagnostics(file_id)
                .unwrap_or_default()
                .iter()
                .map(|d| convert::diagnostic_to_lsp(d, &idx))
                .collect();

            // Cross-file diagnostics filtered to this file
            let analysis = db.analyze().clone();
            for diag in &analysis.diagnostics {
                if diag.file == file_id {
                    diags.push(convert::diagnostic_to_lsp(diag, &idx));
                }
            }

            diags
        };

        self.client
            .publish_diagnostics(uri.clone(), lsp_diags, None)
            .await;
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
    analysis: AnalysisResult,
    source: String,
    file_id: brink_ir::FileId,
    /// (`FileId`, path, source) for all files in the db.
    all_files: Vec<(brink_ir::FileId, String, String)>,
}

impl Backend {
    /// Take a consistent snapshot under a single lock acquisition.
    fn navigation_snapshot(&self, path: &str) -> Option<NavigationSnapshot> {
        let mut db = lock_db(&self.db);
        let file_id = db.file_id(path)?;
        let source = db.source(file_id)?.to_owned();
        let analysis = db.analyze().clone();
        let all_files: Vec<_> = db
            .file_ids()
            .filter_map(|fid| {
                let p = db.file_path(fid)?.to_owned();
                let s = db.source(fid)?.to_owned();
                Some((fid, p, s))
            })
            .collect();
        Some(NavigationSnapshot {
            analysis,
            source,
            file_id,
            all_files,
        })
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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

        self.publish_diagnostics_for_file(&params.text_document.uri, &path)
            .await;
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

        self.publish_diagnostics_for_file(&params.text_document.uri, &path)
            .await;
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

        self.publish_diagnostics_for_file(&params.text_document.uri, &path)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        tracing::debug!(uri = %params.text_document.uri, "did_close");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        {
            let mut db = lock_db(&self.db);
            db.remove_file(&path);
        }

        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
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

        // Find a resolution whose range contains the cursor (in this file)
        let Some(resolved) = snap.analysis.resolutions.iter().find(|r| {
            r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset)
        }) else {
            return Ok(None);
        };

        let def_id = resolved.target;

        let Some(info) = snap.analysis.index.symbols.get(&def_id) else {
            return Ok(None);
        };

        // Find the target file in our snapshot
        let Some((_, target_path, target_source)) =
            snap.all_files.iter().find(|(fid, _, _)| *fid == info.file)
        else {
            return Ok(None);
        };

        let target_idx = LineIndex::new(target_source);
        let target_range = convert::to_lsp_range(info.range, &target_idx);
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

        // Find which definition the cursor is on
        let def_id = snap
            .analysis
            .resolutions
            .iter()
            .find(|r| {
                r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset)
            })
            .map(|r| r.target)
            .or_else(|| {
                // Maybe the cursor is on a definition site
                snap.analysis
                    .index
                    .symbols
                    .values()
                    .find(|info| {
                        info.file == snap.file_id
                            && (info.range.contains(offset) || info.range.start() == offset)
                    })
                    .map(|info| info.id)
            });

        let Some(def_id) = def_id else {
            return Ok(None);
        };

        let mut locations = Vec::new();

        // Include the definition itself if requested
        if params.context.include_declaration
            && let Some(info) = snap.analysis.index.symbols.get(&def_id)
            && let Some((_, def_path, def_source)) =
                snap.all_files.iter().find(|(fid, _, _)| *fid == info.file)
            && let Ok(uri) = Url::from_file_path(def_path)
        {
            let def_idx = LineIndex::new(def_source);
            locations.push(Location {
                uri,
                range: convert::to_lsp_range(info.range, &def_idx),
            });
        }

        // Collect all reference sites that resolve to this definition.
        for resolved in &snap.analysis.resolutions {
            if resolved.target != def_id {
                continue;
            }

            if let Some((_, file_path, file_source)) = snap
                .all_files
                .iter()
                .find(|(fid, _, _)| *fid == resolved.file)
                && let Ok(uri) = Url::from_file_path(file_path)
            {
                let file_idx = LineIndex::new(file_source);
                locations.push(Location {
                    uri,
                    range: convert::to_lsp_range(resolved.range, &file_idx),
                });
            }
        }

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

        // Find what the cursor is on
        let def_id = snap
            .analysis
            .resolutions
            .iter()
            .find(|r| {
                r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset)
            })
            .map(|r| r.target)
            .or_else(|| {
                snap.analysis
                    .index
                    .symbols
                    .values()
                    .find(|info| {
                        info.file == snap.file_id
                            && (info.range.contains(offset) || info.range.start() == offset)
                    })
                    .map(|info| info.id)
            });

        let Some(def_id) = def_id else {
            return Ok(None);
        };

        let Some(info) = snap.analysis.index.symbols.get(&def_id) else {
            return Ok(None);
        };

        let kind_str = match info.kind {
            brink_ir::SymbolKind::Knot => "knot",
            brink_ir::SymbolKind::Stitch => "stitch",
            brink_ir::SymbolKind::Variable => "variable",
            brink_ir::SymbolKind::Constant => "constant",
            brink_ir::SymbolKind::List => "list",
            brink_ir::SymbolKind::ListItem => "list item",
            brink_ir::SymbolKind::External => "external function",
            brink_ir::SymbolKind::Label => "label",
        };

        let params_str = if info.params.is_empty() {
            String::new()
        } else {
            let parts: Vec<_> = info
                .params
                .iter()
                .map(|p| {
                    let mut s = String::new();
                    if p.is_ref {
                        s.push_str("ref ");
                    }
                    if p.is_divert {
                        s.push_str("-> ");
                    }
                    s.push_str(&p.name);
                    s
                })
                .collect();
            format!("({})", parts.join(", "))
        };

        let detail_str = info
            .detail
            .as_deref()
            .map_or(String::new(), |d| format!(" [{d}]"));

        let file_note = snap
            .all_files
            .iter()
            .find(|(fid, _, _)| *fid == info.file)
            .map_or(String::new(), |(_, p, _)| format!("\n\n*Defined in `{p}`*"));

        let value = format!(
            "**{kind_str}** `{}{params_str}`{detail_str}{file_note}",
            info.name
        );

        let hover_range = snap
            .analysis
            .resolutions
            .iter()
            .find(|r| {
                r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset)
            })
            .map(|r| convert::to_lsp_range(r.range, &idx));

        Ok(Some(Hover {
            contents: tower_lsp::lsp_types::HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: hover_range,
        }))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "signature_help",
        );
        Ok(None)
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

        let items: Vec<CompletionItem> = snap
            .analysis
            .index
            .symbols
            .values()
            .map(|info| {
                let kind = match info.kind {
                    brink_ir::SymbolKind::Knot => CompletionItemKind::MODULE,
                    brink_ir::SymbolKind::Stitch | brink_ir::SymbolKind::External => {
                        CompletionItemKind::FUNCTION
                    }
                    brink_ir::SymbolKind::Variable | brink_ir::SymbolKind::Constant => {
                        CompletionItemKind::VARIABLE
                    }
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
                    label: info.name.clone(),
                    kind: Some(kind),
                    detail,
                    ..Default::default()
                }
            })
            .collect();

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
        let mut symbols = Vec::new();

        // Knots with their stitches as children
        for knot in &hir.knots {
            let children: Vec<_> = knot
                .stitches
                .iter()
                .map(|stitch| {
                    #[expect(deprecated, reason = "DocumentSymbol requires this field")]
                    tower_lsp::lsp_types::DocumentSymbol {
                        name: stitch.name.text.clone(),
                        detail: None,
                        kind: tower_lsp::lsp_types::SymbolKind::METHOD,
                        tags: None,
                        deprecated: None,
                        range: convert::to_lsp_range(stitch.name.range, &idx),
                        selection_range: convert::to_lsp_range(stitch.name.range, &idx),
                        children: None,
                    }
                })
                .collect();

            #[expect(deprecated, reason = "DocumentSymbol requires this field")]
            let sym = tower_lsp::lsp_types::DocumentSymbol {
                name: knot.name.text.clone(),
                detail: if knot.is_function {
                    Some("function".to_owned())
                } else {
                    None
                },
                kind: tower_lsp::lsp_types::SymbolKind::FUNCTION,
                tags: None,
                deprecated: None,
                range: convert::to_lsp_range(knot.name.range, &idx),
                selection_range: convert::to_lsp_range(knot.name.range, &idx),
                children: if children.is_empty() {
                    None
                } else {
                    Some(children)
                },
            };
            symbols.push(sym);
        }

        // Top-level declarations from manifest
        let decl_groups: &[(
            &[brink_ir::DeclaredSymbol],
            tower_lsp::lsp_types::SymbolKind,
        )] = &[
            (
                &manifest.variables,
                tower_lsp::lsp_types::SymbolKind::VARIABLE,
            ),
            (&manifest.lists, tower_lsp::lsp_types::SymbolKind::ENUM),
            (
                &manifest.externals,
                tower_lsp::lsp_types::SymbolKind::FUNCTION,
            ),
        ];

        for (decls, kind) in decl_groups {
            for decl in *decls {
                #[expect(deprecated, reason = "DocumentSymbol requires this field")]
                let sym = tower_lsp::lsp_types::DocumentSymbol {
                    name: decl.name.clone(),
                    detail: None,
                    kind: *kind,
                    tags: None,
                    deprecated: None,
                    range: convert::to_lsp_range(decl.range, &idx),
                    selection_range: convert::to_lsp_range(decl.range, &idx),
                    children: None,
                };
                symbols.push(sym);
            }
        }

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        tracing::debug!(query = %params.query, "workspace_symbol");

        let (analysis, all_files) = {
            let mut db = lock_db(&self.db);
            let analysis = db.analyze().clone();
            let files: Vec<_> = db
                .file_ids()
                .filter_map(|fid| {
                    let p = db.file_path(fid)?.to_owned();
                    let s = db.source(fid)?.to_owned();
                    Some((fid, p, s))
                })
                .collect();
            (analysis, files)
        };

        let query = params.query.to_lowercase();
        let mut results = Vec::new();

        for info in analysis.index.symbols.values() {
            if !query.is_empty() && !info.name.to_lowercase().contains(&query) {
                continue;
            }

            let Some((_, file_path, file_source)) =
                all_files.iter().find(|(fid, _, _)| *fid == info.file)
            else {
                continue;
            };
            let Ok(uri) = Url::from_file_path(file_path) else {
                continue;
            };

            let idx = LineIndex::new(file_source);

            #[expect(deprecated, reason = "SymbolInformation requires this field")]
            let sym = SymbolInformation {
                name: info.name.clone(),
                kind: convert::symbol_kind_to_lsp(info.kind),
                tags: None,
                deprecated: None,
                location: Location {
                    uri,
                    range: convert::to_lsp_range(info.range, &idx),
                },
                container_name: None,
            };
            results.push(sym);
        }

        Ok(Some(results))
    }

    // ── Semantic tokens ──────────────────────────────────────────────

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        tracing::debug!(uri = %params.text_document.uri, "semantic_tokens_full");
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: vec![],
        })))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        tracing::debug!(uri = %params.text_document.uri, "semantic_tokens_range");
        Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
            result_id: None,
            data: vec![],
        })))
    }

    // ── Refactoring ──────────────────────────────────────────────────

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "prepare_rename");
        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            new_name = %params.new_name,
            "rename",
        );
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "code_action");
        Ok(Some(vec![]))
    }

    async fn code_action_resolve(&self, action: CodeAction) -> Result<CodeAction> {
        tracing::debug!(title = %action.title, "code_action_resolve");
        Ok(action)
    }

    // ── Formatting ───────────────────────────────────────────────────

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        tracing::debug!(uri = %params.text_document.uri, "formatting");
        Ok(None)
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        tracing::debug!(uri = %params.text_document.uri, "range_formatting");
        Ok(None)
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

        let idx = LineIndex::new(source);
        let mut ranges = Vec::new();

        // Root-level block content
        collect_block_folds(&hir.root_content, &idx, &mut ranges);

        for knot in &hir.knots {
            let knot_range = knot.ptr.text_range();
            let (start_line, _) = idx.line_col(knot_range.start());
            let (end_line, _) = idx.line_col(knot_range.end());
            if end_line > start_line {
                ranges.push(FoldingRange {
                    start_line,
                    start_character: None,
                    end_line,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: Some(format!("== {} ==", knot.name.text)),
                });
            }

            collect_block_folds(&knot.body, &idx, &mut ranges);

            for stitch in &knot.stitches {
                let stitch_range = stitch.ptr.text_range();
                let (s_start, _) = idx.line_col(stitch_range.start());
                let (s_end, _) = idx.line_col(stitch_range.end());
                if s_end > s_start {
                    ranges.push(FoldingRange {
                        start_line: s_start,
                        start_character: None,
                        end_line: s_end,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some(format!("= {}", stitch.name.text)),
                    });
                }

                collect_block_folds(&stitch.body, &idx, &mut ranges);
            }
        }

        Ok(Some(ranges))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        tracing::debug!(uri = %params.text_document.uri, "inlay_hint");
        Ok(None)
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

// ─── Folding range helpers ──────────────────────────────────────────

fn push_fold(
    range: rowan::TextRange,
    collapsed: Option<String>,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    let (start_line, _) = idx.line_col(range.start());
    let (end_line, _) = idx.line_col(range.end());
    if end_line > start_line {
        out.push(FoldingRange {
            start_line,
            start_character: None,
            end_line,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: collapsed,
        });
    }
}

fn collect_block_folds(block: &brink_ir::Block, idx: &LineIndex, out: &mut Vec<FoldingRange>) {
    for stmt in &block.stmts {
        collect_stmt_folds(stmt, idx, out);
    }
}

fn collect_stmt_folds(stmt: &brink_ir::Stmt, idx: &LineIndex, out: &mut Vec<FoldingRange>) {
    match stmt {
        brink_ir::Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                push_fold(choice.ptr.text_range(), None, idx, out);
                collect_block_folds(&choice.body, idx, out);
            }
            if let Some(gather) = &cs.gather {
                // Gather content may contain inline folds
                if let Some(content) = &gather.content {
                    collect_content_folds(content, idx, out);
                }
            }
        }
        brink_ir::Stmt::Conditional(cond) => {
            push_fold(cond.ptr.text_range(), Some("{...}".to_owned()), idx, out);
            for branch in &cond.branches {
                collect_block_folds(&branch.body, idx, out);
            }
        }
        brink_ir::Stmt::Sequence(seq) => {
            push_fold(seq.ptr.text_range(), Some("{...}".to_owned()), idx, out);
            for branch in &seq.branches {
                collect_block_folds(branch, idx, out);
            }
        }
        brink_ir::Stmt::Content(content) => {
            collect_content_folds(content, idx, out);
        }
        _ => {}
    }
}

fn collect_content_folds(
    content: &brink_ir::Content,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    collect_content_part_folds(&content.parts, idx, out);
}

fn collect_content_part_folds(
    parts: &[brink_ir::ContentPart],
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    for part in parts {
        match part {
            brink_ir::ContentPart::InlineConditional(cond) => {
                push_fold(cond.ptr.text_range(), Some("{...}".to_owned()), idx, out);
                for branch in &cond.branches {
                    collect_block_folds(&branch.body, idx, out);
                }
            }
            brink_ir::ContentPart::InlineSequence(seq) => {
                push_fold(seq.ptr.text_range(), Some("{...}".to_owned()), idx, out);
                for branch in &seq.branches {
                    collect_block_folds(branch, idx, out);
                }
            }
            _ => {}
        }
    }
}
