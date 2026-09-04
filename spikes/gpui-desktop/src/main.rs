//! SPIKE — a GPUI-native brink studio shell, Zed-style.
//!
//! Question under test: can the existing Rust analysis engine (`brink-ide` /
//! `brink-db`) be driven directly from a native GPUI window, with a real code
//! editor widget, WITHOUT the wasm + CodeMirror + React stack in between?
//!
//! What this proves out:
//! - a window with a file list, a code editor, a problems strip, a status bar
//! - semantic-token highlighting from `brink-ide` (no tree-sitter grammar)
//! - hover, completion and diagnostics wired straight into `IdeSession`
//! - per-keystroke re-analysis cost measured on the main thread
//!
//! What it deliberately does not do: save, undo/redo integration with the
//! session, multiple windows, the Player, or anything the studio shell spec
//! rules on. It is a probe, not a product.

mod binder;
mod icons;

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

use anyhow::Result;
use brink_ide::session::IdeSession;
use brink_ir::{FileId, LineIndex, TextRange};
use gpui::{
    App, Application, Bounds, Context, Entity, Focusable as _, IntoElement, Render, SharedString,
    Subscription, Task, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme, Root, TitleBar, h_flex,
    input::{
        CompletionProvider, Editor, EditorState, HoverProvider, InputEvent, InputHighlighter, Rope,
        RopeExt,
    },
    label::Label,
    v_flex,
};
use lsp_types as lsp;

// ── Project model ────────────────────────────────────────────────────

/// The analysis side: one `IdeSession` over every source file under `root`,
/// keyed root-relative with forward slashes (the same key convention the
/// compiler uses).
struct Project {
    root: PathBuf,
    files: Vec<String>,
    session: IdeSession,
    /// `[project] entry` from `brink.toml`, root-relative — the file the
    /// Binder marks with the brand icon.
    entry: Option<String>,
}

/// One knot (with its stitches) for the Binder's Structure mode — the shape
/// `brink_ide::document::document_symbols` produces, flattened to what the
/// tree needs.
#[derive(Clone, Debug)]
struct SymbolNode {
    name: String,
    start: usize,
    full_start: usize,
    full_end: usize,
    is_function: bool,
    children: Vec<SymbolNode>,
}

type Shared = Rc<RefCell<Project>>;

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "node_modules"
}

