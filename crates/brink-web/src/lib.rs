use std::cell::RefCell;

use brink_ide::session::IdeSession;
use brink_runtime::FastRng;
use rowan::{TextRange, TextSize};
use serde::Serialize;
use wasm_bindgen::prelude::*;

// ── Compilation ─────────────────────────────────────────────────────

/// Compile ink source and return JSON with diagnostics or story data.
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let result = brink_compiler::compile("main.ink", |_path| Ok(source.to_owned()));

    match result {
        Ok(output) => {
            let warnings: Vec<DiagnosticJs> = output
                .warnings
                .iter()
                .map(|d| DiagnosticJs {
                    message: d.message.clone(),
                    start: d.range.start().into(),
                    end: d.range.end().into(),
                    severity: format!("{:?}", d.code.severity()),
                })
                .collect();

            let data = output.data;
            let mut bytes = Vec::new();
            brink_format::write_inkb(&data, &mut bytes);

            let resp = CompileResult {
                ok: true,
                story_bytes: Some(bytes),
                warnings,
                error: None,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
        Err(e) => {
            let mut diagnostics = Vec::new();
            let mut error_msg = None;

            match e {
                brink_compiler::CompileError::Diagnostics(diags) => {
                    diagnostics = diags
                        .iter()
                        .map(|d| DiagnosticJs {
                            message: d.message.clone(),
                            start: d.range.start().into(),
                            end: d.range.end().into(),
                            severity: format!("{:?}", d.code.severity()),
                        })
                        .collect();
                }
                other => {
                    error_msg = Some(format!("{other}"));
                }
            }

            let resp = CompileResult {
                ok: false,
                story_bytes: None,
                warnings: diagnostics,
                error: error_msg,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
    }
}

#[derive(Serialize)]
struct CompileResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    story_bytes: Option<Vec<u8>>,
    warnings: Vec<DiagnosticJs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct DiagnosticJs {
    message: String,
    start: u32,
    end: u32,
    severity: String,
}

// ── Runtime ─────────────────────────────────────────────────────────

/// A running story instance. Owns Program + Story to avoid lifetime issues in wasm.
#[wasm_bindgen]
pub struct StoryRunner {
    // We store Program in a Box to get a stable address, and the Story
    // borrows from it. We use raw pointer + RefCell to work around the
    // self-referential borrow.
    program: Box<brink_runtime::Program>,
    // Safety: story borrows from program which is heap-pinned and never moved.
    // We only access story through &mut self methods (single-threaded wasm).
    story: RefCell<Option<brink_runtime::Story<'static, FastRng>>>,
}

#[wasm_bindgen]
impl StoryRunner {
    /// Create a new story runner from compiled story bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(story_bytes: &[u8]) -> Result<StoryRunner, JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let program = Box::new(
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?,
        );

        // Safety: we pin the Program in a Box and keep it alive for the
        // lifetime of StoryRunner. The Story borrows it, but we transmute
        // the lifetime to 'static. This is safe because:
        // 1. program is heap-allocated and never moved
        // 2. story is dropped before program (struct drop order)
        // 3. wasm is single-threaded
        let program_ptr: *const brink_runtime::Program = &raw const *program;
        // SAFETY: program is heap-allocated via Box and never moved. Story borrows
        // it for 'static, but StoryRunner keeps the Box alive and drops story first
        // (struct field drop order). wasm is single-threaded.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let story = brink_runtime::Story::<FastRng>::new(program_ref);

        Ok(StoryRunner {
            program,
            story: RefCell::new(Some(story)),
        })
    }

    /// Continue the story. Returns JSON with text, choices, and status.
    pub fn continue_story(&self) -> Result<String, JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let result = story
            .continue_maximally()
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;

        let resp = match result {
            brink_runtime::StepResult::Done { text, tags } => StepResultJs {
                status: "continue",
                text,
                choices: Vec::new(),
                tags,
            },
            brink_runtime::StepResult::Choices {
                text,
                choices,
                tags,
            } => StepResultJs {
                status: "choices",
                text,
                choices: choices
                    .into_iter()
                    .map(|c| ChoiceJs {
                        text: c.text,
                        index: c.index,
                        tags: c.tags,
                    })
                    .collect(),
                tags,
            },
            brink_runtime::StepResult::Ended { text, tags } => StepResultJs {
                status: "ended",
                text,
                choices: Vec::new(),
                tags,
            },
        };

        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Choose an option by index.
    pub fn choose(&self, index: usize) -> Result<(), JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        story
            .choose(index)
            .map_err(|e| JsError::new(&format!("choose error: {e}")))
    }

    /// Reset: create a fresh story from the same program.
    pub fn reset(&self) {
        let program_ptr: *const brink_runtime::Program = &raw const *self.program;
        // SAFETY: same invariants as in `new` — Box is pinned and outlives the Story.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let story = brink_runtime::Story::<FastRng>::new(program_ref);
        *self.story.borrow_mut() = Some(story);
    }
}

