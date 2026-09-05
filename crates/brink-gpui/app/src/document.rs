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
    CompletionProvider, EditorState, HoverProvider, Inlay, InputEvent, InputHighlighter,
    InputHighlighterFactory, Rope, RopeExt as _,
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
    /// The highlighter's factory, kept so a theme change can reinstall it:
    /// the highlighter snapshots the TODO band's colours when it updates.
    /// `None` for `brink.toml`, which the kit's own TOML highlighter paints.
    factory: Option<InputHighlighterFactory>,
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
        // `brink.toml` is a document like any other (the maintainer's call
        // for the native studio, 2026-09-05 — unlike the web studio, which
        // routes it to Settings): the same tab, the same shared buffer, the
        // worker re-applying the config on each edit. What differs is the
        // language: it is TOML, so the kit's own highlighter paints it and
        // brink's hover, completion and inlays stay out of it.
        let config = project.read(cx).is_config(&path);
        let factory = (!config).then(|| highlighter_factory(project.downgrade(), path.clone()));
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(brink_gpui_shell::settings::AppSettings::get(cx).show_gutters)
                .language(if config { "toml" } else { "brink" });

            if let Some(factory) = &factory {
                // Installed BEFORE gpui-component's Input render, whose
                // `ensure_highlighter_factory` only fills an empty slot — so
                // this wins and the tree-sitter path is never consulted.
                state.set_highlighter_factory(factory.clone(), cx);

                let origin = cx.entity().entity_id();
                let lsp = state.lsp_mut();
                lsp.hover_provider = Some(Rc::new(BrinkHover {
                    project: project.downgrade(),
                    path: path.clone(),
                    origin,
                }));
                lsp.completion_provider = Some(Rc::new(BrinkCompletion {
                    project: project.downgrade(),
                    path: path.clone(),
                    origin,
                }));
            }
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

        // The highlighter snapshots the theme's TODO-band colours when it
        // updates, so a theme switch reinstalls it (one reparse of the file,
        // on a switch — nothing per keystroke).
        let on_theme = cx.observe_global::<gpui_component::Theme>(|this, cx| {
            let Some(factory) = this.factory.clone() else {
                return;
            };
            this.editor.update(cx, |state, cx| {
                state.set_highlighter_factory(factory, cx);
            });
        });

        // The gutter and inlay toggles are settings; every open editor
        // follows them live.
        let on_settings = cx.observe_global_in::<brink_gpui_shell::settings::AppSettings>(
            window,
            |this, window, cx| {
                let show = brink_gpui_shell::settings::AppSettings::get(cx).show_gutters;
                this.editor.update(cx, |state, cx| {
                    state.set_line_number(show, window, cx);
                });
                this.refresh(cx);
            },
        );

        let this = Self {
            path,
            editor,
            project,
            factory,
            group: None,
            _subscriptions: vec![on_change, on_project, on_theme, on_settings],
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
    ///
    /// Not a method run inside the document's own `update`: selecting the
    /// tab makes the dock read the panel back (`PanelView::visible`, on
    /// the way to focusing it), and a read during the entity's own update
    /// is a panic. It went unnoticed while every reveal landed on the tab
    /// already showing — `select_tab` returns early then — and surfaced
    /// the first time a Binder click reached a tab behind another.
    pub fn activate(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let Some(group) = this.read(cx).group.clone() else {
            return;
        };
        let me = PanelId::from(this.entity_id());
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
            // A TODO note's band IS its presentation in the editor; a
            // squiggle under it would double-mark it (the studio does the
            // same). It still reaches Problems and TODOs.
            .filter(|d| d.code != crate::todos::TODO_CODE)
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
        // O(project) for the sake of files nobody has open. The config has
        // none to ask for.
        if self.factory.is_none()
            || !brink_gpui_shell::settings::AppSettings::get(cx).show_inlay_hints
        {
            self.editor
                .update(cx, |state, cx| state.set_inlays(Vec::new(), cx));
            return;
        }
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
    /// Every `TODO:` line — the studio's `.brink-todo` band (ruled
    /// 2026-08-23, "Inky-grade visibility"): the whole line in the theme's
    /// `todo_band` with `todo_ink` text, its keyword bold. Laid here, over
    /// the token colours, because the two would otherwise race: the
    /// editor composes decoration and syntax colours through an unordered
    /// set, and the band must win on every word.
    todo_lines: Vec<TodoLine>,
    band: (gpui::Hsla, gpui::Hsla),
}

/// One `TODO:` line: its full extent and its `TODO:` keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TodoLine {
    pub line: Range<usize>,
    pub keyword: Range<usize>,
}