fn collect_sources(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !is_ignored_dir(&name) {
                collect_sources(&path, root, out);
            }
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if (ext == "brink" || ext == "ink")
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

impl Project {
    fn load(root: PathBuf) -> Result<Self> {
        let started = Instant::now();
        let mut files = Vec::new();
        collect_sources(&root, &root, &mut files);
        anyhow::ensure!(
            !files.is_empty(),
            "no .brink/.ink files under {}",
            root.display()
        );

        let mut session = IdeSession::new();
        for key in &files {
            let text = std::fs::read_to_string(root.join(key))?;
            session.update_source(key, text);
        }

        // brink.toml: same discovery walk the compiler/CLI use.
        let mut entry: Option<String> = None;
        let tree = brink_driver::RealFs::new(&root);
        let mut options = brink_analyzer::AnalysisOptions::default();
        if let Ok(Some(config_key)) =
            brink_project_config::discover_from_entry_in_tree(&tree, &files[0])
        {
            let config_path = root.join(&config_key);
            let text = std::fs::read_to_string(&config_path)?;
            let (config, warnings) =
                brink_project_config::parse_str_at(config_path.display().to_string(), &text)?;
            for w in &warnings {
                eprintln!("warning: [{}] {w}", config_path.display());
            }
            for w in options.apply_project_config(&config, false, false) {
                eprintln!("warning: [{}] {w}", config_path.display());
            }
            entry = config.entry.clone();
            eprintln!("config: {config_key}");
        }
        session.apply_analysis_options(&options);
        session.refresh_analysis();

        eprintln!(
            "loaded {} files from {} in {:.1} ms",
            files.len(),
            root.display(),
            started.elapsed().as_secs_f64() * 1e3
        );
        Ok(Self {
            root,
            files,
            session,
            entry,
        })
    }

    /// Bring the session up to date with the editor's rope, if it drifted,
    /// and re-establish analysis. Returns the analyze time in ms (0 if the
    /// text was already current).
    fn sync(&mut self, key: &str, text: &str) -> f64 {
        let current = self
            .session
            .file_id(key)
            .and_then(|id| self.session.source(id))
            .is_some_and(|s| s == text);
        if current {
            return 0.0;
        }
        let started = Instant::now();
        self.session.update_and_analyze(key, text.to_owned());
        started.elapsed().as_secs_f64() * 1e3
    }

    fn file_id(&self, key: &str) -> Option<FileId> {
        self.session.file_id(key)
    }

    fn diagnostics(&self, key: &str) -> Vec<lsp::Diagnostic> {
        let Some(id) = self.file_id(key) else {
            return Vec::new();
        };
        let Some(source) = self.session.source(id) else {
            return Vec::new();
        };
        let idx = LineIndex::new(source);
        let types = self.session.type_policy();
        let lints = self.session.lint_policy().clone();
        self.session
            .db()
            .diagnostics(id)
            .unwrap_or(&[])
            .iter()
            .filter_map(|d| {
                let sev = brink_analyzer::effective_severity(d.code, types, &lints)?;
                Some(lsp::Diagnostic {
                    range: to_lsp_range(d.range, &idx),
                    severity: Some(match sev {
                        brink_ir::Severity::Error => lsp::DiagnosticSeverity::ERROR,
                        brink_ir::Severity::Warning => lsp::DiagnosticSeverity::WARNING,
                        brink_ir::Severity::Info => lsp::DiagnosticSeverity::INFORMATION,
                        brink_ir::Severity::Hint => lsp::DiagnosticSeverity::HINT,
                    }),
                    code: Some(lsp::NumberOrString::String(d.code.as_str().to_owned())),
                    source: Some("brink".to_owned()),
                    message: d.message.clone(),
                    ..Default::default()
                })
            })
            .collect()
    }

    /// Every diagnostic as `(file, start byte, is_error)` — the Binder's
    /// mark inputs. Warnings and errors only: Info/Hint never mark.
    fn diagnostic_points(&self) -> Vec<(String, usize, bool)> {
        let types = self.session.type_policy();
        let lints = self.session.lint_policy().clone();
        let mut out = Vec::new();
        for key in &self.files {
            let Some(id) = self.session.file_id(key) else {
                continue;
            };
            for d in self.session.db().diagnostics(id).unwrap_or(&[]) {
                let Some(sev) = brink_analyzer::effective_severity(d.code, types, &lints) else {
                    continue;
                };
                let is_error = match sev {
                    brink_ir::Severity::Error => true,
                    brink_ir::Severity::Warning => false,
                    _ => continue,
                };
                out.push((key.clone(), usize::from(d.range.start()), is_error));
            }
        }
        out
    }

    /// The compile closure, as root-relative keys. Empty before the first
    /// analysis — which the Binder reads as "nothing to contradict" rather
    /// than "everything is out of scope".
    fn closure(&self) -> std::collections::HashSet<String> {
        self.session
            .compilation_closure_paths()
            .into_iter()
            .collect()
    }

    /// A file's knots and stitches for Structure mode.
    fn symbols(&self, key: &str) -> Vec<SymbolNode> {
        let Some(id) = self.session.file_id(key) else {
            return Vec::new();
        };
        let (Some(hir), Some(manifest), Some(source)) = (
            self.session.hir(id),
            self.session.manifest(id),
            self.session.source(id),
        ) else {
            return Vec::new();
        };
        brink_ide::document::document_symbols(hir, manifest, source)
            .into_iter()
            .map(|s| convert_symbol(&s))
            .collect()
    }
}

fn convert_symbol(symbol: &brink_ide::document::DocumentSymbol) -> SymbolNode {
    SymbolNode {
        name: symbol.name.clone(),
        start: usize::from(symbol.range.start()),
        full_start: usize::from(symbol.full_range.start()),
        full_end: usize::from(symbol.full_range.end()),
        is_function: symbol
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("function")),
        children: symbol.children.iter().map(convert_symbol).collect(),
    }
}

