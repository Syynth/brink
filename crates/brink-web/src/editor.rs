use std::collections::BTreeMap;

use brink_ide::session::IdeSession;
use rowan::{TextRange, TextSize};
use wasm_bindgen::prelude::*;

use crate::compile::{CompileResult, DiagnosticJs};
use crate::editor_dto::{
    CallWidgetSiteJs, ChangeSpecJs, CodeActionJs, ColorHintJs, CompletionItemJs, DeclaredGroupJs,
    DocumentSymbolJs, FileOutlineJs, FoldRangeJs, GroupStateJs, GroupWidgetSiteJs,
    HirLineContainerJs, HirProjectionJs, HirSpanJs, HoverInfoJs, IncludeInfoJs, InlayHintJs,
    LocationJs, ParamLabelJs, ProjectFileJs, SignatureInfoJs, SlotStateJs, SlotWidgetJs,
    StoryGraphEdgeJs, StoryGraphEdgeOccurrenceJs, StoryGraphJs, StoryGraphNodeJs, TokenJs,
    ValueItemJs, code_action_kind_str, convert_document_symbol, declared_group_js,
    dedupe_out_of_scope, diagnostic_to_js, fold_kind_str, inlay_hint_kind_str, span_kind_str,
    story_edge_kind_str, story_node_kind_str, symbol_kind_str, typed_detail,
};
use crate::editor_refactor::{
    AutoImportJs, dir_error_json, dir_move_result_json, error_json, gated_move_json,
    move_result_json_simple, structural_result_json,
};

// ── EditorSession ───────────────────────────────────────────────────

/// Stateful IDE session for the web editor. Wraps `IdeSession` and exposes
/// all IDE queries as methods that return JSON strings.
/// A view context scopes the editor to a sub-region of a file.
/// When active, `update_source` splices the fragment into the full file
/// at `[start, end)`, and IDE responses adjust offsets relative to the view.
#[derive(Clone, Copy)]
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

/// A document handle's state: the file it addresses plus an optional
/// sub-file view context (fragment handles).
struct DocState {
    path: String,
    view: Option<ViewContext>,
}

