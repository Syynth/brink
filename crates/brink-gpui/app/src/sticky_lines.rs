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

use gpui::{AnyElement, App, Pixels, SharedString, Window, div, prelude::*, px};
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
/// header line is above `top` — a header still on screen pins nothing.
pub(crate) fn pinned_at(symbols: &[Symbol], text: &str, top: usize) -> Vec<PinnedLine> {
    let holds = |s: &&Symbol| (s.full_start as usize) <= top && top < s.full_end as usize;
    let above = |s: &Symbol| line_start(text, s.full_start as usize) < top;
    let mut pinned = Vec::new();
    let Some(knot) = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Knot)
        .find(holds)
    else {
        return pinned;
    };
    if !above(knot) {
        return pinned;
    }
    pinned.push(header(text, knot, false));
    if let Some(stitch) = knot
        .children
        .iter()
        .filter(|s| s.kind == SymbolKind::Stitch)
        .find(holds)
        && above(stitch)
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
}

/// Gap between a line number's right edge and the text, as the kit's
/// gutter lays it out.
const NUMBER_GAP: f32 = 10.;

/// The pinned lines, as an overlay for the top of the host's view: each
/// header on the editor's surface, in its colour, with its line number,
/// over a soft rule. A click goes to the header. `None` when nothing pins.
pub(crate) fn render(
    pinned: &[PinnedLine],
    geometry: &Geometry,
    on_click: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> Option<AnyElement> {
    if pinned.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let hsla = brink_gpui_shell::theme::hsla;
    let (knot, stitch, number) = (
        hsla(tokens.syn_namespace),
        hsla(tokens.syn_function),
        theme.muted_foreground,
    );
    let rule = theme
        .sidebar_border
        .opacity(brink_gpui_shell::workspace::DIVIDER_STRENGTH);
    let rows = pinned.iter().map(|pin| {
        let on_click = on_click.clone();
        let offset = pin.offset;
        div()
            .id(SharedString::from(format!("pinned-{offset}")))
            .relative()
            .w_full()
            .h(geometry.line_height)
            .cursor_pointer()
            .hover(|s| s.bg(theme.muted.opacity(0.35)))
            .on_click(move |_, window, cx| on_click(offset, window, cx))
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .w(geometry.text_left - px(NUMBER_GAP))
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
    });
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bg(theme.background)
            .border_b_1()
            .border_color(rule)
            .font_family(geometry.font.clone())
            .text_size(geometry.font_size)
            .line_height(geometry.line_height)
            .children(rows)
            .into_any_element(),
    )
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

    #[test]
    fn nothing_pins_above_the_first_knot_or_on_its_header() {
        assert!(pinned_at(&outline(), TEXT, 0).is_empty());
        assert!(pinned_at(&outline(), TEXT, at("=== start")).is_empty());
    }

    #[test]
    fn the_knot_pins_once_its_header_has_scrolled_past() {
        let pinned = pinned_at(&outline(), TEXT, at("Lamp."));
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].text, "=== start ===");
        assert_eq!(pinned[0].line, 1);
        assert_eq!(pinned[0].offset, at("=== start"));
    }

    #[test]
    fn a_stitch_pins_under_its_knot() {
        assert_eq!(
            pinned_at(&outline(), TEXT, at("= second")).len(),
            1,
            "its header is in view"
        );
        let pinned = pinned_at(&outline(), TEXT, at("Fast."));
        let texts: Vec<_> = pinned.iter().map(|p| (p.text.as_str(), p.stitch)).collect();
        assert_eq!(texts, [("=== start ===", false), ("= second", true)]);
    }

    #[test]
    fn the_next_knot_replaces_the_last() {
        let pinned = pinned_at(&outline(), TEXT, at("Stalls."));
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].text, "=== market ===");
    }
}
