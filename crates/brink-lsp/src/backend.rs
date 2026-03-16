use std::collections::HashMap;
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
    ParameterInformation, ParameterLabel, Position, PrepareRenameResponse, Range, ReferenceParams,
    RenameOptions, RenameParams, SaveOptions, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensOptions, SemanticTokensParams, SemanticTokensRangeParams,
    SemanticTokensRangeResult, SemanticTokensResult, SemanticTokensServerCapabilities,
    ServerCapabilities, ServerInfo, SignatureHelp, SignatureHelpOptions, SignatureHelpParams,
    SignatureInformation, SymbolInformation, TextDocumentPositionParams,
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

        let Some(info) = find_def_at_offset(&snap, offset) else {
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

        let value = if let Some(info) = find_def_at_offset(&snap, offset) {
            let kind_str = match info.kind {
                brink_ir::SymbolKind::Knot => "knot",
                brink_ir::SymbolKind::Stitch => "stitch",
                brink_ir::SymbolKind::Variable => "variable",
                brink_ir::SymbolKind::Constant => "constant",
                brink_ir::SymbolKind::List => "list",
                brink_ir::SymbolKind::ListItem => "list item",
                brink_ir::SymbolKind::External => "external function",
                brink_ir::SymbolKind::Label => "label",
                brink_ir::SymbolKind::Param => "parameter",
                brink_ir::SymbolKind::Temp => "temp variable",
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

            format!(
                "**{kind_str}** `{}{params_str}`{detail_str}{file_note}",
                info.name
            )
        } else if let Some(builtin) =
            word_at_offset(&snap.source, offset).and_then(builtin_hover_text)
        {
            builtin
        } else {
            return Ok(None);
        };

        let hover_range = snap
            .analysis
            .resolutions
            .iter()
            .find(|r| {
                r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset)
            })
            .map(|r| convert::to_lsp_range(r.range, &idx))
            .or_else(|| {
                // For locals/builtins matched by word text, compute range from word bounds.
                let word_range = word_range_at_offset(&snap.source, offset)?;
                Some(convert::to_lsp_range(word_range, &idx))
            });

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

        let Some((func_name, active_param)) = find_call_context(&snap.source, byte_offset) else {
            return Ok(None);
        };

        // Look up the function in the symbol index.
        let info = snap.analysis.index.symbols.values().find(|info| {
            matches!(
                info.kind,
                brink_ir::SymbolKind::Knot
                    | brink_ir::SymbolKind::Stitch
                    | brink_ir::SymbolKind::External
            ) && info.name == func_name
                && !info.params.is_empty()
        });

        let Some(info) = info else {
            return Ok(None);
        };

        let param_infos: Vec<ParameterInformation> = info
            .params
            .iter()
            .map(|p| {
                let label = if p.is_ref {
                    format!("ref {}", p.name)
                } else if p.is_divert {
                    format!("-> {}", p.name)
                } else {
                    p.name.clone()
                };
                ParameterInformation {
                    label: ParameterLabel::Simple(label),
                    documentation: None,
                }
            })
            .collect();

        let param_labels: Vec<String> = param_infos
            .iter()
            .map(|p| match &p.label {
                ParameterLabel::Simple(s) => s.clone(),
                ParameterLabel::LabelOffsets(_) => String::new(),
            })
            .collect();

        let signature_label = format!("{}({})", func_name, param_labels.join(", "));

        #[expect(
            clippy::cast_possible_truncation,
            reason = "active param index fits in u32"
        )]
        let active = active_param.min(info.params.len().saturating_sub(1)) as u32;

        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label: signature_label,
                documentation: info
                    .detail
                    .as_ref()
                    .map(|d| tower_lsp::lsp_types::Documentation::String(d.clone())),
                parameters: Some(param_infos),
                active_parameter: Some(active),
            }],
            active_signature: Some(0),
            active_parameter: Some(active),
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

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let (source, root, analysis, file_id) = {
            let mut db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return Ok(None);
            };
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            let analysis = db.analyze().clone();
            (source, root, analysis, file_id)
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

        let (source, root, analysis, file_id) = {
            let mut db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return Ok(None);
            };
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            let analysis = db.analyze().clone();
            (source, root, analysis, file_id)
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

        let Some(info) = find_def_at_offset(&snap, offset) else {
            return Ok(None);
        };

        // Builtins and externals cannot be renamed
        if matches!(info.kind, brink_ir::SymbolKind::External) {
            return Ok(None);
        }

        // Return the range of the symbol under the cursor (reference or definition site)
        let rename_range = snap
            .analysis
            .resolutions
            .iter()
            .find(|r| {
                r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset)
            })
            .map(|r| r.range)
            .or_else(|| (info.file == snap.file_id).then_some(info.range));

        let Some(range) = rename_range else {
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

        let Some(info) = find_def_at_offset(&snap, offset) else {
            return Ok(None);
        };

        if matches!(info.kind, brink_ir::SymbolKind::External) {
            return Ok(None);
        }

        let def_id = info.id;
        let new_name = &params.new_name;

        // Collect all edits grouped by file URI
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        // 1. Rename the definition site
        if let Some((_, def_path, def_source)) =
            snap.all_files.iter().find(|(fid, _, _)| *fid == info.file)
            && let Ok(uri) = Url::from_file_path(def_path)
        {
            let def_idx = LineIndex::new(def_source);
            changes.entry(uri).or_default().push(TextEdit {
                range: convert::to_lsp_range(info.range, &def_idx),
                new_text: new_name.clone(),
            });
        }

        // 2. Rename all reference sites
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
                changes.entry(uri).or_default().push(TextEdit {
                    range: convert::to_lsp_range(resolved.range, &file_idx),
                    new_text: new_name.clone(),
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

        Ok(Some(collect_code_actions(
            &source,
            params.text_document.uri.as_ref(),
            params.range.start,
        )))
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

        let new_source = match kind {
            Some("sort_knots") => sort_knots_in_source(&source),
            Some("sort_stitches") => sort_stitches_in_knot(&source, knot_name),
            Some("format_knot") => format_region(&source, knot_name, None),
            Some("format_stitch") => {
                let stitch_name = data
                    .get("stitch")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                format_region(&source, knot_name, Some(stitch_name))
            }
            _ => return Ok(action),
        };

        if new_source == source {
            return Ok(action);
        }

        let edits = diff_to_edits(&source, &new_source);
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

        Ok(Some(diff_to_edits(&source, &formatted)))
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

        let all_edits = diff_to_edits(&source, &formatted);
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

        let idx = LineIndex::new(source);
        let mut ranges = Vec::new();

        // Root-level block content
        collect_block_folds(&hir.root_content, source, &idx, &mut ranges);

        for knot in &hir.knots {
            push_fold(knot.ptr.text_range(), None, source, &idx, &mut ranges);

            collect_block_folds(&knot.body, source, &idx, &mut ranges);

            for stitch in &knot.stitches {
                push_fold(stitch.ptr.text_range(), None, source, &idx, &mut ranges);

                collect_block_folds(&stitch.body, source, &idx, &mut ranges);
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

// ─── Definition lookup ─────────────────────────────────────────────

/// Find the definition id for the symbol at `offset`.
///
/// Tries, in order: resolved references, declaration sites, then local
/// variables (params/temps) by identifier text.
fn find_def_at_offset(
    snap: &NavigationSnapshot,
    offset: rowan::TextSize,
) -> Option<&brink_ir::SymbolInfo> {
    // 1. Resolved reference at this position
    let def_id = snap
        .analysis
        .resolutions
        .iter()
        .find(|r| r.file == snap.file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.target)
        // 2. Declaration site at this position
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

    def_id.and_then(|id| snap.analysis.index.symbols.get(&id))
}

// ─── Builtin hover ─────────────────────────────────────────────────

/// Return hover markdown for an ink built-in function, or `None` if not a builtin.
fn builtin_hover_text(name: &str) -> Option<String> {
    let (signature, description) = match name {
        "CHOICE_COUNT" => ("CHOICE_COUNT()", "Number of currently available choices"),
        "TURNS_SINCE" => (
            "TURNS_SINCE(-> knot)",
            "Turns since a knot was last visited (-1 if never)",
        ),
        "READ_COUNT" => (
            "READ_COUNT(-> knot)",
            "Number of times a knot has been visited",
        ),
        "RANDOM" => (
            "RANDOM(min, max)",
            "Random integer between min and max (inclusive)",
        ),
        "SEED_RANDOM" => ("SEED_RANDOM(seed)", "Seed the random number generator"),
        "INT" => ("INT(value)", "Cast to integer"),
        "FLOAT" => ("FLOAT(value)", "Cast to float"),
        "FLOOR" => ("FLOOR(value)", "Round down to nearest integer"),
        "CEILING" => ("CEILING(value)", "Round up to nearest integer"),
        "POW" => ("POW(base, exp)", "Raise base to the power of exp"),
        "MIN" => ("MIN(a, b)", "Minimum of two values"),
        "MAX" => ("MAX(a, b)", "Maximum of two values"),
        "LIST_COUNT" => ("LIST_COUNT(list)", "Number of items in a list value"),
        "LIST_MIN" => ("LIST_MIN(list)", "Lowest-valued item in a list"),
        "LIST_MAX" => ("LIST_MAX(list)", "Highest-valued item in a list"),
        "LIST_ALL" => ("LIST_ALL(list)", "All possible items for a list's type"),
        "LIST_INVERT" => ("LIST_INVERT(list)", "Items not in the list (from its type)"),
        "LIST_RANGE" => (
            "LIST_RANGE(list, min, max)",
            "Items in list between min and max",
        ),
        "LIST_RANDOM" => ("LIST_RANDOM(list)", "Random item from a list"),
        "LIST_VALUE" => ("LIST_VALUE(item)", "Numeric value of a list item"),
        "LIST_FROM_INT" => (
            "LIST_FROM_INT(list, n)",
            "Item at numeric position n in a list type",
        ),
        _ => return None,
    };
    Some(format!("**built-in** `{signature}`\n\n{description}"))
}

// ─── Signature help helpers ────────────────────────────────────────

/// Find the function call context at the given byte offset.
///
/// Returns `(function_name, active_parameter_index)` if the cursor is inside
/// a function call's parentheses, e.g. `myFunc(a, |)` → `("myFunc", 1)`.
fn find_call_context(source: &str, byte_offset: usize) -> Option<(String, usize)> {
    let before = source.get(..byte_offset)?;

    // Scan backwards to find the matching open paren, tracking nesting.
    let mut depth = 0i32;
    let mut commas = 0usize;
    let mut paren_pos = None;

    for (i, ch) in before.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            ',' if depth == 0 => commas += 1,
            '\n' if depth == 0 => return None, // don't cross line boundaries at depth 0
            _ => {}
        }
    }

    let paren_pos = paren_pos?;

    // Extract the identifier immediately before the open paren.
    let before_paren = before[..paren_pos].trim_end();
    if before_paren.is_empty() {
        return None;
    }

    // Walk backwards over identifier characters.
    let name_end = before_paren.len();
    let name_start = before_paren
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(name_end, |(i, _)| i);

    let name = &before_paren[name_start..name_end];
    if name.is_empty() {
        return None;
    }

    Some((name.to_owned(), commas))
}

// ─── Hover helpers ─────────────────────────────────────────────────

/// Extract the identifier word surrounding `offset` in `source`.
fn word_at_offset(source: &str, offset: rowan::TextSize) -> Option<&str> {
    let pos: usize = offset.into();
    if pos >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    // The cursor must be on a word character
    if !is_word(bytes[pos]) {
        return None;
    }
    let mut start = pos;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = pos + 1;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    Some(&source[start..end])
}

/// Like `word_at_offset` but returns the `TextRange` of the word.
fn word_range_at_offset(source: &str, offset: rowan::TextSize) -> Option<rowan::TextRange> {
    let pos: usize = offset.into();
    if pos >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if !is_word(bytes[pos]) {
        return None;
    }
    let mut start = pos;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = pos + 1;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    let start = u32::try_from(start).ok()?;
    let end = u32::try_from(end).ok()?;
    Some(rowan::TextRange::new(
        rowan::TextSize::from(start),
        rowan::TextSize::from(end),
    ))
}

// ─── Folding range helpers ──────────────────────────────────────────

fn push_fold(
    range: rowan::TextRange,
    collapsed: Option<String>,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    let start_byte = usize::from(range.start());
    let end_byte = usize::from(range.end()).min(source.len());
    let slice = &source[start_byte..end_byte];

    // Trim leading whitespace to find the real start line
    let trimmed_start = start_byte + (slice.len() - slice.trim_start().len());
    // Trim trailing whitespace to find the real end line
    let trimmed_end = start_byte + slice.trim_end().len();

    if trimmed_start >= trimmed_end {
        return;
    }

    let (start_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(trimmed_start).unwrap_or(u32::MAX),
    ));
    let (end_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(trimmed_end).unwrap_or(u32::MAX),
    ));
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

fn collect_block_folds(
    block: &brink_ir::Block,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    for stmt in &block.stmts {
        collect_stmt_folds(stmt, source, idx, out);
    }
}

fn collect_stmt_folds(
    stmt: &brink_ir::Stmt,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    match stmt {
        brink_ir::Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                push_fold(choice.ptr.text_range(), None, source, idx, out);
                collect_block_folds(&choice.body, source, idx, out);
            }
            collect_block_folds(&cs.continuation, source, idx, out);
        }
        brink_ir::Stmt::LabeledBlock(block) => {
            collect_block_folds(block, source, idx, out);
        }
        brink_ir::Stmt::Conditional(cond) => {
            push_fold(
                cond.ptr.text_range(),
                Some("{...}".to_owned()),
                source,
                idx,
                out,
            );
            for branch in &cond.branches {
                collect_block_folds(&branch.body, source, idx, out);
            }
        }
        brink_ir::Stmt::Sequence(seq) => {
            push_fold(
                seq.ptr.text_range(),
                Some("{...}".to_owned()),
                source,
                idx,
                out,
            );
            for branch in &seq.branches {
                collect_block_folds(branch, source, idx, out);
            }
        }
        brink_ir::Stmt::Content(content) => {
            collect_content_folds(content, source, idx, out);
        }
        _ => {}
    }
}

