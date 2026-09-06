//! The **Continuous view** — the project as one manuscript.
//!
//! Every file in binder order in a single scroller, a heading between each,
//! scrolling straight through the boundaries
//! (`packages/studio-shell/src/continuous-view.tsx`, ruled 2026-08-26).
//!
//! ## Stacked, not concatenated — and why that survives the port
//!
//! The studio's ruling is that the view is a STACK of per-file documents,
//! not one synthetic buffer: each file keeps its own document handle, so
//! diagnostics, semantic tokens and completion stay per-file and correct.
//! The alternative needs span translation across the entire IDE surface.
//!
//! Nothing about GPUI changes that. Zed *does* have the concatenated answer
//! — `crates/multi_buffer`, which is what its project-search and diagnostics
//! views scroll through — but that is **17,589 lines** wired into Zed's own
//! buffer/language stack, not something `gpui-component`'s editor can be
//! pointed at. Writing our own lands back on the 2026-08-26 objection.
//!
//! ## The one real problem the stack introduces
//!
//! `gpui-base`'s editor computes its visible line range from **its own
//! height** (`element.rs`: `viewport_bottom = viewport_top + input_height`).
//! An editor sized to its content therefore has no viewport smaller than
//! itself, and lays out EVERY line it holds. Stack 44 of those and the whole
//! project lays out on every frame.
//!
//! So the stack is virtualised at the FILE level: GPUI's `list` element
//! (variable-height, unlike `uniform_list`) mounts only the sections near the
//! viewport. Off-screen files cost nothing; on-screen files lay out in full,
//! which is the honest residual cost of a stacked manuscript.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ListAlignment, ListState, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    WeakEntity, Window, div, list, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, h_flex,
    input::{Editor, EditorState, InputEvent},
    v_flex,
};

use crate::document::highlighter_factory;
use crate::icons;
use crate::project::{Project, ProjectEvent};

/// A mounted section: its editor, and the height its file needs.
type Section = (Entity<EditorState>, f32);

/// gpui-component renders the editor at `line_height: relative(1.5)` over the
/// theme's monospace size (`input/editor.rs`), so a row is exactly
/// `mono_font_size * 1.5` — 19.5px at the default 13px. Guessing 20 cost half
/// a pixel a line, which on a 1,300-line file is 650px of blank space at the
/// end of the section: the "big gap after each one".
const LINE_HEIGHT_FACTOR: f32 = 1.5;

/// gpui-component pads a multi-line input by `Size::input_py()` — 8px top and
/// bottom at the default Medium size (`sizing.rs`). Padding AROUND a section
/// reads as half a line of dead space above every file, so sections run at
/// `XSmall`, whose `input_py()` is zero, and the section is exactly its rows.
///
/// Getting this wrong is not cosmetic. A section that can scroll at all
/// swallows the wheel: `on_scroll_wheel` stops propagation only when its own
/// offset actually moved, so only an exactly-sized section lets the event
/// through to the manuscript list.
const SECTION_SIZE: gpui_component::Size = gpui_component::Size::XSmall;

/// Height of the boundary heading between two files.
const HEADING_HEIGHT: f32 = 30.0;

/// Rows of scroll-past-the-end, on the LAST section only.
///
/// A code editor reserves empty space below its final line —
/// `empty_bottom_height` in gpui-base's `element.rs`, which with the default
/// `scroll_beyond_last_line: None` is **half the viewport height**. Each
/// section's viewport is its whole file, so every file in the stack got half
/// its own height of blank space after it. In a manuscript that padding
/// belongs at the END of the manuscript, not after every chapter.
const TRAILING_ROWS: usize = 8;

/// A section's height before it has laid out: its file's line count, so
/// the OUTER list is the only scroller — the same arrangement as the
/// studio's. `rows()` is textarea-only in gpui-base, so the height goes on
/// the element.
///
/// Only a first guess once soft wrap is on. A wrapped section's true height
/// is its *wrapped* row count, which the fork exposes as
/// `EditorState::wrap_row_count` (the toolkit keeps `display_map` private);
/// [`ContinuousView::remeasure_sections`] re-sizes every mounted section
/// against it on each frame, so a resize that re-wraps is caught too.
/// Undersize a section and the editor gets a viewport smaller than its
/// content and starts scrolling ITSELF — the wheel then moves one file
/// instead of the manuscript, and a revealed selection drags the section
/// sideways instead of the list down.
fn section_height(source: &str, line_height: f32) -> f32 {
    display_rows(source) as f32 * line_height
}

/// Rows the editor will actually draw.
///
/// NOT `str::lines()`: that drops the empty final line a trailing newline
/// creates, while the editor renders it. One row short is enough to clip the
/// file's last line AND leave the section scrollable by that row.
fn display_rows(source: &str) -> usize {
    source.split('\n').count().max(1)
}

