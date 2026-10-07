//! Pinned structure lines: the knot and stitch you are inside, held at the
//! top of the view once their header lines have scrolled past it — the
//! JetBrains "sticky lines" (decision log 2026-10-07). Write's manuscript
//! and Script's editors both draw them.
//!
//! What to pin is read off the file's outline, not its text, so a `.brink`
//! file's structure pins the same way an `.ink` file's does. Where the top
//! of the view is comes from the editor's own layout: a line's height in
//! the editor's content, compared with how far the host has scrolled.

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Pixels, SharedString, Window, div, prelude::*,
    px,
};
use gpui_component::{ActiveTheme as _, input::EditorState};

use brink_gpui_model::query::Symbol;
use brink_ir::SymbolKind;

/// One pinned header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PinnedLine {
    /// Where the header line starts: what a click goes to.
    pub offset: usize,
    /// Its line number, from zero.
    pub line: usize,
    /// The line as written, trailing whitespace trimmed.
    pub text: String,
    /// A stitch's header, under its knot's.
    pub stitch: bool,
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

fn header(text: &str, symbol: &Symbol, stitch: bool) -> PinnedLine {
    let start = line_start(text, symbol.full_start as usize);
    let end = text[start..].find('\n').map_or(text.len(), |at| start + at);
    PinnedLine {
        offset: start,
        line: text[..start].matches('\n').count(),
        text: text[start..end].trim_end().to_owned(),
        stitch,
        end: symbol.full_end as usize,
    }
}

/// How far each pinned row is pushed up (zero or less), JetBrains-style:
/// a row never overlaps the header that ends its knot or stitch, so the
/// next one slides it out of the way rather than replacing it outright.
/// The knot's row carries the whole strip; a stitch's row can go sooner.
/// `distance(offset)` is how far below the top of the view the line at
/// `offset` starts — `None` when it is not laid out, which is far enough.
pub(crate) fn pushes(
    pinned: &[PinnedLine],
    row: Pixels,
    distance: impl Fn(usize) -> Option<Pixels>,
) -> Vec<Pixels> {
    let mut pushes: Vec<Pixels> = Vec::with_capacity(pinned.len());
    for (i, pin) in pinned.iter().enumerate() {
        // The knot clears the whole strip below it; a stitch, itself.
        let below = if i == 0 { pinned.len() } else { i + 1 };
        let own = distance(pin.end).map_or(px(0.), |d| (d - row * below as f32).min(px(0.)));
        pushes.push(pushes.first().map_or(own, |strip| own.min(*strip)));
    }
    pushes
}

/// The headers to pin when the line starting at `top` is the first one in
/// view: the knot it is inside, and the stitch, each only once its own
/// header line is above the view — a header still on screen pins nothing.
/// `under` is the line starting just below the knot's pinned row: the
/// stitch is judged there, since its header slides under that row before
/// it reaches the top, and would otherwise vanish for a line.
pub(crate) fn pinned_at(
    symbols: &[Symbol],
    text: &str,
    top: usize,
    under: usize,
) -> Vec<PinnedLine> {
    let holds =
        |at: usize| move |s: &&Symbol| (s.full_start as usize) <= at && at < s.full_end as usize;
    let above = |s: &Symbol, at: usize| line_start(text, s.full_start as usize) < at;
    let mut pinned = Vec::new();
    let Some(knot) = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Knot)
        .find(holds(top))
    else {
        return pinned;
    };
    if !above(knot, top) {
        return pinned;
    }
    pinned.push(header(text, knot, false));
    let stitch_at = |at: usize| {
        knot.children
            .iter()
            .filter(|s| s.kind == SymbolKind::Stitch)
            .find(holds(at))
            .filter(|s| above(s, at))
    };
    // The one whose header has slid under the knot's row, or else the one
    // the top line is still inside — which the next header then pushes
    // out of the way rather than replacing outright.
    if let Some(stitch) = stitch_at(under).or_else(|| stitch_at(top)) {
        pinned.push(header(text, stitch, true));
    }
    pinned
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
fn content_top(state: &EditorState, offset: usize) -> Option<Pixels> {
    let origin = state.text_bounds()?.top() + state.scroll_offset().y;
    state
        .range_to_bounds(&(offset..offset))
        .map(|b| b.top() - origin)
}