fn collect_content_folds(
    content: &brink_ir::Content,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    collect_content_part_folds(&content.parts, source, idx, out);
}

fn collect_content_part_folds(
    parts: &[brink_ir::ContentPart],
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldingRange>,
) {
    for part in parts {
        match part {
            brink_ir::ContentPart::InlineConditional(cond) => {
                push_fold(
                    cond.ptr.text_range(),
                    Some("{...}".to_owned()),
                    source,
                    idx,
                    out,
                );
                for branch in &cond.branches {
                    collect_block_folds(&branch.body, source, idx, out);
                }
            }
            brink_ir::ContentPart::InlineSequence(seq) => {
                push_fold(
                    seq.ptr.text_range(),
                    Some("{...}".to_owned()),
                    source,
                    idx,
                    out,
                );
                for branch in &seq.branches {
                    collect_block_folds(branch, source, idx, out);
                }
            }
            _ => {}
        }
    }
}

// ─── Formatting helpers ─────────────────────────────────────────────

fn format_config_from_options(
    _options: &tower_lsp::lsp_types::FormattingOptions,
) -> brink_fmt::FormatConfig {
    // Always use the formatter's default (2-space indent) regardless of
    // the editor's tab_size setting. Ink indentation is structural, not
    // configurable per-editor.
    brink_fmt::FormatConfig::default()
}

