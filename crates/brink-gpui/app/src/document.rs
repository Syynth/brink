//! One open file: its editor, its highlighter, its providers.
//!
//! **A document owns its own path.** The spike routed everything through a
//! shared `ActiveKey` (`Rc<RefCell<String>>`) that the single editor and
//! every provider read at call time; tabs were then impossible without
//! coordinating four readers. Here each `Document` constructs providers
//! against *itself*, so a second tab is a second entity and nothing has to
//! agree about which file is current.
//!
//! Nothing in this file touches an `IdeSession`. Paint comes from the
//! per-segment [`TokenCache`]; everything else is a worker query.

use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use brink_gpui_model::query::{Completion, CompletionKind, QueryKind, QueryResult};
use brink_gpui_model::tokens::TokenCache;
use brink_ir::LineIndex;
use gpui::EntityInputHandler as _;
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, SharedString, Subscription, Task, WeakEntity, Window,
};
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{PanelId, TabGroup};
use gpui_component::input::{
    CompletionProvider, EditorState, HoverProvider, Inlay, InputEvent, InputHighlighter, Rope,
    RopeExt as _,
};
use lsp_types as lsp;

use crate::project::{Project, ProjectEvent, SourceDelta};

/// Apply another editor's change to this `EditorState` in place, keeping
/// its caret, scroll and undo history — the view following the buffer.
///
/// The delta is checked against what this editor holds; an editor that has
/// somehow fallen out of step is resynced wholesale from `fallback` (the
/// caret is lost, the text never is). Offsets are converted to UTF-16 for
/// the input handler, which speaks that unit.
pub(crate) fn apply_delta(
    state: &mut EditorState,
    delta: &SourceDelta,
    fallback: &str,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    let current = state.value();
    if current.get(delta.range.clone()) == Some(delta.removed.as_str()) {
        let start16 = current[..delta.range.start].encode_utf16().count();
        let end16 = start16 + delta.removed.encode_utf16().count();
        state.replace_text_in_range(Some(start16..end16), &delta.inserted, window, cx);
    } else {
        state.set_value(fallback.to_owned(), window, cx);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DocumentEvent {
    /// The dock made this the displayed tab of its group — what Code view
    /// takes as "the active document" (`code_view.rs`).
    Activated,
    /// The tab was closed; the document has left the dock for good.
    Closed,
}

pub struct Document {
    path: SharedString,
    editor: Entity<EditorState>,
    project: Entity<Project>,
    /// The tab group holding this document, from the dock's `on_added_to`.
    /// What [`Document::activate`] selects the tab through.
    group: Option<WeakEntity<TabGroup>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DocumentEvent> for Document {}

impl Document {
    pub fn new(
        project: Entity<Project>,
        path: impl Into<SharedString>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = path.into();
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .language("brink");

            // Installed BEFORE gpui-component's Input render, whose
            // `ensure_highlighter_factory` only fills an empty slot — so
            // this wins and the tree-sitter path is never consulted.
            let (weak, key) = (project.downgrade(), path.clone());
            state.set_highlighter_factory(
                Rc::new(move |language| {
                    (language == "brink").then(|| {
                        Box::new(BrinkHighlighter::new(weak.clone(), key.clone()))
                            as Box<dyn InputHighlighter>
                    })
                }),
                cx,
            );

            let lsp = state.lsp_mut();
            lsp.hover_provider = Some(Rc::new(BrinkHover {
                project: project.downgrade(),
                path: path.clone(),
            }));
            lsp.completion_provider = Some(Rc::new(BrinkCompletion {
                project: project.downgrade(),
                path: path.clone(),
            }));
            state.set_value(text, window, cx);
            state
        });

        let on_change = cx.subscribe(&editor, |this, editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_edited(&editor, cx);
            }
        });
        let on_project = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| match event {
                // Diagnostics and inlays are analysis products, so they
                // arrive with the analysis rather than being pulled on a
                // timer.
                ProjectEvent::Analyzed => this.refresh(cx),
                // Another editor over this file moved the text; follow it.
                ProjectEvent::SourceChanged {
                    path,
                    origin,
                    delta,
                } if path.as_str() == this.path.as_ref()
                    && *origin != Some(this.editor.entity_id()) =>
                {
                    let fallback = this
                        .project
                        .read(cx)
                        .loaded_source(path)
                        .unwrap_or_default()
                        .to_owned();
                    this.editor.update(cx, |state, cx| {
                        apply_delta(state, delta, &fallback, window, cx);
                    });
                }
                // The tab's unsaved marker reads the project.
                ProjectEvent::Saved => cx.notify(),
                _ => {}
            },
        );

        let this = Self {
            path,
            editor,
            project,
            group: None,
            _subscriptions: vec![on_change, on_project],
        };
        // The editor may normalise what it was given (line endings); if it
        // did, that is an edit like any other.
        let seed = this.editor.read(cx).value().to_string();
        let path = this.path.clone();
        let origin = this.editor.entity_id();
        this.project.update(cx, |project, cx| {
            project.edit(&path, seed, Some(origin), cx);
        });
        this
    }

    #[must_use]
    pub fn path(&self) -> &SharedString {
        &self.path
    }

    /// Whether the file differs from disk — a fact about the file, read from
    /// the project, so every editor over it agrees.
    #[must_use]
    pub fn is_dirty(&self, cx: &App) -> bool {
        self.project.read(cx).is_dirty(&self.path)
    }

    /// Put the caret at a span's start, select the span, and focus the
    /// editor — `docs/studio-shell-spec.md` §6.1's `editor.reveal`, with the
    /// selection standing in for the studio's flash-highlight. An empty
    /// span (what a Binder row emits) just places the caret.
    pub fn reveal(&self, span: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| {
            let position = state.text().offset_to_position(span.start);
            state.set_cursor_position(position, window, cx);
            if span.end > span.start {
                state.set_selected_range(span, cx);
            }
        });
    }

    /// Make this the displayed tab of its group — opening a file that is
    /// already open. A no-op until the dock has placed it.
    pub fn activate(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.group.as_ref() else {
            return;
        };
        let me = PanelId::from(cx.entity().entity_id());
        _ = group.update(cx, |group, cx| {
            let ix = group
                .panels()
                .iter()
                .position(|panel| panel.panel_id(cx) == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        });
    }

    fn on_edited(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let text = editor.read(cx).value().to_string();
        let path = self.path.clone();
        let origin = editor.entity_id();
        self.project.update(cx, |project, cx| {
            project.edit(&path, text, Some(origin), cx);
        });
        // Dirty is the project's to say; the tab reads it when it draws.
        cx.notify();
    }

    /// Fold the analysis the worker just published into the editor.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (rope, source) = {
            let state = self.editor.read(cx);
            (state.text().clone(), state.value().to_string())
        };
        let index = LineIndex::new(&source);
        let diagnostics: Vec<lsp::Diagnostic> = self
            .project
            .read(cx)
            .diagnostics_for(&self.path)
            .iter()
            .map(|d| to_lsp_diagnostic(d, &index))
            .collect();

        self.editor.update(cx, |state, cx| {
            if let Some(set) = state.diagnostics_mut() {
                set.reset(&rope);
                set.extend(diagnostics);
            }
            cx.notify();
        });

        // Inlays are a query rather than part of the analysis broadcast:
        // computing them for every file on every keystroke would be
        // O(project) for the sake of files nobody has open.
        let query = self.project.read(cx).query(
            QueryKind::InlayHints {
                path: self.path.to_string(),
            },
            cx,
        );
        let editor = self.editor.clone();
        let hint_color = cx.theme().muted_foreground;
        let hint_bg = cx.theme().muted.opacity(0.7);
        cx.spawn(async move |_, cx| {
            let Ok(QueryResult::InlayHints(hints)) = query.await else {
                return;
            };
            let inlays: Vec<Inlay> = hints
                .into_iter()
                .map(|h| Inlay {
                    offset: h.offset as usize,
                    text: h.label.into(),
                    style: gpui::HighlightStyle {
                        color: Some(hint_color),
                        background_color: Some(hint_bg),
                        ..Default::default()
                    },
                    swatch: None,
                })
                .collect();
            editor.update(cx, |state, cx| state.set_inlays(inlays, cx));
        })
        .detach();
    }
}

