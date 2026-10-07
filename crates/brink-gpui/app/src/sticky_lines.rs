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
}

/// Where the line holding `offset` starts.
fn line_start(text: &str, offset: usize) -> usize {
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
    }
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
    if let Some(stitch) = knot
        .children
        .iter()
        .filter(|s| s.kind == SymbolKind::Stitch)
        .find(holds(under))
        && above(stitch, under)
    {
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
    leaving: Vec<(PinnedLine, usize, Instant)>,
    /// Bumped each time the strip appears from nothing, so its fade-in
    /// runs again rather than resuming a finished one.
    showing: u64,
    /// Bumped per line let go, for the same reason.
    released: u64,
}

impl Pins {
    /// Take the lines to pin now. `true` when some were let go: the host
    /// asks for a frame after [`PIN_OUT`], to stop drawing them.
    pub(crate) fn update(&mut self, path: Option<&str>, next: Vec<PinnedLine>) -> bool {
        let now = Instant::now();
        self.leaving
            .retain(|(_, _, at)| now.duration_since(*at) < PIN_OUT);
        let same_file = self.path.as_deref() == path;
        let mut released = false;
        for (row, pin) in self.shown.iter().enumerate() {
            if !same_file || !next.iter().any(|n| n.offset == pin.offset) {
                self.leaving.push((pin.clone(), row, now));
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
    let hover = theme.muted.opacity(0.35);
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
    let shown = pins.shown.iter().map(|pin| {
        let on_click = on_click.clone();
        let offset = pin.offset;
        row(pin)
            .id(SharedString::from(format!("pinned-{offset}")))
            .cursor_pointer()
            .hover(|s| s.bg(hover))
            .on_click(move |_, window, cx| on_click(offset, window, cx))
            .with_animation(
                SharedString::from(format!("pin-in-{offset}")),
                Animation::new(PIN_IN).with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(delta).top(px(-4. * (1. - delta))),
            )
            .into_any_element()
    });
    let rows = shown.len().max(
        pins.leaving
            .iter()
            .map(|(_, row, _)| row + 1)
            .max()
            .unwrap_or(0),
    );
    let released = pins.released;
    let leaving = pins.leaving.iter().map(|(pin, at_row, _)| {
        row(pin)
            .id(SharedString::from(format!("unpinned-{}", pin.offset)))
            .absolute()
            .top(geometry.line_height * *at_row as f32)
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
        .top_0()
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
        .children(shown)
        .children(leaving);
    // The strip as a whole: in when it appears from nothing, out when the
    // last line goes — so the shadow never pops.
    Some(if pins.shown.is_empty() {
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
    })
}

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
    fn the_next_knot_replaces_the_last() {
        let pinned = pinned_at(&outline(), TEXT, at("Stalls."), TEXT.len());
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].text, "=== market ===");
    }
}