/// Format only a specific knot or stitch region, leaving the rest unchanged.
///
/// Formats the whole document, then replaces only the lines corresponding to
/// the targeted region. Since formatting can change line lengths (shifting byte
/// offsets), we identify the region by line number in the original source.
fn format_region(source: &str, knot_name: &str, stitch_name: Option<&str>) -> String {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    let Some((ki, knot)) = knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    else {
        return source.to_owned();
    };

    let knot_start: usize = knot.syntax().text_range().start().into();
    let knot_end: usize = if ki + 1 < knots.len() {
        knots[ki + 1].syntax().text_range().start().into()
    } else {
        source.len()
    };

    let (region_start, region_end) = if let Some(sname) = stitch_name {
        let Some(body) = knot.body() else {
            return source.to_owned();
        };
        let stitches: Vec<_> = body.stitches().collect();
        let Some((si, stitch)) = stitches
            .iter()
            .enumerate()
            .find(|(_, s)| s.header().and_then(|h| h.name()).as_deref() == Some(sname))
        else {
            return source.to_owned();
        };
        let start: usize = stitch.syntax().text_range().start().into();
        let end: usize = if si + 1 < stitches.len() {
            stitches[si + 1].syntax().text_range().start().into()
        } else {
            knot_end
        };
        (start, end)
    } else {
        (knot_start, knot_end)
    };

    // Format the whole file
    let config = brink_fmt::FormatConfig::default();
    let formatted = brink_fmt::format(source, &config);

    // Splice: keep original before/after region, use formatted for the region.
    // Because formatting is line-based and preserves structure, the byte offsets
    // in the original source correctly delimit the region to replace.
    // The formatted output has the same structure, so we re-parse it to find the
    // matching region boundaries.
    let fmt_parse = brink_syntax::parse(&formatted);
    let fmt_tree = fmt_parse.tree();

    let fmt_knots: Vec<_> = fmt_tree.knots().collect();
    let Some((fki, fmt_knot)) = fmt_knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    else {
        return source.to_owned();
    };

    let fmt_knot_start: usize = fmt_knot.syntax().text_range().start().into();
    let fmt_knot_end: usize = if fki + 1 < fmt_knots.len() {
        fmt_knots[fki + 1].syntax().text_range().start().into()
    } else {
        formatted.len()
    };

    let (fmt_region_start, fmt_region_end) = if let Some(sname) = stitch_name {
        let Some(body) = fmt_knot.body() else {
            return source.to_owned();
        };
        let fmt_stitches: Vec<_> = body.stitches().collect();
        let Some((fsi, fmt_stitch)) = fmt_stitches
            .iter()
            .enumerate()
            .find(|(_, s)| s.header().and_then(|h| h.name()).as_deref() == Some(sname))
        else {
            return source.to_owned();
        };
        let start: usize = fmt_stitch.syntax().text_range().start().into();
        let end: usize = if fsi + 1 < fmt_stitches.len() {
            fmt_stitches[fsi + 1].syntax().text_range().start().into()
        } else {
            fmt_knot_end
        };
        (start, end)
    } else {
        (fmt_knot_start, fmt_knot_end)
    };

    let mut result = String::with_capacity(formatted.len());
    result.push_str(&source[..region_start]);
    result.push_str(&formatted[fmt_region_start..fmt_region_end]);
    result.push_str(&source[region_end..]);
    result
}