fn to_lsp_diagnostic(
    d: &brink_gpui_model::worker::Diagnostic,
    index: &LineIndex,
) -> lsp::Diagnostic {
    let at = |offset: u32| {
        let (line, character) = index.line_col(rowan::TextSize::from(offset));
        lsp::Position { line, character }
    };
    lsp::Diagnostic {
        range: lsp::Range {
            start: at(d.start),
            end: at(d.end),
        },
        severity: Some(match d.severity {
            brink_ir::Severity::Error => lsp::DiagnosticSeverity::ERROR,
            brink_ir::Severity::Warning => lsp::DiagnosticSeverity::WARNING,
            brink_ir::Severity::Info => lsp::DiagnosticSeverity::INFORMATION,
            brink_ir::Severity::Hint => lsp::DiagnosticSeverity::HINT,
        }),
        code: Some(lsp::NumberOrString::String(d.code.clone())),
        message: d.message.clone(),
        ..Default::default()
    }
}

// ── Paint ────────────────────────────────────────────────────────────

/// The paint seam. `gpui-base` computes semantic tokens through an
/// LSP-shaped provider and then paints *nothing* with them: the paint pass
/// returns early when the editor has no `InputHighlighter`, and
/// `gpui-component` only ever builds one from tree-sitter. Semantic tokens
/// layer over a highlighter; they cannot be the only source.
///
/// So this is the real seam — driven by brink's own CST, with no
/// tree-sitter grammar anywhere. `styles` must return ordered,
/// non-overlapping runs that FULLY COVER the asked range, with
/// `HighlightStyle::default()` in the gaps.
pub struct BrinkHighlighter {
    project: WeakEntity<Project>,
    path: SharedString,
    cache: TokenCache,
    /// Absolute byte ranges + theme token-type names, sorted, disjoint.
    runs: Vec<(std::ops::Range<usize>, SharedString)>,
}

