//! Pinned structure lines: the blocks you are inside — knot, stitch,
//! choice, conditional and its branch, sequence — each held at the top of
//! the view once its opening line has scrolled past it, the JetBrains
//! "sticky lines" (decision log 2026-10-07). Write's manuscript and
//! Script's editors both draw them; the manuscript adds a row for the file
//! itself above them, drawn like its chapter break.
//!
//! What to pin is read off the file's scopes (the worker's structural
//! projection), not its text, so a `.brink` file's structure pins the same
//! way an `.ink` file's does. Where the top
//! of the view is comes from the editor's own layout: a line's height in
//! the editor's content, compared with how far the host has scrolled.

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Pixels, SharedString, Window, div, prelude::*,
    px,
};
use gpui_component::{ActiveTheme as _, input::EditorState};

use brink_gpui_model::query::{Scope, ScopeKind};

/// What a pinned row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinKind {
    /// The file itself: the manuscript's chapter break, pinned.
    File,
    Knot,
    /// A stitch's header, under its knot's.
    Stitch,
    Choice,
    /// A conditional's or sequence's opening line.
    Block,
    /// A conditional's or sequence's branch: its `- …` line.
    Branch,
}

impl PinKind {
    fn of(kind: ScopeKind) -> Self {
        match kind {
            ScopeKind::Knot => Self::Knot,
            ScopeKind::Stitch => Self::Stitch,
            ScopeKind::Choice => Self::Choice,
            ScopeKind::Conditional | ScopeKind::Sequence => Self::Block,
            ScopeKind::Branch => Self::Branch,
        }
    }
}

/// The most scope rows that pin (the file's row aside): the outermost win,
/// as the blocks you are deepest in matter least for where you are.
pub(crate) const MAX_SCOPE_ROWS: usize = 10;

/// One pinned row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PinnedLine {
    /// Where the header line starts: what a click goes to. The file's
    /// start, for the file's row.
    pub offset: usize,
    /// Its line number, from zero.
    pub line: usize,
    /// The line as written, trailing whitespace trimmed; the file's path,
    /// for the file's row (which its host draws).
    pub text: String,
    pub kind: PinKind,
    /// Where its knot or stitch ends: the next one's header, which pushes
    /// this row up out of its way as it arrives.
    pub end: usize,
}

/// Where the line holding `offset` starts.
pub(crate) fn line_start(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .and_then(|before| before.rfind('\n'))
        .map_or(0, |at| at + 1)
}

fn header(text: &str, scope: &Scope) -> PinnedLine {
    let start = line_start(text, scope.start as usize);
    let end = text[start..].find('\n').map_or(text.len(), |at| start + at);
    PinnedLine {
        offset: start,
        line: text[..start].matches('\n').count(),
        text: text[start..end].trim_end().to_owned(),
        kind: PinKind::of(scope.kind),
        end: scope.end as usize,
    }
}

/// How far each pinned row is pushed up (zero or less), JetBrains-style:
/// a row never overlaps the header that ends its knot or stitch, so the
/// next one slides it out of the way rather than replacing it outright.
/// Each row carries the rows under it (a file its knot, a knot its
/// stitch), so a row can go sooner than its parent but never later.
/// `distance(offset)` is how far below the top of the view the line at
/// `offset` starts — `None` when it is not laid out, which is far enough.
pub(crate) fn pushes(
    pinned: &[PinnedLine],
    row: Pixels,
    distance: impl Fn(usize) -> Option<Pixels>,
) -> Vec<Pixels> {
    let mut pushes: Vec<Pixels> = Vec::with_capacity(pinned.len());
    let strip = row * pinned.len() as f32;
    for pin in pinned {
        // A row and every row under it clear the header that ends it.
        let own = distance(pin.end).map_or(px(0.), |d| (d - strip).min(px(0.)));
        pushes.push(pushes.last().map_or(own, |parent| own.min(*parent)));
    }
    pushes
}