// ─── Code action helpers ────────────────────────────────────────────

/// Collect all applicable code actions for the given source and cursor position.
#[expect(clippy::too_many_lines, reason = "sequential action collection")]
fn collect_code_actions(
    source: &str,
    uri_str: &str,
    cursor_pos: Position,
) -> Vec<tower_lsp::lsp_types::CodeActionOrCommand> {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let mut actions = Vec::new();

    // ── Sort knots ──────────────────────────────────────────────
    let knot_names: Vec<String> = tree.knots().filter_map(|k| k.header()?.name()).collect();

    if knot_names.len() >= 2 {
        let already_sorted = knot_names
            .windows(2)
            .all(|w| w[0].to_lowercase() <= w[1].to_lowercase());

        if !already_sorted {
            actions.push(tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(
                CodeAction {
                    title: "Sort knots alphabetically".to_owned(),
                    kind: Some(CodeActionKind::SOURCE),
                    data: Some(serde_json::json!({
                        "kind": "sort_knots",
                        "uri": uri_str,
                    })),
                    ..Default::default()
                },
            ));
        }
    }

    // ── Cursor-scoped actions ───────────────────────────────────
    let idx = LineIndex::new(source);
    let cursor = idx.offset(cursor_pos.line, cursor_pos.character);

    let config = brink_fmt::FormatConfig::default();
    let formatted = brink_fmt::format(source, &config);

    let knots: Vec<_> = tree.knots().collect();
    for (ki, knot) in knots.iter().enumerate() {
        let knot_range = knot.syntax().text_range();
        if cursor < knot_range.start() || cursor > knot_range.end() {
            continue;
        }

        let knot_name = knot.header().and_then(|h| h.name()).unwrap_or_default();

        let knot_start: usize = knot_range.start().into();
        let knot_end: usize = if ki + 1 < knots.len() {
            knots[ki + 1].syntax().text_range().start().into()
        } else {
            source.len()
        };

        // Format knot
        if source.get(knot_start..knot_end) != formatted.get(knot_start..knot_end) {
            actions.push(tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(
                CodeAction {
                    title: format!("Format knot '{knot_name}'"),
                    kind: Some(CodeActionKind::SOURCE),
                    data: Some(serde_json::json!({
                        "kind": "format_knot",
                        "uri": uri_str,
                        "knot": knot_name,
                    })),
                    ..Default::default()
                },
            ));
        }

        // Sort stitches
        let Some(body) = knot.body() else { break };
        let stitches: Vec<_> = body.stitches().collect();

        let stitch_names: Vec<String> =
            stitches.iter().filter_map(|s| s.header()?.name()).collect();

        if stitch_names.len() >= 2 {
            let already_sorted = stitch_names
                .windows(2)
                .all(|w| w[0].to_lowercase() <= w[1].to_lowercase());

            if !already_sorted {
                actions.push(tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(
                    CodeAction {
                        title: format!("Sort stitches in '{knot_name}' alphabetically"),
                        kind: Some(CodeActionKind::SOURCE),
                        data: Some(serde_json::json!({
                            "kind": "sort_stitches",
                            "uri": uri_str,
                            "knot": knot_name,
                        })),
                        ..Default::default()
                    },
                ));
            }
        }

        // Format stitch
        for (si, stitch) in stitches.iter().enumerate() {
            let stitch_range = stitch.syntax().text_range();
            if cursor < stitch_range.start() || cursor > stitch_range.end() {
                continue;
            }

            let stitch_name = stitch.header().and_then(|h| h.name()).unwrap_or_default();

            let stitch_start: usize = stitch_range.start().into();
            let stitch_end: usize = if si + 1 < stitches.len() {
                stitches[si + 1].syntax().text_range().start().into()
            } else {
                knot_end
            };

            if source.get(stitch_start..stitch_end) != formatted.get(stitch_start..stitch_end) {
                actions.push(tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(
                    CodeAction {
                        title: format!("Format stitch '{stitch_name}'"),
                        kind: Some(CodeActionKind::SOURCE),
                        data: Some(serde_json::json!({
                            "kind": "format_stitch",
                            "uri": uri_str,
                            "knot": knot_name,
                            "stitch": stitch_name,
                        })),
                        ..Default::default()
                    },
                ));
            }
            break;
        }

        break;
    }

    actions
}

