//! The **Continuous view** — the project as one manuscript.
//!
//! Every file in binder order in a single scroller, a chapter break opening each,
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
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Editor, EditorState, InputEvent, MoveDown, MoveLeft, MoveRight, MoveUp},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};

use brink_gpui_model::query::{QueryKind, QueryResult, Scope};

use crate::document::{ReadCell, ReadView, manuscript_highlighter_factory};
use crate::project::{Project, ProjectEvent};
use brink_gpui_shell::icons;

/// A mounted section: its editor, and the height its file needs.
type Section = (Entity<EditorState>, f32);

/// Each mounted section's breakpoint column marks, by path, refreshed when
/// the breakpoints change (`crate::gutter`).
type Gutters = Rc<RefCell<HashMap<String, Rc<crate::gutter::Marks>>>>;

/// Each mounted section's fold candidates, by path: the cell its
/// highlighter reports them from, refreshed after each analysis.
type Folds = Rc<RefCell<HashMap<String, crate::document::FoldCell>>>;

/// The fold column the kit's gutter keeps right of the line numbers
/// (`FOLD_ICON_HITBOX_WIDTH`), now that sections fold.
const FOLD_COLUMN: f32 = 18.;

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

/// Line-number digits every section reserves, so sections over files of
/// different lengths share one text column. Four covers any file an
/// author would keep in one piece.
const MANUSCRIPT_GUTTER_DIGITS: usize = 4;

/// Height of the chapter break that opens each file, and the space above
/// its title. Fixed, so a reveal can count rows from a section's top.
const SEPARATOR_HEIGHT: f32 = 92.0;
const SEPARATOR_SPACE_ABOVE: f32 = 36.0;

/// Frames a caret move may take to be brought on screen (`reveal_caret`).
const REVEAL_TRIES: u8 = 3;

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
/// Only a first guess once soft wrap is on. A section's true height is the
/// rows it shows — wrapped, less any folded — which the fork exposes as
/// `EditorState::display_row_count` (the toolkit keeps `display_map`
/// private); [`ContinuousView::remeasure_sections`] re-sizes every mounted
/// section against it on each frame, so a resize that re-wraps is caught
/// too, and a fold asks for that frame.
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
    /// The row height the sections reported BEFORE a theme change, while
    /// they have not yet laid out at the new size: not to be adopted as
    /// the new one (`restyle`).
    stale_line_height: Option<f32>,
    /// A `(path, span)` to select once that file's section exists. Set by
    /// [`ContinuousView::reveal_span`] when the section has not been
    /// mounted yet; the list mounts it on the way there, and the next
    /// render applies the selection.
    pending_reveal: Option<(String, std::ops::Range<usize>)>,
    /// The Read view (W8): whether it is on, and the prose it keeps — shared
    /// with every section's highlighter.
    read: ReadCell,
    /// A handle on this entity for the sections' navigation sink, which
    /// runs from a bare `&mut App`.
    me: WeakEntity<Self>,
    focus: gpui::FocusHandle,
    /// Where the caret is: the section that last had focus, and the byte
    /// offset in it. What Writing mode's sidebar calls the current file,
    /// and what the title bar's knot › stitch is read from.
    caret: Option<(String, usize)>,
    /// The focused section's editor, observed for caret moves. Replaced
    /// whenever another section takes focus.
    caret_watch: Option<Subscription>,
    /// Each section's prose lints, and a fingerprint of the text they were
    /// checked against (`ProseCache`).
    prose: ProseCache,
    /// Parameter hints while a call is being typed, in whichever section.
    signature: crate::signature_help::SignatureHint,
    /// Where the title bar's crumb starts: written with the window x the
    /// text starts at, each time that moves (`set_crumb_anchor`).
    crumb_anchor: Option<Rc<std::cell::Cell<Option<gpui::Pixels>>>>,
    /// The caret moved and the view has yet to check it is on screen: how
    /// many more frames may try. Set on every caret move; zeroed after
    /// layout once it is (`render`). Bounded, so a caret that can never be
    /// placed cannot keep asking for frames.
    reveal_caret: Rc<std::cell::Cell<u8>>,
    /// This view's bounds as of the last frame, measured after layout: the
    /// frame the editors' own bounds are from (they learn them as they
    /// paint). See the crumb's measurement in `render`.
    last_view: Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// Each file's scopes, for the pinned structure lines; asked for when a
    /// file first reaches the top of the view, dropped on each analysis.
    outlines: HashMap<String, Vec<Scope>>,
    outline_pending: std::collections::HashSet<String>,
    /// What is pinned now, and what is fading out.
    pins: crate::sticky_lines::Pins,
    /// A section to focus once it exists: an arrow key crossed into a file
    /// that had not been mounted yet (`cross_file`).
    pending_focus: Option<String>,
    /// Each section's breakpoint column.
    gutters: Gutters,
    /// Each section's fold candidates.
    folds: Folds,
    /// Frames a reveal may wait for its section to lay out, so it can land
    /// on the line's true place (`apply_pending_reveal`).
    reveal_retries: u8,
    _subscriptions: Vec<Subscription>,
}

/// What the manuscript tells its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManuscriptEvent {
    /// The caret moved to `offset` in `path` — or into another file.
    Caret { path: String, offset: usize },
    /// A file separator's `⋯` menu asked for something.
    File { path: String, action: FileAction },
    /// A file operation from the same menu — the Binder's, run by the
    /// studio the same way.
    Outline(crate::binder::BinderEvent),
}

impl gpui::EventEmitter<ManuscriptEvent> for ContinuousView {}