pub struct ContinuousView {
    project: Entity<Project>,
    /// Files in binder order.
    files: Vec<String>,
    /// One editor per file, built the first time its section is mounted. A
    /// section that has never been on screen costs nothing at all.
    editors: Rc<RefCell<HashMap<String, Section>>>,
    /// Each section's edit subscription, kept alive for as long as the view
    /// is. Sections are built inside the `list` closure, which has a bare
    /// `&mut App` rather than a `Context<Self>` — `App::subscribe` is what
    /// makes an edit in a section reach the worker from there.
    section_subs: Rc<RefCell<Vec<Subscription>>>,
    list: ListState,
    /// How many sections have ever been built, and the cost of the last one.
    mounted: Rc<RefCell<(usize, f64)>>,
    /// The editor's REAL row height, once one section has laid out.
    ///
    /// `mono_font_size * 1.5` is what gpui-component asks for, but the row
    /// height it lays out with is rounded, and being a fraction of a pixel
    /// short per row is enough — over a screenful — to leave a section
    /// scrollable, which makes it swallow the wheel again. So the constant is
    /// only a first guess: the first laid-out section reports the true value
    /// through `EditorState::line_height()` and every section is re-measured.
    measured_line_height: Option<f32>,
    /// A `(path, span)` to select once that file's section exists. Set by
    /// [`ContinuousView::reveal_span`] when the section has not been
    /// mounted yet; the list mounts it on the way there, and the next
    /// render applies the selection.
    pending_reveal: Option<(String, std::ops::Range<usize>)>,
    /// A handle on this entity for the sections' navigation sink, which
    /// runs from a bare `&mut App`.
    me: WeakEntity<Self>,
    focus: gpui::FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl ContinuousView {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let files = project.read(cx).files().to_vec();
        let watch = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| match event {
                ProjectEvent::Opened { .. } => this.reload(cx),
                ProjectEvent::SourceChanged {
                    path,
                    origin,
                    delta,
                } => this.on_source_changed(path, *origin, delta, window, cx),
                _ => {}
            },
        );
        // A theme or font-size change re-sizes every row and recolours the
        // band, so the sections are rebuilt.
        cx.observe_global::<gpui_component::Theme>(|this, cx| this.reload(cx))
            .detach();
        Self {
            list: ListState::new(files.len(), ListAlignment::Top, px(600.)),
            project,
            files,
            editors: Rc::new(RefCell::new(HashMap::new())),
            section_subs: Rc::new(RefCell::new(Vec::new())),
            mounted: Rc::new(RefCell::new((0, 0.0))),
            measured_line_height: None,
            pending_reveal: None,
            me: cx.weak_entity(),
            focus: cx.focus_handle(),
            _subscriptions: vec![watch],
        }
    }

    /// A new project invalidates every section: different files, different
    /// text, different count.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.files = self.project.read(cx).files().to_vec();
        self.editors.borrow_mut().clear();
        self.section_subs.borrow_mut().clear();
        self.measured_line_height = None;
        self.list = ListState::new(self.files.len(), ListAlignment::Top, px(600.));
        cx.notify();
    }

    /// A file's text moved — in another editor, or in this section itself.
    /// Either way the section's height follows the new line count; only a
    /// change from elsewhere is applied to the section's buffer.
    fn on_source_changed(
        &mut self,
        path: &str,
        origin: Option<gpui::EntityId>,
        delta: &crate::project::SourceDelta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((editor, _)) = self.editors.borrow().get(path).cloned() else {
            return;
        };
        if origin != Some(editor.entity_id()) {
            let fallback = self
                .project
                .read(cx)
                .loaded_source(path)
                .unwrap_or_default()
                .to_owned();
            editor.update(cx, |state, cx| {
                crate::document::apply_delta(state, delta, &fallback, window, cx);
            });
        }
        let Some(index) = self.files.iter().position(|f| f == path) else {
            return;
        };
        let line_height = self
            .measured_line_height
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
        let trailing = if index + 1 == self.files.len() {
            TRAILING_ROWS
        } else {
            0
        };
        let rows = editor.read(cx).wrap_row_count().max(1);
        let height = (rows + trailing) as f32 * line_height;
        if let Some(section) = self.editors.borrow_mut().get_mut(path) {
            section.1 = height;
        }
        // Drop the list's cached height for this one item; the scroll
        // position survives, which `reset` would not give.
        self.list.splice(index..index + 1, 1);
        cx.notify();
    }

    /// Scroll the manuscript to a file — what every navigation surface
    /// (binder, search, problems) ends up doing in this view, because the
    /// per-file editors do not scroll: this list does.
    pub fn reveal(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.files.iter().position(|f| f == path) {
            self.list.scroll_to_reveal_item(index);
            cx.notify();
        }
    }

    /// Scroll to a file AND select a span inside it — a definition or a
    /// reference. If the section is not mounted yet the selection is
    /// parked; the scroll mounts it and the next render applies it.
    pub fn reveal_span(
        &mut self,
        path: &str,
        span: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.reveal(path, cx);
        // Applied from `render`, AFTER `remeasure_sections`: a section whose
        // wrapped height differs from its estimate is spliced there, and
        // `ListState::splice` zeroes the scroll offset inside the spliced
        // item — a scroll applied before that pass was thrown away by it.
        self.pending_reveal = Some((path.to_owned(), span));
        cx.notify();
    }

    fn apply_pending_reveal(&mut self, cx: &mut Context<Self>) {
        let Some((path, span)) = self.pending_reveal.clone() else {
            return;
        };
        let Some((editor, _)) = self.editors.borrow().get(&path).cloned() else {
            return;
        };
        let Some(index) = self.files.iter().position(|f| f == &path) else {
            self.pending_reveal = None;
            return;
        };
        self.pending_reveal = None;
        // The section does not scroll — the list does — so "show this span"
        // is a list offset: the heading, then the span's row, backed off a
        // few rows so the target is not pinned to the top edge.
        let line = editor
            .read(cx)
            .value()
            .get(..span.start)
            .map_or(0, |before| before.matches('\n').count());
        let line_height = self
            .measured_line_height
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
        let offset = (HEADING_HEIGHT + line as f32 * line_height - 4.0 * line_height).max(0.0);
        self.list.scroll_to(gpui::ListOffset {
            item_ix: index,
            offset_in_item: px(offset),
        });
        editor.update(cx, |state, cx| {
            state.set_selected_range(span, cx);
            cx.notify();
        });
        cx.notify();
    }

    /// The section whose editor has focus, as a navigation site — what a
    /// keyboard command acts on in this view.
    #[must_use]
    pub fn focused_section(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<crate::navigation::EditorSite> {
        let editors = self.editors.borrow();
        let (path, (editor, _)) = editors
            .iter()
            .find(|(_, (editor, _))| editor.read(cx).focus_handle(cx).is_focused(window))?;
        Some(crate::navigation::EditorSite {
            editor: editor.clone(),
            project: self.project.clone(),
            path: path.clone().into(),
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a section is built from the list closure with everything it needs in hand"
    )]
    fn build_editor(
        project: &Entity<Project>,
        me: &WeakEntity<Self>,
        section_subs: &Rc<RefCell<Vec<Subscription>>>,
        path: &str,
        is_last: bool,
        line_height_override: Option<f32>,
        window: &mut Window,
        cx: &mut App,
    ) -> Section {
        let source = project
            .read(cx)
            .loaded_source(path)
            .unwrap_or_default()
            .to_owned();
        let line_height = line_height_override
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
        let trailing = if is_last { TRAILING_ROWS } else { 0 };
        let height = section_height(&source, line_height) + trailing as f32 * line_height;

        let key: SharedString = path.to_owned().into();
        let weak = project.downgrade();
        let manuscript = me.clone();
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .language("brink")
                // Prose wraps (maintainer, 2026-09-05); the section is
                // re-sized to its wrapped rows — see `section_height`.
                .soft_wrap(true)
                // A fold hides rows, and a section is exactly its rows — a
                // folded section would be taller than its content and
                // start scrolling itself (see `section_height`). The
                // manuscript is a reading surface; folding belongs to the
                // tabs.
                .folding(false)
                // See `TRAILING_ROWS`.
                .scroll_beyond_last_line(Some(trailing));
            state.set_highlighter_factory(highlighter_factory(weak.clone(), key.clone()), cx);

            // The same providers a tab's editor gets — navigation must not
            // depend on which view a file is read in. What differs is the
            // sink: a target is shown by scrolling the manuscript to it.
            let origin = cx.entity().entity_id();
            crate::document::install_language_providers(&mut state, &weak, key.clone(), origin);
            let navigate: crate::navigation::Navigate = Rc::new(move |path, span, _window, cx| {
                let _ = manuscript.update(cx, |this, cx| this.reveal_span(path, span, cx));
            });
            crate::navigation::install(&mut state, project, key.clone(), origin, navigate);

            state.set_value(source, window, cx);
            state
        });

        // The spike got re-analysis as a side effect of the highlighter,
        // which called `sync` on every paint. That is exactly the
        // instrumentation-in-the-hot-path shape the design forbids, so the
        // edit is pushed explicitly instead.
        let edited_project = project.clone();
        let edited_path = path.to_owned();
        section_subs.borrow_mut().push(cx.subscribe(
            &state,
            move |state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let origin = state.entity_id();
                    edited_project.update(cx, |project, cx| {
                        project.edit(&edited_path, text, Some(origin), cx);
                    });
                }
            },
        ));

        (state, height)
    }

    /// Adopt the real row height as soon as any section has laid out, and
    /// re-measure every section against it. Runs once.
    fn adopt_measured_line_height(&mut self, cx: &mut Context<Self>) {
        if self.measured_line_height.is_some() {
            return;
        }
        let Some(real) = self
            .editors
            .borrow()
            .values()
            .find_map(|(editor, _)| editor.read(cx).line_height())
            .map(f32::from)
        else {
            return;
        };
        self.measured_line_height = Some(real);
        self.remeasure_sections(cx);
        self.list.remeasure();
        cx.notify();
    }

    /// Size every mounted section to the rows its editor will actually draw
    /// — the wrapped count, once it has laid out. Runs every frame; it is a
    /// read per mounted section, and it is what keeps a section exact across
    /// a resize that re-wraps its lines.
    fn remeasure_sections(&mut self, cx: &mut Context<Self>) {
        let line_height = self
            .measured_line_height
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
        let last = self.files.len().saturating_sub(1);
        let mut changed: Vec<usize> = Vec::new();
        for (path, section) in self.editors.borrow_mut().iter_mut() {
            let Some(index) = self.files.iter().position(|f| f == path) else {
                continue;
            };
            let rows = section.0.read(cx).wrap_row_count().max(1);
            let trailing = if index == last { TRAILING_ROWS } else { 0 };
            let height = (rows + trailing) as f32 * line_height;
            if (section.1 - height).abs() > 0.5 {
                section.1 = height;
                changed.push(index);
            }
        }
        if changed.is_empty() {
            return;
        }
        // `splice` keeps the scroll position for every item but the one it
        // touches — that one's offset is zeroed — so the position is put
        // back afterwards. The list clamps it to the new height at layout.
        let top = self.list.logical_scroll_top();
        for index in &changed {
            self.list.splice(*index..index + 1, 1);
        }
        if changed.contains(&top.item_ix) {
            self.list.scroll_to(top);
        }
        cx.notify();
    }
}