#[derive(Serialize)]
struct StepResultJs {
    status: &'static str,
    text: String,
    choices: Vec<ChoiceJs>,
    tags: Vec<Vec<String>>,
}

#[derive(Serialize)]
struct ChoiceJs {
    text: String,
    index: usize,
    tags: Vec<String>,
}

// ── EditorSession ───────────────────────────────────────────────────

/// Stateful IDE session for the web editor. Wraps `IdeSession` and exposes
/// all IDE queries as methods that return JSON strings.
#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
    /// The active file path for IDE queries.
    active_path: String,
}

impl Default for EditorSession {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl EditorSession {
    /// Create a new empty editor session.
    #[wasm_bindgen(constructor)]
    pub fn new() -> EditorSession {
        EditorSession {
            session: IdeSession::new(),
            active_path: "main.ink".to_owned(),
        }
    }

    /// Update the active file's source text. Reparses, lowers, and analyzes.
    pub fn update_source(&mut self, source: &str) {
        self.session
            .update_and_analyze(&self.active_path, source.to_owned());
    }

    /// Add or update a file by path. Re-analyzes the project.
    pub fn update_file(&mut self, path: &str, source: &str) {
        self.session.update_and_analyze(path, source.to_owned());
    }

    /// Remove a file from the project.
    pub fn remove_file(&mut self, path: &str) {
        self.session.remove_file(path);
    }

    /// Switch the active file for IDE queries. Returns false if the file is not loaded.
    pub fn set_active_file(&mut self, path: &str) -> bool {
        if self.session.file_id(path).is_some() {
            path.clone_into(&mut self.active_path);
            true
        } else {
            false
        }
    }

    /// Get the current active file path.
    pub fn active_file(&self) -> String {
        self.active_path.clone()
    }

    /// List all loaded files. Returns JSON `[{path}]`.
    pub fn list_files(&self) -> String {
        let db = self.session.db();
        let files: Vec<ProjectFileJs> = db
            .file_ids()
            .filter_map(|id| {
                db.file_path(id)
                    .map(|p| ProjectFileJs { path: p.to_owned() })
            })
            .collect();
        serde_json::to_string(&files).unwrap_or_default()
    }

    /// Get the source text for a file. Returns JSON string or `"null"`.
    pub fn get_file_source(&self, path: &str) -> String {
        let source = self
            .session
            .file_id(path)
            .and_then(|id| self.session.source(id));
        match source {
            Some(s) => serde_json::to_string(s).unwrap_or_default(),
            None => "null".to_owned(),
        }
    }

    /// Get document symbols for a specific file. Returns JSON `DocumentSymbol[]`.
    pub fn file_symbols(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let syms = brink_ide::document::document_symbols(hir, manifest);
        let items: Vec<DocumentSymbolJs> = syms.into_iter().map(convert_document_symbol).collect();
        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compile the project using all loaded files. Returns JSON `CompileResult`.
    pub fn compile_project(&self, entry: &str) -> String {
        let session = &self.session;
        let result = brink_compiler::compile(entry, |path| {
            session
                .file_id(path)
                .and_then(|id| session.source(id))
                .map(str::to_owned)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("file not found: {path}"),
                    )
                })
        });