/// The highlighter every editor over a file installs — Code view's tab,
/// a manuscript section, a Search card. One place, so a change to what a
/// highlighter needs (today: the theme's band colours) reaches all three.
pub(crate) fn highlighter_factory(
    project: WeakEntity<Project>,
    path: SharedString,
) -> InputHighlighterFactory {
    Rc::new(move |language| {
        (language == "brink").then(|| {
            Box::new(BrinkHighlighter::new(project.clone(), path.clone()))
                as Box<dyn InputHighlighter>
        })
    })
}

/// The lines holding `TODO:` notes, from the notes' byte ranges: each
/// widened to its whole line, with the `TODO` keyword (and the colon that
/// follows it) located for the bold.
pub(crate) fn todo_lines(source: &str, notes: &[Range<usize>]) -> Vec<TodoLine> {
    let mut out: Vec<TodoLine> = Vec::new();
    for note in notes {
        let start = note.start.min(source.len());
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[start..]
            .find('\n')
            .map_or(source.len(), |i| start + i);
        if line_start >= line_end || out.last().is_some_and(|l| l.line.start == line_start) {
            continue;
        }
        let line = &source[line_start..line_end];
        let keyword = match line.find("TODO") {
            Some(at) => {
                let word = line_start + at;
                let mut end = word + 4;
                let trimmed = source[end..line_end].trim_start();
                if trimmed.starts_with(':') {
                    end = line_end - trimmed.len() + 1;
                }
                word..end
            }
            None => line_start..line_start,
        };
        out.push(TodoLine {
            line: line_start..line_end,
            keyword,
        });
    }
    out
}

