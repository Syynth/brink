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
    ParentElement as _, Render, SharedString, Styled as _, Window, div, list, px,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
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
/// bottom at the default Medium size (`sizing.rs`). A section that forgets it
/// is 16px shorter than its content, so the editor's own viewport is smaller
/// than what it holds and the wheel scrolls THE SECTION instead of the
/// manuscript. That is the whole of the "scroll happens within one file"
/// bug: `on_scroll_wheel` stops propagation only when its offset actually
/// moved, so as long as a section cannot scroll, the event reaches the list.
const INPUT_PY: f32 = 8.0;

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
    source.lines().count().max(1) as f32 * line_height
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
        window: &mut Window,
        cx: &mut App,
    ) -> Section {
        let source = project
            .borrow()
            .file_id(path)
            .and_then(|id| project.borrow().session.source(id).map(str::to_owned))
            .unwrap_or_default();
        let line_height = f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR;
        let trailing = if is_last { TRAILING_ROWS } else { 0 };
        let height =
            section_height(&source, line_height) + trailing as f32 * line_height + 2.0 * INPUT_PY;
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
        let theme = cx.theme();
        let (surface, border, muted, fg) = (
            theme.background,
            theme.border,
            theme.muted_foreground,
            theme.foreground,
        );
        let files = self.files.clone();
        let count = files.len();
        let project = self.project.clone();
        let editors = self.editors.clone();
        let mounted = self.mounted.clone();

        v_flex().size_full().bg(surface).child(
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
                        ContinuousView::build_editor(&project, &path, is_last, window, cx)
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
                    .child(
                        // The heading between files. Not sticky: GPUI has no
                        // `position: sticky`, and a real port would draw the
                        // current section's heading as an overlay on the
                        // scroller instead.
                        h_flex()
                            .w_full()
                            .h(px(30.))
                            .px_4()
                            .gap_2()
                            .items_center()
                            .bg(theme_bg(cx))
                            .border_t_1()
                            .border_b_1()
                            .border_color(border)
                            .child(icons::icon(icons::FILE, px(12.), muted))
                            .child(div().text_xs().text_color(fg).child(path.clone())),
                    )
                    .child(
                        Editor::new(&editor)
                            .bordered(false)
                            .appearance(false)
                            .h(px(height)),
                    )
                    .into_any_element()
            })
            .flex_1(),
        )
    }
}

fn theme_bg(cx: &App) -> gpui::Hsla {
    cx.theme().sidebar
}
