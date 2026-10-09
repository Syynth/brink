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
/// The trail's rail, at the column's left edge.
const RAIL: f32 = 3.;

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
    /// Where the running story has been in this file (Write mode's
    /// manuscript only), and the colours it is drawn in.
    trail: RefCell<Trail>,
    trail_colours: Cell<TrailColours>,
}

/// A file's lines as the running story has used them, 0-based (decision
/// log 2026-10-09, "Write-mode player revamp"): played lines carry a rail
/// in the gutter; the active line — the one on screen in the Player — a
/// band and a rail in the accent; a held line (stopped at, not yet
/// played) the same in amber.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Trail {
    pub played: Vec<usize>,
    pub active: Vec<usize>,
    pub held: Option<usize>,
    /// Lines rewound past (#3665): a dashed rail until the story moves on.
    pub undone: Vec<usize>,
    /// Where the next line starts — what ▶ plays. A promise, not a fact:
    /// an edit or a choice can change it, so it is only a faint band.
    pub next: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TrailColours {
    played: Hsla,
    active: Hsla,
    held: Hsla,
}

/// The part of the story's trail in `path`, as lines of `text` (0-based).
/// A source span can cover several lines (glue joins them), and each of
/// them is marked.
pub(crate) fn trail_in(trail: &crate::player::PlayTrail, path: &str, text: &str) -> Trail {
    let line_of = |at: u32| {
        text.get(..(at as usize).min(text.len()))
            .map_or(0, |before| before.matches('\n').count())
    };
    let lines = |loc: &brink_gpui_model::query::Location| {
        let first = line_of(loc.start);
        let last = line_of(loc.end.saturating_sub(1).max(loc.start));
        first..=last
    };
    let mut played: Vec<usize> = trail
        .played
        .iter()
        .filter(|loc| loc.path == path)
        .flat_map(lines)
        .collect();
    played.sort_unstable();
    played.dedup();
    Trail {
        played,
        active: trail
            .active
            .iter()
            .filter(|loc| loc.path == path)
            .flat_map(lines)
            .collect(),
        held: trail
            .held
            .as_ref()
            .filter(|(p, _)| p == path)
            .and_then(|(_, line)| (*line as usize).checked_sub(1)),
        undone: {
            let mut undone: Vec<usize> = trail
                .undone
                .iter()
                .filter(|loc| loc.path == path)
                .flat_map(lines)
                .collect();
            undone.sort_unstable();
            undone.dedup();
            undone
        },
        next: trail
            .next
            .as_ref()
            .filter(|(p, _)| p == path)
            .and_then(|(_, line)| (*line as usize).checked_sub(1)),
    }
}

/// How a line stands on the trail; the held line wins, then the active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stand {
    Next,
    Undone,
    Played,
    Active,
    Held,
}

impl Trail {
    fn stand(&self, line: usize) -> Option<Stand> {
        if self.held == Some(line) {
            Some(Stand::Held)
        } else if self.active.contains(&line) {
            Some(Stand::Active)
        } else if self.played.binary_search(&line).is_ok() {
            Some(Stand::Played)
        } else if self.undone.contains(&line) {
            Some(Stand::Undone)
        } else if self.next == Some(line) {
            Some(Stand::Next)
        } else {
            None
        }
    }
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
        let hsla = brink_gpui_shell::theme::hsla;
        self.armed.set(hsla(tokens.error));
        self.play.set(hsla(tokens.success));
        self.trail_colours.set(TrailColours {
            played: hsla(tokens.info).opacity(0.5),
            active: hsla(tokens.accent),
            held: hsla(tokens.warning),
        });
    }

    /// Take this file's trail; `true` when it changed (the editor then
    /// needs redrawing).
    pub(crate) fn set_trail(&self, trail: Trail) -> bool {
        if *self.trail.borrow() == trail {
            return false;
        }
        *self.trail.borrow_mut() = trail;
        true
    }

    /// A line's rail colour, if it is on the trail.
    fn rail(&self, line: usize) -> Option<Hsla> {
        let colours = self.trail_colours.get();
        self.trail
            .borrow()
            .stand(line)
            .and_then(|stand| match stand {
                Stand::Next => None,
                Stand::Undone | Stand::Played => Some(colours.played),
                Stand::Active => Some(colours.active),
                Stand::Held => Some(colours.held),
            })
    }

    /// Whether a line was rewound past.
    fn is_undone(&self, line: usize) -> bool {
        self.trail.borrow().stand(line) == Some(Stand::Undone)
    }

    /// A line's band, if it has one: the active and held lines do.
    fn band(&self, line: usize) -> Option<Hsla> {
        let colours = self.trail_colours.get();
        match self.trail.borrow().stand(line)? {
            Stand::Played | Stand::Undone => None,
            // Outlined by the manuscript (dashed, a promise), not banded.
            Stand::Next => None,
            Stand::Active => Some(colours.active.opacity(0.16)),
            Stand::Held => Some(colours.held.opacity(0.14)),
        }
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
            let undone = marks.is_undone(line);
            let rail = marks.rail(line).map(|colour| {
                let rail = div().absolute().left_0().top_0().bottom_0();
                if undone {
                    // Rewound past: the played rail, dashed.
                    rail.border_l(px(RAIL)).border_dashed().border_color(colour)
                } else {
                    rail.w(px(RAIL)).bg(colour)
                }
            });
            cell.relative()
                .children(rail)
                .child(mark)
                .into_any_element()
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
        Rc::new(move |line: usize| {
            if marks.at(line) == Some(true) {
                return Some(marks.armed.get());
            }
            // The active and held lines' numbers take their rail's colour.
            marks.band(line).and_then(|_| marks.rail(line))
        })
    };
    state.set_line_number_colors(Some(colours), cx);
    let bands = {
        let marks = marks.clone();
        Rc::new(move |line: usize| marks.band(line))
    };
    state.set_line_backgrounds(Some(bands), cx);
    marks
}
