//! The editor gutter's breakpoint column, Zed's layout (decision log
//! 2026-10-08): a column left of the line numbers where a press toggles a
//! breakpoint and hovering a line shows a faint dot where one would go. A
//! set breakpoint is a red dot and turns its line's number red; one that
//! bound to no code — it can never be hit — is a hollow ring, its number
//! left alone. Script's editors and Write's sections both have it.
//!
//! A knot's or stitch's header line plays from there instead: hovering it
//! shows ▶, and a press starts the story at that knot or stitch — the one
//! column shared with breakpoints, as the web editor rules it (2026-08-29:
//! breakpoints belong on statement lines, and a header line's breakpoint
//! is F9's).
//!
//! The kit draws the column ([`GutterMarks`]); this decides what is in it.
//! The marks are read off the project into a cell each editor shares with
//! its host, which refreshes it when the breakpoints change: the kit's
//! number-colour hook runs without the app at hand to ask.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use brink_gpui_model::query::{QueryKind, QueryResult};
use brink_ir::SymbolKind;
use gpui::{
    App, Context, Entity, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _,
    WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::input::{EditorState, GUTTER_MARK_GROUP, GutterMarks};
use gpui_component::{Icon, IconName, tooltip::Tooltip};

use crate::binder::PlayFromHere;
use crate::project::Project;

/// The column's width, and its dot's.
const COLUMN: f32 = 16.;
const DOT: f32 = 10.;
/// The ▶'s size.
const PLAY: f32 = 12.;

/// How much wider the gutter is for the column — for a host that sizes
/// its text column from the gutter's width.
pub(crate) const COLUMN_WIDTH: f32 = COLUMN;

/// One editor's breakpoints, as its gutter draws them: each 1-based line
/// and whether it bound to any code; and the colour a set one is drawn in.
#[derive(Default)]
pub(crate) struct Marks {
    lines: RefCell<Vec<(u32, bool)>>,
    armed: Cell<Hsla>,
    /// Knot and stitch header lines (0-based), with the path a play from
    /// there enters at (`knot` or `knot.stitch`).
    headers: RefCell<Vec<(usize, String)>>,
    play: Cell<Hsla>,
}

impl Marks {
    /// The mark on a 0-based line: `Some(bound)`, or `None`.
    fn at(&self, line: usize) -> Option<bool> {
        let number = u32::try_from(line + 1).ok()?;
        self.lines
            .borrow()
            .iter()
            .find(|(l, _)| *l == number)
            .map(|(_, bound)| *bound)
    }

    /// The play path of a 0-based header line, if it is one.
    fn header_at(&self, line: usize) -> Option<String> {
        self.headers
            .borrow()
            .iter()
            .find(|(l, _)| *l == line)
            .map(|(_, path)| path.clone())
    }

    /// Read the file's marks and the theme's colours afresh.
    pub(crate) fn refresh(&self, project: &Project, path: &str, cx: &App) {
        *self.lines.borrow_mut() = project.breakpoints_in(path);
        let tokens = brink_gpui_shell::theme::current(cx).tokens;
        self.armed.set(brink_gpui_shell::theme::hsla(tokens.error));
        self.play.set(brink_gpui_shell::theme::hsla(tokens.success));
    }
}

/// Ask the worker for the file's knots and stitches and mark their header
/// lines as playable — after each analysis, since they move as it changes.
/// A function is a knot that cannot be played into, so it gets none.
pub(crate) fn refresh_headers(
    marks: &Rc<Marks>,
    editor: &Entity<EditorState>,
    project: &Entity<Project>,
    path: &str,
    cx: &mut App,
) {
    let query = project.read(cx).query(
        QueryKind::DocumentSymbols {
            path: path.to_owned(),
        },
        cx,
    );
    let marks = marks.clone();
    let editor = editor.downgrade();
    cx.spawn(async move |cx| {
        let Ok(QueryResult::DocumentSymbols(symbols)) = query.await else {
            return;
        };
        let _ = editor.update(cx, |state, cx| {
            let text = state.value();
            let line_of = |at: u32| {
                text.get(..at as usize)
                    .map_or(0, |before| before.matches('\n').count())
            };
            let mut headers = Vec::new();
            for knot in symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Knot && !s.is_function)
            {
                headers.push((line_of(knot.start), knot.name.clone()));
                for stitch in knot
                    .children
                    .iter()
                    .filter(|s| s.kind == SymbolKind::Stitch)
                {
                    headers.push((
                        line_of(stitch.start),
                        format!("{}.{}", knot.name, stitch.name),
                    ));
                }
            }
            *marks.headers.borrow_mut() = headers;
            cx.notify();
        });
    })
    .detach();
}

/// Give an editor over `path` its breakpoint column. The returned marks
/// are the host's to [`Marks::refresh`] when the breakpoints change (and
/// to notify the editor after).
pub(crate) fn install(
    state: &mut EditorState,
    project: WeakEntity<Project>,
    path: SharedString,
    cx: &mut Context<EditorState>,
) -> Rc<Marks> {
    let marks = Rc::new(Marks::default());
    if let Some(project) = project.upgrade() {
        marks.refresh(project.read(cx), &path, cx);
    }
    let render = {
        let marks = marks.clone();
        Rc::new(move |line: usize, _: &mut Window, _: &mut App| {
            let armed = marks.armed.get();
            let cell = div().size_full().flex().items_center().justify_center();
            let dot = div().size(px(DOT)).rounded_full();
            let mark = match (marks.at(line), marks.header_at(line)) {
                (Some(true), _) => dot.bg(armed).into_any_element(),
                // Can never be hit: a ring, not a dot.
                (Some(false), _) => dot.border_1().border_color(armed).into_any_element(),
                // A header line plays from there, under the pointer.
                (None, Some(_)) => div()
                    .id(("play-from-here", line))
                    .invisible()
                    .group_hover(GUTTER_MARK_GROUP, |s| s.visible())
                    .tooltip(|window, cx| Tooltip::new("Play from here").build(window, cx))
                    .child(
                        Icon::new(IconName::Play)
                            .size(px(PLAY))
                            .text_color(marks.play.get()),
                    )
                    .into_any_element(),
                // Where a breakpoint would go, under the pointer.
                (None, None) => dot
                    .bg(armed.opacity(0.35))
                    .invisible()
                    .group_hover(GUTTER_MARK_GROUP, |s| s.visible())
                    .into_any_element(),
            };
            cell.child(mark).into_any_element()
        })
    };
    let on_click = {
        let path = path.clone();
        let marks = marks.clone();
        Rc::new(move |line: usize, window: &mut Window, cx: &mut App| {
            // A header line with no breakpoint on it plays from there.
            if marks.at(line).is_none()
                && let Some(play) = marks.header_at(line)
            {
                window.dispatch_action(Box::new(PlayFromHere { path: play }), cx);
                return;
            }
            let Some(project) = project.upgrade() else {
                return;
            };
            let number = u32::try_from(line + 1).unwrap_or(u32::MAX);
            project.update(cx, |project, cx| {
                project.toggle_breakpoint(&path, number, cx);
            });
        })
    };
    state.set_gutter_marks(
        Some(GutterMarks {
            width: px(COLUMN),
            render,
            on_click,
        }),
        cx,
    );
    let colours = {
        let marks = marks.clone();
        Rc::new(move |line: usize| (marks.at(line) == Some(true)).then(|| marks.armed.get()))
    };
    state.set_line_number_colors(Some(colours), cx);
    marks
}