/// The blocks to pin, outermost first. `rows[r]` is the start of the line
/// just under `r` pinned rows: each level of nesting is judged there, since
/// a block's opening line slides under the rows above it before it reaches
/// the top, and would otherwise vanish for a line. At each level the
/// outermost block inside the last one pinned is taken that holds that line
/// and opened above it — or, failing that, the one the line a row up is
/// still inside, which the next opening line then pushes out of the way
/// rather than replacing outright. Only blocks of more than one line pin,
/// and never two on the same line (a conditional and the first branch
/// written on its brace line).
pub(crate) fn pinned_at(scopes: &[Scope], text: &str, rows: &[usize]) -> Vec<PinnedLine> {
    let line_of = |at: usize| line_start(text, at.min(text.len()));
    let spans_lines = |s: &Scope| {
        let last = (s.end as usize).saturating_sub(1).max(s.start as usize);
        line_of(last) > line_of(s.start as usize)
    };
    let mut pinned: Vec<PinnedLine> = Vec::new();
    let mut parent: Option<&Scope> = None;
    for (r, &at) in rows.iter().enumerate() {
        let pick = |at: usize| {
            scopes
                .iter()
                .filter(|s| {
                    let opens = line_of(s.start as usize);
                    parent.is_none_or(|p| *s != p && p.start <= s.start && s.end <= p.end)
                        && !pinned.iter().any(|pin| pin.offset == opens)
                        && spans_lines(s)
                        && (s.start as usize) <= at
                        && at < s.end as usize
                        && opens < at
                })
                .min_by_key(|s| (s.start, std::cmp::Reverse(s.end)))
        };
        let found = pick(at).or_else(|| r.checked_sub(1).and_then(|up| pick(rows[up])));
        let Some(scope) = found else {
            break;
        };
        pinned.push(header(text, scope));
        parent = Some(scope);
    }
    pinned
}

/// How many rows will pin over `offset` once it is near the top: one per
/// block it is inside that opened on an earlier line, one per line at most,
/// up to [`MAX_SCOPE_ROWS`].
pub(crate) fn rows_over(scopes: &[Scope], text: &str, offset: usize) -> usize {
    let line = line_start(text, offset.min(text.len()));
    let mut lines: Vec<usize> = scopes
        .iter()
        .filter(|s| (s.start as usize) <= offset && offset < s.end as usize)
        .map(|s| line_start(text, s.start as usize))
        .filter(|opens| *opens < line)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines.len().min(MAX_SCOPE_ROWS)
}

/// The start of the line at `content_y` in the editor's content (zero at
/// its first line, whatever it has scrolled), looking only at lines that
/// start inside `within` — the ones the editor has laid out.
pub(crate) fn line_at(
    state: &EditorState,
    content_y: Pixels,
    within: Range<usize>,
) -> Option<usize> {
    let text = state.value();
    let origin = state.text_bounds()?.top() + state.scroll_offset().y;
    let mut starts: Vec<usize> = Vec::new();
    let first = line_start(&text, within.start.min(text.len()));
    starts.push(first);
    let end = within.end.min(text.len());
    starts.extend(
        text[first..end]
            .match_indices('\n')
            .map(|(at, _)| first + at + 1)
            .filter(|at| *at < end),
    );
    let top_of = |offset: usize| {
        state
            .range_to_bounds(&(offset..offset))
            .map(|b| b.top() - origin)
    };
    let count = starts.partition_point(|start| top_of(*start).is_some_and(|y| y <= content_y));
    count.checked_sub(1).map(|i| starts[i])
}

/// Where the line at `offset` starts in the editor's content (zero at its
/// first line, whatever it has scrolled); `None` when it is not laid out.
pub(crate) fn content_top(state: &EditorState, offset: usize) -> Option<Pixels> {
    let origin = state.text_bounds()?.top() + state.scroll_offset().y;
    state
        .range_to_bounds(&(offset..offset))
        .map(|b| b.top() - origin)
}

/// What to pin with the top of the view at `top` in the editor's content,
/// looking at lines in `within`, and how far each row is pushed. `file`
/// is the host's row for the file itself, which goes on top; the knot and
/// stitch are then judged a row lower, under it.
pub(crate) fn pin(
    state: &EditorState,
    scopes: &[Scope],
    top: Pixels,
    within: Range<usize>,
    file: Option<PinnedLine>,
) -> Option<(Vec<PinnedLine>, Vec<Pixels>)> {
    let row = state.line_height()?;
    let lead = row * usize::from(file.is_some()) as f32;
    // The line under each number of pinned rows, as far as there are lines.
    let rows: Vec<usize> = (0..MAX_SCOPE_ROWS)
        .map_while(|r| line_at(state, top + lead + row * r as f32, within.clone()))
        .collect();
    if rows.is_empty() {
        return None;
    }
    let text = state.value();
    let mut pinned: Vec<PinnedLine> = file.into_iter().collect();
    pinned.extend(pinned_at(scopes, &text, &rows));
    let pushes = pushes(&pinned, row, |offset| {
        content_top(state, line_start(&text, offset)).map(|y| y - top)
    });
    Some((pinned, pushes))
}