#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
    /// The active file path for IDE queries (legacy singleton API).
    active_path: String,
    /// Optional sub-file view context for focused editing (legacy singleton API).
    view: Option<ViewContext>,
    /// Open document handles, keyed by id. `BTreeMap` for deterministic
    /// iteration order (project rule: never iterate a `HashMap` where order
    /// can affect output).
    docs: BTreeMap<u32, DocState>,
    /// Next document-handle id. Starts at 1 — 0 is the "invalid handle"
    /// sentinel returned by `open_document`/`open_fragment` on failure.
    next_doc_id: u32,
    /// Whether `folding_ranges`/`folding_ranges_doc` compute the
    /// machinery/narrative fold runs (#479). Off by default: the runs only
    /// matter to hosts that implement prose/logic view modes, and computing
    /// them costs a full per-line classification on every folding query.
    fold_runs_enabled: bool,
    /// The T1b compiler dialect (docs/t1b-surface-spec.md §1), set via
    /// `set_language_dialect`. Defaults to `StrictInk`, matching
    /// `AnalysisOptions::default()`. Gates whether stdlib slice 1
    /// completion/signature help are offered (#589, #600), mirroring
    /// `brink-lsp`'s `Backend::dialect` — kept as its own field (rather than
    /// reading back through `session`) because those two call sites need a
    /// plain value, not an `Option<&_>`/reference. Since #611 it is also
    /// forwarded to `IdeSession::set_language_dialect`, so it gates the
    /// background analysis pass's `E051` diagnostic too.
    dialect: brink_analyzer::Dialect,
    /// Whether `set_language_dialect` has been called explicitly on this
    /// session (#1005). `apply_project_config` skips `dialect` when true —
    /// explicit API calls always override a discovered `brink.toml`'s
    /// `[project] dialect`, mirroring the CLI's `--dialect` flag precedence.
    dialect_explicit: bool,
    /// Same as `dialect_explicit`, for `set_type_policy` (#1005).
    types_explicit: bool,
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
            docs: BTreeMap::new(),
            next_doc_id: 1,
            fold_runs_enabled: false,
            dialect: brink_analyzer::Dialect::StrictInk,
            dialect_explicit: false,
            types_explicit: false,
        }
    }

    /// Update the active file's source text. Reparses, lowers, and analyzes.
    ///
    /// When a view context is active, `source` is treated as a fragment that
    /// gets spliced into the full file at `[view.start, view.end)`.
    pub fn update_source(&mut self, source: &str) {
        if let Some(view) = self.view {
            let full = self
                .session
                .file_id(&self.active_path)
                .and_then(|id| self.session.source(id).map(str::to_owned))
                .unwrap_or_default();
            let outcome = splice_fragment(&full, &view, source);
            if let Some(v) = &mut self.view {
                v.end = outcome.new_view_end;
            }
            self.session
                .update_and_analyze(&self.active_path, outcome.spliced);
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

    /// Register (or replace) the host-capability manifest from a JSON string,
    /// then re-analyze. The manifest describes the host's external-function
    /// vocabulary (types, semantic types) for author-time validation and
    /// richer hover/completion. Tooling-only — never affects the runtime.
    pub fn set_host_manifest(&mut self, json: &str) -> Result<(), JsError> {
        let manifest: brink_ir::HostManifest = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid host manifest: {e}")))?;
        self.session.set_host_manifest(manifest);
        Ok(())
    }

    /// Clear any registered host manifest, then re-analyze.
    pub fn clear_host_manifest(&mut self) {
        self.session.clear_host_manifest();
    }

    /// Register (or replace) the dialogue dialect (#368) from a JSON string.
    /// The dialect describes the project's dialogue-line conventions (cues,
    /// parentheticals, dialogue chains) so `line_contexts` can classify
    /// lines without hardcoding any one convention. Tooling-only — never
    /// affects the runtime or analysis; consumed at query time by
    /// `line_contexts`/`line_contexts_doc`. Mirrors `set_host_manifest`.
    pub fn set_dialect(&mut self, json: &str) -> Result<(), JsError> {
        let dialect: brink_ir::DialogueDialect = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid dialect: {e}")))?;
        brink_ir::dialect::validate(&dialect)
            .map_err(|errs| JsError::new(&format!("invalid dialect: {errs:?}")))?;
        let resolved = brink_ir::ResolvedDialect::compile(&dialect)
            .map_err(|e| JsError::new(&format!("invalid dialect: {e}")))?;
        self.session.set_dialect(resolved);
        Ok(())
    }

    /// Clear the registered dialect. `line_contexts` reverts to plain
    /// structural classification.
    pub fn clear_dialect(&mut self) {
        self.session.clear_dialect();
    }

    /// Enable or disable the machinery/narrative fold runs (#479 — off by
    /// default). Hosts that implement prose/logic view modes turn this on
    /// (typically once at mount, alongside activating the fold kinds in the
    /// editor); everyone else skips the per-query run computation entirely.
    /// Session-wide, like `set_dialect`.
    pub fn set_fold_runs_enabled(&mut self, enabled: bool) {
        self.fold_runs_enabled = enabled;
    }

    /// Set the T1b compiler dialect (docs/t1b-surface-spec.md §1, #589,
    /// #600, #611): `"brink"` or `"strict-ink"`; any other value (or never
    /// calling this at all) keeps the `StrictInk` default. Mirrors
    /// `brink-lsp`'s `initializationOptions.dialect` handling. Gates stdlib
    /// slice 1 completion (`completions`/`completions_doc`), dialect-aware
    /// signature help (`signature_help`/`signature_help_doc`), and — since
    /// #611 — the background analysis pass's `E051` "brink extension"
    /// diagnostic: a `brink`-dialect project no longer shows permanent
    /// spurious `E051` on valid extension syntax. Re-analyzes immediately
    /// (like `set_external_check`/`set_semantic_type_check`).
    pub fn set_language_dialect(&mut self, value: &str) {
        self.dialect = match value {
            "brink" => brink_analyzer::Dialect::Brink,
            _ => brink_analyzer::Dialect::StrictInk,
        };
        self.dialect_explicit = true;
        self.session.set_language_dialect(self.dialect);
    }

    /// Set the TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660):
    /// `"strict"` or `"gradual"`; any other value (or never calling this at
    /// all) keeps the `Gradual` default. Mirrors `set_language_dialect`
    /// exactly — this is the compile-facing counterpart of the compiler
    /// CLI's `--types strict`, previously reachable only there (PR #656 left
    /// `IdeSession` hardcoded to `Gradual`). `TypePolicy::Strict` requires
    /// `language_dialect() == "brink"`, or `compile_project`/the background
    /// analysis surface a single project-level `E064` config-error
    /// diagnostic instead of running the normal passes (the caller's
    /// responsibility, same as the CLI). Re-analyzes immediately (like
    /// `set_language_dialect`).
    ///
    /// **wasm-observable**: `compile_project` reads this through
    /// `IdeSession::analysis_options`, so a project that opts into
    /// `types = strict` now gets the `E065`/`E066`/`E067` strict-mode
    /// diagnostics (or the `E064` config error, if `dialect` isn't also
    /// `"brink"`) surfaced through `@brink-lang/web`'s compile/editor entry
    /// points — behavior no wasm consumer could previously reach at all.
    pub fn set_type_policy(&mut self, value: &str) {
        let types = match value {
            "strict" => brink_analyzer::TypePolicy::Strict,
            _ => brink_analyzer::TypePolicy::Gradual,
        };
        self.types_explicit = true;
        self.session.set_type_policy(types);
    }

    /// Parse a `brink.toml` project-settings file (#1005) and apply its
    /// `[project] dialect`/`types` to this session — the wasm/editor-mount
    /// wiring for the config file every compiler mount reads. The CLI
    /// discovers + reads `brink.toml` straight off disk
    /// (`brink_project_config::load_from_entry`); the wasm sandbox has no
    /// filesystem of its own, so the embedder reads the file with its own
    /// host APIs (Node `fs`, the File System Access API, …) and hands the
    /// text here.
    ///
    /// Call this once, at session construction, before any explicit
    /// `set_language_dialect`/`set_type_policy` call — a field already set
    /// explicitly on this session is left untouched (explicit calls always
    /// win over the file, matching the CLI's `--dialect`/`--types`
    /// precedence). Re-analyzes immediately for whichever field the file
    /// actually sets (like `set_language_dialect`/`set_type_policy`
    /// themselves).
    ///
    /// Returns the list of warning strings for unrecognized keys — as JSON
    /// (a `string[]`) — never an error (forward compat). Errors only on
    /// malformed TOML or a recognized key with an invalid value.
    pub fn apply_project_config(&mut self, toml: &str) -> Result<String, JsError> {
        let (config, warnings) = brink_project_config::parse_str(toml)
            .map_err(|e| JsError::new(&format!("invalid brink.toml: {e}")))?;
        if !self.dialect_explicit
            && let Some(dialect) = config.dialect
        {
            self.dialect = dialect;
            self.session.set_language_dialect(dialect);
        }
        if !self.types_explicit
            && let Some(types) = config.types
        {
            self.session.set_type_policy(types);
        }
        Ok(
            serde_json::to_string(&warnings.into_iter().map(|w| w.0).collect::<Vec<_>>())
                .unwrap_or_default(),
        )
    }

    /// Push the host's current values for `host`-source semantic types (Tier 3,
    /// #174) from a JSON object `{ "<type>": [{ "value", "label", "detail"? }] }`
    /// — a full snapshot that **replaces** the cache. The attached host (e.g.
    /// RPG Maker MZ) calls this with its named switches / items / … so the
    /// argument picker + value-label inlay hints stay current. Tooling-only;
    /// no re-analyze (values are consumed at query time, not in analysis).
    pub fn set_host_values(&mut self, json: &str) -> Result<(), JsError> {
        let values: brink_ide::HostValues = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid host values: {e}")))?;
        self.session.set_host_values(values);
        Ok(())
    }

    /// Clear the host-pushed value cache (e.g. on host disconnect). The picker
    /// degrades to plain literal entry for `host`-source params.
    pub fn clear_host_values(&mut self) {
        self.session.clear_host_values();
    }

    /// Set the severity policy for manifest-driven external diagnostics:
    /// `"error"` (default — a registered manifest is binding) or `"off"`.
    pub fn set_external_check(&mut self, level: &str) -> Result<(), JsError> {
        let severity = match level {
            "error" => brink_analyzer::ExternalCheckSeverity::Error,
            "off" => brink_analyzer::ExternalCheckSeverity::Off,
            other => {
                return Err(JsError::new(&format!(
                    "unknown external-check level `{other}` (expected \"error\" or \"off\")"
                )));
            }
        };
        self.session.set_external_check(severity);
        Ok(())
    }

    /// Set the severity policy for unknown-semantic-type diagnostics
    /// (`E040`), parallel to [`Self::set_external_check`] (#532): `"tolerant"`
    /// (default — unresolved types are only diagnosed once a manifest is
    /// registered, #339/#527) or `"error"` (always diagnose, even with no
    /// manifest registered — catches typo'd host semantic-type tags).
    pub fn set_semantic_type_check(&mut self, level: &str) -> Result<(), JsError> {
        let severity = match level {
            "tolerant" => brink_analyzer::SemanticTypeDiagnosticSeverity::Tolerant,
            "error" => brink_analyzer::SemanticTypeDiagnosticSeverity::Error,
            other => {
                return Err(JsError::new(&format!(
                    "unknown semantic-type-check level `{other}` (expected \"tolerant\" or \"error\")"
                )));
            }
        };
        self.session.set_semantic_type_check(severity);
        Ok(())
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
        self.view = Some(self.compute_view_context(&self.active_path, start, end));
    }

    /// Clear the view context, returning to full-file mode.
    pub fn clear_view_context(&mut self) {
        self.view = None;
    }

    /// Get the source text for the current view. Returns the fragment if a view
    /// context is active, or the full file otherwise. Returns a JSON string.
    pub fn get_view_source(&self) -> String {
        self.get_view_source_impl(&self.active_path, self.view.as_ref())
    }

    // ── Document handles ────────────────────────────────────────────
    //
    // Multi-document addressing: each handle pairs a file path with an
    // optional fragment view, so N live editor views can issue IDE queries
    // independently. The legacy active-file/view-context singleton above is
    // untouched by everything below. See the `*_doc` query variants.

    /// Open a full-file document handle on `path`. Returns the handle id,
    /// or `0` (never a valid id) if the file is not loaded.
    pub fn open_document(&mut self, path: &str) -> u32 {
        if self.session.file_id(path).is_none() {
            return 0;
        }
        self.insert_doc(DocState {
            path: path.to_owned(),
            view: None,
        })
    }

    /// Open a fragment document handle scoping `path` to `[start, end)`
    /// (UTF-16 offsets, same convention as `set_view_context`). Returns the
    /// handle id, or `0` (never a valid id) if the file is not loaded.
    pub fn open_fragment(&mut self, path: &str, start: u32, end: u32) -> u32 {
        if self.session.file_id(path).is_none() {
            return 0;
        }
        let view = self.compute_view_context(path, start, end);
        self.insert_doc(DocState {
            path: path.to_owned(),
            view: Some(view),
        })
    }

    /// Close a document handle. Returns `false` if the handle was unknown.
    pub fn close_document(&mut self, doc: u32) -> bool {
        self.docs.remove(&doc).is_some()
    }

    /// Replace a document's content: full-file replace for file handles,
    /// fragment splice for fragment handles (the handle's own view range is
    /// updated to cover the new fragment). Reparses, lowers, and analyzes.
    ///
    /// Returns a change-spec JSON object `{path, start, end, text?}`
    /// describing what actually changed in the file, in UTF-16 **file**
    /// coordinates: `[start, end)` is the replaced range of the file's
    /// previous content. The inserted text is the `source` argument the
    /// caller already has — unless `text` is present, in which case the
    /// fragment splice appended a `\n` separator and `text` carries what was
    /// actually inserted (`source` + `"\n"`). Returns `"null"` for an
    /// unknown handle.
    ///
    /// Other handles on the same file keep their ranges as-is; rebasing
    /// sibling fragment views from the change spec is the caller's job.
    pub fn update_document(&mut self, doc: u32, source: &str) -> String {
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let path = state.path.clone();
        let view = state.view;
        let full = self
            .session
            .file_id(&path)
            .and_then(|id| self.session.source(id).map(str::to_owned))
            .unwrap_or_default();

        let spec = if let Some(view) = view {
            let outcome = splice_fragment(&full, &view, source);
            if let Some(v) = self.docs.get_mut(&doc).and_then(|s| s.view.as_mut()) {
                v.end = outcome.new_view_end;
            }
            let spec = ChangeSpecJs {
                path: path.clone(),
                start: byte_to_utf16(&full, outcome.replaced_start),
                end: byte_to_utf16(&full, outcome.replaced_end),
                text: outcome.inserted_separator.then(|| format!("{source}\n")),
            };
            self.session.update_and_analyze(&path, outcome.spliced);
            spec
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            let full_len = full.len() as u32;
            let spec = ChangeSpecJs {
                path: path.clone(),
                start: 0,
                end: byte_to_utf16(&full, full_len),
                text: None,
            };
            self.session.update_and_analyze(&path, source.to_owned());
            spec
        };
        serde_json::to_string(&spec).unwrap_or_default()
    }

    // ── Document-handle query variants ──────────────────────────────
    //
    // Same offset conventions as the singleton queries above (UTF-16,
    // view-relative per handle) and same JSON response shapes. An unknown
    // handle returns the same empty sentinel as a missing file.

    /// Get the source text for a document handle's view (fragment or full
    /// file). Returns a JSON string, or `"null"` for an unknown handle.
    pub fn get_view_source_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.get_view_source_impl(&d.path, d.view.as_ref())
    }

    /// Compute per-line context for a document handle. Returns JSON array of `LineContext`.
    pub fn line_contexts_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.line_contexts_impl(&d.path, d.view.as_ref())
    }

    /// Compute semantic tokens for a document handle. Returns JSON array of tokens.
    pub fn semantic_tokens_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.semantic_tokens_impl(&d.path, d.view.as_ref())
    }

    /// The HIR structural projection for a document handle (#454): a JSON
    /// object `{ "spans": [...], "lines": [[...], ...] }` — nested semantic
    /// spans plus the per-line container stack for rails.
    pub fn hir_spans_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "{\"spans\":[],\"lines\":[]}".to_owned();
        };
        self.hir_spans_impl(&d.path, d.view.as_ref())
    }

    /// Compute completions for a document handle at the given offset. Returns JSON array.
    pub fn completions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.completions_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute hover info for a document handle at the given offset. Returns JSON or "null".
    pub fn hover_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.hover_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute goto-definition for a document handle at the given offset. Returns JSON or "null".
    pub fn goto_definition_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.goto_definition_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Find all references for a document handle. Returns JSON array.
    pub fn find_references_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.find_references_impl(&d.path, d.view.as_ref(), offset, true)
    }

    /// Check if rename is possible for a document handle. Returns JSON or "null".
    pub fn prepare_rename_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.prepare_rename_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute code actions for a document handle. Returns JSON array.
    pub fn code_actions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.code_actions_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute inlay hints for a document handle. Returns JSON array.
    pub fn inlay_hints_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.inlay_hints_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Color hints (`hex_color` argument literals) for a document handle, for
    /// the built-in color picker (#174-adjacent). Returns JSON array of
    /// `{ start, end, value }` (UTF-16 offsets).
    pub fn color_hints_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.color_hints_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Argument-widget sites for a document handle (argument-widget spec §4):
    /// every call's per-parameter slots + state (Filled / Empty / Expr), for
    /// inline editing and empty-slot filling. Returns a JSON array of
    /// `{ callee, slots: [{ param_name, widget?, type_name?, state }] }`
    /// (UTF-16 offsets).
    pub fn argument_widgets_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.argument_widgets_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Compute signature help for a document handle. Returns JSON or "null".
    pub fn signature_help_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.signature_help_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute folding ranges for a document handle. Returns JSON array.
    pub fn folding_ranges_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.folding_ranges_impl(&d.path, d.view.as_ref())
    }

    /// Compute document symbols (outline) for a document handle. Returns JSON array.
    pub fn document_symbols_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.document_symbols_impl(&d.path)
    }

    /// Convert a line element for a document handle. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element_doc(&self, doc: u32, offset: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.convert_element_impl(&d.path, d.view.as_ref(), offset, target)
    }

    /// Format a document handle's file (sort knots). Returns the formatted
    /// source as a JSON string.
    pub fn format_document_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "\"\"".to_owned();
        };
        self.format_document_impl(&d.path)
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

        let source = self.session.source(file_id).unwrap_or("");
        let syms = brink_ide::document::document_symbols(hir, manifest, source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();
        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compile the project using all loaded files. Returns JSON `CompileResult`.
    pub fn compile_project(&self, entry: &str) -> String {
        let session = &self.session;
        // Carry the registered host manifest into compilation so its
        // diagnostics (type/arity/domain) surface alongside compiler output.
        let result = brink_compiler::compile_with_options(
            entry,
            |path| {
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
            },
            session.analysis_options(),
        );

        // Convert a diagnostic against its OWN file's source (offsets are
        // file-relative); the resolved diagnostic already carries that file's
        // path, so an INCLUDEd file's error lands on the right tab instead of
        // collapsing onto the entry.
        let to_js = |d: &brink_compiler::ResolvedDiagnostic| {
            let src = session.source(d.file).unwrap_or("");
            diagnostic_to_js(d, src)
        };

        match result {
            Ok(output) => {
                let warnings: Vec<DiagnosticJs> = output.warnings.iter().map(to_js).collect();

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
                        diagnostics = diags.iter().map(to_js).collect();
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

            let source = db.source(id).unwrap_or("");
            let syms = brink_ide::document::document_symbols(hir, manifest, source);
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

    /// Whole-project story graph (studio-shell spec §4.1): knot/stitch nodes
    /// plus `END`/`DONE` pseudo-nodes, and divert/choice/tunnel/thread edges.
    /// Function knots and function-call edges are excluded. Node spans and
    /// edge-occurrence spans are UTF-16 offsets in their own file; each edge
    /// lists the divert sites that produced it (#371). Deterministically
    /// ordered (nodes by id, edges by from/to/kind, occurrences by
    /// file/span). Returns JSON `StoryGraph`, or `"null"` when no analysis
    /// is available.
    pub fn story_graph(&self) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };
        let db = self.session.db();
        let files: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = db
            .file_ids()
            .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
            .collect();
        let graph = brink_ide::story_graph::story_graph(analysis, &files);

        let nodes: Vec<StoryGraphNodeJs> = graph
            .nodes
            .into_iter()
            .map(|n| {
                let (file, start, end) = match (n.file, n.range) {
                    (Some(f), Some(r)) => {
                        let src = db.source(f).unwrap_or("");
                        (
                            db.file_path(f).map(str::to_owned),
                            Some(byte_to_utf16(src, r.start().into())),
                            Some(byte_to_utf16(src, r.end().into())),
                        )
                    }
                    _ => (None, None, None),
                };
                StoryGraphNodeJs {
                    id: n.id,
                    name: n.name,
                    kind: story_node_kind_str(n.kind),
                    file,
                    start,
                    end,
                    parent: n.parent,
                }
            })
            .collect();
        let edges: Vec<StoryGraphEdgeJs> = graph
            .edges
            .into_iter()
            .map(|e| StoryGraphEdgeJs {
                from: e.from,
                to: e.to,
                kind: story_edge_kind_str(e.kind),
                occurrences: e
                    .occurrences
                    .iter()
                    .filter_map(|o| {
                        let file = db.file_path(o.file)?.to_owned();
                        let src = db.source(o.file).unwrap_or("");
                        Some(StoryGraphEdgeOccurrenceJs {
                            file,
                            start: byte_to_utf16(src, o.range.start().into()),
                            end: byte_to_utf16(src, o.range.end().into()),
                        })
                    })
                    .collect(),
            })
            .collect();

        serde_json::to_string(&StoryGraphJs { nodes, edges }).unwrap_or_default()
    }

    /// Compute per-line context from the HIR. Returns JSON array of `LineContext`.
    pub fn line_contexts(&self) -> String {
        self.line_contexts_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute semantic tokens. Returns JSON array of tokens.
    pub fn semantic_tokens(&self) -> String {
        self.semantic_tokens_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute completions at the given byte offset. Returns JSON array.
    pub fn completions(&self, offset: u32) -> String {
        self.completions_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute hover info at the given byte offset. Returns JSON or "null".
    pub fn hover(&self, offset: u32) -> String {
        self.hover_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute goto-definition at the given byte offset. Returns JSON or "null".
    pub fn goto_definition(&self, offset: u32) -> String {
        self.goto_definition_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Find all references. Returns JSON array.
    pub fn find_references(&self, offset: u32) -> String {
        self.find_references_impl(&self.active_path, self.view.as_ref(), offset, true)
    }

    /// Find all references at an explicit file path + offset, with control over
    /// whether the declaration itself is included. Document-agnostic: resolves
    /// the file by `path` against the session, not the active document. Returns
    /// a JSON `Location[]` array (`"[]"` if the path or analysis is unavailable).
    pub fn find_references_at(&self, path: &str, offset: u32, include_declaration: bool) -> String {
        self.find_references_impl(path, None, offset, include_declaration)
    }

    /// Find all references to a symbol identified by its canonical name. Resolves
    /// the symbol via the analysis index; returns `"[]"` (fail-safe, deterministic)
    /// if the name is unknown or ambiguous (more than one matching definition).
    /// Otherwise locates the symbol's declaration (file + range start) and returns
    /// its references as a JSON `Location[]` array.
    pub fn references_to_symbol(&self, symbol_name: &str, include_declaration: bool) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };
        // Resolve the symbol name to a single definition. Unknown or ambiguous
        // names fail safe to an empty result rather than guessing.
        let ids = match analysis.index.by_name.get(symbol_name) {
            Some(ids) if ids.len() == 1 => ids,
            _ => return "[]".to_owned(),
        };
        let Some(info) = analysis.index.symbols.get(&ids[0]) else {
            return "[]".to_owned();
        };
        let Some(path) = self.session.file_path(info.file) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(info.file) else {
            return "[]".to_owned();
        };
        // The impl expects a UTF-16, view-relative offset; with no view that is
        // the file-absolute UTF-16 offset of the declaration's name start.
        let offset = byte_to_utf16(source, info.range.start().into());
        let path = path.to_owned();
        self.find_references_impl(&path, None, offset, include_declaration)
    }

    /// Check if rename is possible. Returns JSON or "null".
    pub fn prepare_rename(&self, offset: u32) -> String {
        self.prepare_rename_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute code actions. Returns JSON array.
    pub fn code_actions(&self, offset: u32) -> String {
        self.code_actions_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Apply a code action selected from [`code_actions`](Self::code_actions).
    ///
    /// `data_json` is the `data` field of a `CodeAction` (the self-describing,
    /// internally-tagged discriminator). `offset` is the cursor position the
    /// action was offered at — unused for the source-level actions (format /
    /// sort / structural move) but accepted for parity with the other queries
    /// and so future cursor-scoped actions need no signature change.
    ///
    /// Returns `StructuralResult`-shaped JSON: `new_source` for the primary file plus
    /// any `cross_file_edits` for structural moves, or `ok: false` with an
    /// `error` when the data is malformed or the action is a no-op.
    pub fn resolve_code_action(&self, data_json: &str, offset: u32) -> String {
        self.resolve_code_action_impl(&self.active_path, self.view.as_ref(), data_json, offset)
    }

    /// Document-handle variant of [`resolve_code_action`](Self::resolve_code_action).
    pub fn resolve_code_action_doc(&self, doc: u32, data_json: &str, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return error_json("unknown document handle");
        };
        self.resolve_code_action_impl(&d.path, d.view.as_ref(), data_json, offset)
    }

    /// Compute inlay hints. Returns JSON array.
    pub fn inlay_hints(&self, start: u32, end: u32) -> String {
        self.inlay_hints_impl(&self.active_path, self.view.as_ref(), start, end)
    }

    /// Compute signature help. Returns JSON or "null".
    pub fn signature_help(&self, offset: u32) -> String {
        self.signature_help_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute folding ranges. Returns JSON array.
    pub fn folding_ranges(&self) -> String {
        self.folding_ranges_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute document symbols (outline). Returns JSON array.
    pub fn document_symbols(&self) -> String {
        self.document_symbols_impl(&self.active_path)
    }

    /// Convert a line element to a different type. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element(&self, offset: u32, target: &str) -> String {
        self.convert_element_impl(&self.active_path, self.view.as_ref(), offset, target)
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
        self.format_document_impl(&self.active_path)
    }

    /// Reorder a stitch within its parent knot. Returns JSON `StructuralResult` or error string.
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

    /// Move a stitch from one knot to another. Returns JSON `StructuralResult` or error.
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
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Rename or move a file, rewriting every `INCLUDE` that resolves to it
    /// (inbound) plus the moved file's own relative includes (outbound).
    /// Returns JSON `StructuralResult` or error: `new_source` is the moved file's
    /// content to write at `new`, `cross_file_edits` carry the referencing
    /// files' rewrites. The op computes edits only — the caller applies them
    /// (write `new`, remove `old`).
    pub fn rename_file(&self, old: &str, new: &str) -> String {
        match brink_ide::file_rename::rename_file(&self.session, old, new) {
            Ok(result) => structural_result_json(&self.session, &result, old),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Atomically rename or move a directory (#314): relocate every file under
    /// `old_prefix` to `new_prefix`, rewriting all affected `INCLUDE`s against a
    /// single pre-move snapshot (moved files' outbound includes, inbound includes
    /// from files outside the folder, and intra-folder sibling includes — all
    /// mutually consistent). Returns JSON `DirMoveResult`: `moved_files` are the
    /// relocated files (`old_path`, `new_path`, rewritten `new_source`),
    /// `cross_file_edits` carry the outside referrers' rewrites. `safe` +
    /// `introduced_diagnostics` are the shared safe-by-default breakage gate. The
    /// op computes edits only — the caller writes the new files, removes the old
    /// ones, and applies the inbound edits.
    pub fn rename_dir(&self, old_prefix: &str, new_prefix: &str) -> String {
        match brink_ide::dir_rename::rename_dir(&self.session, old_prefix, new_prefix) {
            Ok(result) => dir_move_result_json(&self.session, &result),
            Err(e) => dir_error_json(&e.to_string()),
        }
    }

    /// Ensure `current` `INCLUDE`s `target` (#312 F core).
    ///
    /// Returns JSON `{ ok, already_reachable, edit?: TextEdit, error? }`. When
    /// `target` is already reachable from `current`'s INCLUDE graph the op is a
    /// no-op (`already_reachable: true`, no `edit`). Otherwise `edit` is the
    /// byte-range insertion the caller applies to `current`'s source.
    pub fn auto_import_include(&self, current: &str, target: &str) -> String {
        let resp = match brink_ide::auto_import::ensure_include(&self.session, current, target) {
            Ok(result) => AutoImportJs {
                ok: true,
                already_reachable: result.already_reachable,
                edit: result.edit,
                error: None,
            },
            Err(e) => AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some(e.to_string()),
            },
        };
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Auto-import `target` into the file backing document handle `doc` (#312 F,
    /// completion-accept path). Same `{ ok, already_reachable, edit?, error? }`
    /// shape as [`auto_import_include`], but the edit's `from`/`to` are
    /// **whole-file UTF-16** offsets (the INCLUDE block is a whole-file concept
    /// regardless of a fragment view), so the editor can apply it to the file
    /// source directly. Idempotent — no edit when `target` is already reachable.
    pub fn auto_import_include_doc(&self, doc: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("unknown document handle".to_owned()),
            })
            .unwrap_or_default();
        };
        let current = d.path.clone();
        let resp = match brink_ide::auto_import::ensure_include(&self.session, &current, target) {
            Ok(result) => {
                // Convert the byte-offset edit to whole-file UTF-16 so it can be
                // applied against the file source (or a whole-file view).
                let edit = result.edit.and_then(|e| {
                    let source = self.source_of(&current)?;
                    Some(brink_ide::line_convert::TextEdit {
                        from: byte_to_utf16(source, e.from),
                        to: byte_to_utf16(source, e.to),
                        insert: e.insert,
                    })
                });
                AutoImportJs {
                    ok: true,
                    already_reachable: result.already_reachable,
                    edit,
                    error: None,
                }
            }
            Err(e) => AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some(e.to_string()),
            },
        };
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Auto-import `target` into the file backing document handle `doc` **and
    /// apply the INCLUDE edit out-of-band**, rebasing every open fragment view
    /// on that file (#312 F, fragment-view completion-accept path).
    ///
    /// A fragment (symbol-tab / "play from here") view cannot dispatch the
    /// whole-file INCLUDE edit into its own CM document — the INCLUDE lives
    /// above the fragment. So the caller applies it here. A raw whole-file
    /// replace ([`update_file`]) would prepend the INCLUDE but leave every open
    /// fragment handle's stored `ViewContext` pointing at pre-shift byte
    /// offsets, so the next fragment splice would clobber the INCLUDE line and
    /// surrounding content. This method inserts the INCLUDE *and* shifts the
    /// byte range (and start line) of every fragment view on the file that
    /// begins at/after the insertion point, keeping them consistent.
    ///
    /// Returns the same `{ ok, already_reachable, edit?, error? }` shape as
    /// [`auto_import_include_doc`]. On success the `edit` (whole-file UTF-16)
    /// **describes the shift that was already applied** — the caller must NOT
    /// re-apply it; it exists only so the caller can rebase its own TS-side
    /// fragment-range mirror by the UTF-16 delta before inserting the symbol
    /// text into the fragment view. When `target` is already reachable this is
    /// a no-op (`already_reachable: true`, no `edit`).
    pub fn auto_import_apply_include_doc(&mut self, doc: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("unknown document handle".to_owned()),
            })
            .unwrap_or_default();
        };
        let current = d.path.clone();
        let result = match brink_ide::auto_import::ensure_include(&self.session, &current, target) {
            Ok(result) => result,
            Err(e) => {
                return serde_json::to_string(&AutoImportJs {
                    ok: false,
                    already_reachable: false,
                    edit: None,
                    error: Some(e.to_string()),
                })
                .unwrap_or_default();
            }
        };

        // Already reachable, or no edit produced: nothing to apply.
        let Some(edit) = result.edit.filter(|_| !result.already_reachable) else {
            return serde_json::to_string(&AutoImportJs {
                ok: true,
                already_reachable: result.already_reachable,
                edit: None,
                error: None,
            })
            .unwrap_or_default();
        };

        // `ensure_include` returns byte offsets for `from`/`to` into the current
        // file source. Apply the insertion to the whole-file source.
        let Some(source) = self.source_of(&current).map(str::to_owned) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("current file source unavailable".to_owned()),
            })
            .unwrap_or_default();
        };
        let from = (edit.from as usize).min(source.len());
        let to = (edit.to as usize).clamp(from, source.len());
        let mut merged = String::with_capacity(source.len() + edit.insert.len());
        merged.push_str(&source[..from]);
        merged.push_str(&edit.insert);
        merged.push_str(&source[to..]);

        // Rebase every open fragment view on this file whose range starts at or
        // after the insertion point. The edit removes `to - from` bytes and
        // inserts `edit.insert`, so downstream offsets shift by the net delta;
        // start lines shift by (inserted newlines − removed newlines).
        #[expect(
            clippy::cast_possible_wrap,
            reason = "ink files are always < 4GB, so byte counts fit i64"
        )]
        let byte_delta = edit.insert.len() as i64 - (to - from) as i64;
        let removed_newlines = count_newlines(&source[from..to]);
        let inserted_newlines = count_newlines(&edit.insert);
        let line_delta = i64::from(inserted_newlines) - i64::from(removed_newlines);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ink files are always < 4GB"
        )]
        let insert_at = from as u32;
        for state in self.docs.values_mut() {
            if state.path != current {
                continue;
            }
            let Some(view) = state.view.as_mut() else {
                continue;
            };
            rebase_view(view, insert_at, byte_delta, line_delta);
        }

        // The whole-file UTF-16 edit that was applied, so the caller can rebase
        // its own TS-side fragment range mirror by the UTF-16 delta. This edit
        // is NOT for the caller to re-apply (it is already applied) — it merely
        // describes the shift.
        let applied_edit = brink_ide::line_convert::TextEdit {
            from: byte_to_utf16(&source, edit.from),
            to: byte_to_utf16(&source, edit.to),
            insert: edit.insert,
        };

        self.session.update_and_analyze(&current, merged);

        serde_json::to_string(&AutoImportJs {
            ok: true,
            already_reachable: false,
            edit: Some(applied_edit),
            error: None,
        })
        .unwrap_or_default()
    }

    /// Promote a stitch to a top-level knot. Returns JSON `StructuralResult` or error.
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
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder a knot within the top-level knot list. Returns JSON `StructuralResult` or error.
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

    /// Reorder all stitches in a knot to match `order` (a permutation of the
    /// knot's stitch names). Used by drag-and-drop and multi-select moves,
    /// which know the full destination order. Returns JSON `StructuralResult` or error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen requires owned Vec<String> across the boundary"
    )]
    pub fn reorder_stitches(&self, path: &str, knot: &str, order: Vec<String>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        match brink_ide::structural_move::reorder_stitches(source, knot, &order) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder all top-level knots to match `order` (a permutation of the knot
    /// names). Returns JSON `StructuralResult` or error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen requires owned Vec<String> across the boundary"
    )]
    pub fn reorder_knots(&self, path: &str, order: Vec<String>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        match brink_ide::structural_move::reorder_knots(source, &order) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Demote a top-level knot to a stitch inside another knot. Returns JSON `StructuralResult` or error.
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
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Delete a knot (`stitch` empty) or a stitch, safe-by-default (#316).
    ///
    /// Removes the knot's whole region (header, body, nested stitches) or the
    /// named stitch's region, then runs the breakage gate: every divert /
    /// thread / tunnel / call that targeted the removed symbol now dangles, and
    /// those introduced diagnostics travel out so the caller can show a breakage
    /// report and apply the delete only on an explicit force. Returns the
    /// unified `StructuralResult` JSON (`new_source` for `path`, `safe`,
    /// `introduced_diagnostics`) or an error.
    pub fn delete_symbol(&self, path: &str, knot: &str, stitch: &str) -> String {
        let stitch = (!stitch.is_empty()).then_some(stitch);
        match brink_ide::structural_delete::delete_symbol(&self.session, path, knot, stitch) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Extract the selected lines into a new top-level `=== name ===` knot,
    /// replacing the selection with a tunnel call `-> name ->` (#315 H).
    ///
    /// `start_offset`/`end_offset` are whole-file UTF-16 offsets into `path`'s
    /// source (converted to bytes here). The selection is snapped to whole lines;
    /// the new knot is appended at end of file and ends with a `->->` return.
    /// Returns the unified `StructuralResult` JSON — `safe` is false and
    /// `introduced_diagnostics` is populated when the extraction pulls a
    /// weave/gather label or a local/temp reference out of scope. On failure a
    /// `StructuralResult`-shaped error is returned.
    pub fn extract_to_knot(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        name: &str,
    ) -> String {
        let Some(source) = self.source_of(path) else {
            return error_json("file not loaded");
        };
        let start = utf16_to_byte(source, start_offset) as usize;
        let end = utf16_to_byte(source, end_offset) as usize;
        match brink_ide::extract::extract_to_knot(&self.session, path, start, end, name) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Extract the selected lines into a new `=== function name() ===`, replacing
    /// the selection with the call — `{name()}` for a single value expression,
    /// `~ name()` for a statement (#315 H). Same offset/gate semantics as
    /// [`extract_to_knot`](Self::extract_to_knot).
    pub fn extract_to_function(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        name: &str,
    ) -> String {
        let Some(source) = self.source_of(path) else {
            return error_json("file not loaded");
        };
        let start = utf16_to_byte(source, start_offset) as usize;
        let end = utf16_to_byte(source, end_offset) as usize;
        match brink_ide::extract::extract_to_function(&self.session, path, start, end, name) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Rename a knot or stitch by name, safe-by-default. Returns a
    /// `StructuralResult`-shaped JSON payload (`new_source` for `path`,
    /// `cross_file_edits` for referencing files) extended with
    /// `introduced_diagnostics` and a `safe` flag. When `safe` is false the
    /// rename would introduce the listed diagnostics — the caller shows a
    /// breakage report and applies the (already-computed) edits only on an
    /// explicit force. An empty `stitch` renames the knot itself.
    pub fn rename_symbol(&self, path: &str, knot: &str, stitch: &str, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(hir) = self.session.hir(file_id) else {
            return error_json("no analysis");
        };
        let stitch = (!stitch.is_empty()).then_some(stitch);
        let Some(offset) = brink_ide::rename::declaration_offset(hir, knot, stitch) else {
            return error_json("symbol not found");
        };
        match brink_ide::rename::rename_safe(&self.session, file_id, offset, new_name) {
            Some(result) => structural_result_json(&self.session, &result, path),
            None => error_json("cannot rename this symbol"),
        }
    }

    /// Rename the symbol at a UTF-16 **file** offset, safe-by-default — the
    /// offset-based sibling of `rename_symbol`, used by the editor's F2 (which
    /// resolves any symbol under the cursor, not just knots/stitches). Returns
    /// the same `RenameResultJs` payload. The offset is a whole-file UTF-16
    /// offset (the caller folds any fragment-view origin in); it is converted
    /// to a byte offset here.
    pub fn rename_symbol_at(&self, path: &str, offset: u32, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let abs_offset = self.to_absolute(path, None, offset);
        match brink_ide::rename::rename_safe(
            &self.session,
            file_id,
            TextSize::new(abs_offset),
            new_name,
        ) {
            Some(result) => structural_result_json(&self.session, &result, path),
            None => error_json("cannot rename this symbol"),
        }
    }
}

// ── View context helpers (private, not wasm-exported) ───────────────
//
// Every IDE query is parameterized over `(path, view)` — the file being
// addressed and an optional sub-file view context. The legacy singleton API
// passes `(active_path, view)`; the document-handle API passes the handle's
// entry. Both funnel into the same `*_impl` bodies below.

impl EditorSession {
    /// Allocate the next handle id and insert `state`. Ids are monotonically
    /// increasing and never reused within a session.
    fn insert_doc(&mut self, state: DocState) -> u32 {
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.insert(id, state);
        id
    }

    /// Source text of a file, if loaded.
    fn source_of(&self, path: &str) -> Option<&str> {
        self.session
            .file_id(path)
            .and_then(|id| self.session.source(id))
    }

    /// Convert a UTF-16 view-relative offset (the boundary convention) to a
    /// file-absolute **byte** offset for `brink-ide`/rowan.
    ///
    /// When a view context is given the offset is relative to the displayed
    /// fragment (`source[view.start..view.end]`); otherwise it's relative to
    /// the whole file.
    fn to_absolute(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> u32 {
        let Some(source) = self.source_of(path) else {
            return offset;
        };
        match view {
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
    fn to_relative(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> Option<u32> {
        let source = self.source_of(path)?;
        match view {
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
    fn to_relative_line(view: Option<&ViewContext>, line: u32) -> Option<u32> {
        view.map_or(Some(line), |v| {
            (line >= v.start_line).then(|| line - v.start_line)
        })
    }

    /// Compute the end line of the view in the current source.
    fn view_end_line(&self, path: &str, view: &ViewContext) -> Option<u32> {
        let source = self.source_of(path)?;
        let byte_end = (view.end as usize).min(source.len());
        Some(count_newlines(&source[..byte_end]))
    }

    /// Compute a `ViewContext` scoping `path` to `[start, end)` (UTF-16
    /// offsets): converts the boundary offsets to bytes, trims trailing blank
    /// lines (keeping at most one newline), detects the newline separator at
    /// the boundary, and records the 0-based start line.
    fn compute_view_context(&self, path: &str, start: u32, end: u32) -> ViewContext {
        // Boundary offsets are UTF-16 code units; convert to bytes for the
        // internal byte-indexed logic below (and stored ViewContext range).
        let (start, end) = match self.source_of(path) {
            Some(s) => (utf16_to_byte(s, start), utf16_to_byte(s, end)),
            None => (start, end),
        };
        // Check if there's a newline right at `end` (the separator between this
        // section and the next). If so, we'll ensure it's preserved after splices.
        // Trim trailing blank lines from the view range and check if there's a
        // newline separator at the boundary that should be preserved across splices.
        let (end, trailing_newline) = self.source_of(path).map_or((end, false), |s| {
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
                    && s.as_bytes().get((trimmed_end as usize).wrapping_sub(1)) == Some(&b'\n'));
            (trimmed_end, has_nl)
        });

        let start_line = self.source_of(path).map_or(0, |s| {
            let byte_start = (start as usize).min(s.len());
            count_newlines(&s[..byte_start])
        });
        ViewContext {
            start,
            end,
            start_line,
            trailing_newline,
        }
    }
}

// ── IDE query implementations (private, parameterized) ──────────────

impl EditorSession {
    fn line_contexts_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(source), Some(root)) = (
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let Some(projection) = self.session.projection(file_id) else {
            return "[]".to_owned();
        };
        let contexts = match self.session.dialect() {
            Some(dialect) => brink_ide::line_context::line_contexts_with_dialect(
                source,
                &root,
                &projection,
                dialect,
            ),
            None => brink_ide::line_context::line_contexts(source, &root, &projection),
        };
        if let Some(v) = view {
            let start = v.start_line as usize;
            let end_line = self
                .view_end_line(path, v)
                .map_or(contexts.len(), |l| l as usize);
            let slice = &contexts[start..end_line.min(contexts.len())];
            serde_json::to_string(slice).unwrap_or_default()
        } else {
            serde_json::to_string(&contexts).unwrap_or_default()
        }
    }

    /// The HIR structural projection for one file (#454 phase 2): spans with
    /// UTF-16 line/char coordinates plus the per-line container stack, as one
    /// JSON object `{ "spans": [...], "lines": [[...], ...] }`.
    ///
    /// Byte→line/UTF-16 conversion happens here (the producer returns byte
    /// ranges). Under a view, span lines are remapped relative to the view's
    /// start (spans entirely above it are dropped) and the `lines` array is
    /// sliced to the view window — the same conventions as `semantic_tokens_impl`
    /// and `line_contexts_impl`.
    fn hir_spans_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        const EMPTY: &str = "{\"spans\":[],\"lines\":[]}";
        let Some(file_id) = self.session.file_id(path) else {
            return EMPTY.to_owned();
        };
        let (Some(hir), Some(analysis), Some(source)) = (
            self.session.hir(file_id),
            self.session.analysis(),
            self.session.source(file_id),
        ) else {
            return EMPTY.to_owned();
        };

        let _ = (hir, analysis);
        let Some(projection) = self.session.projection(file_id) else {
            return EMPTY.to_owned();
        };
        let idx = brink_ide::LineIndex::new(source);

        let spans: Vec<HirSpanJs> = projection
            .spans
            .iter()
            .filter_map(|s| {
                let (abs_start_line, start_char) = idx.line_col(s.range.start());
                let (abs_end_line, end_char) = idx.line_col(s.range.end());
                // Drop spans that end above the view; clamp ones straddling its
                // start so partially-visible containers keep their rails.
                // Non-containers straddling the start are dropped instead —
                // clamping a multi-line inline span (a `{ cond: … }` construct
                // extent, a multi-line content node) to (0, 0) would paint a
                // mark from the view's top-left over unrelated text.
                let end_line = Self::to_relative_line(view, abs_end_line)?;
                let (start_line, start_char) = match Self::to_relative_line(view, abs_start_line) {
                    Some(l) => (l, start_char),
                    None if s.kind.is_container() => (0, 0),
                    None => return None,
                };
                Some(HirSpanJs {
                    start_line,
                    start_char,
                    end_line,
                    end_char,
                    kind: span_kind_str(s.kind),
                    container: s.kind.is_container(),
                    depth: s.depth,
                    def_id: s.def_id,
                    target_id: s.target_id,
                    handle: s.handle,
                })
            })
            .collect();

        let lines: Vec<Vec<HirLineContainerJs>> = {
            let all = &projection.lines;
            let (start, end) = view.map_or((0, all.len()), |v| {
                let start = v.start_line as usize;
                let end = self
                    .view_end_line(path, v)
                    .map_or(all.len(), |l| l as usize);
                (start.min(all.len()), end.min(all.len()))
            });
            all[start.min(end)..end]
                .iter()
                .map(|stack| {
                    stack
                        .containers
                        .iter()
                        .map(|c| HirLineContainerJs {
                            kind: span_kind_str(c.kind),
                            handle: c.handle,
                            depth: c.depth,
                        })
                        .collect()
                })
                .collect()
        };

        serde_json::to_string(&HirProjectionJs { spans, lines })
            .unwrap_or_else(|_| EMPTY.to_owned())
    }

    fn semantic_tokens_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
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
                let line = Self::to_relative_line(view, t.line)?;
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

    fn completions_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let ctx = brink_ide::detect_completion_context(source, abs_offset as usize);
        let scope = brink_ide::cursor_scope(source, abs_offset as usize);

        // Auto-import (#312 F): symbols declared in files NOT reachable from the
        // current file's INCLUDE graph are still offered, but tagged as
        // out-of-scope so the editor can render a "from <file>" affordance and
        // insert the INCLUDE on accept. Reachability includes the current file
        // itself; locals (params/temps) carry no owning importable file.
        let reachable = self
            .session
            .file_id(path)
            .map(|id| self.session.db().reachable_from(id));

        // T1e (docs/t1e-spec.md §2, issue #850): `ref` argument ROOT
        // position — completion right after `ref ` narrows to durable
        // cells only (`VAR`s, the E080 rule every `ref lvalue-path` root
        // must satisfy), instead of the full `FunctionArgs` set (which also
        // offers CONST/param/temp/ListItem — none of them a legal `ref`
        // root, so offering them there would suggest an argument that's
        // guaranteed to fail analysis). Path *continuations* (`ref npc.`,
        // `ref inventory[`) aren't narrowed here — see
        // `ref_arg_root_prefix`'s own doc for why that's out of scope for
        // "where cheap".
        let ref_root = brink_ide::ref_arg_root_prefix(source, abs_offset as usize);

        let symbol_items = analysis
            .index
            .symbols
            .values()
            .filter(|info| brink_ide::is_visible_in_context(&ctx, info, &scope))
            .filter(|info| ref_root.is_none() || info.kind == brink_ir::SymbolKind::Variable)
            .map(|info| {
                let is_local = matches!(
                    info.kind,
                    brink_ir::SymbolKind::Param | brink_ir::SymbolKind::Temp
                );
                // A symbol is out of scope when its declaring file is not
                // reachable from the current file. Locals are never imported.
                let out_of_scope = !is_local
                    && reachable
                        .as_ref()
                        .is_some_and(|set| !set.contains(&info.file));
                let source_file = if out_of_scope {
                    self.session.file_path(info.file).map(str::to_owned)
                } else {
                    None
                };
                CompletionItemJs {
                    name: info.name.clone(),
                    kind: symbol_kind_str(info.kind).to_owned(),
                    // Callables get a typed signature from /// docs or the host
                    // manifest, if any; otherwise the kind-derived detail.
                    detail: typed_detail(analysis, info).or_else(|| info.detail.clone()),
                    insert: None,
                    out_of_scope,
                    source_file,
                }
            });

        // Host value picker (#174): in an argument slot whose param has a value
        // source, offer its labelled values first (display the label, insert the
        // literal) — static items from the manifest, or `host` items from the
        // pushed cache.
        let mut items: Vec<CompletionItemJs> = Vec::new();
        // Host value-picker literals aren't legal `ref` roots either (#850) —
        // gate the same way the symbol-kind filter above does.
        if matches!(ctx, brink_ide::CompletionContext::FunctionArgs) && ref_root.is_none() {
            items.extend(
                brink_ide::signature::argument_value_completions(
                    analysis,
                    source,
                    abs_offset as usize,
                    Some(self.session.host_values()),
                )
                .into_iter()
                .map(|v| CompletionItemJs {
                    name: v.label,
                    kind: "value".to_owned(),
                    detail: v.detail,
                    insert: Some(v.value),
                    out_of_scope: false,
                    source_file: None,
                }),
            );
        }

        // Multiple definitions of one name (#312 F): when a name is declared in
        // several out-of-scope files, keep only the nearest by relative-path
        // distance so the auto-import targets a single deterministic file. In-
        // scope duplicates (already reachable) are left untouched — they insert
        // no INCLUDE. `dedupe_out_of_scope` sorts, so the result is stable.
        let symbol_items = dedupe_out_of_scope(path, symbol_items.collect());
        items.extend(symbol_items);

        // Stdlib slice 1 completion (docs/t1b-surface-spec.md §5, #589,
        // #600) — brink dialect only ("never offered in StrictInk"); an
        // author-defined symbol of the same name is already offered above
        // (shadowing, per §5), mirroring brink-lsp's `completion` handler.
        items.extend(
            brink_ide::stdlib_completions(&ctx, self.dialect)
                .iter()
                .map(|f| CompletionItemJs {
                    name: f.name.to_owned(),
                    kind: "stdlib".to_owned(),
                    detail: Some(f.signature_label()),
                    insert: None,
                    out_of_scope: false,
                    source_file: None,
                }),
        );

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn hover_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let project_files = [(file_id, path.to_owned(), source.to_owned())];

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::hover::hover(
            analysis,
            self.session.db(),
            file_id,
            source,
            TextSize::new(abs_offset),
            &project_files,
        ) {
            Some(info) => {
                let js = HoverInfoJs {
                    content: info.content,
                    start: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.start().into())),
                    end: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.end().into())),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    fn goto_definition_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::navigation::goto_definition(analysis, file_id, TextSize::new(abs_offset)) {
            Some(loc) => {
                let db = self.session.db();
                let file_path = db.file_path(loc.file).unwrap_or_default().to_owned();
                let (start, end) = if loc.file == file_id {
                    // Same file: adjust to view-relative UTF-16 offsets
                    (
                        self.to_relative(path, view, loc.range.start().into())
                            .unwrap_or(loc.range.start().into()),
                        self.to_relative(path, view, loc.range.end().into())
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

    fn find_references_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        offset: u32,
        include_declaration: bool,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let refs = brink_ide::navigation::find_references(
            analysis,
            file_id,
            TextSize::new(abs_offset),
            include_declaration,
        );

        let db = self.session.db();
        let items: Vec<LocationJs> = refs
            .iter()
            .filter_map(|loc| {
                if loc.file == file_id {
                    // Same file: adjust offsets, filter out-of-view
                    let start = self.to_relative(path, view, loc.range.start().into())?;
                    let end = self.to_relative(path, view, loc.range.end().into())?;
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

    fn prepare_rename_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::rename::prepare_rename(analysis, file_id, TextSize::new(abs_offset)) {
            Some(range) => {
                let start = self.to_relative(path, view, range.start().into());
                let end = self.to_relative(path, view, range.end().into());
                match (start, end) {
                    (Some(s), Some(e)) => {
                        let js = LocationJs {
                            file: path.to_owned(),
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

    fn code_actions_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let mut actions = brink_ide::code_actions::code_actions(source, abs_offset as usize);

        // Auto-import quick-fix (M-4, modules-spec §2/§9): a cursor on an
        // out-of-scope module reference (`E025`) offers an `AddImport` action.
        // Session-aware (needs the whole-project module view), so it is merged
        // here rather than in the source-only `code_actions` path; it resolves
        // through the same `resolve_code_action` seam as a pure source rewrite.
        actions.extend(brink_ide::import_fix::import_actions(
            self.session.db(),
            file_id,
            abs_offset,
        ));

        let items: Vec<CodeActionJs> = actions
            .iter()
            .map(|a| CodeActionJs {
                title: a.title.clone(),
                kind: code_action_kind_str(&a.kind).to_owned(),
                data: serde_json::to_value(&a.data).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn resolve_code_action_impl(
        &self,
        path: &str,
        _view: Option<&ViewContext>,
        data_json: &str,
        _offset: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let data: brink_ide::code_actions::CodeActionData = match serde_json::from_str(data_json) {
            Ok(d) => d,
            Err(e) => return error_json(&format!("invalid code-action data: {e}")),
        };

        // Structural moves (move / promote / demote) need analysis context;
        // everything else (format / sort / reorder) is a pure source rewrite.
        if let Some(analysis) = self.session.analysis()
            && let Some(result) =
                brink_ide::code_actions::resolve_structural_action(source, analysis, file_id, &data)
        {
            return gated_move_json(&self.session, result, path);
        }

        match brink_ide::code_actions::resolve_code_action(source, &data) {
            Some(new_source) => move_result_json_simple(new_source, path),
            None => error_json("code action produced no change"),
        }
    }

    fn inlay_hints_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::inlay_hints::inlay_hints(
            &root,
            analysis,
            self.session.db(),
            file_id,
            range,
            Some(self.session.host_values()),
        );

        let items: Vec<InlayHintJs> = hints
            .iter()
            .filter_map(|h| {
                let offset = self.to_relative(path, view, h.offset.into())?;
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

    fn color_hints_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::color::color_hints(&root, analysis, range);

        let items: Vec<ColorHintJs> = hints
            .iter()
            .filter_map(|h| {
                let start = self.to_relative(path, view, h.start.into())?;
                let end = self.to_relative(path, view, h.end.into())?;
                Some(ColorHintJs {
                    start,
                    end,
                    value: h.value.clone(),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn argument_widgets_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        use brink_ide::argument_widgets::SlotState;
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let sites = brink_ide::argument_widgets::argument_widgets(
            &root,
            analysis,
            range,
            Some(self.session.host_values()),
        );

        let out: Vec<CallWidgetSiteJs> = sites
            .iter()
            .map(|site| {
                let slots = site
                    .slots
                    .iter()
                    .map(|slot| {
                        // Map byte offsets to UTF-16; a slot whose offsets fall
                        // outside the view degrades to a non-actionable Expr.
                        let state = match &slot.state {
                            SlotState::Filled { start, end, value } => {
                                match (
                                    self.to_relative(path, view, (*start).into()),
                                    self.to_relative(path, view, (*end).into()),
                                ) {
                                    (Some(start), Some(end)) => SlotStateJs::Filled {
                                        start,
                                        end,
                                        value: value.clone(),
                                    },
                                    _ => SlotStateJs::Expr,
                                }
                            }
                            SlotState::Empty {
                                insert_at,
                                needs_leading_comma,
                            } => match self.to_relative(path, view, (*insert_at).into()) {
                                Some(insert_at) => SlotStateJs::Empty {
                                    insert_at,
                                    needs_leading_comma: *needs_leading_comma,
                                },
                                None => SlotStateJs::Expr,
                            },
                            SlotState::Expr => SlotStateJs::Expr,
                        };
                        SlotWidgetJs {
                            param_name: slot.param_name.clone(),
                            widget: slot.widget.clone(),
                            type_name: slot.type_name.clone(),
                            values: slot
                                .values
                                .iter()
                                .map(|v| ValueItemJs {
                                    value: v.value.clone(),
                                    label: v.label.clone(),
                                    detail: v.detail.clone(),
                                })
                                .collect(),
                            state,
                        }
                    })
                    .collect();
                // The call-name span (UTF-16) anchors the form glyph; default to
                // 0 if it falls outside the view (the studio guards end > start).
                let name_start = self
                    .to_relative(path, view, site.name_start.into())
                    .unwrap_or(0);
                let name_end = self
                    .to_relative(path, view, site.name_end.into())
                    .unwrap_or(0);

                // Arg-group widgets (UTF-16); a group with an out-of-view span is
                // dropped (it stays a per-slot affordance).
                let groups: Vec<GroupWidgetSiteJs> = site
                    .groups
                    .iter()
                    .filter_map(|g| self.group_widget_js(path, view, g))
                    .collect();

                // Declared groups carry no document spans, so they need no view
                // translation — the Form renders them and seeds from `slots`.
                let declared_groups: Vec<DeclaredGroupJs> =
                    site.declared_groups.iter().map(declared_group_js).collect();

                CallWidgetSiteJs {
                    callee: site.callee.clone(),
                    name_start,
                    name_end,
                    slots,
                    groups,
                    declared_groups,
                }
            })
            .collect();

        serde_json::to_string(&out).unwrap_or_default()
    }

    /// Map one arg-group widget to its JSON shape (UTF-16); `None` when a span
    /// falls outside the view (the group degrades to per-slot affordances).
    fn group_widget_js(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        g: &brink_ide::argument_widgets::GroupWidgetSite,
    ) -> Option<GroupWidgetSiteJs> {
        use brink_ide::argument_widgets::GroupState;
        let state = match &g.state {
            GroupState::Filled { spans, values } => {
                let mut js_spans = Vec::with_capacity(spans.len());
                for (s, e) in spans {
                    js_spans.push((
                        self.to_relative(path, view, (*s).into())?,
                        self.to_relative(path, view, (*e).into())?,
                    ));
                }
                GroupStateJs::Filled {
                    spans: js_spans,
                    values: values.clone(),
                }
            }
            GroupState::Empty {
                insert_at,
                needs_leading_comma,
            } => GroupStateJs::Empty {
                insert_at: self.to_relative(path, view, (*insert_at).into())?,
                needs_leading_comma: *needs_leading_comma,
            },
        };
        Some(GroupWidgetSiteJs {
            ty: g.ty.clone(),
            surface: g.surface.clone(),
            param_indices: g.param_indices.clone(),
            param_names: g.param_names.clone(),
            state,
            context: g.context.iter().cloned().collect(),
            context_params: g.context_params.iter().cloned().collect(),
        })
    }

    fn signature_help_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::signature::signature_help_with_dialect(
            analysis,
            source,
            abs_offset as usize,
            self.dialect,
        ) {
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

    fn folding_ranges_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source)) = (self.session.hir(file_id), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        // One cached projection feeds both fold families (#476, #480).
        let Some(projection) = self.session.projection(file_id) else {
            return "[]".to_owned();
        };

        // Structural folds (#313 G, #476 weave folds) — never auto-collapsed
        // by a host.
        let mut ranges = brink_ide::folding::folding_ranges(hir, source, &projection);

        // `~ { … }` logic-block + nested if/while/for folds (#589, #600).
        // No dialect gate: brink-syntax always parses the superset grammar
        // and brink-ir always lowers it to this HIR shape regardless of
        // dialect (docs/t1b-surface-spec.md §1) — a logic block folds
        // identically in a strict-ink file (flagged E051) as in a brink one.
        ranges.extend(brink_ide::folding::block_folds(hir, source));

        // Machinery/narrative fold runs (#365): computed from the same
        // per-line classification `line_contexts_impl` exposes, so a
        // registered dialect's declared `nature` (#368) flows into the fold
        // computation exactly as it flows into `line_contexts`. Gated (#479):
        // only hosts that opt in via `set_fold_runs_enabled` pay for it.
        if self.fold_runs_enabled
            && let Some(root) = self.session.syntax_root(file_id)
        {
            let ctx = match self.session.dialect() {
                Some(dialect) => brink_ide::line_context::line_contexts_with_dialect(
                    source,
                    &root,
                    &projection,
                    dialect,
                ),
                None => brink_ide::line_context::line_contexts(source, &root, &projection),
            };
            ranges.extend(brink_ide::folding::machinery_and_narrative_folds(
                &projection,
                source,
                &ctx,
            ));
        }

        let items: Vec<FoldRangeJs> = ranges
            .iter()
            .filter_map(|r| {
                let start_line = Self::to_relative_line(view, r.start_line)?;
                let end_line = Self::to_relative_line(view, r.end_line)?;
                Some(FoldRangeJs {
                    start_line,
                    end_line,
                    collapsed_text: r.collapsed_text.clone(),
                    from_line_start: r.from_line_start,
                    kind: fold_kind_str(r.kind),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn document_symbols_impl(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let source = self.session.source(file_id).unwrap_or("");
        let syms = brink_ide::document::document_symbols(hir, manifest, source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn convert_element_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        offset: u32,
        target: &str,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
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

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::line_convert::convert_element(
            source,
            hir,
            &root,
            abs_offset,
            convert_target,
        ) {
            Some(edit) => match (
                self.to_relative(path, view, edit.from),
                self.to_relative(path, view, edit.to),
            ) {
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

    fn format_document_impl(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "\"\"".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "\"\"".to_owned();
        };

        let formatted = brink_ide::sort_knots_in_source(source);
        serde_json::to_string(&formatted).unwrap_or_default()
    }

    fn get_view_source_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let source = self.source_of(path);
        match (source, view) {
            (Some(s), Some(v)) => {
                let start = (v.start as usize).min(s.len());
                let end = (v.end as usize).min(s.len());
                serde_json::to_string(&s[start..end]).unwrap_or_default()
            }
            (Some(s), None) => serde_json::to_string(s).unwrap_or_default(),
            _ => "null".to_owned(),
        }
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
pub(crate) fn byte_to_utf16(s: &str, byte: u32) -> u32 {
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

// ── Fragment view rebasing ──────────────────────────────────────────

/// Shift a fragment `ViewContext` in place to account for an out-of-band
/// whole-file edit that inserted/removed content at byte `insert_at`.
///
/// `byte_delta` is the net byte change of the edit (inserted − removed) and
/// `line_delta` the net newline change. Only views that begin at or after
/// `insert_at` move — a view before the edit is unaffected. This keeps the
/// stored byte range (and start line) of every open fragment handle consistent
/// with the mutated file, so a subsequent fragment splice targets the correct
/// window instead of clobbering the shifted content.
fn rebase_view(view: &mut ViewContext, insert_at: u32, byte_delta: i64, line_delta: i64) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "offsets stay within a <4GB file and never go negative for a valid edit"
    )]
    let shift = |offset: u32| -> u32 {
        if offset < insert_at {
            offset
        } else {
            (i64::from(offset) + byte_delta).max(0) as u32
        }
    };
    let start_moves = view.start >= insert_at;
    // The insertion point sits at (or before) the view start for the auto-import
    // case (INCLUDE block above the fragment), so both boundaries move together.
    view.start = shift(view.start);
    view.end = shift(view.end);
    if start_moves {
        // The view's start byte shifted, so its first line shifts by the net
        // newline delta of the edit.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "line counts stay within a <4GB file"
        )]
        if line_delta >= 0 {
            view.start_line = view.start_line.saturating_add(line_delta as u32);
        } else {
            view.start_line = view.start_line.saturating_sub((-line_delta) as u32);
        }
    }
}

// ── Fragment splicing ───────────────────────────────────────────────

/// Result of splicing a fragment into its full file.
struct SpliceOutcome {
    /// The new full file content.
    spliced: String,
    /// New byte end of the fragment within the file (excludes any separator).
    new_view_end: u32,
    /// Byte start of the replaced range in the old file content.
    replaced_start: u32,
    /// Byte end (exclusive) of the replaced range in the old file content.
    replaced_end: u32,
    /// Whether a `\n` separator was appended after the fragment.
    inserted_separator: bool,
}

/// Splice `source` into `full` over the view's `[start, end)` byte range.
///
/// If the original boundary had a newline separator and the fragment doesn't
/// end with one, a `\n` separator is inserted after the fragment to prevent
/// merging with the next section. `new_view_end` tracks only the fragment,
/// NOT the separator — the separator lives at `spliced[new_view_end]` and is
/// preserved across splices.
fn splice_fragment(full: &str, view: &ViewContext, source: &str) -> SpliceOutcome {
    let start = (view.start as usize).min(full.len());
    let end = (view.end as usize).clamp(start, full.len());

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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ink files are always < 4GB"
    )]
    SpliceOutcome {
        spliced,
        new_view_end: view.start + source.len() as u32,
        replaced_start: start as u32,
        replaced_end: end as u32,
        inserted_separator: needs_sep,
    }
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
    fn hir_spans_doc_projects_spans_and_line_stacks() {
        // é in the content line shifts bytes vs UTF-16 by 1 for anything after it.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "VAR name = \"x\"\n=== start ===\né {name} here.\n* [Go] -> hub\n=== hub ===\n-> DONE\n",
        );
        assert!(s.set_active_file("main.ink"));
        let doc = s.open_document("main.ink");

        let json = s.hir_spans_doc(doc);
        let p: serde_json::Value = serde_json::from_str(&json).unwrap();
        let spans = p["spans"].as_array().unwrap();
        assert!(!spans.is_empty(), "projection has spans");

        // A knot container span with a handle and a def_id string.
        let knot = spans
            .iter()
            .find(|s| s["kind"] == "knot" && s["container"] == true)
            .expect("knot container span");
        assert!(knot["handle"].is_u64());
        // The knot's decl span carries a $-prefixed DefinitionId string.
        assert!(
            spans
                .iter()
                .any(|s| s["kind"] == "knot"
                    && s["def_id"].as_str().is_some_and(|d| d.starts_with('$'))),
            "knot decl carries a string def_id"
        );
        // The `-> hub` divert resolves: a divert span with a target_id string.
        assert!(
            spans.iter().any(|s| s["kind"] == "divert"
                && s["target_id"].as_str().is_some_and(|d| d.starts_with('$'))),
            "resolved divert carries target_id"
        );

        // The {name} var ref sits after the 2-byte é on line 2: its UTF-16
        // start_char must be 3 (é=1 unit + space + '{'), not the byte offset 4.
        let var_ref = spans
            .iter()
            .find(|s| s["kind"] == "var_ref" && s["start_line"] == 2)
            .expect("var ref span on the é line");
        assert_eq!(var_ref["start_char"].as_u64().unwrap(), 3, "UTF-16 column");

        // Per-line stacks: line 3 (the choice) is inside knot + choice.
        let lines = p["lines"].as_array().unwrap();
        let choice_line = lines[3].as_array().unwrap();
        assert!(
            choice_line.len() >= 2,
            "choice line inside knot + choice: {choice_line:?}"
        );
        // Depth-ordered outermost→innermost.
        let depths: Vec<u64> = choice_line
            .iter()
            .map(|c| c["depth"].as_u64().unwrap())
            .collect();
        assert!(depths.windows(2).all(|w| w[0] <= w[1]), "{depths:?}");
    }

    #[test]
    fn hir_spans_view_drops_straddling_non_containers() {
        // A multi-line construct span (`conditional`) starting above the
        // view must be dropped, not clamped to (0, 0) — a clamped inline
        // mark would paint from the view's top-left over unrelated text.
        // Containers straddling the start keep the clamp (partial rails).
        let src =
            "=== start ===\n{ ready:\nGo now.\nSecond line.\n- else:\nWait here.\n}\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("Second line.").unwrap() as u32;
        let doc = s.open_fragment("main.ink", start, src.len() as u32);

        let json = s.hir_spans_doc(doc);
        let p: serde_json::Value = serde_json::from_str(&json).unwrap();
        let spans = p["spans"].as_array().unwrap();
        assert!(
            !spans.iter().any(|s| s["kind"] == "conditional"),
            "straddling non-container construct span must be dropped: {spans:?}"
        );
        assert!(
            spans.iter().any(|s| s["kind"] == "knot"
                && s["container"] == true
                && s["start_line"] == 0
                && s["start_char"] == 0),
            "straddling knot container is clamped to the view start: {spans:?}"
        );
    }

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

    #[test]
    fn story_graph_edges_carry_utf16_occurrences() {
        // "é\n=== a ===\n-> b\n\n=== b ===\n-> DONE\n"
        //  The é (2 bytes / 1 UTF-16 unit) shifts every later byte offset
        //  1 past its UTF-16 offset. The divert target `b` on line 3 is at
        //  byte 16 → UTF-16 15.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é\n=== a ===\n-> b\n\n=== b ===\n-> DONE\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.story_graph();
        assert_ne!(json, "null");
        let graph: serde_json::Value = serde_json::from_str(&json).unwrap();
        let edges = graph["edges"].as_array().unwrap();

        let edge = |from: &str, to: &str| {
            edges
                .iter()
                .find(|e| e["from"] == from && e["to"] == to)
                .expect("missing edge")
        };

        // a -> b: one occurrence anchored on the target path `b`.
        let occ = &edge("a", "b")["occurrences"][0];
        assert_eq!(occ["file"], "main.ink");
        assert_eq!(occ["start"].as_u64().unwrap(), 15, "must be UTF-16");
        assert_eq!(occ["end"].as_u64().unwrap(), 16);

        // b -> DONE: occurrence spans the whole `-> DONE` statement.
        // Bytes: `-> DONE` starts at byte 29 → UTF-16 28, 7 chars long.
        let occ = &edge("b", "DONE")["occurrences"][0];
        assert_eq!(occ["file"], "main.ink");
        assert_eq!(occ["start"].as_u64().unwrap(), 28);
        assert_eq!(occ["end"].as_u64().unwrap(), 35);
    }

    // ── Cross-file structural-move edits (#12) ──────────────────────

    use crate::editor_refactor::apply_edits;

    #[test]
    fn apply_edits_applies_descending_and_preserves_offsets() {
        let src = "alpha beta gamma";
        // Two edits given out of order; both must land correctly.
        let out = apply_edits(
            src,
            vec![
                (11, 16, "GAMMA".to_owned()), // gamma -> GAMMA
                (0, 5, "ALPHA".to_owned()),   // alpha -> ALPHA
            ],
        );
        assert_eq!(out, "ALPHA beta GAMMA");
    }

    #[test]
    fn apply_edits_skips_out_of_bounds() {
        let src = "short";
        let out = apply_edits(src, vec![(0, 999, "x".to_owned())]);
        assert_eq!(out, "short", "out-of-bounds edit is skipped, not panicking");
    }

    #[test]
    fn cross_file_move_resolves_reference_edit_to_new_source() {
        // `other.ink` diverts into a stitch of `main.ink`; moving that stitch to
        // another knot must produce a cross-file edit updating the divert, now
        // delivered as the full new source of `other.ink`.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== a ===\n= s\nstuff\n-> END\n\n=== b ===\nbee\n-> END\n",
        );
        s.update_file("other.ink", "-> a.s\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.move_stitch("main.ink", "a", "s", "b");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], true, "move should succeed: {json}");

        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other_edit = cfe.iter().find(|e| e["path"] == "other.ink");
        assert!(
            other_edit.is_some(),
            "expected a cross-file edit for other.ink, got {cfe:?}"
        );
        let other = other_edit.unwrap();
        // The divert `-> a.s` must now point at the moved stitch's new path.
        assert!(
            other["new_source"].as_str().unwrap().contains("b.s"),
            "cross-file new_source should reference b.s: {other:?}"
        );
        // It carries the full file source (path-keyed), not byte ranges.
        assert!(other.get("new_source").is_some());
        assert!(other.get("start").is_none());
    }

    // ── Directory rename/move (#314) ────────────────────────────────

    #[test]
    fn rename_dir_returns_moved_files_and_inbound_edits() {
        let mut s = EditorSession::new();
        // main.ink (outside) includes into the folder; a folder file includes an
        // outside lib; two folder siblings include each other.
        s.update_file("main.ink", "INCLUDE chapters/intro.ink\n-> END\n");
        s.update_file("lib.ink", "=== helper ===\n-> END\n");
        s.update_file(
            "chapters/intro.ink",
            "INCLUDE ../lib.ink\nINCLUDE scene.ink\n-> END\n",
        );
        s.update_file("chapters/scene.ink", "=== scene ===\n-> END\n");

        let json = s.rename_dir("chapters", "book/chapters");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], true, "dir move should succeed: {json}");
        assert_eq!(v["safe"], true, "content-preserving move is safe: {json}");

        // Two moved files at their new paths.
        let moved = v["moved_files"].as_array().unwrap();
        assert_eq!(moved.len(), 2);
        let intro = moved
            .iter()
            .find(|m| m["new_path"] == "book/chapters/intro.ink")
            .unwrap();
        assert_eq!(intro["old_path"], "chapters/intro.ink");
        // Outbound: ../lib.ink now two levels deep; sibling stays bare.
        let intro_src = intro["new_source"].as_str().unwrap();
        assert!(
            intro_src.contains("INCLUDE ../../lib.ink"),
            "outbound not recomputed: {intro_src}"
        );
        assert!(
            intro_src.contains("INCLUDE scene.ink"),
            "sibling include should stay bare: {intro_src}"
        );

        // Inbound: main re-points into the new folder, delivered as full source.
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let main_edit = cfe.iter().find(|e| e["path"] == "main.ink").unwrap();
        assert!(
            main_edit["new_source"]
                .as_str()
                .unwrap()
                .contains("INCLUDE book/chapters/intro.ink"),
            "inbound not rewritten: {main_edit:?}"
        );
    }

    #[test]
    fn rename_dir_error_is_dir_move_shaped_json() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "-> END\n");
        let json = s.rename_dir("ghost", "x");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().is_some());
        assert!(v["moved_files"].as_array().unwrap().is_empty());
    }

    // ── Safe symbol rename (#305) ───────────────────────────────────

    #[test]
    fn rename_symbol_safe_rewrites_refs_with_empty_breakage() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "hello", "", "greeting")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "consistent rename is safe: {v}");
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
        let new_source = v["new_source"].as_str().unwrap();
        assert!(new_source.contains("=== greeting ==="));
        assert!(
            new_source.contains("-> greeting"),
            "divert rewritten: {new_source}"
        );
    }

    #[test]
    fn rename_symbol_collision_reports_breakage_and_cross_file_edits() {
        // `other.ink` diverts `-> a`; renaming knot `a` to `b` both collides
        // with the existing `b` (breakage) and rewrites the cross-file divert.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n\n=== b ===\n-> END\n");
        s.update_file("other.ink", "-> a\n");
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "a", "", "b")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], false, "collision is unsafe: {v}");

        let diags = v["introduced_diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "E022"),
            "expected E022 duplicate-knot in breakage report: {diags:?}"
        );
        // Every diag carries the fields the report renders.
        let first = &diags[0];
        for key in ["severity", "code", "message", "path", "line", "col"] {
            assert!(first.get(key).is_some(), "diag missing {key}: {first:?}");
        }

        // The cross-file divert is still rewritten (edits computed regardless).
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other = cfe.iter().find(|e| e["path"] == "other.ink").unwrap();
        assert!(other["new_source"].as_str().unwrap().contains("-> b"));
    }

    #[test]
    fn rename_symbol_at_renames_by_offset_cross_file() {
        // F2's offset-based path: rename the knot whose declaration the cursor
        // sits in, rewriting a divert in another file.
        let mut s = EditorSession::new();
        let main = "=== hello ===\nHi.\n-> END\n";
        s.update_file("main.ink", main);
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset of the `hello` name in `=== hello ===` (ASCII ⇒ UTF-16 == byte).
        let offset = u32::try_from(main.find("hello").unwrap()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol_at("main.ink", offset, "greeting")).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["safe"], true);
        assert!(
            v["new_source"]
                .as_str()
                .unwrap()
                .contains("=== greeting ===")
        );
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other = cfe.iter().find(|e| e["path"] == "other.ink").unwrap();
        assert!(
            other["new_source"]
                .as_str()
                .unwrap()
                .contains("-> greeting")
        );
    }

    #[test]
    fn rename_symbol_unknown_returns_error() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "nope", "", "x")).unwrap();
        assert_eq!(v["ok"], false);
    }

    // ── Unified StructuralResult + deleteSymbol (#316) ──────────────

    #[test]
    fn delete_symbol_referenced_knot_reports_breakage() {
        // `start` diverts to `target`; deleting `target` dangles that divert.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n-> target\n=== target ===\n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "target", "")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(
            v["safe"], false,
            "deleting a referenced knot is unsafe: {v}"
        );
        let diags = v["introduced_diagnostics"].as_array().unwrap();
        assert!(
            !diags.is_empty(),
            "the dangling divert is reported: {diags:?}"
        );
        // Every diag carries the breakage-report fields.
        for key in ["severity", "code", "message", "path", "line", "col"] {
            assert!(diags[0].get(key).is_some(), "diag missing {key}: {diags:?}");
        }
        let new_source = v["new_source"].as_str().unwrap();
        assert!(
            !new_source.contains("=== target ==="),
            "target removed: {new_source}"
        );
    }

    #[test]
    fn delete_symbol_unreferenced_is_safe() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "b", "")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "no references, so safe: {v}");
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn delete_symbol_stitch_keeps_siblings() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== k ===\n= a\nA.\n= b\nB.\n= c\nC.\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "k", "b")).unwrap();
        let new_source = v["new_source"].as_str().unwrap();
        assert!(!new_source.contains("= b"), "b removed: {new_source}");
        assert!(
            new_source.contains("= a") && new_source.contains("= c"),
            "siblings kept: {new_source}"
        );
    }

    #[test]
    fn delete_symbol_unknown_returns_error() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "ghost", "")).unwrap();
        assert_eq!(v["ok"], false);
    }

    // ── extract-selection ops (#315 H) ─────────────────────────────

    #[test]
    fn extract_to_knot_returns_structural_result_with_tunnel_call() {
        let mut s = EditorSession::new();
        let src = "=== start ===\nHello.\nWorld.\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        // UTF-16 offsets == byte offsets here (ASCII source).
        let start = src.find("Hello.").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "greeting")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "self-contained extraction is safe: {v}");
        let new_source = v["new_source"].as_str().unwrap();
        assert!(new_source.contains("=== greeting ==="), "{new_source}");
        assert!(new_source.contains("-> greeting ->"), "{new_source}");
        assert!(new_source.contains("->->"), "tunnel return: {new_source}");
    }

    #[test]
    fn extract_to_function_returns_structural_result_with_call() {
        let mut s = EditorSession::new();
        let src = "=== start ===\n{2 + 3}\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("{2 + 3}").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_function("main.ink", start, end, "calc")).unwrap();
        assert_eq!(v["ok"], true);
        let new_source = v["new_source"].as_str().unwrap();
        assert!(
            new_source.contains("=== function calc() ==="),
            "{new_source}"
        );
        assert!(new_source.contains("{calc()}"), "inline call: {new_source}");
    }

    #[test]
    fn extract_that_breaks_scope_reports_breakage() {
        let mut s = EditorSession::new();
        let src = "=== start ===\n~ temp count = 3\n{count}\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("{count}").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "shower")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], false, "out-of-scope temp makes it unsafe: {v}");
        assert!(
            !v["introduced_diagnostics"].as_array().unwrap().is_empty(),
            "breakage reported: {v}"
        );
    }

    #[test]
    fn extract_header_crossing_returns_error() {
        let mut s = EditorSession::new();
        let src = "=== a ===\nContent.\n=== b ===\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("Content.").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "x")).unwrap();
        assert_eq!(v["ok"], false, "crossing a header is rejected: {v}");
    }

    #[test]
    fn reorder_returns_safe_with_empty_breakage() {
        // Reorders change no qualification — the unified result is trivially safe.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.reorder_knot("main.ink", "a", 1)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true);
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
        // The unified result still round-trips through the StructuralResult JSON:
        // every field the studio reads is present.
        assert!(v.get("new_source").is_some());
        assert!(v.get("cross_file_edits").is_some());
    }

    #[test]
    fn breaking_move_reports_introduced_diagnostics() {
        // Moving a stitch whose bare same-knot reference can't be requalified
        // into the destination is gated. Here a divert in `other` targets the
        // qualified stitch; the move rewrites it, staying safe — but a move that
        // collides surfaces breakage. We assert the unified result always carries
        // the gate fields regardless of outcome.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== src ===\n= movable\n-> END\n=== dst ===\nDest.\n",
        );
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.move_stitch("main.ink", "src", "movable", "dst")).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v.get("safe").is_some(), "move carries the gate flag: {v}");
        assert!(
            v.get("introduced_diagnostics").is_some(),
            "move carries the breakage list: {v}"
        );
    }

    // ── Document handles (#122) ─────────────────────────────────────

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn two_handles_on_different_files_query_independently() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "=== alpha ===\nA line\n-> END\n");
        s.update_file("b.ink", "hello b\n=== beta ===\nB line\n-> END\n");
        // No set_active_file: the singleton still points at the unloaded
        // default, proving handles don't depend on it.
        let da = s.open_document("a.ink");
        let db = s.open_document("b.ink");
        assert_ne!(da, 0);
        assert_ne!(db, 0);
        assert_ne!(da, db);
        assert_eq!(s.active_file(), "main.ink");

        // hover over each file's knot name resolves per-handle.
        // a.ink: `alpha` at offsets 4..9; b.ink: `beta` at 12..16.
        let ha = s.hover_doc(da, 5);
        let hb = s.hover_doc(db, 13);
        assert!(ha.contains("alpha"), "hover via handle a: {ha}");
        assert!(hb.contains("beta"), "hover via handle b: {hb}");

        // line_contexts are file-specific per handle.
        let la = json(&s.line_contexts_doc(da));
        let lb = json(&s.line_contexts_doc(db));
        assert_eq!(la[0]["element"], "knot_header", "a.ink starts with a knot");
        assert_eq!(lb[0]["element"], "narrative", "b.ink starts with narrative");

        // completions work through a handle with no active file set.
        let ca = json(&s.completions_doc(da, 0));
        let names: Vec<&str> = ca
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| i["name"].as_str())
            .collect();
        assert!(names.contains(&"alpha"), "completions: {names:?}");
        assert!(names.contains(&"beta"), "completions: {names:?}");

        // The singleton queries still target the (unloaded) active file.
        assert_eq!(s.line_contexts(), "[]");
    }

    #[test]
    fn fragment_update_splices_and_reports_change_spec() {
        // é = 2 bytes / 1 UTF-16 unit: every offset past it differs between
        // byte and UTF-16 coordinates, proving the spec is UTF-16.
        //   bytes: "é intro\n"(0..9) "=== a ===\n"(9..19) "A line\n"(19..26)
        //          "=== b ===\n"(26..36) "B line\n"(36..43)
        //   utf16: 0..8 / 8..18 / 18..25 / 25..35 / 35..42
        let full = "é intro\n=== a ===\nA line\n=== b ===\nB line\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", full);

        let file_doc = s.open_document("main.ink");
        // Fragment over knot `a`: UTF-16 [8, 25).
        let frag = s.open_fragment("main.ink", 8, 25);
        assert_ne!(file_doc, 0);
        assert_ne!(frag, 0);
        assert_eq!(
            s.get_view_source_doc(frag),
            serde_json::to_string("=== a ===\nA line\n").unwrap()
        );

        // Update the fragment WITHOUT a trailing newline: the splice inserts
        // a `\n` separator, and the spec must carry the actually-inserted
        // text (source + "\n").
        let spec = json(&s.update_document(frag, "=== a ===\nA new"));
        assert_eq!(spec["path"], "main.ink");
        assert_eq!(spec["start"], 8, "UTF-16 start of replaced range");
        assert_eq!(spec["end"], 25, "UTF-16 end of replaced range");
        assert_eq!(
            spec["text"], "=== a ===\nA new\n",
            "separator nuance: actually-inserted text differs from source"
        );

        let expected = "é intro\n=== a ===\nA new\n=== b ===\nB line\n";
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string(expected).unwrap()
        );
        // The full-file handle on the same file sees the spliced content.
        assert_eq!(
            s.get_view_source_doc(file_doc),
            serde_json::to_string(expected).unwrap()
        );
        // The fragment handle's own view tracked the new fragment extent
        // (excluding the separator).
        assert_eq!(
            s.get_view_source_doc(frag),
            serde_json::to_string("=== a ===\nA new").unwrap()
        );

        // Update again WITH a trailing newline: no separator inserted (one
        // already follows the fragment), so no `text` in the spec. New file
        // coords: fragment is bytes [9, 24) = UTF-16 [8, 23).
        let spec = json(&s.update_document(frag, "=== a ===\nA two"));
        assert_eq!(spec["start"], 8);
        assert_eq!(spec["end"], 23);
        assert!(
            spec.get("text").is_none(),
            "no separator inserted -> no text field: {spec}"
        );
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("é intro\n=== a ===\nA two\n=== b ===\nB line\n").unwrap()
        );
    }

    #[test]
    fn full_file_handle_update_reports_whole_file_spec() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é one\n"); // 7 bytes, 6 UTF-16 units
        let d = s.open_document("main.ink");

        let spec = json(&s.update_document(d, "two\n"));
        assert_eq!(spec["path"], "main.ink");
        assert_eq!(spec["start"], 0);
        assert_eq!(spec["end"], 6, "whole previous file range, in UTF-16");
        assert!(spec.get("text").is_none());
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("two\n").unwrap()
        );
    }

    #[test]
    fn close_reopen_handle_lifecycle() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "=== alpha ===\nA\n-> END\n");

        // Unknown files don't get handles.
        assert_eq!(s.open_document("nope.ink"), 0);
        assert_eq!(s.open_fragment("nope.ink", 0, 1), 0);

        let d1 = s.open_document("a.ink");
        assert_eq!(d1, 1, "ids start at 1");
        assert!(s.close_document(d1));
        assert!(!s.close_document(d1), "double close reports unknown");

        // Closed handles answer with the same sentinels as a missing file.
        assert_eq!(s.hover_doc(d1, 5), "null");
        assert_eq!(s.line_contexts_doc(d1), "[]");
        assert_eq!(s.get_view_source_doc(d1), "null");
        assert_eq!(s.update_document(d1, "x"), "null");

        // Reopen: a fresh id, never a reused one.
        let d2 = s.open_document("a.ink");
        assert_ne!(d2, 0);
        assert_ne!(d2, d1, "handle ids are not reused");
        assert_ne!(s.get_view_source_doc(d2), "null");
    }

    #[test]
    fn singleton_api_unaffected_by_handle_operations() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\nA line\n=== b ===\nB line\n");
        s.update_file("other.ink", "hello\n");
        assert!(s.set_active_file("main.ink"));
        // Singleton view over knot `a`: UTF-16 [0, 17).
        s.set_view_context(0, 17);
        let before = s.get_view_source();
        assert_eq!(
            before,
            serde_json::to_string("=== a ===\nA line\n").unwrap()
        );

        // Handle operations on another file leave the singleton alone.
        let d = s.open_document("other.ink");
        let _ = s.update_document(d, "world\n");
        assert!(s.close_document(d));
        assert_eq!(s.active_file(), "main.ink");
        assert_eq!(s.get_view_source(), before);

        // The singleton splice path still works after handle traffic.
        s.update_source("=== a ===\nA edit");
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("=== a ===\nA edit\n=== b ===\nB line\n").unwrap()
        );
        assert_eq!(
            s.get_view_source(),
            serde_json::to_string("=== a ===\nA edit").unwrap()
        );
    }

    // ── Story graph (#96) ───────────────────────────────────────────

    const GRAPH_MAIN: &str = "é\n=== start ===\n* [Go] -> east.gate\n- -> END\n";
    const GRAPH_EAST: &str = "=== east ===\n= gate\nGate.\n-> start\n";

    #[test]
    fn story_graph_null_without_analysis() {
        let s = EditorSession::new();
        assert_eq!(s.story_graph(), "null");
    }

    #[test]
    fn story_graph_shape_and_utf16_offsets() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", GRAPH_MAIN);
        s.update_file("east.ink", GRAPH_EAST);

        let json = s.story_graph();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Nodes sorted by id; pseudo-node END present (referenced), DONE not.
        let ids: Vec<&str> = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["END", "east", "east.gate", "start"]);

        let nodes = v["nodes"].as_array().unwrap();
        let node = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();

        // `start` is declared after the 2-byte/1-unit `é`: its name sits at
        // byte 7 but UTF-16 offset 6 — the span must be UTF-16.
        let start = node("start");
        assert_eq!(start["kind"], "knot");
        assert_eq!(start["file"], "main.ink");
        assert_eq!(start["start"].as_u64().unwrap(), 6, "must be UTF-16");
        assert_eq!(start["end"].as_u64().unwrap(), 11);
        assert!(start.get("parent").is_none(), "knots carry no parent");

        let gate = node("east.gate");
        assert_eq!(gate["kind"], "stitch");
        assert_eq!(gate["file"], "east.ink");
        assert_eq!(gate["parent"], "east");

        let end = node("END");
        assert_eq!(end["kind"], "end");
        assert!(end.get("file").is_none(), "pseudo-nodes have no file");
        assert!(end.get("start").is_none(), "pseudo-nodes have no span");

        // Edges sorted by (from, to, kind); choice aggregation + cross-file
        // resolution + the auto-enter divert east -> east.gate.
        let edges: Vec<(String, String, String)> = v["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["from"].as_str().unwrap().to_owned(),
                    e["to"].as_str().unwrap().to_owned(),
                    e["kind"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let owned: Vec<(&str, &str, &str)> = edges
            .iter()
            .map(|(f, t, k)| (f.as_str(), t.as_str(), k.as_str()))
            .collect();
        assert_eq!(
            owned,
            vec![
                ("east", "east.gate", "divert"),
                ("east.gate", "start", "divert"),
                ("start", "END", "divert"),
                ("start", "east.gate", "choice"),
            ]
        );
    }

    #[test]
    fn story_graph_deterministic_across_file_insertion_order() {
        let mut a = EditorSession::new();
        a.update_file("main.ink", GRAPH_MAIN);
        a.update_file("east.ink", GRAPH_EAST);

        let mut b = EditorSession::new();
        b.update_file("east.ink", GRAPH_EAST);
        b.update_file("main.ink", GRAPH_MAIN);

        assert_eq!(
            a.story_graph(),
            b.story_graph(),
            "story graph JSON must be identical regardless of insertion order"
        );
    }

    // ── Document-agnostic / symbol-keyed references (#317) ──────────

    fn refs_count(json: &str) -> usize {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v.as_array().map_or(0, Vec::len)
    }

    #[test]
    fn find_references_at_same_file() {
        // Two diverts into `hello` plus its declaration, all in one file.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset on the first `-> hello` reference (the `h` of the target).
        let json = s.find_references_at("main.ink", 3, true);
        // Declaration + two divert references = 3.
        assert_eq!(refs_count(&json), 3, "same-file refs incl. decl: {json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        for loc in v.as_array().unwrap() {
            assert_eq!(loc["file"], "main.ink");
        }
    }

    #[test]
    fn find_references_at_cross_file() {
        // `other.ink` diverts into `hello` declared in `main.ink`.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset on the `hello` of the declaration `=== hello ===` (utf16 = byte).
        let json = s.find_references_at("main.ink", 4, true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files: std::collections::BTreeSet<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["file"].as_str().unwrap())
            .collect();
        assert!(
            files.contains("main.ink") && files.contains("other.ink"),
            "cross-file refs must span both files: {json}"
        );
    }

    #[test]
    fn find_references_at_honors_include_declaration() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> hello\n");
        assert!(s.set_active_file("main.ink"));

        let with_decl = refs_count(&s.find_references_at("main.ink", 3, true));
        let without_decl = refs_count(&s.find_references_at("main.ink", 3, false));
        assert_eq!(
            with_decl,
            without_decl + 1,
            "excluding the declaration drops exactly one result"
        );
    }

    #[test]
    fn references_to_symbol_name_keyed_lookup() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.references_to_symbol("hello", true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files: std::collections::BTreeSet<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["file"].as_str().unwrap())
            .collect();
        assert!(
            files.contains("main.ink") && files.contains("other.ink"),
            "symbol-keyed lookup resolves the declaration + cross-file ref: {json}"
        );

        // include_declaration is honored through the symbol-keyed path too.
        let with_decl = refs_count(&json);
        let without_decl = refs_count(&s.references_to_symbol("hello", false));
        assert_eq!(with_decl, without_decl + 1);
    }

    #[test]
    fn references_to_symbol_nonexistent_is_empty() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        assert_eq!(
            s.references_to_symbol("does_not_exist", true),
            "[]",
            "unknown symbol fails safe to []"
        );
    }

    // ── resolve_code_action (#321 Track N) ──────────────────────────
    // The code_actions JSON carries a self-describing `data` discriminator;
    // feeding that payload back to resolve_code_action applies the action and
    // returns StructuralResult-shaped JSON with the rewritten source.

    /// The byte offset (cursor) inside the first knot's body — enough to scope
    /// cursor-anchored actions to that knot.
    const UNSORTED_KNOTS: &str = "=== beta ===\nhi\n-> END\n=== alpha ===\nyo\n-> END\n";

    fn find_action<'a>(
        actions: &'a serde_json::Value,
        title_contains: &str,
    ) -> &'a serde_json::Value {
        actions
            .as_array()
            .expect("code_actions returns a JSON array")
            .iter()
            .find(|a| {
                a["title"]
                    .as_str()
                    .is_some_and(|t| t.contains(title_contains))
            })
            .expect("a code action whose title matches the expected substring")
    }

    #[test]
    fn code_action_data_is_self_describing() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(0)).expect("valid JSON array");
        let sort = find_action(&actions, "Sort knots");
        // The tagged discriminator must be present and round-trippable.
        assert_eq!(
            sort["data"]["action"], "SortKnots",
            "data carries the tagged discriminator: {actions}"
        );
    }

    #[test]
    fn resolve_sort_action_yields_sorted_source() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(0)).expect("valid JSON array");
        let data = find_action(&actions, "Sort knots")["data"].to_string();

        let result: serde_json::Value = serde_json::from_str(&s.resolve_code_action(&data, 0))
            .expect("valid StructuralResult JSON");
        assert_eq!(result["ok"], true, "resolve succeeds: {result}");
        let new_source = result["new_source"]
            .as_str()
            .expect("new_source is present and a string");
        assert!(
            !new_source.is_empty(),
            "sort action produces non-empty edits"
        );
        // alpha now precedes beta.
        let alpha = new_source.find("alpha").expect("alpha knot present");
        let beta = new_source.find("beta").expect("beta knot present");
        assert!(
            alpha < beta,
            "knots are sorted alphabetically: {new_source:?}"
        );
    }

    #[test]
    fn references_to_symbol_ambiguous_is_empty() {
        // A knot and a variable share the name `dup`. They are different kinds,
        // so both land under one `by_name` key → ambiguous (two ids).
        let mut s = EditorSession::new();
        s.update_file("main.ink", "VAR dup = 0\n=== dup ===\nhi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        assert_eq!(
            s.references_to_symbol("dup", true),
            "[]",
            "ambiguous symbol name fails safe to []"
        );
    }

    #[test]
    fn completions_tag_out_of_scope_symbols_with_source_file() {
        // main.ink INCLUDEs included.ink but NOT economy.ink. A knot from the
        // reachable file is in scope; one from the unreachable file is tagged
        // out-of-scope with its source path (#312 F).
        let mut s = EditorSession::new();
        s.update_file("included.ink", "=== reachable_knot ===\nhi.\n-> END\n");
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        let main = "INCLUDE included.ink\n=== start ===\n-> re\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        // Cursor after `-> re` (a divert context, which surfaces knots).
        let offset = u32::try_from(main.find("-> re").expect("divert present") + 5)
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");

        let reachable = arr
            .iter()
            .find(|i| i["name"] == "reachable_knot")
            .expect("reachable knot offered");
        assert!(
            reachable.get("out_of_scope").is_none(),
            "in-scope knot is not flagged out_of_scope: {reachable}"
        );

        let trade = arr
            .iter()
            .find(|i| i["name"] == "trade")
            .expect("out-of-scope knot offered");
        assert_eq!(
            trade["out_of_scope"], true,
            "unreachable knot flagged out_of_scope: {trade}"
        );
        assert_eq!(
            trade["source_file"], "economy.ink",
            "out-of-scope knot carries its source file: {trade}"
        );
    }

    #[test]
    fn completions_after_ref_keyword_offers_only_durable_variables() {
        // T1e (docs/t1e-spec.md §2, issue #850): right after `ref `, only a
        // `VAR` is a legal `ref lvalue-path` root (E080) — a CONST, param,
        // or temp is not, so it must not be offered there even though the
        // ordinary `FunctionArgs` completion set includes all of them.
        let mut s = EditorSession::new();
        let main = "VAR npc = 0\n\
                     CONST MAX_HP = 100\n\
                     === main(amount) ===\n\
                     ~ temp scratch = 0\n\
                     ~ heal(ref \n\
                     -> END\n\n\
                     === function heal(ref hp, k) ===\n\
                     ~ hp = hp + k\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset =
            u32::try_from(main.find("heal(ref \n").expect("call present") + "heal(ref ".len())
                .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");
        let names: Vec<&str> = arr
            .iter()
            .map(|i| i["name"].as_str().expect("name is a string"))
            .collect();

        assert!(
            names.contains(&"npc"),
            "the durable VAR is offered: {names:?}"
        );
        assert!(
            !names.contains(&"MAX_HP"),
            "a CONST is not a legal ref root, must not be offered: {names:?}"
        );
        assert!(
            !names.contains(&"scratch"),
            "a temp is not a legal ref root, must not be offered: {names:?}"
        );
        assert!(
            !names.contains(&"amount"),
            "a param is not a legal ref root, must not be offered: {names:?}"
        );
    }

    #[test]
    fn completions_in_ordinary_arg_position_still_offers_everything() {
        // Sanity check for the test above: without a preceding `ref `, the
        // full FunctionArgs set (including CONST/param/temp) is unaffected.
        let mut s = EditorSession::new();
        let main = "VAR npc = 0\n\
                     CONST MAX_HP = 100\n\
                     === main(amount) ===\n\
                     ~ temp scratch = 0\n\
                     ~ heal(\n\
                     -> END\n\n\
                     === function heal(hp, k) ===\n\
                     ~ hp = hp + k\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from(main.find("heal(\n").expect("call present") + "heal(".len())
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");
        let names: Vec<&str> = arr
            .iter()
            .map(|i| i["name"].as_str().expect("name is a string"))
            .collect();

        assert!(names.contains(&"npc"), "VAR offered: {names:?}");
        assert!(names.contains(&"MAX_HP"), "CONST offered: {names:?}");
        assert!(names.contains(&"scratch"), "temp offered: {names:?}");
    }

    #[test]
    fn completions_dedupe_out_of_scope_keeps_nearest() {
        // Two unreachable files both define `dup`. The nearer one (same dir as
        // the current file) wins deterministically; only one row survives.
        let mut s = EditorSession::new();
        s.update_file("near.ink", "=== dup ===\nn.\n-> END\n");
        s.update_file("deep/far.ink", "=== dup ===\nf.\n-> END\n");
        let main = "=== start ===\n-> du\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from(main.find("-> du").expect("divert present") + 5)
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let dups: Vec<&serde_json::Value> = items
            .as_array()
            .expect("array")
            .iter()
            .filter(|i| i["name"] == "dup")
            .collect();

        assert_eq!(
            dups.len(),
            1,
            "duplicate out-of-scope name collapses to one: {dups:?}"
        );
        assert_eq!(
            dups[0]["source_file"], "near.ink",
            "nearest source file wins: {:?}",
            dups[0]
        );
    }

    #[test]
    fn auto_import_apply_include_doc_rebases_open_fragment_view() {
        // Regression (#312 F): the fragment-view auto-import path. A raw
        // whole-file INCLUDE write shifts the fragment content right but leaves
        // the open fragment handle's view range at pre-shift offsets, so the
        // NEXT fragment splice clobbers the INCLUDE line and the knot header.
        // `auto_import_apply_include_doc` must apply the INCLUDE *and* rebase
        // the open fragment view so the subsequent splice lands correctly.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        let main_src = "=== start ===\nThe cursor is here.\n";
        s.update_file("main.ink", main_src);

        // Open a fragment over the knot BODY ("The cursor is here.\n"). The body
        // begins right after the "=== start ===\n" header (byte 14) and runs to
        // end of file.
        let body_start = "=== start ===\n".len() as u32;
        let body_end = main_src.len() as u32;
        let doc = s.open_fragment("main.ink", body_start, body_end);
        assert_ne!(doc, 0, "fragment handle opened");

        // Accept an out-of-scope completion: auto-import economy.ink into the
        // fragment's file, applying + rebasing out-of-band.
        let applied: serde_json::Value =
            serde_json::from_str(&s.auto_import_apply_include_doc(doc, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(applied["ok"], true, "op ok: {applied}");
        assert_eq!(
            applied["already_reachable"], false,
            "not yet reachable: {applied}"
        );
        // The returned edit DESCRIBES the applied shift (for the caller to
        // rebase its own TS-side range) — it is NOT to be re-applied.
        assert_eq!(
            applied["edit"]["insert"].as_str(),
            Some("INCLUDE economy.ink\n"),
            "returned edit describes the applied INCLUDE shift: {applied}"
        );
        assert_eq!(
            applied["edit"]["from"], 0,
            "INCLUDE inserted at file top: {applied}"
        );

        // The whole file now carries the INCLUDE above the untouched knot.
        let full_after_import = s.source_of("main.ink").expect("source").to_owned();
        assert_eq!(
            full_after_import, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.\n",
            "INCLUDE prepended, knot intact"
        );

        // Now the completion dispatches the accepted symbol into the FRAGMENT
        // view (the edited body). This routes through update_document, which
        // splices at the (now rebased) view range.
        let edited_body = "The cursor is here.trade\n";
        let spec = s.update_document(doc, edited_body);
        assert_ne!(spec, "null", "fragment push produced a change spec");

        // The INCLUDE line and knot header must survive; only the body changed.
        let full_after_push = s.source_of("main.ink").expect("source");
        assert_eq!(
            full_after_push, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.trade\n",
            "INCLUDE + header intact; only the fragment body was replaced"
        );
    }

    #[test]
    fn raw_update_file_then_fragment_push_corrupts_without_rebase() {
        // Documents the pre-fix bug (#312 F): applying the INCLUDE via the raw
        // whole-file `update_file` (which does NOT rebase open fragment views)
        // and then pushing the fragment splices at the STALE view range,
        // clobbering the INCLUDE line and the knot header. This is exactly the
        // corruption `auto_import_apply_include_doc` avoids. If this assertion
        // ever flips to producing clean output, `update_file` grew rebase
        // semantics and the fragment auto-import path can be simplified.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\n-> END\n");
        let main_src = "=== start ===\nThe cursor is here.\n";
        s.update_file("main.ink", main_src);

        let body_start = "=== start ===\n".len() as u32;
        let doc = s.open_fragment("main.ink", body_start, main_src.len() as u32);

        // OLD path: prepend INCLUDE via a raw whole-file replace (no rebase).
        s.update_file("main.ink", &format!("INCLUDE economy.ink\n{main_src}"));
        // Then push the edited fragment — splices at the stale [14, 34) range.
        s.update_document(doc, "The cursor is here.trade\n");

        let corrupted = s.source_of("main.ink").expect("source");
        assert_ne!(
            corrupted, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.trade\n",
            "raw path corrupts — this is the bug the apply-and-rebase op fixes"
        );
    }

    #[test]
    fn auto_import_apply_include_doc_idempotent_when_reachable() {
        // When the target is already reachable, the apply-and-rebase op is a
        // no-op: no INCLUDE added, view range untouched.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\n-> END\n");
        let main_src = "INCLUDE economy.ink\n=== start ===\nbody.\n";
        s.update_file("main.ink", main_src);
        // Re-analyze so the INCLUDE edge binds to the now-loaded target.
        s.update_file("main.ink", main_src);

        let body_start = "INCLUDE economy.ink\n=== start ===\n".len() as u32;
        let doc = s.open_fragment("main.ink", body_start, main_src.len() as u32);
        assert_ne!(doc, 0);

        let applied: serde_json::Value =
            serde_json::from_str(&s.auto_import_apply_include_doc(doc, "economy.ink"))
                .expect("valid JSON");
        assert_eq!(applied["already_reachable"], true, "already reachable");
        assert!(applied.get("edit").is_none(), "no edit");

        // Pushing the fragment still lands correctly (view range never moved).
        s.update_document(doc, "body.\n-> trade\n");
        assert_eq!(
            s.source_of("main.ink").expect("source"),
            "INCLUDE economy.ink\n=== start ===\nbody.\n-> trade\n"
        );
    }

    #[test]
    fn auto_import_doc_edit_is_utf16_and_idempotent() {
        // The doc-based auto-import returns a whole-file edit for an unreachable
        // target, and no edit once the file already reaches it.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        s.update_file("main.ink", "=== start ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let doc = s.open_document("main.ink");

        let first: serde_json::Value =
            serde_json::from_str(&s.auto_import_include_doc(doc, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(first["ok"], true, "op ok: {first}");
        assert_eq!(
            first["already_reachable"], false,
            "not yet reachable: {first}"
        );
        let insert = first["edit"]["insert"]
            .as_str()
            .expect("edit carries an insert string");
        assert!(
            insert.contains("INCLUDE economy.ink"),
            "insert adds the INCLUDE: {first}"
        );

        // Apply the edit, re-analyze, then a second call is a no-op.
        s.update_file("main.ink", &format!("{insert}=== start ===\n-> END\n"));
        assert!(s.set_active_file("main.ink"));
        let doc2 = s.open_document("main.ink");
        let second: serde_json::Value =
            serde_json::from_str(&s.auto_import_include_doc(doc2, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(second["already_reachable"], true, "now reachable: {second}");
        assert!(
            second.get("edit").is_none(),
            "idempotent — no second INCLUDE: {second}"
        );
    }

    #[test]
    fn resolve_format_action_yields_formatted_source() {
        // A knot whose body has a formatting deviation the formatter fixes.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== k ===\n* [opt]   trailing spaces   \n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        // Cursor inside the knot body.
        let offset = 12;
        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(offset)).expect("valid JSON array");
        let format = find_action(&actions, "Format knot");
        assert_eq!(
            format["data"]["action"], "FormatKnot",
            "format action carries its discriminator: {actions}"
        );
        let data = format["data"].to_string();

        let result: serde_json::Value = serde_json::from_str(&s.resolve_code_action(&data, offset))
            .expect("valid StructuralResult JSON");
        assert_eq!(result["ok"], true, "resolve succeeds: {result}");
        let new_source = result["new_source"]
            .as_str()
            .expect("new_source is present and a string");
        assert!(
            !new_source.is_empty(),
            "format action produces non-empty edits"
        );
    }

    #[test]
    fn resolve_rejects_malformed_data() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let result: serde_json::Value =
            serde_json::from_str(&s.resolve_code_action("{ not valid }", 0))
                .expect("error is still StructuralResult-shaped JSON");
        assert_eq!(result["ok"], false, "malformed data -> ok:false: {result}");
        assert!(
            result["error"].as_str().is_some(),
            "an error message is present"
        );
    }

    // ── Dialogue dialect (#368) ──────────────────────────────────────

    #[test]
    fn set_dialect_then_line_contexts_reports_character_kind() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\nHello there.\n");
        assert!(s.set_active_file("main.ink"));

        // No dialect registered yet: no `dialect` facet on any line.
        let before = json(&s.line_contexts());
        assert!(before[1].get("dialect").is_none(), "{before}");

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");

        let after = json(&s.line_contexts());
        assert_eq!(after[1]["dialect"]["kind"], "character", "{after}");
        assert_eq!(after[2]["dialect"]["kind"], "dialogue", "{after}");
    }

    #[test]
    fn folding_ranges_include_machinery_and_narrative_runs() {
        // #365: `folding_ranges()` returns structural folds (from the
        // pre-existing pass) plus machinery/narrative fold runs computed
        // from the same `line_contexts` classification the editor already
        // consumes — a real user path (folding gutter), not a separate
        // code path only a unit test reaches. Runs are opt-in since #479.
        let mut s = EditorSession::new();
        s.set_fold_runs_enabled(true);
        s.update_file(
            "main.ink",
            "=== start ===\n~ temp x = 1\n~ temp y = 2\nHello there.\nHow are you?\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        let kinds: Vec<&str> = ranges
            .as_array()
            .expect("array")
            .iter()
            .map(|r| r["kind"].as_str().expect("kind is a string"))
            .collect();
        assert!(kinds.contains(&"machinery"), "{ranges}");
        assert!(kinds.contains(&"narrative"), "{ranges}");

        let machinery = ranges
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["kind"] == "machinery")
            .expect("machinery fold present");
        assert_eq!(machinery["start_line"], 1);
        assert_eq!(machinery["end_line"], 2);
    }

    #[test]
    fn fold_runs_are_gated_off_by_default() {
        // #479: machinery/narrative runs are computed only when the host
        // opts in via set_fold_runs_enabled; the default folding output is
        // structural-only.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n~ temp x = 1\n~ temp y = 2\nHello\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        assert!(
            ranges
                .as_array()
                .expect("array")
                .iter()
                .all(|r| r["kind"] == "structural"),
            "no run kinds without opt-in: {ranges}"
        );

        s.set_fold_runs_enabled(true);
        let ranges = json(&s.folding_ranges());
        assert!(
            ranges
                .as_array()
                .expect("array")
                .iter()
                .any(|r| r["kind"] == "machinery"),
            "machinery runs appear once enabled: {ranges}"
        );
    }

    #[test]
    fn folding_ranges_respect_registered_dialect_nature() {
        // A dialect-classified cue+dialogue pair is Narrative-natured (the
        // at-cue preset) — the fold run must follow the registered dialect,
        // not a hardcoded kind list.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\nHello there.\n");
        assert!(s.set_active_file("main.ink"));

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");
        s.set_fold_runs_enabled(true);

        let ranges = json(&s.folding_ranges());
        let has_narrative_run = ranges
            .as_array()
            .expect("array")
            .iter()
            .any(|r| r["kind"] == "narrative" && r["start_line"] == 1 && r["end_line"] == 2);
        assert!(has_narrative_run, "{ranges}");
    }

    #[test]
    fn clear_dialect_reverts_to_plain_classification() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\n");
        assert!(s.set_active_file("main.ink"));

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");
        assert_eq!(json(&s.line_contexts())[1]["dialect"]["kind"], "character");

        s.clear_dialect();
        let after = json(&s.line_contexts());
        assert!(after[1].get("dialect").is_none(), "{after}");
    }

    // `set_dialect`'s error path constructs a `JsError`, which panics when
    // called on a non-wasm target ("cannot call wasm-bindgen imported
    // functions on non-wasm targets") — same constraint as every other
    // `Result<_, JsError>`-returning method in this file (see
    // `StoryRunner::new`, `continue_story`, etc., whose error paths are only
    // exercised under `binding_wasm_tests` below). The rejection tests live
    // there; validated JSON acceptance is what's tested natively above.
    //
    // `WebSession` test coverage lives in `websession_wasm_tests` below —
    // its methods take `&JsValue`/`Vec<JsValue>` args, and constructing a
    // `JsValue` (e.g. `JsValue::from_f64`) itself panics off wasm32 (not just
    // the error paths), unlike `StoryRunner`'s native-testable subset.

    // ── Lines table (#366) ────────────────────────────────────────────
    // `StoryRunner::lines_table`'s happy path never constructs a `JsError`
    // (only the `serde_json` error branch would, and that can't fail for a
    // `LinesJson` value), so — like `set_dialect`'s acceptance path above —
    // it's exercised natively here rather than under `binding_wasm_tests`.

    fn compiled(src: &str) -> crate::story_runner::StoryRunner {
        let out = brink_compiler::compile("main.ink", |_path| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        crate::story_runner::StoryRunner::new(&bytes).expect("runner constructs")
    }

    #[test]
    fn lines_table_reports_text_and_source_span() {
        let runner = compiled("=== start ===\nHello, world!\n-> END\n");
        let table = runner.lines_table().expect("lines_table succeeds");
        let v: serde_json::Value = serde_json::from_str(&table).expect("valid json");

        assert_eq!(v["version"], 1);
        let scopes = v["scopes"].as_array().expect("scopes array");
        let scope = scopes
            .iter()
            .find(|s| s["name"] == "start")
            .expect("start scope present");
        let line = scope["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .find(|l| l["content"] == "Hello, world!")
            .expect("the line's plain text content is present");
        let source = &line["source"];
        assert_eq!(source["file"], "main.ink", "{table}");
        assert!(
            source["range_start"].as_u64().is_some() && source["range_end"].as_u64().is_some(),
            "source span present: {table}"
        );
    }

    #[test]
    fn lines_table_resolves_includes_project_wide() {
        let out = brink_compiler::compile("main.ink", |path| match path {
            "main.ink" => Ok(
                "INCLUDE included.ink\n=== start ===\n-> other.other_stitch\n-> END\n".to_owned(),
            ),
            "included.ink" => {
                Ok("=== other ===\n= other_stitch\nFrom the included file.\n-> END\n".to_owned())
            }
            _ => Err(std::io::Error::other(format!("unknown file {path}"))),
        })
        .expect("multi-file project compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        let runner = crate::story_runner::StoryRunner::new(&bytes).expect("runner constructs");

        let table = runner.lines_table().expect("lines_table succeeds");
        let v: serde_json::Value = serde_json::from_str(&table).expect("valid json");
        let scopes = v["scopes"].as_array().expect("scopes array");
        let has_included_line = scopes.iter().any(|scope| {
            scope["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|l| l["content"] == "From the included file.")
        });
        assert!(
            has_included_line,
            "the lines table covers the whole project, INCLUDEs resolved: {table}"
        );
        let included_source_file = scopes
            .iter()
            .flat_map(|scope| scope["lines"].as_array().into_iter().flatten())
            .find(|l| l["content"] == "From the included file.")
            .and_then(|l| l["source"]["file"].as_str().map(str::to_owned));
        assert_eq!(
            included_source_file.as_deref(),
            Some("included.ink"),
            "source span attributes the line to its own included file: {table}"
        );
    }

    // ── #600: wire the #589 IDE features into the wasm bridge ─────────

    #[test]
    fn stdlib_completions_offered_only_after_opting_into_brink_dialect() {
        // Before `set_language_dialect("brink")` the bridge defaults to
        // `StrictInk` (matching `AnalysisOptions::default()`) — stdlib slice
        // 1 names (docs/t1b-surface-spec.md §5) must not be offered.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n~ temp x = \nEND\n");
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from("=== start ===\n~ temp x = ".len()).expect("fits u32");
        let before = json(&s.completions(offset));
        assert!(
            before
                .as_array()
                .expect("array")
                .iter()
                .all(|i| i["name"] != "len"),
            "stdlib names withheld under the StrictInk default: {before}"
        );

        s.set_language_dialect("brink");
        let after = json(&s.completions(offset));
        let len_item = after
            .as_array()
            .expect("array")
            .iter()
            .find(|i| i["name"] == "len")
            .expect("stdlib `len` offered once brink dialect is set");
        assert_eq!(len_item["kind"], "stdlib", "{len_item}");
        assert_eq!(len_item["detail"], "len(x) -> int", "{len_item}");
    }

    #[test]
    fn stdlib_completions_never_offered_for_strict_ink_value() {
        // Any value other than the exact string "brink" (including a typo
        // or an explicit "strict-ink") keeps the StrictInk default, mirroring
        // brink-lsp's `initializationOptions.dialect` handling.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n~ temp x = \nEND\n");
        assert!(s.set_active_file("main.ink"));
        s.set_language_dialect("strict-ink");

        let offset = u32::try_from("=== start ===\n~ temp x = ".len()).expect("fits u32");
        let items = json(&s.completions(offset));
        assert!(
            items
                .as_array()
                .expect("array")
                .iter()
                .all(|i| i["name"] != "len"),
            "{items}"
        );
    }

    #[test]
    fn signature_help_is_dialect_aware_for_stdlib_mutators() {
        // #600: `signature_help` must call `signature_help_with_dialect` —
        // the lvalue-mutator rendering (`push(a: lvalue, v)`,
        // docs/t1b-surface-spec.md §5) only surfaces once brink dialect is
        // set, and never under the StrictInk default.
        let src = "=== start ===\n~ push(inventory, \"sword\")\n-> END\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));

        let offset =
            u32::try_from(src.find("push(").expect("call present") + "push(".len()).expect("fits");

        let before = s.signature_help(offset);
        assert_eq!(before, "null", "no stdlib signature help under StrictInk");

        s.set_language_dialect("brink");
        let sig = json(&s.signature_help(offset));
        assert_eq!(sig["label"], "push(a: lvalue, v)", "{sig}");
        assert_eq!(sig["active_parameter"], 0, "{sig}");
    }

    #[test]
    fn folding_ranges_include_logic_block_folds() {
        // #600: `folding_ranges` must call `block_folds` — a `~ { … }`
        // logic block folds as its own structural region, with no dialect
        // gate (it folds identically in a strict-ink file, where it is
        // flagged E051, as in a brink one).
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n~ {\n    temp x = 1\n    temp y = 2\n}\nHello.\n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        let block = ranges
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["kind"] == "structural" && r["start_line"] == 1)
            .expect("logic block folds as a structural region");
        assert_eq!(block["end_line"], 4, "{block}");
    }

    // ── #611: thread the declared dialect into background analysis ────

    /// A `~ { … }` multi-line logic block is brink-extension syntax
    /// (docs/t1b-surface-spec.md §1) — flagged `E051` under the default
    /// `StrictInk` dialect, silent under `Brink`.
    const BRINK_EXT_SRC: &str = "=== start ===\n~ {\n    temp x = 1\n}\n-> END\n";

    #[test]
    fn strict_ink_default_flags_e051_in_background_analysis() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "StrictInk default: E051 stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_brink_suppresses_e051_in_background_analysis() {
        // #611: before this fix, `set_language_dialect` only updated the
        // bridge's local `dialect` field (consumed by completions/signature
        // help) — the `IdeSession`'s background analysis pass never saw it,
        // so a brink-dialect project kept showing spurious `E051`.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("brink");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "brink dialect: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_strict_ink_value_keeps_e051() {
        // Any value other than the exact string "brink" (including an
        // explicit "strict-ink") keeps the StrictInk default.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("strict-ink");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "explicit strict-ink: E051 stands: {:?}",
            analysis.diagnostics
        );
    }

    // ── TM-3 typed-mode policy (#660) ──────────────────────────────────

    /// #660: `set_type_policy` defaults to `Gradual` (byte-identical to
    /// pre-#619 behavior) until called — before this fix `EditorSession` had
    /// no way to reach `types = strict` at all (only the compiler CLI's
    /// `--types strict` could).
    #[test]
    fn type_policy_defaults_to_gradual_no_e065() {
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "default (gradual) must not flag E065: {:?}",
            analysis.diagnostics
        );
    }

    /// #660: `types = strict` under the default `StrictInk` dialect is a
    /// project-level config error (`E064`) — reached through
    /// `set_type_policy`, mirroring the compiler CLI's
    /// `--types strict --dialect strict-ink`.
    #[test]
    fn set_type_policy_strict_with_strict_ink_dialect_flags_e064() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> END\n");
        s.set_type_policy("strict");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E064),
            "types=strict + dialect=strict-ink (default): expected E064: {:?}",
            analysis.diagnostics
        );
    }

    /// #660 counterpart: `types = strict` + `dialect = brink` turns on the
    /// Unknown-escape check (`E065`) — proving `set_type_policy` reaches the
    /// real strict-mode checks, not just the config-error path, and that
    /// `compile_project` (which reads `IdeSession::analysis_options`) would
    /// see the same policy.
    #[test]
    fn set_type_policy_strict_with_brink_dialect_flags_e065() {
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "types=strict + dialect=brink: expected E065 on unused param `x`: {:?}",
            analysis.diagnostics
        );

        // The compile entry point (`compile_project`) reads the same
        // session's `analysis_options`, so the wasm-observable compile path
        // sees the strict-mode diagnostic too, not just background analysis.
        // E065 is error-severity, so compilation fails (`ok: false`).
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(v["ok"], false, "{result}");
        let has_unknown_escape = v["warnings"].as_array().is_some_and(|ws| {
            ws.iter()
                .any(|w| w["message"].as_str().unwrap_or("").contains("Unknown"))
        });
        assert!(
            has_unknown_escape,
            "compile_project should surface the strict-mode E065 diagnostic too: {result}"
        );
    }

    /// #660: any value other than the exact string "strict" (including an
    /// explicit "gradual") keeps the `Gradual` default.
    #[test]
    fn set_type_policy_gradual_value_keeps_default() {
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.set_type_policy("gradual");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "explicit gradual: no E065: {:?}",
            analysis.diagnostics
        );
    }

    // ── Project config file (#1005) ────────────────────────────────────

    #[test]
    fn apply_project_config_applies_dialect_from_file() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s
            .apply_project_config("[project]\ndialect = \"brink\"\n")
            .expect("valid brink.toml");
        assert_eq!(warnings, "[]");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "brink.toml's dialect = brink: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_applies_types_from_file() {
        let mut s = EditorSession::new();
        s.apply_project_config("[project]\ndialect = \"brink\"\ntypes = \"strict\"\n")
            .expect("valid brink.toml");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink.toml's types = strict: expected E065 on unused param `x`: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_no_file_values_leaves_defaults() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s.apply_project_config("").expect("empty document is valid");
        assert_eq!(warnings, "[]");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "no [project] table: StrictInk default stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_after_explicit_set_language_dialect_is_a_no_op_for_dialect() {
        // #1005 precedence: an explicit `set_language_dialect` call always
        // wins over a later `apply_project_config` — the file only ever
        // supplies a default.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("strict-ink");
        s.apply_project_config("[project]\ndialect = \"brink\"\n")
            .expect("valid brink.toml");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "explicit set_language_dialect(\"strict-ink\") must survive a later \
             apply_project_config(dialect = brink): {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_before_explicit_set_language_dialect_lets_explicit_win() {
        // The opposite ordering: config first (typical embedder flow — load
        // at mount time), then an explicit call after — last-write-wins,
        // same as the CLI's flag always overriding the file.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.apply_project_config("[project]\ndialect = \"brink\"\n")
            .expect("valid brink.toml");
        s.set_language_dialect("strict-ink");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "a later explicit set_language_dialect must win: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_reports_unknown_keys_as_warnings() {
        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[project]\ndialect = \"brink\"\nfuture_key = \"x\"\n")
            .expect("unknown keys are warnings, not errors");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].contains("project.future_key"));
    }

    // ── Issue #1004: manifest-typed external params under strict ──────────
    //
    // The maintainer's exact repro (issue #1004 final comment): a host
    // manifest typing an `EXTERNAL`'s param must make `compile_project`'s
    // warnings channel clean under `dialect = brink, types = strict`. Before
    // the fix the compile-path strict pass never escape-checked external
    // declarations at all, so the guard below (a genuinely-unresolvable
    // manifest `ty` still reports) pins that the clean result is real
    // consumption of the manifest signature, not blanket suppression.

    /// A manifest whose `TypeRef`s type an external's params resolves them in
    /// strict inference — `compile_project().warnings` is EMPTY, and the story
    /// still compiles (`ok: true`).
    #[test]
    fn issue_1004_manifest_typed_external_param_is_clean_under_strict() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n",
        );
        s.set_host_manifest(
            r#"{ "types": [{ "name": "thing_id", "base": "int", "values": { "source": "host" } }],
                "externals": [{ "name": "get_thing", "params": [{ "name": "id", "ty": "thing_id" }], "returns": "float", "kind": "query" }] }"#,
        )
        .expect("valid manifest");
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(
            v["ok"], true,
            "manifest-typed external must compile: {result}"
        );
        let warnings = v["warnings"].as_array().expect("warnings array");
        assert!(
            warnings.is_empty(),
            "a manifest-typed external param must not escape under strict: {result}"
        );
    }

    /// The don't-over-suppress guard: a *registered* external whose
    /// `ManifestParam.ty` fails to resolve (empty `ty`) still escapes — the
    /// warnings channel is NON-empty, the wire object carries the `E065`
    /// `code`, and its span anchors at the external's own declaration (the
    /// `get_thing` name in `EXTERNAL get_thing(id)`, bytes 9..18), not an
    /// arbitrary fixed line.
    #[test]
    fn issue_1004_unresolvable_external_param_escapes_with_code_and_range() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n",
        );
        s.set_host_manifest(
            r#"{ "externals": [{ "name": "get_thing", "params": [{ "name": "id", "ty": "" }], "returns": "float", "kind": "query" }] }"#,
        )
        .expect("valid manifest");
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let warnings = v["warnings"].as_array().expect("warnings array");
        let escape = warnings
            .iter()
            .find(|w| w["code"] == "E065")
            .expect("expected an E065 escape from the unresolvable external param");
        assert!(
            escape["message"]
                .as_str()
                .unwrap_or_default()
                .contains("get_thing"),
            "escape must name the offending external: {result}"
        );
        // Anchored at the `get_thing` name in `EXTERNAL get_thing(id)`, not
        // line 1 / a shared fixed span.
        assert_eq!(
            escape["start"], 9,
            "escape anchors at the decl span: {result}"
        );
        assert_eq!(
            escape["end"], 18,
            "escape anchors at the decl span: {result}"
        );
    }

    /// An `EXTERNAL` with no manifest entry at all stays entirely unchecked
    /// under strict (the deliberate "unregistered external = unchecked"
    /// posture): there is no in-language way to type its bare-identifier
    /// params, so this never emits an unactionable escape.
    #[test]
    fn issue_1004_unregistered_external_stays_unchecked_under_strict() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n",
        );
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(
            v["ok"], true,
            "unregistered external must compile: {result}"
        );
        assert!(
            !v["warnings"]
                .as_array()
                .expect("warnings")
                .iter()
                .any(|w| w["code"] == "E065"),
            "an unregistered external's params must stay unchecked: {result}"
        );
    }
}

