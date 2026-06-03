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
                .map(|d| diagnostic_to_js(d, source))
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
                    diagnostics = diags.iter().map(|d| diagnostic_to_js(d, source)).collect();
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
    base_line_tables: Vec<Vec<brink_format::LineEntry>>,
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
        let (prog, line_tables) =
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?;
        let program = Box::new(prog);

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

        let story = brink_runtime::Story::<FastRng>::new(program_ref, line_tables.clone());

        Ok(StoryRunner {
            program,
            base_line_tables: line_tables,
            story: RefCell::new(Some(story)),
        })
    }

    /// Continue the story maximally. Returns JSON array of `Line` objects.
    pub fn continue_story(&self) -> Result<String, JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let lines = story
            .continue_maximally()
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;

        let resp: Vec<LineJs> = lines.into_iter().map(line_to_js).collect();

        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Continue the story by a single line. Returns JSON for one `Line` object.
    pub fn continue_single(&self) -> Result<String, JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let line = story
            .continue_single()
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;

        let resp = line_to_js(line);

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

        let story =
            brink_runtime::Story::<FastRng>::new(program_ref, self.base_line_tables.clone());
        *self.story.borrow_mut() = Some(story);
    }
}

fn line_to_js(line: brink_runtime::Line) -> LineJs {
    match line {
        brink_runtime::Line::Text { text, tags } => LineJs {
            r#type: "text",
            text,
            tags,
            choices: None,
        },
        brink_runtime::Line::Choices {
            text,
            tags,
            choices,
        } => LineJs {
            r#type: "choices",
            text,
            tags,
            choices: Some(
                choices
                    .into_iter()
                    .map(|c| ChoiceJs {
                        text: c.text,
                        index: c.index,
                        tags: c.tags,
                    })
                    .collect(),
            ),
        },
        brink_runtime::Line::Done { text, tags } => LineJs {
            r#type: "done",
            text,
            tags,
            choices: None,
        },
        brink_runtime::Line::End { text, tags } => LineJs {
            r#type: "end",
            text,
            tags,
            choices: None,
        },
    }
}