/// What to pin with the top of the view at `top` in the editor's content,
/// looking at lines in `within`, and how far each row is pushed.
pub(crate) fn pin(
    state: &EditorState,
    symbols: &[Symbol],
    top: Pixels,
    within: Range<usize>,
) -> Option<(Vec<PinnedLine>, Vec<Pixels>)> {
    let row = state.line_height()?;
    let line = line_at(state, top, within.clone())?;
    let under = line_at(state, top + row, within).unwrap_or(line);
    let text = state.value();
    let pinned = pinned_at(symbols, &text, line, under);
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
            if !same_file || !next.iter().any(|n| n.offset == pin.offset) {
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

    fn is_empty(&self) -> bool {
        self.shown.is_empty() && self.leaving.is_empty()
    }
}

/// The pinned lines, as an overlay for the top of the host's view: each
/// header on the editor's surface, in its colour, with its line number,
/// with a soft shadow below. A line fades and drops in as it pins and
/// fades out as it lets go; the strip itself fades in and out with its
/// shadow. A click goes to the header. `None` when nothing is drawn.
pub(crate) fn render(
    pins: &Pins,
    geometry: &Geometry,
    on_click: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> Option<AnyElement> {
    if pins.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let hsla = brink_gpui_shell::theme::hsla;
    let (knot, stitch, number, surface) = (
        hsla(tokens.syn_namespace),
        hsla(tokens.syn_function),
        theme.muted_foreground,
        theme.background,
    );
    // Opaque: a pinned row covers the text scrolled under it, hovered too.
    let hover = surface.blend(theme.muted.opacity(0.35));
    let row = |pin: &PinnedLine| {
        div()
            .relative()
            .w_full()
            .h(geometry.line_height)
            .bg(surface)
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
                    .text_color(if pin.stitch { stitch } else { knot })
                    .child(pin.text.clone()),
            )
    };
    // The strip moves with the knot's push; a stitch row's own push is on
    // top of that.
    let strip_push = pins.push.first().copied().unwrap_or(px(0.));
    // Deepest first, each at its own row: a stitch pushed up slides
    // behind its knot's row, which paints over it.
    let line_height = geometry.line_height;
    let shown = pins.shown.iter().enumerate().rev().map(|(index, pin)| {
        let on_click = on_click.clone();
        let offset = pin.offset;
        let at = line_height * index as f32 + pins.push.get(index).copied().unwrap_or(px(0.))
            - strip_push;
        row(pin)
            .absolute()
            .id(SharedString::from(format!("pinned-{offset}")))
            .cursor_pointer()
            .hover(|s| s.bg(hover))
            .on_click(move |_, window, cx| on_click(offset, window, cx))
            .with_animation(
                SharedString::from(format!("pin-in-{offset}")),
                Animation::new(PIN_IN).with_easing(gpui::ease_out_quint()),
                move |el, delta| el.opacity(delta).top(at - px(4. * (1. - delta))),
            )
            .into_any_element()
    });
    let rows = shown.len().max(
        pins.leaving
            .iter()
            .map(|(_, row, _, _)| row + 1)
            .max()
            .unwrap_or(0),
    );
    let released = pins.released;
    let leaving = pins.leaving.iter().map(|(pin, at_row, pushed, _)| {
        row(pin)
            .id(SharedString::from(format!("unpinned-{}", pin.offset)))
            .absolute()
            .top(geometry.line_height * *at_row as f32 + *pushed - strip_push)
            .with_animation(
                SharedString::from(format!("pin-out-{}-{released}", pin.offset)),
                Animation::new(PIN_OUT),
                |el, delta| el.opacity(1. - delta),
            )
            .into_any_element()
    });
    let strip = div()
        .id("pinned-lines")
        .absolute()
        .top(strip_push)
        .left_0()
        .right_0()
        .h(geometry.line_height * rows as f32)
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
            .h(geometry.line_height * rows as f32 + px(SHADOW_ROOM))
            .overflow_hidden()
            .child(strip)
            .into_any_element(),
    )
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
                        Fast.\n\
                        === market ===\n\
                        Stalls.\n";

    fn symbol(
        name: &str,
        kind: SymbolKind,
        header: &str,
        end: &str,
        children: Vec<Symbol>,
    ) -> Symbol {
        let full_start = u32::try_from(TEXT.find(header).expect("header")).expect("fits");
        let full_end = u32::try_from(TEXT.find(end).unwrap_or(TEXT.len())).expect("fits");
        Symbol {
            name: name.to_owned(),
            kind,
            start: full_start,
            full_start,
            full_end,
            is_function: false,
            value: None,
            children,
        }
    }

    fn outline() -> Vec<Symbol> {
        let second = symbol(
            "second",
            SymbolKind::Stitch,
            "= second",
            "=== market",
            vec![],
        );
        vec![
            symbol(
                "start",
                SymbolKind::Knot,
                "=== start",
                "=== market",
                vec![second],
            ),
            symbol("market", SymbolKind::Knot, "=== market", "\u{0}", vec![]),
        ]
    }

    fn at(line: &str) -> usize {
        TEXT.find(line).expect("line")
    }

    /// The line after `line`'s: what sits under the knot's pinned row.
    fn next(line: &str) -> usize {
        let at = at(line);
        at + TEXT[at..].find('\n').expect("a line end") + 1
    }

    #[test]
    fn nothing_pins_above_the_first_knot_or_on_its_header() {
        assert!(pinned_at(&outline(), TEXT, 0, next("VAR")).is_empty());
        assert!(pinned_at(&outline(), TEXT, at("=== start"), next("=== start")).is_empty());
    }

    #[test]
    fn the_knot_pins_once_its_header_has_scrolled_past() {
        let pinned = pinned_at(&outline(), TEXT, at("Lamp."), next("Lamp."));
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].text, "=== start ===");
        assert_eq!(pinned[0].line, 1);
        assert_eq!(pinned[0].offset, at("=== start"));
    }

    #[test]
    fn a_stitch_pins_under_its_knot() {
        assert_eq!(
            pinned_at(&outline(), TEXT, at("Lamp."), at("= second")).len(),
            1,
            "its header is in view, under the knot's row"
        );
        // Its header has slid under the knot's row: it pins there.
        let pinned = pinned_at(&outline(), TEXT, at("= second"), next("= second"));
        let texts: Vec<_> = pinned.iter().map(|p| (p.text.as_str(), p.stitch)).collect();
        assert_eq!(texts, [("=== start ===", false), ("= second", true)]);
        let pinned = pinned_at(&outline(), TEXT, at("Run."), at("Fast."));
        let texts: Vec<_> = pinned.iter().map(|p| (p.text.as_str(), p.stitch)).collect();
        assert_eq!(texts, [("=== start ===", false), ("= second", true)]);
    }

    #[test]
    fn the_stitch_stays_while_the_next_header_pushes_it() {
        // The top line is the stitch's last; the next knot's header is the
        // line under the knot's row. The stitch stays, to be pushed.
        let pinned = pinned_at(&outline(), TEXT, at("Fast."), at("=== market"));
        let texts: Vec<_> = pinned.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["=== start ===", "= second"]);
    }

    #[test]
    fn the_strip_is_pushed_up_by_what_comes_next() {
        let pinned = pinned_at(&outline(), TEXT, at("Run."), at("Fast."));
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
        let mut pinned = pinned_at(&outline(), TEXT, at("Run."), at("Fast."));
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
        let pinned = pinned_at(&outline(), TEXT, at("Stalls."), TEXT.len());
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].text, "=== market ===");
    }
}
