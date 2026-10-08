//! The editor gutter's breakpoint column, Zed's layout (decision log
//! 2026-10-08): a column left of the line numbers where a press toggles a
//! breakpoint and hovering a line shows a faint dot where one would go. A
//! set breakpoint is a red dot and turns its line's number red; one that
//! bound to no code — it can never be hit — is a hollow ring, its number
//! left alone. Script's editors and Write's sections both have it.
//!
//! The kit draws the column ([`GutterMarks`]); this decides what is in it.
//! The marks are read off the project into a cell each editor shares with
//! its host, which refreshes it when the breakpoints change: the kit's
//! number-colour hook runs without the app at hand to ask.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    App, Context, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _, WeakEntity,
    Window, div, prelude::*, px,
};
use gpui_component::input::{EditorState, GUTTER_MARK_GROUP, GutterMarks};

use crate::project::Project;

/// The column's width, and its dot's.
const COLUMN: f32 = 16.;
const DOT: f32 = 10.;

/// How much wider the gutter is for the column — for a host that sizes
/// its text column from the gutter's width.
pub(crate) const COLUMN_WIDTH: f32 = COLUMN;

/// One editor's breakpoints, as its gutter draws them: each 1-based line
/// and whether it bound to any code; and the colour a set one is drawn in.
#[derive(Default)]
pub(crate) struct Marks {
    lines: RefCell<Vec<(u32, bool)>>,
    armed: Cell<Hsla>,
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

    /// Read the file's marks and the theme's colour afresh.
    pub(crate) fn refresh(&self, project: &Project, path: &str, cx: &App) {
        *self.lines.borrow_mut() = project.breakpoints_in(path);
        let tokens = brink_gpui_shell::theme::current(cx).tokens;
        self.armed.set(brink_gpui_shell::theme::hsla(tokens.error));
    }
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
            let dot = div().size(px(DOT)).rounded_full();
            let dot = match marks.at(line) {
                Some(true) => dot.bg(armed),
                // Can never be hit: a ring, not a dot.
                Some(false) => dot.border_1().border_color(armed),
                // Where one would go, under the pointer.
                None => dot
                    .bg(armed.opacity(0.35))
                    .invisible()
                    .group_hover(GUTTER_MARK_GROUP, |s| s.visible()),
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(dot)
                .into_any_element()
        })
    };
    let on_click = {
        let path = path.clone();
        Rc::new(move |line: usize, _: &mut Window, cx: &mut App| {
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