/// Where a host draws its pinned lines, in the overlay's own coordinates.
pub(crate) struct Geometry {
    /// The text's left edge.
    pub text_left: Pixels,
    pub line_height: Pixels,
    /// The editor's face for them: the code face, or the Read view's.
    pub font: SharedString,
    pub font_size: Pixels,
    /// Whether the editor's gutter has a fold column between the numbers
    /// and the text (Script's does; the manuscript's sections do not).
    pub folds: bool,
    /// The text column's left edge and width, for the file's row to centre
    /// over, as the chapter break does.
    pub column: (Pixels, Pixels),
}

impl Geometry {
    /// Where a line number ends, as the kit's gutter lays it out
    /// (`input/base/element.rs`): `LINE_NUMBER_RIGHT_MARGIN` (10px) short
    /// of the text, and the fold column's `FOLD_ICON_HITBOX_WIDTH` (18px)
    /// shorter again when there is one.
    fn number_right(&self) -> Pixels {
        self.text_left - px(10.) - if self.folds { px(18.) } else { px(0.) }
    }
}

/// How long a pinned line takes to come in, and to go.
const PIN_IN: Duration = Duration::from_millis(140);
pub(crate) const PIN_OUT: Duration = Duration::from_millis(120);

/// What a host has pinned, and what it has just let go of: a line that
/// unpins stays drawn at its row for [`PIN_OUT`] while it fades, which is
/// all the out-animation needs.
#[derive(Default)]
pub(crate) struct Pins {
    /// The file the pins are in. Another file's lines are other lines,
    /// whatever their offsets.
    path: Option<String>,
    shown: Vec<PinnedLine>,
    /// Each shown row's push, from [`pushes`].
    push: Vec<Pixels>,
    /// Let go: the line, its row, its push then, and when.
    leaving: Vec<(PinnedLine, usize, Pixels, Instant)>,
    /// Bumped each time the strip appears from nothing, so its fade-in
    /// runs again rather than resuming a finished one.
    showing: u64,
    /// Bumped per line let go, for the same reason.
    released: u64,
}

impl Pins {
    /// Take the lines to pin now. `true` when some were let go: the host
    /// asks for a frame after [`PIN_OUT`], to stop drawing them.
    pub(crate) fn update(
        &mut self,
        path: Option<&str>,
        next: Vec<PinnedLine>,
        push: Vec<Pixels>,
        row: Pixels,
    ) -> bool {
        let now = Instant::now();
        self.leaving
            .retain(|(_, _, _, at)| now.duration_since(*at) < PIN_OUT);
        let same_file = self.path.as_deref() == path;
        let mut released = false;
        for (index, pin) in self.shown.iter().enumerate() {
            if !same_file
                || !next
                    .iter()
                    .any(|n| n.offset == pin.offset && n.kind == pin.kind)
            {
                let pushed = self.push.get(index).copied().unwrap_or(px(0.));
                // Pushed clean out of view, it has already gone: fading it
                // at its row would bring it back for a moment.
                if row * (index + 1) as f32 + pushed <= px(0.) {
                    continue;
                }
                self.leaving.push((pin.clone(), index, pushed, now));
                released = true;
            }
        }
        if released {
            self.released += 1;
        }
        if self.shown.is_empty() && !next.is_empty() {
            self.showing += 1;
        }
        self.path = path.map(str::to_owned);
        self.shown = next;
        self.push = push;
        released
    }

    /// How many rows are pinned now.
    pub(crate) fn shown_rows(&self) -> usize {
        self.shown.len()
    }

    fn is_empty(&self) -> bool {
        self.shown.is_empty() && self.leaving.is_empty()
    }
}