impl BrinkHighlighter {
    /// One highlighter per open view of a file. The Continuous view builds
    /// its own per section, which is why this is not private: every section
    /// is a different file on screen at once, so a highlighter that followed
    /// "the active file" would paint them all the same.
    pub fn new(project: WeakEntity<Project>, path: SharedString) -> Self {
        let cache = TokenCache::new(&path);
        Self {
            project,
            path,
            cache,
            runs: Vec::new(),
        }
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
        cx: &mut Context<EditorState>,
    ) {
        let source = text.to_string();
        let Some(project) = self.project.upgrade() else {
            self.runs.clear();
            return;
        };
        // The `kinds` join may lag the source by an analysis. That is the
        // one staleness this design permits, and it costs refinement only:
        // an identifier not yet known to name a knot still paints as an
        // identifier, and every structural token is decided by the parse
        // that just ran.
        let raw = {
            let project = project.read(cx);
            self.cache.update(&source, project.kinds_for(&self.path))
        };

        let names = brink_ir::semantic_tokens::token_type_names();
        let index = LineIndex::new(&source);
        let mut runs: Vec<(std::ops::Range<usize>, SharedString)> = Vec::with_capacity(raw.len());
        for token in &raw {
            let Some(name) = names.get(token.token_type as usize) else {
                continue;
            };
            let start = usize::from(index.offset(token.line, token.start_char));
            let end = usize::from(index.offset(token.line, token.start_char + token.length));
            if start < end {
                runs.push((start..end, SharedString::from(*name)));
            }
        }
        runs.sort_by_key(|(range, _)| range.start);
        // Keep the runs disjoint: a later token overlapping the previous one
        // is dropped rather than allowed to split a run.
        let mut disjoint: Vec<(std::ops::Range<usize>, SharedString)> =
            Vec::with_capacity(runs.len());
        for (range, name) in runs {
            if disjoint
                .last()
                .is_some_and(|(prev, _)| prev.end > range.start)
            {
                continue;
            }
            disjoint.push((range, name));
        }
        self.runs = disjoint;
    }

    fn styles(
        &self,
        range: &std::ops::Range<usize>,
        resolver: &dyn gpui_component::input::HighlightStyleResolver,
    ) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
        let mut out: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
        let mut cursor = range.start;
        let lo = self.runs.partition_point(|(r, _)| r.end <= range.start);
        for (token, name) in &self.runs[lo..] {
            if token.start >= range.end {
                break;
            }
            let start = token.start.max(range.start);
            let end = token.end.min(range.end);
            if start > cursor {
                out.push((cursor..start, gpui::HighlightStyle::default()));
            }
            out.push((start..end, resolver.style(name).unwrap_or_default()));
            cursor = end;
        }
        if cursor < range.end {
            out.push((cursor..range.end, gpui::HighlightStyle::default()));
        }
        out
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<gpui_component::input::FoldRange> {
        Vec::new()
    }
}

// ── Providers ────────────────────────────────────────────────────────

struct BrinkHover {
    project: WeakEntity<Project>,
    path: SharedString,
}

