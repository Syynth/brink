//! SPIKE — the studio's **Continuous view**, natively.
//!
//! The project as one manuscript: every file in binder order in a single
//! scroller, a heading between each, scrolling straight through the
//! boundaries (`packages/studio-shell/src/continuous-view.tsx`, ruled
//! 2026-08-26).
//!
//! ## Stacked, not concatenated — and why that decision survives the port
//!
//! The studio's ruling is that the view is a STACK of per-file documents,
//! not one synthetic buffer: each file keeps its own document handle, so
//! diagnostics, semantic tokens and completion stay per-file and correct.
//! The alternative needs span translation across the entire IDE surface.
//!
//! Nothing about GPUI changes that. Zed *does* have the concatenated
//! answer — `crates/multi_buffer`, which is what its project-search and
//! diagnostics views scroll through — but that is **17,589 lines** in a
//! crate wired into Zed's own buffer/language stack, not something
//! `gpui-component`'s editor can be pointed at. Writing our own would land
//! us back at exactly the objection the 2026-08-26 ruling made.
//!
//! ## The one real problem the port introduces
//!
//! `gpui-base`'s editor computes its visible line range from **its own
//! height** (`element.rs`: `viewport_bottom = viewport_top + input_height`).
//! An editor sized to its content therefore has no viewport smaller than
//! itself, and lays out EVERY line it holds. Stack 44 of those and the whole
//! project is laid out on every frame.
//!
//! So the stack has to be virtualised at the FILE level: GPUI's `list`
//! element (variable-height, unlike `uniform_list`) mounts only the sections
//! near the viewport. Off-screen files cost nothing; on-screen files lay out
//! in full, which is the honest residual cost of a stacked manuscript and is
//! what the measurement below reports.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, IntoElement, ListAlignment, ListState,
    ParentElement as _, Render, SharedString, Styled as _, Window, div, list,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, h_flex,
    input::{Editor, EditorState, InputHighlighter},
    v_flex,
};

use crate::{Shared, binder::BinderEvent, icons};

/// A mounted section: its editor, and the height its file needs.
type Section = (Entity<EditorState>, f32);

/// gpui-component renders the editor at `line_height: relative(1.5)` over
/// the theme's monospace size (`input/editor.rs`), so a row is exactly
/// `mono_font_size * 1.5` — 19.5px at the default 13px. Guessing 20 here
/// cost half a pixel a line, which on a 1,300-line file is 650px of blank
/// space at the end of the section: the "big gap after each one".
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

/// A section is sized to its file's content, so the OUTER list is the only
/// scroller — the same arrangement as the studio's, where the per-file
/// editors size to content and the manuscript scroller owns the scrolling.
/// `rows()` is textarea-only in gpui-base, so the height goes on the element.
///
/// **This is only exact with soft wrap off.** With wrapping on, a section's
/// height is its *wrapped* row count, which lives in `InputBaseState`'s
/// `display_map` — `pub(super)`, so a consumer cannot read it, and
/// `line_height()` alone is not enough. Undersize the section and the editor
/// gets a viewport smaller than its content and starts scrolling ITSELF,
/// which is the second bug: the wheel moves one file instead of the
/// manuscript. Sections therefore run unwrapped until gpui-base publishes a
/// content height.
fn section_height(source: &str, line_height: f32) -> f32 {
    display_rows(source) as f32 * line_height
}

/// Rows the editor will actually draw.
///
/// NOT `str::lines()`: that drops the empty final line a trailing newline
/// creates, while the editor renders it. One row short is enough to clip the
/// file's last line AND leave the section scrollable by that row — which is
/// the residual "extra scrolling" after the padding fix.
fn display_rows(source: &str) -> usize {
    source.split('\n').count().max(1)
}

/// Rows of scroll-past-the-end, on the LAST section only.
///
/// A code editor reserves empty space below its final line —
/// `empty_bottom_height` in gpui-base's `element.rs`, which with the default
/// `scroll_beyond_last_line: None` is **half the viewport height**. Each
/// section's viewport is its whole file, so every file in the stack got half
/// its own height of blank space after it. In a manuscript that padding
/// belongs at the END of the manuscript, not after every chapter, so every
/// section but the last pins it to zero.
const TRAILING_ROWS: usize = 8;