// ── Dialect (#368) wasm-only error-path tests ─────────────────────────
//
// `set_dialect`'s rejection path constructs a `JsError`, which panics on a
// non-wasm target (`wasm-bindgen`'s "cannot call wasm-bindgen imported
// functions on non-wasm targets") — the same constraint every other
// `Result<_, JsError>` method in this file has. Acceptance-path coverage
// lives in the native `mod tests` above; rejection coverage lives here.
#[cfg(all(test, target_arch = "wasm32"))]
mod dialect_wasm_tests {
    use super::EditorSession;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn set_dialect_rejects_invalid_json() {
        let mut s = EditorSession::new();
        assert!(s.set_dialect("{ not valid }").is_err());
    }

    #[wasm_bindgen_test]
    fn set_dialect_rejects_undeclared_chain_kind() {
        let mut s = EditorSession::new();
        // Corrupt the preset: reference a kind nothing declares.
        let json_str =
            serde_json::to_string(&brink_ir::DialogueDialect::default()).expect("serializes");
        let mut value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        value["chain"][0]["after"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String("nonexistent".to_owned()));
        assert!(s.set_dialect(&value.to_string()).is_err());
    }

    #[wasm_bindgen_test]
    fn apply_project_config_rejects_malformed_toml() {
        let mut s = EditorSession::new();
        assert!(s.apply_project_config("this is not [ valid toml").is_err());
    }

    #[wasm_bindgen_test]
    fn apply_project_config_rejects_invalid_dialect_value() {
        let mut s = EditorSession::new();
        assert!(
            s.apply_project_config("[project]\ndialect = \"sideways\"\n")
                .is_err()
        );
    }
}