/// The pinned lines, as an overlay for the top of the host's view: each
/// header on the editor's surface, in its colour, with its line number,
/// with a soft shadow below. A line fades and drops in as it pins and
/// fades out as it lets go; the strip itself fades in and out with its
/// shadow. A click goes to the header (`on_click` gets the row). The
/// file's row, which only the host knows how to draw, comes from
/// `file_row`. `None` when nothing is drawn.
pub(crate) fn render(
    pins: &Pins,
    geometry: &Geometry,
    on_click: impl Fn(&PinnedLine, &mut Window, &mut App) + Clone + 'static,
    file_row: &dyn Fn(&PinnedLine) -> AnyElement,
    cx: &App,
) -> Option<AnyElement> {
    if pins.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let hsla = brink_gpui_shell::theme::hsla;
    let (knot, stitch, number, surface, prose, marker) = (
        hsla(tokens.syn_namespace),
        hsla(tokens.syn_function),
        theme.muted_foreground,
        theme.background,
        theme.foreground,
        hsla(tokens.syn_marker.unwrap_or(tokens.syn_operator)),
    );
    // Opaque: a pinned row covers the text scrolled under it, hovered too.
    let hover = surface.blend(theme.muted.opacity(0.35));
    let row = |pin: &PinnedLine| {
        let base = div()
            .relative()
            .w_full()
            .h(geometry.line_height)
            .bg(surface);
        match pin.kind {
            // Centred over the column, as the chapter break it stands for.
            PinKind::File => base.child(
                div()
                    .absolute()
                    .left(geometry.column.0)
                    .w(geometry.column.1)
                    .h_full()
                    .flex()
                    .justify_center()
                    .items_center()
                    .child(file_row(pin)),
            ),
            kind => base
                .child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .w(geometry.number_right())
                        .text_right()
                        .text_color(number)
                        .child((pin.line + 1).to_string()),
                )
                .child(
                    div()
                        .absolute()
                        .left(geometry.text_left)
                        .right(px(0.))
                        .truncate()
                        .text_color(match kind {
                            PinKind::Knot => knot,
                            PinKind::Stitch => stitch,
                            _ => prose,
                        })
                        .child(line_text(pin, marker)),
                ),
        }
    };
    let key = |pin: &PinnedLine| format!("{:?}-{}", pin.kind, pin.offset);
    // The strip moves with the knot's push; a stitch row's own push is on
    // top of that.
    let strip_push = pins.push.first().copied().unwrap_or(px(0.));
    // Deepest first, each at its own row: a stitch pushed up slides
    // behind its knot's row, which paints over it.
    let line_height = geometry.line_height;
    let shown = pins.shown.iter().enumerate().rev().map(|(index, pin)| {
        let on_click = on_click.clone();
        let target = pin.clone();
        let at = line_height * index as f32 + pins.push.get(index).copied().unwrap_or(px(0.))
            - strip_push;
        row(pin)
            .absolute()
            .id(SharedString::from(format!("pinned-{}", key(pin))))
            .cursor_pointer()
            .hover(|s| s.bg(hover))
            .on_click(move |_, window, cx| on_click(&target, window, cx))
            .with_animation(
                SharedString::from(format!("pin-in-{}", key(pin))),
                Animation::new(PIN_IN).with_easing(gpui::ease_out_quint()),
                move |el, delta| el.opacity(delta).top(at - px(4. * (1. - delta))),
            )
            .into_any_element()
    });
    // Where the strip ends: its lowest row's bottom, pushes counted, so
    // its shadow follows a row pushed up behind its parent rather than
    // falling across the text coming up under it.
    let bottom = pins
        .push
        .iter()
        .enumerate()
        .map(|(index, pushed)| line_height * (index + 1) as f32 + *pushed - strip_push)
        .chain(pins.leaving.iter().map(|(_, at_row, pushed, _)| {
            line_height * (*at_row + 1) as f32 + *pushed - strip_push
        }))
        .fold(px(0.), Pixels::max);
    let released = pins.released;
    let leaving = pins.leaving.iter().map(|(pin, at_row, pushed, _)| {
        row(pin)
            .id(SharedString::from(format!("unpinned-{}", key(pin))))
            .absolute()
            .top(geometry.line_height * *at_row as f32 + *pushed - strip_push)
            .with_animation(
                SharedString::from(format!("pin-out-{}-{released}", key(pin))),
                Animation::new(PIN_OUT),
                |el, delta| el.opacity(1. - delta),
            )
            .into_any_element()
    });
    let strip = div()
        .id("pinned-lines")
        .absolute()
        .top(strip_push)
        // A press on a pinned row is the row's, not the text's under it —
        // or the editor puts its caret there before the row's click goes
        // to the header. The wheel still scrolls through it.
        .when(!pins.shown.is_empty(), |el| el.block_mouse_except_scroll())
        .left_0()
        .right_0()
        .h(bottom)
        .shadow(vec![gpui::BoxShadow {
            color: gpui::black().opacity(0.35),
            offset: gpui::point(px(0.), px(2.)),
            blur_radius: px(5.),
            spread_radius: px(0.),
            inset: false,
        }])
        .font_family(geometry.font.clone())
        .text_size(geometry.font_size)
        .line_height(geometry.line_height)
        .children(leaving)
        .children(shown);
    // The strip as a whole: in when it appears from nothing, out when the
    // last line goes — so the shadow never pops.
    let strip = if pins.shown.is_empty() {
        strip
            .with_animation(
                SharedString::from(format!("pins-out-{released}")),
                Animation::new(PIN_OUT),
                |el, delta| el.opacity(1. - delta),
            )
            .into_any_element()
    } else {
        strip
            .with_animation(
                SharedString::from(format!("pins-in-{}", pins.showing)),
                Animation::new(PIN_IN).with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(delta),
            )
            .into_any_element()
    };
    // Clipped at the top of the view, which a pushed strip slides out of —
    // with room below for its shadow.
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h((strip_push + bottom).max(px(0.)) + px(SHADOW_ROOM))
            .overflow_hidden()
            .child(strip)
            .into_any_element(),
    )
}