/// Sort knot definitions in the source alphabetically by name.
///
/// Returns the full source with knots reordered. The preamble (everything before
/// the first knot) is preserved. Each knot's slice runs from its start to just
/// before the next knot (or EOF).
fn sort_knots_in_source(source: &str) -> String {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    if knots.len() < 2 {
        return source.to_owned();
    }

    // Separate trailing whitespace after the last knot's AST node so it
    // stays in place after sorting.
    let last_knot_ast_end: usize = knots
        .last()
        .map_or(source.len(), |k| k.syntax().text_range().end().into());
    let trailing = &source[last_knot_ast_end..];

    // Build (name, source_slice) pairs. Each knot owns the text from its start
    // to just before the next knot (or the last knot's AST end).
    let mut knot_slices: Vec<(String, &str)> = Vec::with_capacity(knots.len());
    for (i, knot) in knots.iter().enumerate() {
        let name = knot.header().and_then(|h| h.name()).unwrap_or_default();
        let start: usize = knot.syntax().text_range().start().into();
        let end: usize = if i + 1 < knots.len() {
            knots[i + 1].syntax().text_range().start().into()
        } else {
            last_knot_ast_end
        };
        knot_slices.push((name, &source[start..end]));
    }

    // Preamble: everything before the first knot
    let preamble_end: usize = knots[0].syntax().text_range().start().into();
    let preamble = &source[..preamble_end];

    // Sort by name, case-insensitive
    knot_slices.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut result = String::with_capacity(source.len());
    result.push_str(preamble);
    for (_, slice) in &knot_slices {
        result.push_str(slice);
    }
    result.push_str(trailing);

    result
}