fn to_lsp_range(range: TextRange, idx: &LineIndex) -> lsp::Range {
    let (sl, sc) = idx.line_col(range.start());
    let (el, ec) = idx.line_col(range.end());
    lsp::Range {
        start: lsp::Position::new(sl, sc),
        end: lsp::Position::new(el, ec),
    }
}

// ── LSP-shaped providers over the session ────────────────────────────

/// Which file the editor currently shows; shared by every provider.
type ActiveKey = Rc<RefCell<String>>;

/// The syntax-highlighting seam.
///
/// **The spike's central finding about painting.** gpui-base's LSP
/// `DocumentRangeSemanticTokensProvider` looks like the natural home for
/// brink's semantic tokens, but the paint pass returns early when the editor
/// has no [`InputHighlighter`] (`element.rs`'s `highlight_lines`), so
/// semantic tokens alone never reach the screen — they only *layer over* a
/// highlighter that already exists. gpui-component builds one from
/// tree-sitter, and there is no `.brink` grammar.
///
/// So this is the real seam: a highlighter driven by brink's own CST +
/// analysis, with no tree-sitter grammar anywhere. `styles` must return
/// ordered, non-overlapping runs that FULLY COVER the asked range, with
/// `HighlightStyle::default()` in the gaps.
struct BrinkHighlighter {
    project: Shared,
    active: ActiveKey,
    /// Absolute byte ranges + theme token-type names, sorted, disjoint.
    tokens: Vec<(std::ops::Range<usize>, SharedString)>,
}

impl BrinkHighlighter {
    fn new(project: Shared, active: ActiveKey) -> Self {
        Self {
            project,
            active,
            tokens: Vec::new(),
        }
    }

    fn recompute(&mut self, source: &str) {
        let started = Instant::now();
        let key = self.active.borrow().clone();
        let mut project = self.project.borrow_mut();
        let analyze_ms = project.sync(&key, source);
        let Some(id) = project.file_id(&key) else {
            self.tokens.clear();
            return;
        };
        let session = &project.session;
        let Some(analysis) = session.analysis() else {
            self.tokens.clear();
            return;
        };
        let raw = if session.is_native(id) {
            session
                .syntax_root_native(id)
                .map_or_else(Vec::new, |root| {
                    brink_ide::semantic_tokens::semantic_tokens_native(source, &root, analysis, id)
                })
        } else {
            session.syntax_root(id).map_or_else(Vec::new, |root| {
                brink_ide::semantic_tokens::semantic_tokens(source, &root, analysis, id)
            })
        };

        let names = brink_ide::semantic_tokens::token_type_names();
        let idx = LineIndex::new(source);
        let mut out: Vec<(std::ops::Range<usize>, SharedString)> = Vec::with_capacity(raw.len());
        for token in &raw {
            let Some(name) = names.get(token.token_type as usize) else {
                continue;
            };
            let start: usize = usize::from(idx.offset(token.line, token.start_char));
            let end: usize = usize::from(idx.offset(token.line, token.start_char + token.length));
            if start < end {
                out.push((start..end, SharedString::from(*name)));
            }
        }
        out.sort_by_key(|(range, _)| range.start);
        // Keep the runs disjoint: a later token that overlaps the previous
        // one is dropped rather than allowed to split a run.
        let mut disjoint: Vec<(std::ops::Range<usize>, SharedString)> =
            Vec::with_capacity(out.len());
        for (range, name) in out {
            if disjoint
                .last()
                .is_some_and(|(prev, _)| prev.end > range.start)
            {
                continue;
            }
            disjoint.push((range, name));
        }
        self.tokens = disjoint;
        eprintln!(
            "highlight: {} tokens, analyze {analyze_ms:.2} ms, total {:.2} ms",
            self.tokens.len(),
            started.elapsed().as_secs_f64() * 1e3
        );
    }
}

impl InputHighlighter for BrinkHighlighter {
    fn language(&self) -> SharedString {
        SharedString::from("brink")
    }

    fn update(
        &mut self,
        _edit: Option<gpui_component::input::InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.recompute(&text.to_string());
    }