impl ContinuousView {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let files = project.read(cx).files().to_vec();
        let watch = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| match event {
                ProjectEvent::Opened { .. } => this.reload(cx),
                ProjectEvent::Analyzed => {
                    this.refresh_diagnostics(cx);
                    this.refresh_play_lines(cx);
                    if this.read.on.get() {
                        this.sync_prose(cx);
                    }
                    // Draft status is read off the analysis, and the
                    // outlines the pinned lines come from are now stale.
                    this.outlines.clear();
                    cx.notify();
                }
                // The separators' unsaved dots.
                ProjectEvent::Saved => cx.notify(),
                ProjectEvent::BreakpointsChanged => this.refresh_gutters(cx),
                ProjectEvent::SourceChanged {
                    path,
                    origin,
                    delta,
                } => {
                    // The config holds the prose dictionary and dialect: a
                    // change there (an "Add to dictionary") moves every
                    // file's lints, though no file's text moved. The next
                    // analysis checks them all again.
                    if this.project.read(cx).is_config(path) {
                        this.prose.0.borrow_mut().clear();
                    }
                    this.on_source_changed(path, *origin, delta, window, cx);
                }
                _ => {}
            },
        );
        // A theme or font-size change re-sizes every row and recolours the
        // band — in place (`restyle`), never by rebuilding the sections.
        cx.observe_global::<gpui_component::Theme>(|this, cx| this.restyle(cx))
            .detach();
        // The column's width is a setting; a change re-lays the sections,
        // and `remeasure_sections` follows their new wrapped heights.
        cx.observe_global::<brink_gpui_shell::settings::AppSettings>(|_, cx| cx.notify())
            .detach();
        Self {
            list: ListState::new(files.len(), ListAlignment::Top, px(600.)),
            project,
            files,
            editors: Rc::new(RefCell::new(HashMap::new())),
            section_subs: Rc::new(RefCell::new(Vec::new())),
            mounted: Rc::new(RefCell::new((0, 0.0))),
            measured_line_height: None,
            stale_line_height: None,
            pending_reveal: None,
            read: std::rc::Rc::new(ReadView::default()),
            me: cx.weak_entity(),
            focus: cx.focus_handle(),
            caret: None,
            caret_watch: None,
            prose: ProseCache::default(),
            signature: crate::signature_help::SignatureHint::default(),
            crumb_anchor: None,
            reveal_caret: Rc::default(),
            last_view: Rc::default(),
            outlines: HashMap::new(),
            outline_pending: std::collections::HashSet::new(),
            pins: crate::sticky_lines::Pins::default(),
            pending_focus: None,
            gutters: Rc::default(),
            folds: Rc::default(),
            reveal_retries: 0,
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
        self.stale_line_height = None;
        self.list = ListState::new(self.files.len(), ListAlignment::Top, px(600.));
        cx.notify();
    }

    /// A theme or font-size change, applied to the sections that exist.
    ///
    /// It used to be a [`ContinuousView::reload`]: every section's editor
    /// thrown away and rebuilt. That took the caret and the focus of the one
    /// being typed in with it — ⌘= unfocused the text — and the scroll
    /// position too. Now the highlighters are reinstalled (they snapshot
    /// the theme's colours) and the row height is re-measured, so nothing
    /// the author is holding moves.
    fn restyle(&mut self, cx: &mut Context<Self>) {
        // The gutter's red is the theme's.
        self.refresh_gutters(cx);
        let project = self.project.downgrade();
        let mut stale = None;
        for (path, (editor, _)) in self.editors.borrow().iter() {
            let fold_cell = self.folds.borrow().get(path).cloned().unwrap_or_default();
            let factory = manuscript_highlighter_factory(
                project.clone(),
                SharedString::from(path.clone()),
                fold_cell,
                self.read.clone(),
            );
            editor.update(cx, |state, cx| {
                stale = stale.or_else(|| state.line_height().map(f32::from));
                state.set_highlighter_factory(factory, cx);
            });
        }
        // Until the sections lay out at the new size they still report the
        // old row height; the guess from the theme stands in meanwhile.
        self.stale_line_height = stale;
        self.measured_line_height = None;
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
        let rows = editor.read(cx).display_row_count().max(1);
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
            // The file's START, its chapter break. `scroll_to_reveal_
            // item` would do the least scrolling that shows any of the item,
            // which for a long file below the viewport is its last screen —
            // a Binder click then landed on the file's end.
            self.list.scroll_to(gpui::ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
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
        self.reveal_retries = REVEAL_TRIES;
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
        // The section does not scroll — the list does — so "show this span"
        // is a list offset: the chapter break, then the span's line as the
        // section laid it out (prose wraps, so counting newlines lands a
        // far line well short), backed off past the rows that will pin
        // above it, and one more.
        let line_height = self
            .measured_line_height
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
        let (text, y) = {
            let state = editor.read(cx);
            let text = state.value().to_string();
            let line = text
                .get(..span.start)
                .map_or(0, |before| before.matches('\n').count());
            // Its display row, wrapping and folds counted, whether or not
            // the section has laid that far — once it has laid out at all,
            // which is what gives the wrap map its width.
            let y = state
                .line_height()
                .map(|_| state.display_row_of_buffer_line(line) as f32 * line_height);
            (text, y)
        };
        let Some(y) = y else {
            // Never laid out: get it on screen, and place the line on the
            // next frame.
            if self.reveal_retries > 0 {
                self.reveal_retries -= 1;
                self.list.scroll_to(gpui::ListOffset {
                    item_ix: index,
                    offset_in_item: px(0.),
                });
                cx.notify();
            } else {
                self.pending_reveal = None;
            }
            return;
        };
        self.pending_reveal = None;
        let reserve = (self.pinned_rows_at(&path, &text, span.start) + 1) as f32 * line_height;
        let offset = (SEPARATOR_HEIGHT + y - reserve).max(0.0);
        self.list.scroll_to(gpui::ListOffset {
            item_ix: index,
            offset_in_item: px(offset),
        });
        editor.update(cx, |state, cx| {
            state.set_selected_range(span, cx);
            cx.notify();
        });
        // The caret is where the reveal put it, focused or not: the sidebar
        // and the title bar's knot › stitch follow from here.
        self.follow_caret(path, editor, cx);
        cx.notify();
    }

    /// How many rows will pin above `offset` once it is near the top of
    /// the view: the file's own row, and one for each block it is inside
    /// that opened on an earlier line — a knot, a stitch and a block when
    /// the scopes have not arrived.
    fn pinned_rows_at(&self, path: &str, text: &str, offset: usize) -> usize {
        // Before the scopes arrive: room for a knot, a stitch and a block,
        // not the whole cap, or a first jump lands half a screen down.
        let Some(scopes) = self.outlines.get(path) else {
            return 1 + 3;
        };
        1 + crate::sticky_lines::rows_over(scopes, text, offset)
    }

    /// Keep the title bar's crumb over the text: `cell` gets the window x
    /// the text starts at, whenever that moves.
    pub fn set_crumb_anchor(&mut self, cell: Rc<std::cell::Cell<Option<gpui::Pixels>>>) {
        self.crumb_anchor = Some(cell);
    }

    /// Scroll the list to `y` px into the manuscript, for the tests.
    #[cfg(test)]
    pub fn scroll_list_to(&mut self, y: f32, cx: &mut Context<Self>) {
        self.list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: px(y),
        });
        cx.notify();
    }

    /// The pinned lines' text, for the tests.
    #[cfg(test)]
    pub fn pinned_texts(&mut self, cx: &mut Context<Self>) -> Vec<String> {
        self.pinned_lines(cx)
            .map(|(_, lines, _)| lines.into_iter().map(|l| l.text).collect())
            .unwrap_or_default()
    }

    /// Whether every pinned line but the file's carries the editor's
    /// highlighting of it, for the tests.
    #[cfg(test)]
    pub fn pinned_lines_highlighted(&mut self, cx: &mut Context<Self>) -> bool {
        self.pinned_lines(cx).is_some_and(|(_, lines, _)| {
            lines
                .iter()
                .filter(|l| l.kind != crate::sticky_lines::PinKind::File)
                .all(|l| l.styles.iter().any(|(_, style)| style.color.is_some()))
        })
    }

    /// The pinned rows' pushes, for the tests.
    #[cfg(test)]
    pub fn pinned_pushes(&mut self, cx: &mut Context<Self>) -> Vec<f32> {
        self.pinned_lines(cx)
            .map(|(_, _, pushes)| pushes.into_iter().map(f32::from).collect())
            .unwrap_or_default()
    }

    /// A section's laid-out row height, for the tests.
    #[cfg(test)]
    pub fn row_height(&self, path: &str, cx: &App) -> Option<f32> {
        let (editor, _) = self.editors.borrow().get(path).cloned()?;
        editor.read(cx).line_height().map(f32::from)
    }

    /// The file at the top of the view, for the tests.
    #[cfg(test)]
    pub fn current_or_top_file(&self) -> Option<String> {
        self.files
            .get(self.list.logical_scroll_top().item_ix)
            .cloned()
    }

    /// The list's visible area in window coordinates, for the tests.
    #[cfg(test)]
    pub fn viewport(&self) -> gpui::Bounds<gpui::Pixels> {
        self.list.viewport_bounds()
    }

    /// An arrow key at the edge of a file: off its first or last row, or
    /// past its first or last character, the caret carries on into the
    /// neighbouring file — the manuscript reads as one text, so it moves as
    /// one (maintainer, 2026-10-07). Keeps its column across Up and Down.
    /// `false` when the key is the section's own business: not at an edge,
    /// a selection to collapse, a completion or code-action menu open, or
    /// no file beyond.
    fn cross_file(&mut self, dir: Cross, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((path, editor)) = self
            .editors
            .borrow()
            .iter()
            .find(|(_, (editor, _))| editor.read(cx).focus_handle(cx).is_focused(window))
            .map(|(path, (editor, _))| (path.clone(), editor.clone()))
        else {
            return false;
        };
        let state = editor.read(cx);
        if !state.selected_range().is_empty()
            || state.completion_menu_state().open
            || state.code_action_menu_state().open
        {
            return false;
        }
        let text = state.value();
        let (cursor, len) = (state.cursor(), text.len());
        let row = |offset: usize| state.range_to_bounds(&(offset..offset)).map(|b| b.top());
        let at_edge = match dir {
            Cross::Left => cursor == 0,
            Cross::Right => cursor == len,
            Cross::Up => row(cursor).is_some() && row(cursor) == row(0),
            Cross::Down => row(cursor).is_some() && row(cursor) == row(len),
        };
        if !at_edge {
            return false;
        }
        let Some(index) = self.files.iter().position(|f| *f == path) else {
            return false;
        };
        let target = match dir {
            Cross::Up | Cross::Left => index.checked_sub(1),
            Cross::Down | Cross::Right => Some(index + 1),
        };
        let Some(target) = target.and_then(|i| self.files.get(i)).cloned() else {
            return false;
        };
        let column = text[crate::sticky_lines::line_start(&text, cursor)..cursor]
            .chars()
            .count();
        let mounted = self.editors.borrow().get(&target).map(|(e, _)| e.clone());
        let target_text = match &mounted {
            Some(editor) => editor.read(cx).value().to_string(),
            None => self
                .project
                .read(cx)
                .loaded_source(&target)
                .unwrap_or_default()
                .to_owned(),
        };
        // The same column on the line it lands on, or that line's end.
        let on_line = |start: usize| {
            let line = &target_text[start..];
            let line = &line[..line.find('\n').unwrap_or(line.len())];
            start
                + line
                    .char_indices()
                    .nth(column)
                    .map_or(line.len(), |(at, _)| at)
        };
        let offset = match dir {
            Cross::Left => target_text.len(),
            Cross::Right => 0,
            Cross::Up => on_line(crate::sticky_lines::line_start(
                &target_text,
                target_text.len(),
            )),
            Cross::Down => on_line(0),
        };
        match mounted {
            Some(editor) => {
                let handle = editor.read(cx).focus_handle(cx);
                editor.update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
                window.focus(&handle, cx);
                self.follow_caret(target, editor, cx);
            }
            None => {
                self.pending_focus = Some(target.clone());
                self.reveal_span(&target, offset..offset, cx);
            }
        }
        true
    }

    /// Re-read every section's knot and stitch lines for its gutter's ▶,
    /// and its fold candidates.
    fn refresh_play_lines(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<(String, Entity<EditorState>)> = self
            .editors
            .borrow()
            .iter()
            .map(|(path, (editor, _))| (path.clone(), editor.clone()))
            .collect();
        for (path, editor) in editors {
            if let Some(marks) = self.gutters.borrow().get(&path).cloned() {
                crate::gutter::refresh_headers(&marks, &editor, &self.project, &path, cx);
            }
            if let Some(cell) = self.folds.borrow().get(&path).cloned() {
                crate::document::request_folds(&self.project, &path, &editor, &cell, cx);
            }
        }
    }

    /// Read every section's breakpoints afresh and redraw its gutter.
    fn refresh_gutters(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<(String, Entity<EditorState>)> = self
            .editors
            .borrow()
            .iter()
            .map(|(path, (editor, _))| (path.clone(), editor.clone()))
            .collect();
        for (path, editor) in editors {
            if let Some(marks) = self.gutters.borrow().get(&path) {
                marks.refresh(self.project.read(cx), &path, cx);
            }
            editor.update(cx, |_, cx| cx.notify());
        }
    }

    /// Ask the worker for a file's outline, unless it is held or asked for.
    fn request_outline(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.outlines.contains_key(path) || !self.outline_pending.insert(path.to_owned()) {
            return;
        }
        let query = self.project.read(cx).query(
            QueryKind::Scopes {
                path: path.to_owned(),
            },
            cx,
        );
        let path = path.to_owned();
        cx.spawn(async move |this, cx| {
            let answer = query.await;
            let _ = this.update(cx, |this, cx| {
                this.outline_pending.remove(&path);
                if let Ok(QueryResult::Scopes(found)) = answer {
                    this.outlines.insert(path, found);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The knot and stitch lines to pin at the top of the view: the file
    /// whose section is there, and the headers above its top line. `None`
    /// while that file's chapter break is still on screen.
    fn pinned_lines(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(
        String,
        Vec<crate::sticky_lines::PinnedLine>,
        Vec<gpui::Pixels>,
    )> {
        let top = self.list.logical_scroll_top();
        let path = self.files.get(top.item_ix)?.clone();
        let into_text = top.offset_in_item - px(SEPARATOR_HEIGHT);
        if into_text <= px(0.) {
            return None;
        }
        if !self.outlines.contains_key(&path) {
            self.request_outline(&path, cx);
        }
        let symbols = self.outlines.get(&path).map_or(&[][..], Vec::as_slice);
        let (editor, _) = self.editors.borrow().get(&path).cloned()?;
        let state = editor.read(cx);
        let len = state.value().len();
        // The file's own row, on top: its chapter break, pinned once that
        // has scrolled away, and pushed off by the next file's.
        let file = crate::sticky_lines::PinnedLine {
            offset: 0,
            line: 0,
            text: path.clone(),
            kind: crate::sticky_lines::PinKind::File,
            styles: Vec::new(),
            end: len,
        };
        let (pinned, pushes) =
            crate::sticky_lines::pin(state, symbols, into_text, 0..len, Some(file))?;
        Some((path, pinned, pushes))
    }

    /// Where the pinned lines sit and in what face: over the text column,
    /// in the sections' own rows.
    fn pinned_geometry(&self, path: &str, cx: &App) -> Option<crate::sticky_lines::Geometry> {
        let (editor, _) = self.editors.borrow().get(path).cloned()?;
        let state = editor.read(cx);
        let text_left = state.range_to_bounds(&(0..0))?.left();
        let column = state.input_bounds();
        let view_left = self.list.viewport_bounds().left();
        let theme = cx.theme();
        Some(crate::sticky_lines::Geometry {
            text_left: text_left - view_left,
            column: (column.left() - view_left, column.size.width),
            line_height: px(self
                .measured_line_height
                .unwrap_or_else(|| f32::from(theme.mono_font_size) * LINE_HEIGHT_FACTOR)),
            font: if self.read.on.get() {
                theme.font_family.clone()
            } else {
                theme.mono_font_family.clone()
            },
            font_size: theme.mono_font_size,
            folds: true,
        })
    }

    /// The parameter hint, for the tests.
    #[cfg(test)]
    pub fn signature_hint(&self) -> &crate::signature_help::SignatureHint {
        &self.signature
    }

    /// `path`'s section's editor.
    #[cfg(test)]
    pub fn section_editor(&self, path: &str) -> Option<Entity<EditorState>> {
        self.editors
            .borrow()
            .get(path)
            .map(|(editor, _)| editor.clone())
    }

    /// `path`'s section's focus handle — where a click puts the keyboard.
    #[cfg(test)]
    pub fn section_focus(&self, path: &str, cx: &App) -> Option<gpui::FocusHandle> {
        let (editor, _) = self.editors.borrow().get(path).cloned()?;
        Some(editor.read(cx).focus_handle(cx))
    }

    /// `path`'s section as the tests see it: its editor, whether that has
    /// the keyboard, the height it was given, and the height its rows need.
    #[cfg(test)]
    pub fn probe_section(
        &self,
        path: &str,
        window: &Window,
        cx: &App,
    ) -> Option<(gpui::EntityId, bool, f32, f32)> {
        let (editor, height) = self.editors.borrow().get(path).cloned()?;
        let state = editor.read(cx);
        let rows = state.display_row_count().max(1) as f32;
        let line = f32::from(state.line_height()?);
        let focused = state.focus_handle(cx).is_focused(window);
        let trailing = if self.files.last().is_some_and(|f| f == path) {
            TRAILING_ROWS as f32
        } else {
            0.
        };
        Some((
            editor.entity_id(),
            focused,
            height,
            (rows + trailing) * line,
        ))
    }

    /// Whether the manuscript holds `path` — the story's own files; not
    /// `std`, not `brink.toml`.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.files.iter().any(|f| f == path)
    }

    /// The file the author is in: where the caret is, or — before any
    /// section has had focus — the file at the top of the scroller.
    #[must_use]
    pub fn current_file(&self) -> Option<&str> {
        self.caret
            .as_ref()
            .map(|(path, _)| path.as_str())
            .or_else(|| {
                self.files
                    .get(self.list.logical_scroll_top().item_ix)
                    .map(String::as_str)
            })
    }

    /// Where the caret is, if a section has had focus.
    #[must_use]
    pub fn caret(&self) -> Option<(&str, usize)> {
        self.caret.as_ref().map(|(path, at)| (path.as_str(), *at))
    }

    /// A section took focus: follow its caret from now on. Its editor is
    /// observed rather than polled, and the event goes out only when the
    /// caret actually moved — the editor also notifies to blink.
    fn follow_caret(&mut self, path: String, editor: Entity<EditorState>, cx: &mut Context<Self>) {
        let offset = editor.read(cx).cursor();
        self.set_caret(&path, offset, cx);
        self.caret_watch = Some(cx.observe(&editor, move |this, editor, cx| {
            let offset = editor.read(cx).cursor();
            this.set_caret(&path, offset, cx);
        }));
    }

    fn set_caret(&mut self, path: &str, offset: usize, cx: &mut Context<Self>) {
        if self
            .caret
            .as_ref()
            .is_some_and(|(p, o)| p == path && *o == offset)
        {
            return;
        }
        self.caret = Some((path.to_owned(), offset));
        // The arrow keys move it off screen as readily as typing does; the
        // view follows after layout (decision log 2026-10-07).
        self.reveal_caret.set(REVEAL_TRIES);
        cx.notify();
        cx.emit(ManuscriptEvent::Caret {
            path: path.to_owned(),
            offset,
        });
    }

    /// Put the last analysis's problems on every section that exists —
    /// the squiggles Script mode's tabs already draw. The Write view never
    /// had them: a bad reference raised the problem count and nothing in
    /// the text said where.
    fn refresh_diagnostics(&mut self, cx: &mut Context<Self>) {
        let sections: Vec<(String, Entity<EditorState>)> = self
            .editors
            .borrow()
            .iter()
            .map(|(path, (editor, _))| (path.clone(), editor.clone()))
            .collect();
        for (path, editor) in sections {
            apply_diagnostics(&self.project, &path, &editor, &self.prose, cx);
        }
    }

    /// Whether the Read view is on.
    #[must_use]
    pub fn is_read(&self) -> bool {
        self.read.on.get()
    }

    /// Turn the Read view on or off. A repaint, not a rebuild: the
    /// highlighters read the flag on every paint, and the font is set where
    /// each section's editor is drawn.
    pub fn set_read(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.read.on.replace(on) == on {
            return;
        }
        if on {
            self.sync_prose(cx);
        }
        let faint = on.then(|| crate::document::read_faint(cx));
        for (editor, _) in self.editors.borrow().values() {
            editor.update(cx, |state, cx| apply_read_chrome(state, faint, cx));
        }
        cx.notify();
    }

    /// Copy the last analysis's prose into the shared Read state. Only
    /// while Read is on: off, nothing reads it.
    fn sync_prose(&mut self, cx: &mut Context<Self>) {
        let prose = self
            .project
            .read(cx)
            .prose_spans()
            .iter()
            .map(|(path, spans)| {
                let spans = spans.iter().map(|&(a, b)| a as usize..b as usize).collect();
                (path.clone(), spans)
            })
            .collect();
        *self.read.prose.borrow_mut() = prose;
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
        read: &ReadCell,
        prose: &ProseCache,
        gutters: &Gutters,
        folds: &Folds,
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
                // Every section gets the same gutter (maintainer, 2026-09-05):
                // a 40-line file beside a 2,000-line one would otherwise
                // start its text two digits further left, and the manuscript
                // reads as one column or it does not read at all.
                .min_line_number_digits(MANUSCRIPT_GUTTER_DIGITS)
                .language("brink")
                // Prose wraps (maintainer, 2026-09-05); the section is
                // re-sized to its wrapped rows — see `section_height`.
                .soft_wrap(true)
                // Sections fold as tabs do (maintainer, 2026-10-08): a
                // fold hides rows, so a section is sized by the rows it
                // SHOWS (`display_row_count`), and re-measured whenever
                // that count moves.
                .folding(true)
                // See `TRAILING_ROWS`.
                .scroll_beyond_last_line(Some(trailing));
            let fold_cell = crate::document::FoldCell::default();
            folds
                .borrow_mut()
                .insert(key.to_string(), fold_cell.clone());
            state.set_highlighter_factory(
                manuscript_highlighter_factory(weak.clone(), key.clone(), fold_cell, read.clone()),
                cx,
            );
            // A section mounted while Read is on starts in its chrome.
            let faint = read.on.get().then(|| crate::document::read_faint(cx));
            apply_read_chrome(&mut state, faint, cx);

            // The same providers a tab's editor gets — navigation must not
            // depend on which view a file is read in. What differs is the
            // sink: a target is shown by scrolling the manuscript to it.
            let origin = cx.entity().entity_id();
            crate::document::install_language_providers(&mut state, &weak, key.clone(), origin);
            let navigate: crate::navigation::Navigate = Rc::new(move |path, span, _window, cx| {
                let _ = manuscript.update(cx, |this, cx| this.reveal_span(path, span, cx));
            });
            crate::navigation::install(&mut state, project, key.clone(), origin, navigate);
            // The breakpoint column, as a tab's editor has it.
            let marks = crate::gutter::install(&mut state, weak.clone(), key.clone(), cx);
            gutters.borrow_mut().insert(key.to_string(), marks);

            state.set_value(source, window, cx);
            state
        });
        // Its ▶ lines and its folds, from the analysis the project has.
        if let Some(marks) = gutters.borrow().get(path).cloned() {
            crate::gutter::refresh_headers(&marks, &state, project, path, cx);
        }
        if let Some(cell) = folds.borrow().get(path).cloned() {
            crate::document::request_folds(project, path, &state, &cell, cx);
        }
        // A fold or an unfold changes the rows the section shows, and its
        // height has to follow; watched as a count, so a caret blink or a
        // keystroke that moves no row costs the manuscript nothing.
        {
            let shown = Rc::new(std::cell::Cell::new(0usize));
            let me = me.clone();
            section_subs
                .borrow_mut()
                .push(cx.observe(&state, move |state, cx| {
                    let now = state.read(cx).display_row_count();
                    if shown.replace(now) != now {
                        let _ = me.update(cx, |_, cx| cx.notify());
                    }
                }));
        }

        // The spike got re-analysis as a side effect of the highlighter,
        // which called `sync` on every paint. That is exactly the
        // instrumentation-in-the-hot-path shape the design forbids, so the
        // edit is pushed explicitly instead.
        let edited_project = project.clone();
        let edited_path = path.to_owned();
        let following = me.clone();
        let focused_path = path.to_owned();
        section_subs.borrow_mut().push(cx.subscribe(
            &state,
            move |state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Focus) {
                    let _ = following.update(cx, |this, cx| {
                        this.follow_caret(focused_path.clone(), state.clone(), cx);
                    });
                }
                if matches!(event, InputEvent::Blur) {
                    let _ = following.update(cx, |this, cx| {
                        if this.signature.dismiss() {
                            cx.notify();
                        }
                    });
                }
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let origin = state.entity_id();
                    edited_project.update(cx, |project, cx| {
                        project.edit(&edited_path, text, Some(origin), cx);
                    });
                    // After the edit has gone to the worker: the hint asks
                    // about the text as it now is, and the worker answers
                    // in order.
                    let _ = following.update(cx, |this, cx| {
                        let project = this.project.clone();
                        crate::signature_help::SignatureHint::edited(
                            this,
                            |view: &mut ContinuousView| &mut view.signature,
                            &project,
                            &focused_path,
                            &state,
                            cx,
                        );
                    });
                }
            },
        ));

        // A section built after the analysis landed starts with its
        // problems; later analyses reach it through `refresh_diagnostics`.
        apply_diagnostics(project, path, &state, prose, cx);

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
        // The first frame after a theme change still reads the height from
        // before it: the sections have not drawn at the new size yet. Skip
        // that one frame, once — `take`, so a theme that leaves the height
        // unchanged adopts it next frame rather than waiting forever.
        if let Some(stale) = self.stale_line_height.take()
            && (stale - real).abs() < 0.01
        {
            cx.notify();
            return;
        }
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
            let rows = section.0.read(cx).display_row_count().max(1);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.adopt_measured_line_height(cx);
        self.remeasure_sections(cx);
        self.apply_pending_reveal(cx);
        if let Some(path) = self.pending_focus.clone()
            && let Some((editor, _)) = self.editors.borrow().get(&path).cloned()
        {
            self.pending_focus = None;
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }

        let surface = cx.theme().background;
        let files = self.files.clone();
        let count = files.len();
        let project = self.project.clone();
        let me = self.me.clone();
        let editors = self.editors.clone();
        let section_subs = self.section_subs.clone();
        let mounted = self.mounted.clone();
        let read = self.read.clone();
        let prose = self.prose.clone();
        let gutters = self.gutters.clone();
        let folds = self.folds.clone();
        // The Read view's face: the UI's proportional font, at the editor's
        // own size — so a row is the same height either way and only the
        // wrapping moves, which `remeasure_sections` already follows.
        let read_font = self.read.on.get().then(|| cx.theme().font_family.clone());
        let measured = self.measured_line_height;
        let column = column_width(window, cx);
        // The knot and stitch the top of the view is inside, pinned there.
        let pinned = {
            let (path, lines, pushes) = self
                .pinned_lines(cx)
                .map_or((None, Vec::new(), Vec::new()), |(path, lines, pushes)| {
                    (Some(path), lines, pushes)
                });
            let row = px(self
                .measured_line_height
                .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR));
            if self.pins.update(path.as_deref(), lines, pushes, row) {
                // Lines let go are drawn while they fade; a frame after
                // that stops drawing them.
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(crate::sticky_lines::PIN_OUT)
                        .await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            let top = self
                .files
                .get(self.list.logical_scroll_top().item_ix)
                .cloned();
            top.and_then(|top| {
                let geometry = self.pinned_geometry(&top, cx)?;
                let me = self.me.clone();
                let app: &App = cx;
                let file_row = |pin: &crate::sticky_lines::PinnedLine| {
                    let project = self.project.read(app);
                    let marks = FileMarks {
                        draft: project.is_draft(&pin.text),
                        entry: project.entry() == Some(pin.text.as_str()),
                        dirty: project.is_dirty(&pin.text),
                    };
                    title_row(&pin.text, marks, app).into_any_element()
                };
                crate::sticky_lines::render(
                    &self.pins,
                    &geometry,
                    move |pin, _, cx| {
                        let top = top.clone();
                        let at = pin.offset;
                        let file = pin.kind == crate::sticky_lines::PinKind::File;
                        let _ = me.update(cx, |this, cx| {
                            // The file's row goes to its chapter break.
                            if file {
                                this.reveal(&top, cx);
                            } else {
                                this.reveal_span(&top, at..at, cx);
                            }
                        });
                    },
                    &file_row,
                    app,
                )
            })
        };

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
                                &read,
                                &prose,
                                &gutters,
                                &folds,
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
                    let marks = {
                        let project = project.read(cx);
                        FileMarks {
                            draft: project.is_draft(&path),
                            entry: project.entry() == Some(path.as_str()),
                            dirty: project.is_dirty(&path),
                        }
                    };
                    v_flex()
                        .w_full()
                        .child(separator(&path, column, marks, &me, cx))
                        .child(
                            // The column: centred in the room there is,
                            // never wider than the window allows.
                            h_flex()
                                .w_full()
                                .justify_center()
                                .child(crate::editor_menu::install(
                                    Editor::new(&editor)
                                        .bordered(false)
                                        .appearance(false)
                                        .with_size(SECTION_SIZE)
                                        .when_some(read_font.clone(), |editor, font| {
                                            editor.font_family(font)
                                        })
                                        .when_some(column, |editor, width| {
                                            editor.w(width).max_w_full()
                                        })
                                        .when(column.is_none(), |editor| editor.w_full())
                                        .h(px(height)),
                                    crate::navigation::EditorSite {
                                        editor: editor.clone(),
                                        project: project.clone(),
                                        path: path.clone().into(),
                                    },
                                )),
                        )
                        .into_any_element()
                })
                .flex_1(),
            )
            // After layout: where the text starts, for the title bar's
            // crumb, and whether the caret is still on screen.
            .child({
                let editors = self.editors.clone();
                let anchor = self.crumb_anchor.clone();
                let last_view = self.last_view.clone();
                let reveal = self.reveal_caret.clone();
                // The rows pinned over the top of the view, which a caret
                // under them is as hidden by as by the edge.
                let covered = self.pins.shown_rows();
                let caret = self.caret.clone();
                let files = self.files.clone();
                let list = self.list.clone();
                let line_height = self
                    .measured_line_height
                    .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR);
                gpui::canvas(
                    move |bounds, window, cx| {
                        // Where the text starts, this frame: the column is
                        // centred in this view (as the sections lay it out)
                        // and the text sits the gutter's width into it. The
                        // editors only learn their bounds as they paint —
                        // after this — so theirs are a frame old, which
                        // would leave the crumb trailing a sliding column;
                        // but the gutter they give is the same either way.
                        // The column's left edge in a view of these bounds.
                        let column_left = |view: gpui::Bounds<gpui::Pixels>| match column {
                            Some(width) if width < view.size.width => {
                                view.left() + (view.size.width - width) / 2.
                            }
                            _ => view.left(),
                        };
                        // Last frame's view, the frame the editors' bounds
                        // are from: the text's offset into the column is
                        // read against it, and holds for this one.
                        let last = last_view.replace(Some(bounds));
                        if let Some(cell) = &anchor {
                            let top = files.get(list.logical_scroll_top().item_ix);
                            let into = top.zip(last).and_then(|(path, last)| {
                                let (editor, _) = editors.borrow().get(path).cloned()?;
                                let state = editor.read(cx);
                                let at = state.visible_offset_range()?.start;
                                let text = state.range_to_bounds(&(at..at))?.left();
                                Some(text - column_left(last))
                            });
                            let left = into.map(|into| column_left(bounds) + into);
                            if left.is_some() && cell.get() != left {
                                cell.set(left);
                                window.refresh();
                            }
                        }
                        let tries = reveal.get();
                        if tries > 0 {
                            let placed = caret.as_ref().is_none_or(|(path, offset)| {
                                keep_caret_on_screen(
                                    &list,
                                    &files,
                                    &editors.borrow(),
                                    path,
                                    *offset,
                                    line_height,
                                    covered,
                                    cx,
                                )
                            });
                            if placed {
                                reveal.set(0);
                            } else {
                                // Scrolled to the file; the next frame lays it
                                // out and places the caret exactly.
                                reveal.set(tries - 1);
                                window.refresh();
                            }
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
            })
            .children(pinned)
            .children(self.signature.render(cx))
            // Escape puts the parameter hint away, as the web's does.
            // The arrow keys at a file's edge carry on into the next file.
            // Captured: the section's own handler would move nowhere.
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.cross_file(Cross::Up, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.cross_file(Cross::Down, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveLeft, window, cx| {
                if this.cross_file(Cross::Left, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveRight, window, cx| {
                if this.cross_file(Cross::Right, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.signature.dismiss() {
                    cx.notify();
                }
            }))
    }
}

/// Each section's prose lints, keyed by path, with a fingerprint of the
/// text they were checked against.
///
/// A Script tab re-checks its prose on every analysis; the manuscript holds
/// every file it has ever scrolled past, and doing that for all of them on
/// every keystroke would spell-check the whole story per character. So a
/// file is checked again only when its text has changed since its lints
/// were taken, and the lints it already has are put back after each
/// analysis replaces the compiler's squiggles.
#[derive(Clone, Default)]
struct ProseCache(Rc<RefCell<HashMap<String, CheckedProse>>>);

/// A section's prose lints, and the fingerprint of the text they fit.
type CheckedProse = (u64, Vec<brink_gpui_model::prose::ProseLint>);

/// A fingerprint of a section's text, to tell whether its lints still fit.
fn fingerprint(source: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// Give `editor` the problems the last analysis found in `path`, replacing
/// what it had — as a Script tab does (`Document::refresh`) — and its prose
/// lints with them: the ones it has when its text is unchanged, a fresh
/// check when it has moved. A TODO note's band is its presentation, so it
/// gets no squiggle here either.
fn apply_diagnostics(
    project: &Entity<Project>,
    path: &str,
    editor: &Entity<EditorState>,
    prose: &ProseCache,
    cx: &mut App,
) {
    let (rope, source) = {
        let state = editor.read(cx);
        (state.text().clone(), state.value().to_string())
    };
    let index = brink_ir::LineIndex::new(&source);
    let mut diagnostics: Vec<lsp_types::Diagnostic> = project
        .read(cx)
        .diagnostics_for(path)
        .iter()
        .filter(|d| d.code != crate::todos::TODO_CODE)
        .map(|d| crate::document::to_lsp_diagnostic(d, &index))
        .collect();
    let print = fingerprint(&source);
    let cached = prose
        .0
        .borrow()
        .get(path)
        .filter(|(at, _)| *at == print)
        .map(|(_, lints)| crate::document::prose_diagnostics(lints, &source));
    let fresh = cached.is_some();
    diagnostics.extend(cached.unwrap_or_default());
    editor.update(cx, |state, cx| {
        if let Some(set) = state.diagnostics_mut() {
            set.reset(&rope);
            set.extend(diagnostics);
        }
        cx.notify();
    });
    if !fresh {
        check_prose(project, path, editor, prose, print, cx);
    }
}

/// Ask the worker for `path`'s prose lints, against text whose fingerprint
/// is `print`; keep them, report them to Problems, and put the section's
/// squiggles back together with them — unless the text has moved on again
/// meanwhile, when the next check is the one that counts.
fn check_prose(
    project: &Entity<Project>,
    path: &str,
    editor: &Entity<EditorState>,
    prose: &ProseCache,
    print: u64,
    cx: &mut App,
) {
    let query = project.read(cx).query(
        brink_gpui_model::query::QueryKind::Prose {
            path: path.to_owned(),
        },
        cx,
    );
    let (project, editor, prose, path) = (
        project.clone(),
        editor.clone(),
        prose.clone(),
        path.to_owned(),
    );
    cx.spawn(async move |cx| {
        let Ok(brink_gpui_model::query::QueryResult::Prose(lints)) = query.await else {
            return;
        };
        cx.update(|cx| {
            if fingerprint(editor.read(cx).value().as_ref()) != print {
                return;
            }
            project.update(cx, |project, cx| {
                crate::document::report_prose(project, &path, &lints, cx);
            });
            prose.0.borrow_mut().insert(path.clone(), (print, lints));
            apply_diagnostics(&project, &path, &editor, &prose, cx);
        });
    })
    .detach();
}

/// The editor chrome Read changes (W8): line numbers in `faint`, and no
/// current-line band. `None` puts the theme's back.
fn apply_read_chrome(
    state: &mut EditorState,
    faint: Option<gpui::Hsla>,
    cx: &mut Context<EditorState>,
) {
    state.set_line_number_color(faint, cx);
    state.set_active_line_highlight(faint.is_none(), cx);
}

/// The manuscript column's width (Settings ▸ Appearance ▸ Manuscript
/// width), or `None` for full width.
///
/// In characters of the editor's MONOSPACE face, the CSS `ch` — so it
/// follows ⌘= / ⌘-, and stays put when Read swaps in the proportional face
/// (which then fits more words in the same column). The gutter's digits
/// and its margin come on top, so the setting counts text, not chrome.
fn column_width(window: &Window, cx: &App) -> Option<gpui::Pixels> {
    let chars = brink_gpui_shell::settings::AppSettings::get(cx).manuscript_width;
    if chars <= 0. {
        return None;
    }
    let theme = cx.theme();
    let font = gpui::font(theme.mono_font_family.clone());
    let text = window.text_system();
    let ch = text
        .ch_advance(text.resolve_font(&font), theme.mono_font_size)
        .map_or(f32::from(theme.mono_font_size) * 0.6, f32::from);
    // The gutter: the breakpoint column, its digits, one column of
    // spacing, and the margins the kit sets either side of the text.
    let gutter = crate::gutter::COLUMN_WIDTH
        + (MANUSCRIPT_GUTTER_DIGITS as f32 + 1.) * ch
        + FOLD_COLUMN
        + 24.;
    Some(px(chars * ch + gutter))
}

/// Scroll so the caret at `offset` in `path` is on screen: below the
/// pinned rows covering the top (`covered`, plus one to spare) and a couple
/// of rows clear of the bottom. `true` once it is; `false` when its section
/// has never laid out, in which case the list is scrolled to it for the
/// next frame to settle.
///
/// Where the caret sits is counted in display rows — soft wrap and folds
/// included — from where the list put its section this frame: a line the
/// section has not laid out has no bounds, or stale ones, and acting on
/// those scrolled the view far past the caret.
#[expect(
    clippy::too_many_arguments,
    reason = "the canvas closure's captures, passed through"
)]
fn keep_caret_on_screen(
    list: &ListState,
    files: &[String],
    editors: &HashMap<String, Section>,
    path: &str,
    offset: usize,
    line_height: f32,
    covered: usize,
    cx: &App,
) -> bool {
    let Some(index) = files.iter().position(|f| f == path) else {
        return true;
    };
    let Some((editor, _)) = editors.get(path) else {
        return true;
    };
    let state = editor.read(cx);
    if state.line_height().is_none() {
        list.scroll_to(gpui::ListOffset {
            item_ix: index,
            offset_in_item: px(0.),
        });
        return false;
    }
    let text = state.value();
    let line = text
        .get(..offset)
        .map_or(0, |before| before.matches('\n').count());
    let start = crate::sticky_lines::line_start(&text, offset.min(text.len()));
    // Which wrapped row of its line the caret is on, when the line is laid
    // out (both bounds from the same layout, so their difference holds).
    let within = match (
        state.range_to_bounds(&(offset..offset)),
        state.range_to_bounds(&(start..start)),
    ) {
        (Some(at), Some(line_top)) => f32::from(at.top() - line_top.top()).max(0.),
        _ => 0.,
    };
    let in_item =
        SEPARATOR_HEIGHT + state.display_row_of_buffer_line(line) as f32 * line_height + within;
    let Some(item) = list.bounds_for_item(index) else {
        list.scroll_to(gpui::ListOffset {
            item_ix: index,
            offset_in_item: px((in_item - (covered + 1) as f32 * line_height).max(0.)),
        });
        return false;
    };
    let top = item.top() + px(in_item);
    let bottom = top + px(line_height);
    let view = list.viewport_bounds();
    let margin_top = px((covered + 1) as f32 * line_height);
    let margin_bottom = px(2. * line_height);
    if top < view.top() + margin_top {
        list.scroll_by(top - view.top() - margin_top);
    } else if bottom > view.bottom() - margin_bottom {
        list.scroll_by(bottom - view.bottom() + margin_bottom);
    }
    true
}

/// What a file's separator shows about it, read off the project.
#[derive(Clone, Copy, Default)]
struct FileMarks {
    /// Matched a `[project] drafts` glob (decision log: drafts are marked
    /// in their heading, never left out).
    draft: bool,
    /// The story's entry file, drawn with the entry drop as in the Binder.
    entry: bool,
    /// Edits not yet saved.
    dirty: bool,
}

/// Which way an arrow key would carry the caret out of its file.
#[derive(Clone, Copy)]
enum Cross {
    Up,
    Down,
    Left,
    Right,
}

/// What a separator's `⋯` menu asks of the manuscript's host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Play,
    OpenInScript,
    RevealInFiles,
}

/// The file's name as a chapter title: its path without the extension, in
/// capitals, tracked with thin spaces (GPUI text has no letter-spacing).
fn chapter_title(path: &str) -> String {
    let stem = path
        .rsplit_once('.')
        .map_or(path, |(stem, _)| stem)
        .to_uppercase();
    let mut out = String::with_capacity(stem.len() * 4);
    for (i, c) in stem.chars().enumerate() {
        if i > 0 {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out
}

/// A file's title as its chapter break draws it — its icon, its name in
/// tracked capitals, a DRAFT badge, an unsaved dot — and as its pinned row
/// repeats it.
fn title_row(path: &str, marks: FileMarks, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let draft_colour = brink_gpui_shell::theme::hsla(tokens.draft);
    let (icon, icon_colour) = if marks.draft {
        (icons::BrinkIcon::DropDraft, draft_colour)
    } else if marks.entry {
        (icons::BrinkIcon::DropEntryOutline, theme.primary)
    } else {
        (icons::BrinkIcon::Drop, theme.muted_foreground)
    };
    let name_colour = if marks.draft {
        draft_colour
    } else {
        theme.foreground.opacity(0.8)
    };
    h_flex()
        .relative()
        .items_center()
        .gap(px(8.))
        .font_family(theme.font_family.clone())
        .child(icons::icon(icon, px(13.), icon_colour))
        .child(
            div()
                .text_xs()
                .text_color(name_colour)
                .child(chapter_title(path)),
        )
        .when(marks.draft, |el| {
            el.child(
                div()
                    .px(px(4.))
                    .rounded(px(3.))
                    .bg(draft_colour.opacity(0.14))
                    .text_size(px(8.5))
                    .text_color(draft_colour)
                    .child("D\u{2009}R\u{2009}A\u{2009}F\u{2009}T"),
            )
        })
        .when(marks.dirty, |el| {
            el.child(
                div()
                    .size(px(5.))
                    .rounded_full()
                    .bg(theme.foreground.opacity(0.7)),
            )
        })
}

/// The boundary between two files: a chapter break (decision log
/// 2026-10-07). Space, then the file's icon and its name in tracked
/// capitals, centred over the column, over a short rule. An unsaved file
/// shows a dot; a draft is drawn in the draft colour with a small badge.
/// The `⋯` file menu shows on hover.
///
/// Nothing is pinned: the title bar's crumb names the cursor's file, and
/// the knot and stitch at the top of the view pin as their own lines.
fn separator(
    path: &str,
    column: Option<gpui::Pixels>,
    marks: FileMarks,
    me: &WeakEntity<ContinuousView>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let draft_colour = brink_gpui_shell::theme::hsla(tokens.draft);
    let rule = if marks.draft {
        draft_colour.opacity(0.45)
    } else {
        theme.border
    };
    let group = SharedString::from(format!("separator-{path}"));
    let menu = {
        let me = me.clone();
        let path = path.to_owned();
        Button::new(SharedString::from(format!("separator-menu-{path}")))
            .ghost()
            .xsmall()
            .icon(IconName::Ellipsis)
            .dropdown_menu(move |menu, _, _| {
                let emit: crate::symbol_menu::Emit = {
                    let me = me.clone();
                    std::rc::Rc::new(move |event, _, cx| {
                        let _ = me.update(cx, |_, cx| cx.emit(ManuscriptEvent::Outline(event)));
                    })
                };
                let act = |action: FileAction| {
                    let me = me.clone();
                    let path = path.clone();
                    move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                        let path = path.clone();
                        let _ = me.update(cx, |_, cx| {
                            cx.emit(ManuscriptEvent::File { path, action });
                        });
                    }
                };
                let copy = {
                    let path = path.clone();
                    move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
                    }
                };
                let menu = menu
                    .item(PopupMenuItem::new("Play from here").on_click(act(FileAction::Play)))
                    .item(
                        PopupMenuItem::new("Open in Script")
                            .on_click(act(FileAction::OpenInScript)),
                    )
                    .item(
                        PopupMenuItem::new("Reveal in Files")
                            .on_click(act(FileAction::RevealInFiles)),
                    )
                    .item(PopupMenuItem::new("Copy path").on_click(copy))
                    .separator();
                // Then what the Binder offers on the same file.
                crate::file_menu::build(
                    menu,
                    &crate::file_menu::Target::File { path: path.clone() },
                    &emit,
                )
            })
    };
    v_flex()
        .id(group.clone())
        .group(group.clone())
        .w_full()
        .h(px(SEPARATOR_HEIGHT))
        .pt(px(SEPARATOR_SPACE_ABOVE))
        .items_center()
        .gap(px(9.))
        .child(
            // The column, so the title centres over the text rather than
            // the window.
            h_flex()
                .when_some(column, |el, width| el.w(width).max_w_full())
                .when(column.is_none(), |el| el.w_full())
                .h(px(22.))
                .justify_center()
                .child(
                    // The title itself, and the menu hung off its right end
                    // out of the flow, so the title centres on its own.
                    title_row(path, marks, cx)
                        .h_full()
                        // Shown on hover, so a resting break is only its name.
                        .child(
                            div()
                                .absolute()
                                .left_full()
                                .top(px(1.))
                                .pl(px(6.))
                                .invisible()
                                .group_hover(group, |s| s.visible())
                                .child(menu),
                        ),
                ),
        )
        .child(div().w(px(36.)).h(px(1.)).bg(rule))
}