/// Sort stitch definitions within the named knot alphabetically.
///
/// Preserves the knot's preamble content (everything before the first stitch).
/// Each stitch's slice runs from its start to just before the next stitch (or end
/// of the knot body).
fn sort_stitches_in_knot(source: &str, knot_name: &str) -> String {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let Some(knot) = tree
        .knots()
        .find(|k| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    else {
        return source.to_owned();
    };

    let Some(body) = knot.body() else {
        return source.to_owned();
    };

    let stitches: Vec<_> = body.stitches().collect();
    if stitches.len() < 2 {
        return source.to_owned();
    }

    // The knot body region we'll rewrite: from first stitch start to the end of
    // the knot's AST node (which is just before the next knot or EOF — the knot
    // owns trailing content up to the next knot boundary).
    let knot_end: usize = knot.syntax().text_range().end().into();
    let region_start: usize = stitches[0].syntax().text_range().start().into();
    let region_end: usize = knot_end;

    // The last stitch's slice would extend to knot_end, which may include
    // trailing whitespace that belongs to the file structure, not the stitch.
    // Separate that trailing whitespace so it stays in place after sorting.
    let last_stitch_ast_end: usize = stitches
        .last()
        .map_or(region_end, |s| s.syntax().text_range().end().into());
    let trailing = &source[last_stitch_ast_end..region_end];

    let mut stitch_slices: Vec<(String, &str)> = Vec::with_capacity(stitches.len());
    for (i, stitch) in stitches.iter().enumerate() {
        let name = stitch.header().and_then(|h| h.name()).unwrap_or_default();
        let start: usize = stitch.syntax().text_range().start().into();
        let end: usize = if i + 1 < stitches.len() {
            stitches[i + 1].syntax().text_range().start().into()
        } else {
            last_stitch_ast_end
        };
        stitch_slices.push((name, &source[start..end]));
    }

    stitch_slices.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..region_start]);
    for (_, slice) in &stitch_slices {
        result.push_str(slice);
    }
    result.push_str(trailing);
    result.push_str(&source[region_end..]);

    result
}