    fn styles(
        &self,
        range: &std::ops::Range<usize>,
        resolver: &dyn gpui_component::input::HighlightStyleResolver,
    ) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
        let mut runs: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
        let mut cursor = range.start;
        let lo = self.tokens.partition_point(|(r, _)| r.end <= range.start);
        for (token, name) in &self.tokens[lo..] {
            if token.start >= range.end {
                break;
            }
            let start = token.start.max(range.start);
            let end = token.end.min(range.end);
            if start > cursor {
                runs.push((cursor..start, gpui::HighlightStyle::default()));
            }
            let style = resolver.style(name).unwrap_or_default();
            runs.push((start..end, style));
            cursor = end;
        }
        if cursor < range.end {
            runs.push((cursor..range.end, gpui::HighlightStyle::default()));
        }
        runs
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<gpui_component::input::FoldRange> {
        Vec::new()
    }
}

struct BrinkHover {
    project: Shared,
    active: ActiveKey,
}

impl HoverProvider for BrinkHover {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<Option<lsp::Hover>>> {
        let key = self.active.borrow().clone();
        let source = text.to_string();
        let pos = text.offset_to_position(offset);
        let mut project = self.project.borrow_mut();
        project.sync(&key, &source);
        let Some(id) = project.file_id(&key) else {
            return Task::ready(Ok(None));
        };
        let session = &project.session;
        let Some(analysis) = session.analysis() else {
            return Task::ready(Ok(None));
        };
        let idx = LineIndex::new(&source);
        let text_offset = idx.offset(pos.line, pos.character);
        let project_files = session.db().file_metadata();
        let info = brink_ide::hover::hover(
            analysis,
            session.db(),
            id,
            &source,
            text_offset,
            &project_files,
        );
        Task::ready(Ok(info.map(|info| lsp::Hover {
            contents: lsp::HoverContents::Markup(lsp::MarkupContent {
                kind: lsp::MarkupKind::Markdown,
                value: brink_ide::hover::strip_link_refs(&info.content),
            }),
            range: info.range.map(|r| to_lsp_range(r, &idx)),
        })))
    }
}

struct BrinkCompletion {
    project: Shared,
    active: ActiveKey,
}

impl CompletionProvider for BrinkCompletion {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: lsp::CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<lsp::CompletionResponse>> {
        use brink_ide::{
            CompletionContext, cursor_scope, detect_completion_context, is_visible_in_context,
            ref_arg_root_prefix, stdlib_completions,
        };

        let key = self.active.borrow().clone();
        let source = text.to_string();
        let mut project = self.project.borrow_mut();
        project.sync(&key, &source);
        let session = &project.session;
        let Some(analysis) = session.analysis() else {
            return Task::ready(Ok(lsp::CompletionResponse::Array(Vec::new())));
        };

        let ctx = detect_completion_context(&source, offset);
        let scope = cursor_scope(&source, offset);
        let ref_root = ref_arg_root_prefix(&source, offset);
        let mut items = Vec::new();

        if let CompletionContext::DottedPath { ref knot } = ctx {
            let prefix = format!("{knot}.");
            for (name, ids) in &analysis.index.by_name {
                if let Some(suffix) = name.strip_prefix(&*prefix) {
                    for def_id in ids {
                        if let Some(info) = analysis.index.symbols.get(def_id) {
                            items.push(completion_item(info, Some(suffix.to_owned())));
                        }
                    }
                }
            }
            return Task::ready(Ok(lsp::CompletionResponse::Array(items)));
        }

        for info in analysis.index.symbols.values() {
            if !is_visible_in_context(&ctx, info, &scope) {
                continue;
            }
            if ref_root.is_some() && info.kind != brink_ir::SymbolKind::Variable {
                continue;
            }
            items.push(completion_item(info, None));
        }
        for f in stdlib_completions(&ctx, session.language_dialect()) {
            items.push(lsp::CompletionItem {
                label: f.name.to_owned(),
                kind: Some(lsp::CompletionItemKind::FUNCTION),
                detail: Some("stdlib".to_owned()),
                ..Default::default()
            });
        }
        if matches!(
            ctx,
            CompletionContext::Divert | CompletionContext::InlineExpr
        ) {
            for label in ["DONE", "END"] {
                items.push(lsp::CompletionItem {
                    label: label.to_owned(),
                    kind: Some(lsp::CompletionItemKind::KEYWORD),
                    detail: Some("built-in".to_owned()),
                    ..Default::default()
                });
            }
        }
        Task::ready(Ok(lsp::CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '>' | '@'))
    }
}