/// Lay the band over already-styled runs: inside a TODO line every run
/// takes the ink colour on the band background, and the keyword goes bold.
/// Runs are split at the band's and the keyword's edges; nothing outside
/// a TODO line is touched.
pub(crate) fn overlay_todo(
    runs: Vec<(Range<usize>, gpui::HighlightStyle)>,
    todos: &[TodoLine],
    (band_bg, ink): (gpui::Hsla, gpui::Hsla),
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    if todos.is_empty() {
        return runs;
    }
    let mut out = Vec::with_capacity(runs.len());
    for (range, style) in runs {
        let mut cuts: Vec<usize> = vec![range.start, range.end];
        for t in todos {
            for at in [t.line.start, t.line.end, t.keyword.start, t.keyword.end] {
                if at > range.start && at < range.end {
                    cuts.push(at);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let piece = pair[0]..pair[1];
            let on = todos
                .iter()
                .find(|t| t.line.start <= piece.start && piece.end <= t.line.end);
            let mut style = style;
            if let Some(t) = on {
                style.color = Some(ink);
                style.background_color = Some(band_bg);
                if t.keyword.start <= piece.start && piece.end <= t.keyword.end {
                    style.font_weight = Some(gpui::FontWeight::BOLD);
                }
            }
            out.push((piece, style));
        }
    }
    out
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
            todo_lines: Vec::new(),
            band: (gpui::Hsla::default(), gpui::Hsla::default()),
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
        self.todo_lines = todo_lines(&source, &self.cache.todo_ranges());
        let tokens = brink_gpui_shell::theme::current(cx).tokens;
        self.band = (
            brink_gpui_shell::theme::hsla(tokens.todo_band),
            brink_gpui_shell::theme::hsla(tokens.todo_ink),
        );

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
            // Brink's roles ride Zed's names in the theme's table — the
            // mapping is the shell's (`theme::syntax_key`), one place.
            let style = brink_gpui_shell::theme::syntax_key(name)
                .and_then(|key| resolver.style(key))
                .unwrap_or_default();
            out.push((start..end, style));
            cursor = end;
        }
        if cursor < range.end {
            out.push((cursor..range.end, gpui::HighlightStyle::default()));
        }
        overlay_todo(out, &self.todo_lines, self.band)
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<gpui_component::input::FoldRange> {
        Vec::new()
    }
}

// ── Providers ────────────────────────────────────────────────────────

/// Put the editor's text in front of the query it is about to send.
///
/// `query.rs` relies on the channel's order: an `Edit` ahead of a query
/// means the query never sees text older than the keystroke that prompted
/// it. But the editor asks its providers **synchronously, inside the
/// keystroke**, while `Document::on_edited` runs from the `Change` event,
/// which gpui delivers after the current update — so without this the
/// query overtook the edit and reached the worker with an offset past the
/// text it held (a completion at the end of the file panicked the analysis
/// thread with `end byte index 199 is out of bounds for string of length
/// 198`). An identical text is a no-op in `Project::edit`, so the seed
/// costs nothing when the document is already current.
fn seed_edit(
    project: &Entity<Project>,
    path: &SharedString,
    text: &Rope,
    origin: gpui::EntityId,
    cx: &mut App,
) {
    let text = text.to_string();
    project.update(cx, |project, cx| {
        project.edit(path, text, Some(origin), cx);
    });
}

struct BrinkHover {
    project: WeakEntity<Project>,
    path: SharedString,
    /// The editor this provider belongs to — the origin of the seed edit.
    origin: gpui::EntityId,
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
        seed_edit(&project, &self.path, text, self.origin, cx);
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
    origin: gpui::EntityId,
}

impl CompletionProvider for BrinkCompletion {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: lsp::CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<lsp::CompletionResponse>> {
        let Some(project) = self.project.upgrade() else {
            return Task::ready(Ok(lsp::CompletionResponse::Array(Vec::new())));
        };
        seed_edit(&project, &self.path, text, self.origin, cx);
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

#[cfg(test)]
mod tests {
    use super::*;

    const INK: &str = "TODO: at the top\n=== k ===\nHello.\n  TODO (art) sketch\nTODO\n-> DONE\n";

    fn notes() -> Vec<Range<usize>> {
        ["TODO: at the top", "TODO (art) sketch", "TODO\n"]
            .iter()
            .map(|s| {
                let at = INK.find(s).unwrap();
                at..at + s.trim_end().len()
            })
            .collect()
    }

    #[test]
    fn a_note_widens_to_its_line_and_finds_its_keyword() {
        let lines = todo_lines(INK, &notes());
        assert_eq!(lines.len(), 3);
        assert_eq!(&INK[lines[0].line.clone()], "TODO: at the top");
        assert_eq!(
            &INK[lines[0].keyword.clone()],
            "TODO:",
            "the colon rides along"
        );
        assert_eq!(&INK[lines[1].line.clone()], "  TODO (art) sketch");
        assert_eq!(
            &INK[lines[1].keyword.clone()],
            "TODO",
            "no colon, none taken"
        );
        assert_eq!(&INK[lines[2].line.clone()], "TODO");
        // Two notes on one line collapse to one band.
        let twice = [notes()[0].clone(), notes()[0].clone()];
        assert_eq!(todo_lines(INK, &twice).len(), 1);
    }

    #[test]
    fn the_band_overrides_every_colour_inside_and_nothing_outside() {
        let lines = todo_lines(INK, &notes());
        let peach = gpui::Hsla {
            h: 0.1,
            s: 0.9,
            l: 0.7,
            a: 1.,
        };
        let band = (gpui::Hsla::default(), gpui::Hsla::default());
        let runs = vec![
            (
                0..5,
                gpui::HighlightStyle {
                    color: Some(peach),
                    ..Default::default()
                },
            ),
            (5..INK.len(), gpui::HighlightStyle::default()),
        ];
        let out = overlay_todo(runs, &lines, band);
        // Pieces inside the first line: "TODO:" bold, " at the top" plain ink.
        let keyword = out.iter().find(|(r, _)| *r == (0..5)).unwrap();
        assert_eq!(keyword.1.font_weight, Some(gpui::FontWeight::BOLD));
        assert_eq!(keyword.1.color, Some(band.1));
        assert_eq!(keyword.1.background_color, Some(band.0));
        let rest = out.iter().find(|(r, _)| r.start == 5).unwrap();
        assert_eq!(rest.0.end, 16, "cut at the line's end");
        assert_eq!(rest.1.font_weight, None);
        assert_eq!(rest.1.color, Some(band.1));
        // The knot header after it is untouched.
        let header = out.iter().find(|(r, _)| r.start == 16).unwrap();
        assert_eq!(header.1.background_color, None);
        assert_eq!(header.1.color, None);
        // Every byte is covered exactly once, in order.
        let mut at = 0;
        for (r, _) in &out {
            assert_eq!(r.start, at);
            at = r.end;
        }
        assert_eq!(at, INK.len());
    }
}