/// Replace the entire document with the formatted text via a single `TextEdit`.
fn diff_to_edits(old: &str, new: &str) -> Vec<TextEdit> {
    let old_lines: Vec<&str> = old.lines().collect();

    // Compute end position of the old document.
    let end = if old_lines.is_empty() {
        Position::new(0, 0)
    } else {
        #[expect(clippy::cast_possible_truncation, reason = "line count fits in u32")]
        let last_line = (old_lines.len() - 1) as u32;
        #[expect(clippy::cast_possible_truncation, reason = "line length fits in u32")]
        let last_col = old_lines.last().map_or(0, |l| l.len() as u32);
        // If old ends with a newline, the cursor is at the start of the next line.
        if old.ends_with('\n') {
            Position::new(last_line + 1, 0)
        } else {
            Position::new(last_line, last_col)
        }
    };

    vec![TextEdit {
        range: Range {
            start: Position::new(0, 0),
            end,
        },
        new_text: new.to_owned(),
    }]
}

/// Check whether two LSP ranges overlap.
fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
}

// ─── Completion helpers ─────────────────────────────────────────────

/// What kind of completion context the cursor is in.
enum CompletionContext {
    /// After `->` — show divert targets.
    Divert,
    /// After `knot_name.` — show children of that knot.
    DottedPath { knot: String },
    /// Inside `{ }` — inline expression.
    InlineExpr,
    /// On a `~` logic line.
    Logic,
    /// Inside `( )` — function arguments.
    FunctionArgs,
    /// Default — show everything.
    General,
}

/// Determine the completion context by scanning backwards from the cursor.
fn detect_completion_context(source: &str, byte_offset: usize) -> CompletionContext {
    // Find line start.
    let line_start = source[..byte_offset].rfind('\n').map_or(0, |pos| pos + 1);
    let line_prefix = &source[line_start..byte_offset];
    let trimmed = line_prefix.trim_start();

    let is_logic_line = trimmed.starts_with('~');

    // Scan backwards through the line prefix for context clues.
    // More specific contexts (parens, braces, divert) take priority over the
    // logic-line fallback.
    let bytes = line_prefix.as_bytes();
    let mut brace_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut i = bytes.len();

    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => brace_depth += 1,
            b'{' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                } else {
                    return CompletionContext::InlineExpr;
                }
            }
            b')' => paren_depth += 1,
            b'(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                } else {
                    return CompletionContext::FunctionArgs;
                }
            }
            b'>' if i > 0 && bytes[i - 1] == b'-' && brace_depth == 0 && paren_depth == 0 => {
                return CompletionContext::Divert;
            }
            b'.' if brace_depth == 0 && paren_depth == 0 => {
                // Check for identifier before the dot.
                let before_dot = &line_prefix[..i];
                let ident_start = before_dot
                    .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                    .map_or(0, |p| p + 1);
                let knot = &before_dot[ident_start..];
                if !knot.is_empty() {
                    return CompletionContext::DottedPath {
                        knot: knot.to_owned(),
                    };
                }
            }
            _ => {}
        }
    }

    if is_logic_line {
        return CompletionContext::Logic;
    }

    CompletionContext::General
}

/// The scope (knot/stitch) containing the cursor.
struct CursorScope {
    knot: Option<String>,
    stitch: Option<String>,
}