/// A pinned line's text, its leading markers (`*`, `+`, `-`, `{`) in the
/// marker colour for a choice or a block's line, as the editor draws them.
fn line_text(pin: &PinnedLine, marker: gpui::Hsla) -> gpui::StyledText {
    let text = SharedString::from(pin.text.clone());
    if matches!(pin.kind, PinKind::Knot | PinKind::Stitch | PinKind::File) {
        return gpui::StyledText::new(text);
    }
    let lead = pin
        .text
        .char_indices()
        .find(|(_, c)| !matches!(c, '*' | '+' | '-' | '{' | ' ' | '\t'))
        .map_or(pin.text.len(), |(at, _)| at);
    let style = gpui::HighlightStyle {
        color: Some(marker),
        ..Default::default()
    };
    gpui::StyledText::new(text).with_highlights(vec![(0..lead, style)])
}

/// Space under the strip its shadow draws into.
const SHADOW_ROOM: f32 = 10.;

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "VAR x = 1\n\
                        === start ===\n\
                        Lamp.\n\
                        = second\n\
                        Run.\n\
                        * [Hide]\n\
                        \x20 Dark.\n\
                        \x20 {\n\
                        \x20 - here:\n\
                        \x20     Near.\n\
                        \x20     Nearer.\n\
                        \x20 - else:\n\
                        \x20     Far.\n\
                        \x20 }\n\
                        Fast.\n\
                        === market ===\n\
                        Stalls.\n";

    fn at(line: &str) -> usize {
        TEXT.find(line).expect("line")
    }

    /// The start of the line after the one holding `line`.
    fn next(line: &str) -> usize {
        let at = at(line);
        at + TEXT[at..].find('\n').expect("a line end") + 1
    }

    fn scope(kind: ScopeKind, opens: &str, ends: &str) -> Scope {
        Scope {
            kind,
            start: u32::try_from(at(opens)).expect("fits"),
            end: u32::try_from(TEXT.find(ends).unwrap_or(TEXT.len())).expect("fits"),
        }
    }

    fn scopes() -> Vec<Scope> {
        vec![
            scope(ScopeKind::Knot, "=== start", "=== market"),
            scope(ScopeKind::Stitch, "= second", "=== market"),
            scope(ScopeKind::Choice, "* [Hide]", "Fast."),
            scope(ScopeKind::Conditional, "{", "Fast."),
            scope(ScopeKind::Branch, "- here:", "  - else:"),
            scope(ScopeKind::Branch, "- else:", "  }"),
            scope(ScopeKind::Knot, "=== market", "\u{0}"),
        ]
    }

    fn texts(pinned: &[PinnedLine]) -> Vec<&str> {
        pinned.iter().map(|p| p.text.trim()).collect()
    }

    #[test]
    fn nothing_pins_above_the_first_knot_or_on_its_header() {
        assert!(pinned_at(&scopes(), TEXT, &[0, next("VAR")]).is_empty());
        assert!(pinned_at(&scopes(), TEXT, &[at("=== start"), next("=== start")]).is_empty());
    }

    #[test]
    fn the_knot_pins_once_its_header_has_scrolled_past() {
        let pinned = pinned_at(&scopes(), TEXT, &[at("Lamp."), next("Lamp.")]);
        assert_eq!(texts(&pinned), ["=== start ==="]);
        assert_eq!(pinned[0].line, 1);
        assert_eq!(pinned[0].offset, at("=== start"));
    }

    #[test]
    fn a_stitch_pins_under_its_knot() {
        assert_eq!(
            texts(&pinned_at(&scopes(), TEXT, &[at("Lamp."), at("= second")])),
            ["=== start ==="],
            "its header is in view, under the knot's row"
        );
        // Its header has slid under the knot's row: it pins there.
        let pinned = pinned_at(&scopes(), TEXT, &[at("= second"), next("= second")]);
        assert_eq!(texts(&pinned), ["=== start ===", "= second"]);
        assert_eq!(pinned[1].kind, PinKind::Stitch);
    }

    #[test]
    fn choices_conditionals_and_branches_pin_inside_their_stitch() {
        // Deep in the `- here:` branch: knot, stitch, choice, conditional,
        // branch — each judged a row lower than the last.
        let rows = [
            at("Dark."),
            at("{"),
            at("- here:"),
            at("Near."),
            at("Nearer."),
        ];
        let pinned = pinned_at(&scopes(), TEXT, &rows);
        assert_eq!(
            texts(&pinned),
            ["=== start ===", "= second", "* [Hide]", "{", "- here:"]
        );
        assert_eq!(
            pinned.iter().map(|p| p.kind).collect::<Vec<_>>(),
            [
                PinKind::Knot,
                PinKind::Stitch,
                PinKind::Choice,
                PinKind::Block,
                PinKind::Branch
            ]
        );
    }

    #[test]
    fn the_next_branch_replaces_the_last() {
        let rows = [at("Dark."), at("{"), at("- else:"), at("Far."), TEXT.len()];
        let pinned = pinned_at(&scopes(), TEXT, &rows);
        assert_eq!(pinned.last().map(|p| p.text.trim()), Some("- else:"));
        assert_eq!(pinned.len(), 5);
    }

    #[test]
    fn the_stitch_stays_while_the_next_header_pushes_it() {
        // The top line is the stitch's last; the next knot's header is the
        // line under the knot's row. The stitch stays, to be pushed.
        let pinned = pinned_at(&scopes(), TEXT, &[at("Fast."), at("=== market")]);
        assert_eq!(texts(&pinned), ["=== start ===", "= second"]);
    }

    #[test]
    fn the_rows_over_a_line_count_the_blocks_it_is_inside() {
        assert_eq!(rows_over(&scopes(), TEXT, at("Lamp.")), 1);
        assert_eq!(rows_over(&scopes(), TEXT, at("Nearer.")), 5);
        assert_eq!(rows_over(&scopes(), TEXT, at("=== start")), 0);
    }

    #[test]
    fn the_strip_is_pushed_up_by_what_comes_next() {
        let pinned = pinned_at(&scopes(), TEXT, &[at("Run."), next("Run.")]);
        let row = px(20.);
        // Far below: nothing moves.
        assert_eq!(pushes(&pinned, row, |_| None), [px(0.), px(0.)]);
        assert_eq!(pushes(&pinned, row, |_| Some(px(200.))), [px(0.), px(0.)]);
        // The next knot's header one row below the top: the two-row strip
        // is pushed up by a row, and the stitch row goes with it.
        assert_eq!(
            pushes(&pinned, row, |_| Some(px(20.))),
            [px(-20.), px(-20.)]
        );
        // Half a row into the strip's last row.
        assert_eq!(
            pushes(&pinned, row, |_| Some(px(30.))),
            [px(-10.), px(-10.)]
        );
    }

    #[test]
    fn a_stitch_row_can_be_pushed_on_its_own() {
        let mut pinned = pinned_at(&scopes(), TEXT, &[at("Run."), next("Run.")]);
        // The stitch ends sooner than the knot (another stitch follows).
        pinned[1].end = at("Fast.");
        let row = px(20.);
        let distance = |offset: usize| {
            Some(if offset == at("Fast.") {
                px(30.)
            } else {
                px(400.)
            })
        };
        assert_eq!(pushes(&pinned, row, distance), [px(0.), px(-10.)]);
    }

    #[test]
    fn the_next_knot_replaces_the_last() {
        let pinned = pinned_at(&scopes(), TEXT, &[at("Stalls."), TEXT.len()]);
        assert_eq!(texts(&pinned), ["=== market ==="]);
    }
}