pub struct ContinuousView {
    project: Shared,
    /// Files in binder order. Owned by the host, not this view (§7.2: the
    /// order is a studio concept, not the shell's business).
    files: Vec<String>,
    /// One editor per file, built the first time its section is mounted.
    /// A section that has never been on screen costs nothing at all.
    editors: Rc<RefCell<HashMap<String, Section>>>,
    list: ListState,
    /// How many sections have ever been built, and the cost of the last one
    /// — the number that answers "is this viable".
    mounted: Rc<RefCell<(usize, f64)>>,
    /// The editor's REAL row height, once one section has laid out.
    ///
    /// `mono_font_size * 1.5` is what gpui-component asks for, but the row
    /// height it actually lays out with is rounded, and being a fraction of a
    /// pixel short per row is enough — over a screenful — to leave a section
    /// scrollable, which makes it swallow the wheel again. So the constant is
    /// only ever a first guess: the first laid-out section reports the true
    /// value through `EditorState::line_height()` and every section is
    /// re-measured against it.
    measured_line_height: Option<f32>,
}

impl ContinuousView {
    pub fn new(project: Shared, cx: &mut Context<Self>) -> Self {
        let files = project.borrow().files.clone();
        let list = ListState::new(files.len(), ListAlignment::Top, px(600.));
        let _ = cx;
        Self {
            project,
            files,
            editors: Rc::new(RefCell::new(HashMap::new())),
            list,
            mounted: Rc::new(RefCell::new((0, 0.0))),
            measured_line_height: None,
        }
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

    fn build_editor(
        project: &Shared,
        path: &str,
        is_last: bool,
        line_height_override: Option<f32>,
        window: &mut Window,
        cx: &mut App,
    ) -> Section {
        let source = project
            .borrow()
            .file_id(path)
            .and_then(|id| project.borrow().session.source(id).map(str::to_owned))
            .unwrap_or_default();
        let line_height = line_height_override
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
        let trailing = if is_last { TRAILING_ROWS } else { 0 };
        let height = section_height(&source, line_height) + trailing as f32 * line_height;
        let key: SharedString = path.to_owned().into();
        let project = project.clone();
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .language("brink")
                // See `section_height`: wrapping would make the section's
                // true height unknowable from outside the crate.
                .soft_wrap(false)
                // See `TRAILING_ROWS`.
                .scroll_beyond_last_line(Some(trailing));
            let (hp, ha) = (project.clone(), key.clone());
            state.set_highlighter_factory(
                Rc::new(move |language| {
                    (language == "brink").then(|| {
                        Box::new(crate::section_highlighter(hp.clone(), ha.as_ref()))
                            as Box<dyn InputHighlighter>
                    })
                }),
                cx,
            );
            state.set_value(source, window, cx);
            state
        });
        (state, height)
    }
}

impl EventEmitter<BinderEvent> for ContinuousView {}

impl Render for ContinuousView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = cx.theme().background;
        let files = self.files.clone();
        let count = files.len();
        let project = self.project.clone();
        let editors = self.editors.clone();
        let mounted = self.mounted.clone();
        let measured = self.measured_line_height;

        // Adopt the real row height as soon as any section has laid out, and
        // re-measure every section against it. Runs once.
        if self.measured_line_height.is_none() {
            let real = self
                .editors
                .borrow()
                .values()
                .find_map(|(editor, _)| editor.read(cx).line_height())
                .map(f32::from);
            if let Some(real) = real {
                self.measured_line_height = Some(real);
                let trailing = TRAILING_ROWS as f32 * real;
                let last = self.files.last().cloned();
                for (path, section) in self.editors.borrow_mut().iter_mut() {
                    let rows = section.0.read(cx).text().to_string();
                    section.1 = display_rows(&rows) as f32 * real
                        + if Some(path) == last.as_ref() {
                            trailing
                        } else {
                            0.0
                        };
                }
                self.list.remeasure();
                cx.notify();
            }
        }

        // The file the top of the scroller is currently inside — `list`
        // reports its topmost visible item, which is exactly that.
        let sticky = self
            .files
            .get(self.list.logical_scroll_top().item_ix)
            .cloned();

        v_flex()
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
                            let is_last = index + 1 == count;
                            ContinuousView::build_editor(
                                &project, &path, is_last, measured, window, cx,
                            )
                        })
                        .clone();
                    if fresh {
                        let mut stats = mounted.borrow_mut();
                        stats.0 += 1;
                        stats.1 = started.elapsed().as_secs_f64() * 1e3;
                        eprintln!(
                            "continuous: mounted section {} ({}) in {:.2} ms — {} live",
                            stats.0, path, stats.1, stats.0
                        );
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
/// GPUI has no `position: sticky`, so the manuscript draws this twice: inline
/// at each boundary, and again as an overlay pinned to the top of the
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