fn completion_item(info: &brink_ir::SymbolInfo, label: Option<String>) -> lsp::CompletionItem {
    use brink_ir::SymbolKind as K;
    let kind = match info.kind {
        K::Knot => lsp::CompletionItemKind::MODULE,
        K::Stitch => lsp::CompletionItemKind::METHOD,
        K::Variable | K::Param | K::Temp => lsp::CompletionItemKind::VARIABLE,
        K::Constant => lsp::CompletionItemKind::CONSTANT,
        K::List | K::ListItem => lsp::CompletionItemKind::ENUM,
        K::External => lsp::CompletionItemKind::FUNCTION,
        K::Label => lsp::CompletionItemKind::REFERENCE,
        K::Struct => lsp::CompletionItemKind::STRUCT,
    };
    lsp::CompletionItem {
        label: label.unwrap_or_else(|| info.name.clone()),
        kind: Some(kind),
        detail: Some(format!("{:?}", info.kind).to_lowercase()),
        ..Default::default()
    }
}

// ── The window ───────────────────────────────────────────────────────

struct Workspace {
    project: Shared,
    active: ActiveKey,
    active_index: usize,
    binder: Entity<binder::Binder>,
    editor: Entity<EditorState>,
    problems: Vec<(u32, String, String)>,
    last_analyze_ms: f64,
    worst_analyze_ms: f64,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Result<Self> {
        let project: Shared = Rc::new(RefCell::new(Project::load(root)?));
        let active: ActiveKey = Rc::new(RefCell::new(project.borrow().files[0].clone()));

        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .language("brink");
            // Installed BEFORE gpui-component's Input render, whose
            // `ensure_highlighter_factory` only fills an empty slot — so
            // this wins and the tree-sitter path is never consulted.
            let (hp, ha) = (project.clone(), active.clone());
            state.set_highlighter_factory(
                Rc::new(move |language| {
                    (language == "brink").then(|| {
                        Box::new(BrinkHighlighter::new(hp.clone(), ha.clone()))
                            as Box<dyn InputHighlighter>
                    })
                }),
                cx,
            );
            let lsp = state.lsp_mut();
            lsp.hover_provider = Some(Rc::new(BrinkHover {
                project: project.clone(),
                active: active.clone(),
            }));
            lsp.completion_provider = Some(Rc::new(BrinkCompletion {
                project: project.clone(),
                active: active.clone(),
            }));
            state
        });

        let editor_sub = cx.subscribe(&editor, |this, editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.reanalyze(&editor, cx);
            }
        });

        let binder = cx.new(|cx| binder::Binder::new(project.clone(), window, cx));
        let binder_sub = cx.subscribe_in(
            &binder,
            window,
            |this, binder, event: &binder::BinderEvent, window, cx| {
                let binder::BinderEvent::Open { path, offset } = event;
                this.open_path(path, *offset, window, cx);
                // Revealing an offset focuses the editor (`set_cursor_position`
                // does), which would kill the binder's own arrow-key
                // navigation after the first click. A panel click opens the
                // document but keeps focus in the panel — Zed's project-panel
                // behaviour, and the studio's.
                let handle = binder.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            },
        );

        let mut this = Self {
            project,
            active,
            active_index: 0,
            binder,
            editor,
            problems: Vec::new(),
            last_analyze_ms: 0.0,
            worst_analyze_ms: 0.0,
            _subscriptions: vec![editor_sub, binder_sub],
        };
        this.open(0, window, cx);
        Ok(this)
    }

    fn open(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let key = {
            let project = self.project.borrow();
            let Some(key) = project.files.get(index).cloned() else {
                return;
            };
            key
        };
        self.active_index = index;
        self.open_path(&key, None, window, cx);
    }

    /// Open a file and, for a symbol row, reveal its offset — what the
    /// Binder emits when a row is activated.
    fn open_path(
        &mut self,
        key: &str,
        offset: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `!key.is_empty()` guards the first open: `active` starts as the
        // first file's key so the providers have something to name before
        // anything is loaded, and a bare equality check would take that as
        // "already open" and never load the text.
        let already_open = !self.editor.read(cx).value().is_empty() && *self.active.borrow() == key;
        if !already_open {
            let text = {
                let project = self.project.borrow();
                project
                    .file_id(key)
                    .and_then(|id| project.session.source(id))
                    .unwrap_or("")
                    .to_owned()
            };
            *self.active.borrow_mut() = key.to_owned();
            if let Some(i) = self.project.borrow().files.iter().position(|f| f == key) {
                self.active_index = i;
            }
            let editor = self.editor.clone();
            editor.update(cx, |state, cx| state.set_value(text, window, cx));
            self.reanalyze(&editor, cx);
        }
        if let Some(offset) = offset {
            let editor = self.editor.clone();
            editor.update(cx, |state, cx| {
                let position = state.text().offset_to_position(offset);
                state.set_cursor_position(position, window, cx);
            });
        }
    }

    fn reanalyze(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let key = self.active.borrow().clone();
        let (rope, text) = {
            let state = editor.read(cx);
            (state.text().clone(), state.value().to_string())
        };
        let (ms, diagnostics) = {
            let mut project = self.project.borrow_mut();
            let ms = project.sync(&key, &text);
            (ms, project.diagnostics(&key))
        };
        if ms > 0.0 {
            self.last_analyze_ms = ms;
            self.worst_analyze_ms = self.worst_analyze_ms.max(ms);
            eprintln!("analyze: {ms:.2} ms ({} diagnostics)", diagnostics.len());
        }
        self.problems = diagnostics
            .iter()
            .map(|d| {
                let code = match &d.code {
                    Some(lsp::NumberOrString::String(s)) => s.clone(),
                    _ => String::new(),
                };
                (d.range.start.line, code, d.message.clone())
            })
            .collect();
        editor.update(cx, |state, cx| {
            if let Some(set) = state.diagnostics_mut() {
                set.reset(&rope);
                set.extend(diagnostics);
            }
            cx.notify();
        });
        cx.notify();
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let files = self.project.borrow().files.clone();

        let sidebar = div().w(px(260.)).h_full().child(self.binder.clone());

        let problems = v_flex()
            .h(px(140.))
            .px_2()
            .py_1()
            .gap_0p5()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(format!("Problems ({})", self.problems.len())),
            )
            .children(self.problems.iter().take(6).map(|(line, code, message)| {
                div().text_color(theme.foreground).child(format!(
                    "{}:{}  {code}  {message}",
                    self.active.borrow(),
                    line + 1
                ))
            }));

        let status = h_flex()
            .h(px(24.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(theme.sidebar)
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(self.project.borrow().root.display().to_string())
            .child(format!("{} files", files.len()))
            .child(format!("analyze {:.1} ms", self.last_analyze_ms))
            .child(format!("worst {:.1} ms", self.worst_analyze_ms))
            .child(SharedString::from(
                "GPUI spike — semantic tokens / hover / completion live",
            ));

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(TitleBar::new().child(Label::new("brink — GPUI spike")))
            .child(
                h_flex().flex_1().min_h_0().child(sidebar).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(Editor::new(&self.editor).flex_1().bordered(false))
                        .child(problems),
                ),
            )
            .child(status)
    }
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../tests/tier1-native/conventions-cross-file"));
    let root = root.canonicalize().unwrap_or(root);

    // gpui-pre publishes the core without a platform backend; the macOS/
    // Windows/Linux implementations live in `gpui-pre-platform`.
    Application::with_platform(gpui_platform::current_platform(false)).run(move |cx| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..TitleBar::window_options()
        };
        let root = root.clone();
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| match Workspace::new(root, window, cx) {
                Ok(ws) => ws,
                Err(err) => {
                    eprintln!("failed to open project: {err:#}");
                    std::process::exit(2);
                }
            });
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            eprintln!("failed to open window: {err:#}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