impl HoverProvider for BrinkHover {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<lsp::Hover>>> {
        let Some(project) = self.project.upgrade() else {
            return Task::ready(Ok(None));
        };
        let source = text.to_string();
        let query = project.read(cx).query(
            QueryKind::Hover {
                path: self.path.to_string(),
                offset: u32::try_from(offset).unwrap_or(u32::MAX),
            },
            cx,
        );
        cx.background_spawn(async move {
            let QueryResult::Hover(Some(info)) = query.await? else {
                return Ok(None);
            };
            let index = LineIndex::new(&source);
            Ok(Some(lsp::Hover {
                contents: lsp::HoverContents::Markup(lsp::MarkupContent {
                    kind: lsp::MarkupKind::Markdown,
                    value: info.markdown,
                }),
                range: info.range.map(|(start, end)| lsp::Range {
                    start: position(&index, start),
                    end: position(&index, end),
                }),
            }))
        })
    }
}

struct BrinkCompletion {
    project: WeakEntity<Project>,
    path: SharedString,
}

impl CompletionProvider for BrinkCompletion {
    fn completions(
        &self,
        _text: &Rope,
        offset: usize,
        _trigger: lsp::CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<lsp::CompletionResponse>> {
        let Some(project) = self.project.upgrade() else {
            return Task::ready(Ok(lsp::CompletionResponse::Array(Vec::new())));
        };
        let query = project.read(cx).query(
            QueryKind::Completions {
                path: self.path.to_string(),
                offset: u32::try_from(offset).unwrap_or(u32::MAX),
            },
            cx,
        );
        cx.background_spawn(async move {
            let QueryResult::Completions(items) = query.await? else {
                return Ok(lsp::CompletionResponse::Array(Vec::new()));
            };
            Ok(lsp::CompletionResponse::Array(
                items.iter().map(completion_item).collect(),
            ))
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '>' | '@'))
    }
}

fn position(index: &LineIndex, offset: u32) -> lsp::Position {
    let (line, character) = index.line_col(rowan::TextSize::from(offset));
    lsp::Position { line, character }
}

/// The one place brink's kinds are mapped onto LSP's, so the two vocabularies
/// meet exactly once.
fn completion_item(item: &Completion) -> lsp::CompletionItem {
    use brink_ir::SymbolKind as K;
    let (kind, detail) = match item.kind {
        CompletionKind::Symbol(symbol) => (
            match symbol {
                K::Knot => lsp::CompletionItemKind::MODULE,
                K::Stitch => lsp::CompletionItemKind::METHOD,
                K::Variable | K::Param | K::Temp => lsp::CompletionItemKind::VARIABLE,
                K::Constant => lsp::CompletionItemKind::CONSTANT,
                K::List | K::ListItem => lsp::CompletionItemKind::ENUM,
                K::External => lsp::CompletionItemKind::FUNCTION,
                K::Label => lsp::CompletionItemKind::REFERENCE,
                K::Struct => lsp::CompletionItemKind::STRUCT,
            },
            format!("{symbol:?}").to_lowercase(),
        ),
        CompletionKind::StdlibFunction => (lsp::CompletionItemKind::FUNCTION, "stdlib".to_owned()),
        CompletionKind::Builtin => (lsp::CompletionItemKind::KEYWORD, "built-in".to_owned()),
    };
    lsp::CompletionItem {
        label: item.label.clone(),
        kind: Some(kind),
        detail: Some(detail),
        ..Default::default()
    }
}

// ── The document as a dock panel ─────────────────────────────────────
//
// A `Document` IS the panel, rather than being hosted by an editor panel
// that owns a tab strip of its own. The dock's `TabPanel` already gives
// tabs, drag-between-groups and splits, so a second open file is simply a
// second panel in the centre — which is Zed's `Item` model, and the reason
// the document had to own its path first.

impl gpui::Focusable for Document {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl EventEmitter<gpui_component::dock::PanelEvent> for Document {}

impl gpui_component::dock::BasePanel for Document {
    fn panel_name(&self) -> &'static str {
        "Document"
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            cx.emit(DocumentEvent::Activated);
        }
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }

    fn on_removed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.group = None;
        cx.emit(DocumentEvent::Closed);
    }
}

impl gpui_component::dock::Panel for Document {
    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let name = self
            .path
            .rsplit('/')
            .next()
            .unwrap_or(self.path.as_ref())
            .to_owned();
        // The unsaved marker is the tab's own affordance; a separate dot
        // elsewhere would be a second place to keep in step.
        SharedString::from(if self.is_dirty(cx) {
            format!("{name} •")
        } else {
            name
        })
    }

    /// The editor runs edge to edge under its tab, as an editor does.
    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl gpui::Render for Document {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        gpui_component::v_flex().size_full().child(
            gpui_component::input::Editor::new(&self.editor)
                .flex_1()
                .bordered(false),
        )
    }
}