#[derive(Serialize)]
struct LineJs {
    r#type: &'static str,
    text: String,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<ChoiceJs>>,
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
/// A view context scopes the editor to a sub-region of a file.
/// When active, `update_source` splices the fragment into the full file
/// at `[start, end)`, and IDE responses adjust offsets relative to the view.
struct ViewContext {
    /// Byte offset where the view begins in the full file.
    start: u32,
    /// Byte offset where the view ends (exclusive) in the full file.
    end: u32,
    /// 0-based line number of the view start (for line-based IDE responses).
    start_line: u32,
    /// Whether `full[original_end..]` started with `\n` when the context was set.
    /// When true, `update_source` ensures a `\n` separator is maintained after
    /// the fragment, so edits at the end don't merge with the next section.
    trailing_newline: bool,
}

#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
    /// The active file path for IDE queries.
    active_path: String,
    /// Optional sub-file view context for focused editing.
    view: Option<ViewContext>,
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
            view: None,
        }
    }

    /// Update the active file's source text. Reparses, lowers, and analyzes.
    ///
    /// When a view context is active, `source` is treated as a fragment that
    /// gets spliced into the full file at `[view.start, view.end)`.
    pub fn update_source(&mut self, source: &str) {
        if let Some(ref mut view) = self.view {
            let full = self
                .session
                .file_id(&self.active_path)
                .and_then(|id| self.session.source(id).map(str::to_owned))
                .unwrap_or_default();
            let start = view.start as usize;
            let end = (view.end as usize).min(full.len());

            let after = &full[end..];
            // If the original boundary had a newline separator and the fragment
            // doesn't end with one, insert a newline to prevent merging.
            let needs_sep = view.trailing_newline
                && !source.ends_with('\n')
                && !after.starts_with('\n')
                && !after.is_empty();
            let sep = if needs_sep { "\n" } else { "" };
            let mut spliced = String::with_capacity(start + source.len() + sep.len() + after.len());
            spliced.push_str(&full[..start]);
            spliced.push_str(source);
            spliced.push_str(sep);
            spliced.push_str(after);
            // view.end tracks only the fragment, NOT the separator.
            // The separator lives at full[view.end] and is preserved across splices.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            {
                view.end = view.start + source.len() as u32;
            }
            self.session.update_and_analyze(&self.active_path, spliced);
        } else {
            self.session
                .update_and_analyze(&self.active_path, source.to_owned());
        }
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
    /// Clears any active view context (view is file-specific).
    pub fn set_active_file(&mut self, path: &str) -> bool {
        if self.session.file_id(path).is_some() {
            path.clone_into(&mut self.active_path);
            self.view = None;
            true
        } else {
            false
        }
    }

    /// Set a view context scoping the editor to `[start, end)` of the active file.
    /// IDE queries will adjust offsets relative to this range.
    pub fn set_view_context(&mut self, start: u32, end: u32) {
        // Boundary offsets are UTF-16 code units; convert to bytes for the
        // internal byte-indexed logic below (and stored ViewContext range).
        let (start, end) = match self.active_source() {
            Some(s) => (utf16_to_byte(s, start), utf16_to_byte(s, end)),
            None => (start, end),
        };
        // Check if there's a newline right at `end` (the separator between this
        // section and the next). If so, we'll ensure it's preserved after splices.
        // Trim trailing blank lines from the view range and check if there's a
        // newline separator at the boundary that should be preserved across splices.
        let (end, trailing_newline) = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))
            .map_or((end, false), |s| {
                let e = (end as usize).min(s.len());
                let start_usize = (start as usize).min(e);
                let view = &s[start_usize..e];
                // Trim trailing newlines (keep at most one)
                let trimmed = view.trim_end_matches('\n');
                let keep = if trimmed.len() < view.len() {
                    trimmed.len() + 1
                } else {
                    view.len()
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let trimmed_end = (start_usize + keep).min(e) as u32;
                // Check if there's a newline right after the trimmed end
                let has_nl = s.as_bytes().get(trimmed_end as usize) == Some(&b'\n')
                    || (trimmed_end > 0
                        && s.as_bytes().get((trimmed_end as usize).wrapping_sub(1))
                            == Some(&b'\n'));
                (trimmed_end, has_nl)
            });

        let start_line = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))
            .map_or(0, |s| {
                let byte_start = (start as usize).min(s.len());
                count_newlines(&s[..byte_start])
            });
        self.view = Some(ViewContext {
            start,
            end,
            start_line,
            trailing_newline,
        });
    }

    /// Clear the view context, returning to full-file mode.
    pub fn clear_view_context(&mut self) {
        self.view = None;
    }

    /// Get the source text for the current view. Returns the fragment if a view
    /// context is active, or the full file otherwise. Returns a JSON string.
    pub fn get_view_source(&self) -> String {
        let source = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id));
        match (source, &self.view) {
            (Some(s), Some(v)) => {
                let start = (v.start as usize).min(s.len());
                let end = (v.end as usize).min(s.len());
                serde_json::to_string(&s[start..end]).unwrap_or_default()
            }
            (Some(s), None) => serde_json::to_string(s).unwrap_or_default(),
            _ => "null".to_owned(),
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
        let source = self.session.source(file_id).unwrap_or("");
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();
        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compile the project using all loaded files. Returns JSON `CompileResult`.
    pub fn compile_project(&self, entry: &str) -> String {
        let session = &self.session;
        // Diagnostic ranges are byte offsets into the compiled entry file; we
        // convert them to UTF-16 for the editor against that file's source.
        let diag_source = session
            .file_id(entry)
            .and_then(|id| session.source(id))
            .unwrap_or("");
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
                    .map(|d| diagnostic_to_js(d, diag_source))
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
                            .map(|d| diagnostic_to_js(d, diag_source))
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
            let source = db.source(id).unwrap_or("");
            let items: Vec<DocumentSymbolJs> = syms
                .into_iter()
                .map(|s| convert_document_symbol(s, source))
                .collect();
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
        if let Some(v) = &self.view {
            let start = v.start_line as usize;
            let end_line = self.view_end_line().map_or(contexts.len(), |l| l as usize);
            let slice = &contexts[start..end_line.min(contexts.len())];
            serde_json::to_string(slice).unwrap_or_default()
        } else {
            serde_json::to_string(&contexts).unwrap_or_default()
        }
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
            .filter_map(|t| {
                let line = self.to_relative_line(t.line)?;
                Some(TokenJs {
                    line,
                    start_char: t.start_char,
                    length: t.length,
                    token_type: t.token_type,
                    modifiers: t.modifiers,
                })
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

        let abs_offset = self.to_absolute(offset);
        let ctx = brink_ide::detect_completion_context(source, abs_offset as usize);
        let scope = brink_ide::cursor_scope(source, abs_offset as usize);

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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::hover::hover(
            analysis,
            file_id,
            source,
            TextSize::new(abs_offset),
            &project_files,
        ) {
            Some(info) => {
                let js = HoverInfoJs {
                    content: info.content,
                    start: info.range.and_then(|r| self.to_relative(r.start().into())),
                    end: info.range.and_then(|r| self.to_relative(r.end().into())),
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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::navigation::goto_definition(analysis, file_id, TextSize::new(abs_offset)) {
            Some(loc) => {
                let db = self.session.db();
                let file_path = db.file_path(loc.file).unwrap_or_default().to_owned();
                let (start, end) = if loc.file == file_id {
                    // Same file: adjust to view-relative UTF-16 offsets
                    (
                        self.to_relative(loc.range.start().into())
                            .unwrap_or(loc.range.start().into()),
                        self.to_relative(loc.range.end().into())
                            .unwrap_or(loc.range.end().into()),
                    )
                } else {
                    // Cross-file: convert bytes to UTF-16 in the target file
                    let src = self.session.source(loc.file).unwrap_or("");
                    (
                        byte_to_utf16(src, loc.range.start().into()),
                        byte_to_utf16(src, loc.range.end().into()),
                    )
                };
                let js = LocationJs {
                    file: file_path,
                    start,
                    end,
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

        let abs_offset = self.to_absolute(offset);
        let refs = brink_ide::navigation::find_references(
            analysis,
            file_id,
            TextSize::new(abs_offset),
            true,
        );

        let db = self.session.db();
        let items: Vec<LocationJs> = refs
            .iter()
            .filter_map(|loc| {
                if loc.file == file_id {
                    // Same file: adjust offsets, filter out-of-view
                    let start = self.to_relative(loc.range.start().into())?;
                    let end = self.to_relative(loc.range.end().into())?;
                    Some(LocationJs {
                        file: db.file_path(loc.file).unwrap_or_default().to_owned(),
                        start,
                        end,
                    })
                } else {
                    // Cross-file: convert bytes to UTF-16 in the target file
                    let src = self.session.source(loc.file).unwrap_or("");
                    Some(LocationJs {
                        file: db.file_path(loc.file).unwrap_or_default().to_owned(),
                        start: byte_to_utf16(src, loc.range.start().into()),
                        end: byte_to_utf16(src, loc.range.end().into()),
                    })
                }
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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::rename::prepare_rename(analysis, file_id, TextSize::new(abs_offset)) {
            Some(range) => {
                let start = self.to_relative(range.start().into());
                let end = self.to_relative(range.end().into());
                match (start, end) {
                    (Some(s), Some(e)) => {
                        let js = LocationJs {
                            file: self.active_path.clone(),
                            start: s,
                            end: e,
                        };
                        serde_json::to_string(&js).unwrap_or_default()
                    }
                    _ => "null".to_owned(),
                }
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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::rename::rename(analysis, file_id, TextSize::new(abs_offset), new_name) {
            Some(result) => {
                let edits: Vec<FileEditJs> = result
                    .edits
                    .iter()
                    .filter_map(|e| {
                        let start = self.to_relative(e.range.start().into())?;
                        let end = self.to_relative(e.range.end().into())?;
                        Some(FileEditJs {
                            start,
                            end,
                            new_text: e.new_text.clone(),
                        })
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

        let abs_offset = self.to_absolute(offset);
        let actions = brink_ide::code_actions::code_actions(source, abs_offset as usize);

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

        let abs_start = self.to_absolute(start);
        let abs_end = self.to_absolute(end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::inlay_hints::inlay_hints(&root, analysis, range);

        let items: Vec<InlayHintJs> = hints
            .iter()
            .filter_map(|h| {
                let offset = self.to_relative(h.offset.into())?;
                Some(InlayHintJs {
                    offset,
                    label: h.label.clone(),
                    kind: inlay_hint_kind_str(&h.kind).to_owned(),
                    padding_right: h.padding_right,
                })
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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::signature::signature_help(analysis, source, abs_offset as usize) {
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
            .filter_map(|r| {
                let start_line = self.to_relative_line(r.start_line)?;
                let end_line = self.to_relative_line(r.end_line)?;
                Some(FoldRangeJs {
                    start_line,
                    end_line,
                    collapsed_text: r.collapsed_text.clone(),
                })
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
        let source = self.session.source(file_id).unwrap_or("");
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();

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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::line_convert::convert_element(
            source,
            hir,
            &root,
            abs_offset,
            convert_target,
        ) {
            Some(edit) => match (self.to_relative(edit.from), self.to_relative(edit.to)) {
                (Some(from), Some(to)) => {
                    let adjusted = brink_ide::line_convert::TextEdit {
                        from,
                        to,
                        insert: edit.insert,
                    };
                    serde_json::to_string(&adjusted).unwrap_or_default()
                }
                _ => "null".to_owned(),
            },
            None => "null".to_owned(),
        }
    }

    /// Get resolved INCLUDE paths for a file. Returns JSON `[{path, resolved, loaded}]`.
    pub fn file_includes(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(hir) = self.session.hir(file_id) else {
            return "[]".to_owned();
        };

        let db = self.session.db();
        let items: Vec<IncludeInfoJs> = hir
            .includes
            .iter()
            .map(|inc| {
                let resolved = brink_db::resolve_include_path(path, &inc.file_path);
                let loaded = db.file_id(&resolved).is_some();
                IncludeInfoJs {
                    path: inc.file_path.clone(),
                    resolved,
                    loaded,
                }
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
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

    /// Reorder a stitch within its parent knot. Returns JSON `MoveResult` or error string.
    ///
    /// `path`: file containing the knot.
    /// `direction`: 1 = down, -1 = up.
    pub fn reorder_stitch(&self, path: &str, knot: &str, stitch: &str, direction: i32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let dir = if direction >= 0 {
            brink_ide::structural_move::Direction::Down
        } else {
            brink_ide::structural_move::Direction::Up
        };

        match brink_ide::structural_move::reorder_stitch(source, knot, stitch, dir) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Move a stitch from one knot to another. Returns JSON `MoveResult` or error.
    ///
    /// `path`: file containing both knots.
    pub fn move_stitch(&self, path: &str, src_knot: &str, stitch: &str, dest_knot: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::move_stitch(
            source, analysis, file_id, src_knot, stitch, dest_knot,
        ) {
            Ok(result) => move_result_json(result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Promote a stitch to a top-level knot. Returns JSON `MoveResult` or error.
    ///
    /// `path`: file containing the knot.
    pub fn promote_stitch(&self, path: &str, knot: &str, stitch: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::promote_stitch_to_knot(
            source, analysis, file_id, knot, stitch,
        ) {
            Ok(result) => move_result_json(result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder a knot within the top-level knot list. Returns JSON `MoveResult` or error.
    ///
    /// `path`: file containing the knot.
    /// `direction`: 1 = down, -1 = up.
    pub fn reorder_knot(&self, path: &str, knot: &str, direction: i32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let dir = if direction >= 0 {
            brink_ide::structural_move::Direction::Down
        } else {
            brink_ide::structural_move::Direction::Up
        };

        match brink_ide::structural_move::reorder_knot(source, knot, dir) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Demote a top-level knot to a stitch inside another knot. Returns JSON `MoveResult` or error.
    ///
    /// `path`: file containing both knots.
    pub fn demote_knot(&self, path: &str, knot: &str, dest_knot: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::demote_knot_to_stitch(
            source, analysis, file_id, knot, dest_knot,
        ) {
            Ok(result) => move_result_json(result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }
}

// ── View context helpers (private, not wasm-exported) ───────────────

impl EditorSession {
    /// Source text of the active file, if loaded.
    fn active_source(&self) -> Option<&str> {
        self.session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))
    }

    /// Convert a UTF-16 view-relative offset (the boundary convention) to a
    /// file-absolute **byte** offset for `brink-ide`/rowan.
    ///
    /// When a view context is active the offset is relative to the displayed
    /// fragment (`source[view.start..view.end]`); otherwise it's relative to
    /// the whole file.
    fn to_absolute(&self, offset: u32) -> u32 {
        let Some(source) = self.active_source() else {
            return offset;
        };
        match self.view.as_ref() {
            Some(v) => {
                let start = floor_char_boundary(source, v.start as usize);
                let end = floor_char_boundary(source, (v.end as usize).max(start));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let abs = start as u32 + utf16_to_byte(&source[start..end], offset);
                abs
            }
            None => utf16_to_byte(source, offset),
        }
    }

    /// Convert a file-absolute **byte** offset (from `brink-ide`) to a
    /// UTF-16 view-relative offset for the editor.
    /// Returns `None` if the offset is outside the view range.
    fn to_relative(&self, offset: u32) -> Option<u32> {
        let source = self.active_source()?;
        match self.view.as_ref() {
            Some(v) => {
                if offset < v.start || offset > v.end {
                    return None;
                }
                let start = floor_char_boundary(source, v.start as usize);
                let end = floor_char_boundary(source, (v.end as usize).max(start));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let byte_in_fragment = (offset as usize).saturating_sub(start) as u32;
                Some(byte_to_utf16(&source[start..end], byte_in_fragment))
            }
            None => Some(byte_to_utf16(source, offset)),
        }
    }

    /// Convert a file-absolute line number (0-based) to a view-relative line.
    /// Returns `None` if the line is before the view start.
    fn to_relative_line(&self, line: u32) -> Option<u32> {
        self.view.as_ref().map_or(Some(line), |v| {
            (line >= v.start_line).then(|| line - v.start_line)
        })
    }

    /// Compute the end line of the view in the current source.
    fn view_end_line(&self) -> Option<u32> {
        let v = self.view.as_ref()?;
        let source = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))?;
        let byte_end = (v.end as usize).min(source.len());
        Some(count_newlines(&source[..byte_end]))
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn count_newlines(s: &str) -> u32 {
    s.matches('\n').count() as u32
}

// ── UTF-16 ↔ byte offset conversion ─────────────────────────────────
//
// The wasm `EditorSession` boundary speaks UTF-16 code-unit offsets, to
// match CodeMirror / JS string indexing on the TypeScript side. Internally
// (rowan, `TextSize`, `&str`) everything is byte offsets. These helpers
// translate at the boundary. Both clamp to the end of `s` when the input
// falls past the string, and round a position that lands inside a multi-unit
// boundary up to the next char start (CodeMirror never produces such inputs,
// but the clamp keeps us panic-free).

/// Convert a byte offset within `s` to a UTF-16 code-unit offset.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn byte_to_utf16(s: &str, byte: u32) -> u32 {
    let byte = byte as usize;
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if i >= byte {
            return units;
        }
        units += c.len_utf16() as u32;
    }
    units
}

/// Convert a UTF-16 code-unit offset within `s` to a byte offset.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn utf16_to_byte(s: &str, utf16: u32) -> u32 {
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if units >= utf16 {
            return i as u32;
        }
        units += c.len_utf16() as u32;
    }
    s.len() as u32
}

/// Largest byte index `<= i` that is a char boundary in `s` (clamped to `len`).
/// Keeps fragment slicing panic-free if a stored byte offset ever lands inside
/// a multibyte char.
fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

// ── Serialization types ─────────────────────────────────────────────

#[derive(Serialize)]
struct ProjectFileJs {
    path: String,
}

#[derive(Serialize)]
struct IncludeInfoJs {
    path: String,
    resolved: String,
    loaded: bool,
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
    file: String,
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
    full_start: u32,
    full_end: u32,
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

/// Convert a compiler diagnostic to JSON, translating its byte range to UTF-16
/// offsets against `source` (the file the range refers to).
fn diagnostic_to_js(d: &brink_ir::Diagnostic, source: &str) -> DiagnosticJs {
    DiagnosticJs {
        message: d.message.clone(),
        start: byte_to_utf16(source, d.range.start().into()),
        end: byte_to_utf16(source, d.range.end().into()),
        severity: format!("{:?}", d.code.severity()),
    }
}

/// Convert a symbol tree to JSON, translating byte ranges to UTF-16 offsets
/// against `source` (the file the symbols belong to).
fn convert_document_symbol(
    sym: brink_ide::document::DocumentSymbol,
    source: &str,
) -> DocumentSymbolJs {
    DocumentSymbolJs {
        name: sym.name,
        kind: symbol_kind_str(sym.kind).to_owned(),
        detail: sym.detail,
        start: byte_to_utf16(source, sym.range.start().into()),
        end: byte_to_utf16(source, sym.range.end().into()),
        full_start: byte_to_utf16(source, sym.full_range.start().into()),
        full_end: byte_to_utf16(source, sym.full_range.end().into()),
        children: sym
            .children
            .into_iter()
            .map(|c| convert_document_symbol(c, source))
            .collect(),
    }
}

// ── Structural move helpers ──────────────────────────────────────────

#[derive(Serialize)]
struct MoveResultJs {
    ok: bool,
    /// The file path this result applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_source: Option<String>,
    cross_file_edits: Vec<CrossFileEditJs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct CrossFileEditJs {
    file: u32,
    start: u32,
    end: u32,
    new_text: String,
}

fn move_result_json(result: brink_ide::structural_move::MoveResult, path: &str) -> String {
    let edits: Vec<CrossFileEditJs> = result
        .cross_file_edits
        .iter()
        .map(|e| CrossFileEditJs {
            file: e.file.0,
            start: e.range.start().into(),
            end: e.range.end().into(),
            new_text: e.new_text.clone(),
        })
        .collect();
    let resp = MoveResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: Some(result.new_source),
        cross_file_edits: edits,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

fn move_result_json_simple(new_source: String, path: &str) -> String {
    let resp = MoveResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: Some(new_source),
        cross_file_edits: Vec::new(),
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

fn error_json(msg: &str) -> String {
    let resp = MoveResultJs {
        ok: false,
        path: None,
        new_source: None,
        cross_file_edits: Vec::new(),
        error: Some(msg.to_owned()),
    };
    serde_json::to_string(&resp).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        reason = "tiny literal test strings; offsets cannot overflow u32"
    )]
    use super::{byte_to_utf16, utf16_to_byte};

    #[test]
    fn ascii_is_identity() {
        let s = "hello world";
        for i in 0..=s.len() as u32 {
            assert_eq!(byte_to_utf16(s, i), i, "byte_to_utf16 at {i}");
            assert_eq!(utf16_to_byte(s, i), i, "utf16_to_byte at {i}");
        }
    }

    #[test]
    fn two_byte_char_caf_e() {
        // "café": c=0, a=1, f=2, é=3..5 (2 bytes, 1 UTF-16 unit), len=5 bytes / 4 units
        let s = "café";
        assert_eq!(byte_to_utf16(s, 3), 3); // start of é
        assert_eq!(byte_to_utf16(s, 5), 4); // end of string
        assert_eq!(utf16_to_byte(s, 3), 3);
        assert_eq!(utf16_to_byte(s, 4), 5);
    }

    #[test]
    fn three_byte_em_dash() {
        // "a—b": a=0, —=1..4 (U+2014, 3 bytes, 1 unit), b=4..5
        let s = "a—b";
        assert_eq!(byte_to_utf16(s, 4), 2); // start of b
        assert_eq!(byte_to_utf16(s, 5), 3); // end
        assert_eq!(utf16_to_byte(s, 2), 4);
        assert_eq!(utf16_to_byte(s, 3), 5);
    }

    #[test]
    fn four_byte_astral_emoji() {
        // "a😀b": a=0, 😀=1..5 (U+1F600, 4 bytes, 2 UTF-16 units), b=5..6
        let s = "a😀b";
        assert_eq!(byte_to_utf16(s, 1), 1); // start of emoji
        assert_eq!(byte_to_utf16(s, 5), 3); // after emoji (1 + 2 units)
        assert_eq!(byte_to_utf16(s, 6), 4); // end
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 6);
    }

    #[test]
    fn round_trip_on_char_boundaries() {
        let s = "x—y😀zé!";
        for (i, _) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
            let units = byte_to_utf16(s, i as u32);
            assert_eq!(utf16_to_byte(s, units), i as u32, "round-trip at byte {i}");
        }
    }

    #[test]
    fn clamps_past_end() {
        let s = "café";
        assert_eq!(byte_to_utf16(s, 999), 4); // total UTF-16 length
        assert_eq!(utf16_to_byte(s, 999), 5); // total byte length
    }

    // ── End-to-end boundary tests ───────────────────────────────────
    // These prove the EditorSession surfaces UTF-16 offsets even when a
    // non-ASCII char shifts the byte/UTF-16 mapping. The é before the knot
    // makes every byte offset past it 1 larger than its UTF-16 offset.

    use super::EditorSession;

    #[test]
    fn document_symbols_returns_utf16_offsets() {
        // "é\n=== k ===\n": é = 2 bytes / 1 UTF-16 unit, so the knot header
        // starts at byte 3 but UTF-16 offset 2; the name `k` at byte 7 / unit 6.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é\n=== k ===\nhi\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.document_symbols();
        let syms: serde_json::Value = serde_json::from_str(&json).unwrap();
        let knot = &syms[0];
        assert_eq!(knot["name"], "k");
        // UTF-16 offsets, not bytes (byte would be 7 / full_start 3).
        assert_eq!(
            knot["start"].as_u64().unwrap(),
            6,
            "name start must be UTF-16"
        );
        assert_eq!(
            knot["full_start"].as_u64().unwrap(),
            2,
            "knot full_start must be UTF-16"
        );
    }

    #[test]
    fn goto_definition_round_trips_utf16() {
        // Divert target on line 1, knot on line 3, with é shifting offsets.
        // "é -> k\n\n=== k ===\nhi\n"
        //  byte:  é(0..2) space(2) -(3) >(4) space(5) k(6) \n(7) ...
        //  utf16: é(0..1) space(1) -(2) >(3) space(4) k(5) \n(6) ...
        // Cursor on the `k` of `-> k` is UTF-16 offset 5.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é -> k\n\n=== k ===\nhi\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.goto_definition(5);
        assert_ne!(json, "null", "should resolve the divert target");
        let loc: serde_json::Value = serde_json::from_str(&json).unwrap();
        // goto resolves to the knot's name `k` inside `=== k ===`. That `k` is
        // at byte 13 but UTF-16 offset 12 (one é before it). A byte-based
        // result would be 13 — so 12 proves both the input offset (UTF-16 5 →
        // byte 6, the divert's `k`) and the output (byte 13 → UTF-16 12).
        assert_eq!(
            loc["start"].as_u64().unwrap(),
            12,
            "definition start must be UTF-16, not bytes"
        );
    }
}