impl gpui::Focusable for ContinuousView {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for ContinuousView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.adopt_measured_line_height(cx);
        self.remeasure_sections(cx);
        self.apply_pending_reveal(cx);

        let surface = cx.theme().background;
        let files = self.files.clone();
        let count = files.len();
        let project = self.project.clone();
        let me = self.me.clone();
        let editors = self.editors.clone();
        let section_subs = self.section_subs.clone();
        let mounted = self.mounted.clone();
        let measured = self.measured_line_height;

        // The file the top of the scroller is currently inside — `list`
        // reports its topmost visible item, which is exactly that.
        let sticky = self
            .files
            .get(self.list.logical_scroll_top().item_ix)
            .cloned();

        v_flex()
            .id("continuous")
            // The view's focus handle must be in the tree: the shell moves
            // focus here on a switch, and a handle nothing tracks is a dead
            // end for every shortcut.
            .track_focus(&self.focus)
            .size_full()
            .bg(surface)
            .relative()
            .child(
                list(self.list.clone(), move |index, window, cx| {
                    let Some(path) = files.get(index).cloned() else {
                        return div().into_any_element();
                    };
                    let started = Instant::now();
                    let fresh = !editors.borrow().contains_key(&path);
                    let (editor, height) = editors
                        .borrow_mut()
                        .entry(path.clone())
                        .or_insert_with(|| {
                            ContinuousView::build_editor(
                                &project,
                                &me,
                                &section_subs,
                                &path,
                                index + 1 == count,
                                measured,
                                window,
                                cx,
                            )
                        })
                        .clone();
                    if fresh {
                        let mut stats = mounted.borrow_mut();
                        stats.0 += 1;
                        stats.1 = started.elapsed().as_secs_f64() * 1e3;
                    }
                    v_flex()
                        .w_full()
                        .child(heading(&path, cx))
                        .child(
                            Editor::new(&editor)
                                .bordered(false)
                                .appearance(false)
                                .with_size(SECTION_SIZE)
                                .h(px(height)),
                        )
                        .into_any_element()
                })
                .flex_1(),
            )
            .when_some(sticky, |el, path| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .child(heading(&path, cx)),
                )
            })
    }
}

/// The boundary between two files.
///
/// GPUI has no `position: sticky`, so the manuscript draws this twice:
/// inline at each boundary, and again as an overlay pinned to the top of the
/// scroller showing whichever file is currently under it — which is what
/// makes the heading read as sticky.
fn heading(path: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .h(px(HEADING_HEIGHT))
        .px_4()
        .gap_2()
        .items_center()
        .bg(theme.sidebar)
        .border_t_1()
        .border_b_1()
        .border_color(theme.border)
        .child(icons::icon(icons::FILE, px(12.), theme.muted_foreground))
        .child(
            div()
                .text_xs()
                .text_color(theme.foreground)
                .child(path.to_owned()),
        )
}