        match result {
            Ok(output) => {
                let warnings: Vec<DiagnosticJs> = output
                    .warnings
                    .iter()
                    .map(|d| DiagnosticJs {
                        message: d.message.clone(),
                        start: d.range.start().into(),
                        end: d.range.end().into(),
                        severity: format!("{:?}", d.code.severity()),
                    })
                    .collect();

                let data = output.data;
                let mut bytes = Vec::new();
                brink_format::write_inkb(&data, &mut bytes);

                let resp = CompileResult {
                    ok: true,
                    story_bytes: Some(bytes),
                    warnings,
                    error: None,
                };
                serde_json::to_string(&resp).unwrap_or_default()
            }
            Err(e) => {
                let mut diagnostics = Vec::new();
                let mut error_msg = None;

                match e {
                    brink_compiler::CompileError::Diagnostics(diags) => {
                        diagnostics = diags
                            .iter()
                            .map(|d| DiagnosticJs {
                                message: d.message.clone(),
                                start: d.range.start().into(),
                                end: d.range.end().into(),
                                severity: format!("{:?}", d.code.severity()),
                            })
                            .collect();
                    }
                    other => {
                        error_msg = Some(format!("{other}"));
                    }
                }

                let resp = CompileResult {
                    ok: false,
                    story_bytes: None,
                    warnings: diagnostics,
                    error: error_msg,
                };
                serde_json::to_string(&resp).unwrap_or_default()
            }
        }
    }

    /// Get project outline — all files with their symbols. Returns JSON `[{path, symbols}]`.
    pub fn project_outline(&self) -> String {
        let db = self.session.db();
        let mut outline: Vec<FileOutlineJs> = Vec::new();

        for id in db.file_ids() {
            let Some(path) = db.file_path(id) else {
                continue;
            };
            let (Some(hir), Some(manifest)) = (db.hir(id), db.manifest(id)) else {
                outline.push(FileOutlineJs {
                    path: path.to_owned(),
                    symbols: Vec::new(),
                });
                continue;
            };

            let syms = brink_ide::document::document_symbols(hir, manifest);
            let items: Vec<DocumentSymbolJs> =
                syms.into_iter().map(convert_document_symbol).collect();
            outline.push(FileOutlineJs {
                path: path.to_owned(),
                symbols: items,
            });
        }

        // Sort by path for deterministic output
        outline.sort_by(|a, b| a.path.cmp(&b.path));
        serde_json::to_string(&outline).unwrap_or_default()
    }

    /// Compute per-line context from the HIR. Returns JSON array of `LineContext`.
    pub fn line_contexts(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source), Some(root)) = (
            self.session.hir(file_id),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let contexts = brink_ide::line_context::line_contexts(hir, source, &root);
        serde_json::to_string(&contexts).unwrap_or_default()
    }

    /// Compute semantic tokens. Returns JSON array of tokens.
    pub fn semantic_tokens(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source), Some(root)) = (
            self.session.analysis(),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let raw = brink_ide::semantic_tokens::semantic_tokens(source, &root, analysis, file_id);

        let tokens: Vec<TokenJs> = raw
            .iter()
            .map(|t| TokenJs {
                line: t.line,
                start_char: t.start_char,
                length: t.length,
                token_type: t.token_type,
                modifiers: t.modifiers,
            })
            .collect();

        serde_json::to_string(&tokens).unwrap_or_default()
    }

    /// Compute completions at the given byte offset. Returns JSON array.
    pub fn completions(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let ctx = brink_ide::detect_completion_context(source, offset as usize);
        let scope = brink_ide::cursor_scope(source, offset as usize);

        let items: Vec<CompletionItemJs> = analysis
            .index
            .symbols
            .values()
            .filter(|info| brink_ide::is_visible_in_context(&ctx, info, &scope))
            .map(|info| CompletionItemJs {
                name: info.name.clone(),
                kind: symbol_kind_str(info.kind).to_owned(),
                detail: info.detail.clone(),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute hover info at the given byte offset. Returns JSON or "null".
    pub fn hover(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let project_files = [(file_id, self.active_path.clone(), source.to_owned())];

        match brink_ide::hover::hover(
            analysis,
            file_id,
            source,
            TextSize::new(offset),
            &project_files,
        ) {
            Some(info) => {
                let js = HoverInfoJs {
                    content: info.content,
                    start: info.range.map(|r| r.start().into()),
                    end: info.range.map(|r| r.end().into()),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Compute goto-definition at the given byte offset. Returns JSON or "null".
    pub fn goto_definition(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        match brink_ide::navigation::goto_definition(analysis, file_id, TextSize::new(offset)) {
            Some(loc) => {
                let js = LocationJs {
                    start: loc.range.start().into(),
                    end: loc.range.end().into(),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Find all references. Returns JSON array.
    pub fn find_references(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };

        let refs =
            brink_ide::navigation::find_references(analysis, file_id, TextSize::new(offset), true);

        let items: Vec<LocationJs> = refs
            .iter()
            .map(|loc| LocationJs {
                start: loc.range.start().into(),
                end: loc.range.end().into(),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Check if rename is possible. Returns JSON or "null".
    pub fn prepare_rename(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        match brink_ide::rename::prepare_rename(analysis, file_id, TextSize::new(offset)) {
            Some(range) => {
                let js = LocationJs {
                    start: range.start().into(),
                    end: range.end().into(),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Compute rename edits. Returns JSON array or "null".
    pub fn rename(&self, offset: u32, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        match brink_ide::rename::rename(analysis, file_id, TextSize::new(offset), new_name) {
            Some(result) => {
                let edits: Vec<FileEditJs> = result
                    .edits
                    .iter()
                    .map(|e| FileEditJs {
                        start: e.range.start().into(),
                        end: e.range.end().into(),
                        new_text: e.new_text.clone(),
                    })
                    .collect();
                serde_json::to_string(&edits).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Compute code actions. Returns JSON array.
    pub fn code_actions(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "[]".to_owned();
        };

        let actions = brink_ide::code_actions::code_actions(source, offset as usize);

        let items: Vec<CodeActionJs> = actions
            .iter()
            .map(|a| CodeActionJs {
                title: a.title.clone(),
                kind: code_action_kind_str(&a.kind).to_owned(),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute inlay hints. Returns JSON array.
    pub fn inlay_hints(&self, start: u32, end: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let range = TextRange::new(TextSize::new(start), TextSize::new(end));
        let hints = brink_ide::inlay_hints::inlay_hints(&root, analysis, range);

        let items: Vec<InlayHintJs> = hints
            .iter()
            .map(|h| InlayHintJs {
                offset: h.offset.into(),
                label: h.label.clone(),
                kind: inlay_hint_kind_str(&h.kind).to_owned(),
                padding_right: h.padding_right,
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute signature help. Returns JSON or "null".
    pub fn signature_help(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        match brink_ide::signature::signature_help(analysis, source, offset as usize) {
            Some(info) => {
                let js = SignatureInfoJs {
                    label: info.label,
                    documentation: info.documentation,
                    parameters: info
                        .parameters
                        .iter()
                        .map(|p| ParamLabelJs {
                            label: p.label.clone(),
                        })
                        .collect(),
                    active_parameter: info.active_parameter,
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Compute folding ranges. Returns JSON array.
    pub fn folding_ranges(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source)) = (self.session.hir(file_id), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let ranges = brink_ide::folding::folding_ranges(hir, source);

        let items: Vec<FoldRangeJs> = ranges
            .iter()
            .map(|r| FoldRangeJs {
                start_line: r.start_line,
                end_line: r.end_line,
                collapsed_text: r.collapsed_text.clone(),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute document symbols (outline). Returns JSON array.
    pub fn document_symbols(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let syms = brink_ide::document::document_symbols(hir, manifest);
        let items: Vec<DocumentSymbolJs> = syms.into_iter().map(convert_document_symbol).collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Convert a line element to a different type. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element(&self, offset: u32, target: &str) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let (Some(hir), Some(source), Some(root)) = (
            self.session.hir(file_id),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "null".to_owned();
        };

        let convert_target = match target {
            "narrative" => brink_ide::line_convert::ConvertTarget::Narrative,
            "choice" => brink_ide::line_convert::ConvertTarget::Choice { sticky: false },
            "sticky_choice" => brink_ide::line_convert::ConvertTarget::Choice { sticky: true },
            "gather" => brink_ide::line_convert::ConvertTarget::Gather,
            "choice_body" => brink_ide::line_convert::ConvertTarget::ChoiceBody,
            _ => return "null".to_owned(),
        };

        match brink_ide::line_convert::convert_element(source, hir, &root, offset, convert_target) {
            Some(edit) => serde_json::to_string(&edit).unwrap_or_default(),
            None => "null".to_owned(),
        }
    }

    /// Format the document (sort knots). Returns the formatted source as a JSON string.
    pub fn format_document(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "\"\"".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "\"\"".to_owned();
        };

        let formatted = brink_ide::sort_knots_in_source(source);
        serde_json::to_string(&formatted).unwrap_or_default()
    }
}

// ── Serialization types ─────────────────────────────────────────────

#[derive(Serialize)]
struct ProjectFileJs {
    path: String,
}

#[derive(Serialize)]
struct FileOutlineJs {
    path: String,
    symbols: Vec<DocumentSymbolJs>,
}

#[derive(Serialize)]
struct CompletionItemJs {
    name: String,
    kind: String,
    detail: Option<String>,
}

#[derive(Serialize)]
struct HoverInfoJs {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<u32>,
}

#[derive(Serialize)]
struct LocationJs {
    start: u32,
    end: u32,
}

#[derive(Serialize)]
struct FileEditJs {
    start: u32,
    end: u32,
    new_text: String,
}

#[derive(Serialize)]
struct InlayHintJs {
    offset: u32,
    label: String,
    kind: String,
    padding_right: bool,
}

#[derive(Serialize)]
struct SignatureInfoJs {
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<String>,
    parameters: Vec<ParamLabelJs>,
    active_parameter: u32,
}

#[derive(Serialize)]
struct ParamLabelJs {
    label: String,
}

#[derive(Serialize)]
struct FoldRangeJs {
    start_line: u32,
    end_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    collapsed_text: Option<String>,
}

#[derive(Serialize)]
struct DocumentSymbolJs {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    start: u32,
    end: u32,
    children: Vec<DocumentSymbolJs>,
}

#[derive(Serialize)]
struct CodeActionJs {
    title: String,
    kind: String,
}

#[derive(Serialize)]
struct TokenJs {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

// ── Helper functions ────────────────────────────────────────────────

fn symbol_kind_str(kind: brink_ir::SymbolKind) -> &'static str {
    match kind {
        brink_ir::SymbolKind::Knot => "knot",
        brink_ir::SymbolKind::Stitch => "stitch",
        brink_ir::SymbolKind::Variable => "variable",
        brink_ir::SymbolKind::Constant => "constant",
        brink_ir::SymbolKind::List => "list",
        brink_ir::SymbolKind::ListItem => "list_item",
        brink_ir::SymbolKind::External => "external",
        brink_ir::SymbolKind::Label => "label",
        brink_ir::SymbolKind::Param => "param",
        brink_ir::SymbolKind::Temp => "temp",
    }
}

fn code_action_kind_str(kind: &brink_ide::code_actions::CodeActionKind) -> &'static str {
    match kind {
        brink_ide::code_actions::CodeActionKind::QuickFix => "quickfix",
        brink_ide::code_actions::CodeActionKind::Refactor => "refactor",
        brink_ide::code_actions::CodeActionKind::Source => "source",
    }
}

fn inlay_hint_kind_str(kind: &brink_ide::inlay_hints::InlayHintKind) -> &'static str {
    match kind {
        brink_ide::inlay_hints::InlayHintKind::Parameter => "parameter",
    }
}

fn convert_document_symbol(sym: brink_ide::document::DocumentSymbol) -> DocumentSymbolJs {
    DocumentSymbolJs {
        name: sym.name,
        kind: symbol_kind_str(sym.kind).to_owned(),
        detail: sym.detail,
        start: sym.range.start().into(),
        end: sym.range.end().into(),
        children: sym
            .children
            .into_iter()
            .map(convert_document_symbol)
            .collect(),
    }
}

// ── Legacy stateless functions (token legend) ───────────────────────

/// Get token type names for the legend.
#[wasm_bindgen]
pub fn token_type_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_type_names()).unwrap_or_default()
}

/// Get token modifier names for the legend.
#[wasm_bindgen]
pub fn token_modifier_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_modifier_names()).unwrap_or_default()
}