/// Determine which knot/stitch the cursor is inside.
fn cursor_scope(source: &str, byte_offset: usize) -> CursorScope {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let cursor = rowan::TextSize::from(u32::try_from(byte_offset).unwrap_or(u32::MAX));

    let mut result = CursorScope {
        knot: None,
        stitch: None,
    };

    for knot in tree.knots() {
        let range = knot.syntax().text_range();
        if cursor < range.start() || cursor > range.end() {
            continue;
        }
        result.knot = knot.header().and_then(|h| h.name());

        if let Some(body) = knot.body() {
            for stitch in body.stitches() {
                let sr = stitch.syntax().text_range();
                if cursor >= sr.start() && cursor <= sr.end() {
                    result.stitch = stitch.header().and_then(|h| h.name());
                    break;
                }
            }
        }
        break;
    }

    result
}

/// Check whether a symbol should be shown in the given completion context.
fn is_visible_in_context(
    ctx: &CompletionContext,
    info: &brink_ir::SymbolInfo,
    scope: &CursorScope,
) -> bool {
    use brink_ir::SymbolKind;

    // Scope filter: locals are only visible if we're in their scope.
    if matches!(info.kind, SymbolKind::Param | SymbolKind::Temp)
        && let Some(ref sym_scope) = info.scope
    {
        let knot_matches = scope.knot.as_deref() == sym_scope.knot.as_deref();
        let stitch_visible =
            sym_scope.stitch.is_none() || scope.stitch.as_deref() == sym_scope.stitch.as_deref();
        if !knot_matches || !stitch_visible {
            return false;
        }
    }

    match ctx {
        CompletionContext::Divert => matches!(
            info.kind,
            SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
        ),
        CompletionContext::DottedPath { .. } => {
            // Handled separately in the caller.
            false
        }
        CompletionContext::InlineExpr => matches!(
            info.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Param
                | SymbolKind::Temp
                | SymbolKind::List
                | SymbolKind::ListItem
                | SymbolKind::Knot
                | SymbolKind::External
        ),
        CompletionContext::Logic => matches!(
            info.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Param
                | SymbolKind::Temp
                | SymbolKind::External
        ),
        CompletionContext::FunctionArgs => matches!(
            info.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Param
                | SymbolKind::Temp
                | SymbolKind::ListItem
        ),
        CompletionContext::General => true,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_divert() {
        let src = "-> ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Divert
        ));
    }

    #[test]
    fn context_divert_no_space() {
        let src = "->";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Divert
        ));
    }

    #[test]
    fn context_divert_partial() {
        let src = "-> kno";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Divert
        ));
    }

    #[test]
    fn context_dotted_path() {
        let src = "-> my_knot.";
        let ctx = detect_completion_context(src, src.len());
        assert!(matches!(ctx, CompletionContext::DottedPath { ref knot } if knot == "my_knot"));
    }

    #[test]
    fn context_inline_expr() {
        let src = "Hello {";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::InlineExpr
        ));
    }

    #[test]
    fn context_inline_expr_nested() {
        let src = "Hello {x + ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::InlineExpr
        ));
    }

    #[test]
    fn context_logic_line() {
        let src = "~ x = ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Logic
        ));
    }

    #[test]
    fn context_logic_line_indented() {
        let src = "    ~ temp x = ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Logic
        ));
    }

    #[test]
    fn context_function_args() {
        let src = "~ foo(";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FunctionArgs
        ));
    }

    #[test]
    fn context_function_args_partial() {
        let src = "~ foo(x, ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FunctionArgs
        ));
    }

    #[test]
    fn context_general() {
        let src = "Hello world ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::General
        ));
    }

    #[test]
    fn context_closed_braces_is_general() {
        // Braces are balanced — not inside an expression.
        let src = "{x} and then ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::General
        ));
    }

    #[test]
    fn cursor_scope_in_knot() {
        let src = "=== my_knot ===\nSome text\n";
        let offset = src.find("Some").unwrap_or(src.len());
        let scope = cursor_scope(src, offset);
        assert_eq!(scope.knot.as_deref(), Some("my_knot"));
        assert!(scope.stitch.is_none());
    }

    #[test]
    fn cursor_scope_in_stitch() {
        let src = "=== my_knot ===\n= my_stitch\nSome text\n";
        let offset = src.find("Some").unwrap_or(src.len());
        let scope = cursor_scope(src, offset);
        assert_eq!(scope.knot.as_deref(), Some("my_knot"));
        assert_eq!(scope.stitch.as_deref(), Some("my_stitch"));
    }

    #[test]
    fn cursor_scope_top_level() {
        let src = "Some text before any knot\n";
        let scope = cursor_scope(src, 5);
        assert!(scope.knot.is_none());
        assert!(scope.stitch.is_none());
    }
}
