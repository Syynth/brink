//! The editor's hover card, drawn as the web studio draws it
//! (`packages/ink-editor/src/hover.ts`, `studio-ui/src/styles/editor.css`).
//!
//! `brink_ide::hover` answers a small markdown: a first line naming the
//! symbol (`**label** \`knot.label\``), then sections, then
//! `*Defined in* [\`path\`](#N)`. The kit's generic markdown view drew all
//! of that as bold, italics and grey code blocks, in a cramped box. The web
//! draws its own subset instead, and so does this:
//!
//! - one row per line, ``` fences dropped;
//! - the first line in the monospace face, with a faint rule under it;
//! - `**kind**` in the accent colour, `*Defined in*` muted;
//! - `` `code` `` as a quiet chip in the monospace face;
//! - `[text](#N)` as a link, in the info colour and underlined, that goes
//!   to the definition (`GoToHoverTarget`, which the studio handles by mode).
//!
//! **Why the targets travel beside the markdown.** `brink_ide` refers to a
//! link's target by index, deliberately: a path in a markdown URL has to
//! survive brackets and colons. The kit hands the renderer only the hover
//! itself, so the provider records the targets here ([`remember`]), keyed by
//! the very content it returned; one hover is open at a time, so the last
//! one is the one being drawn.

use std::ops::Range;
use std::rc::Rc;

use brink_gpui_model::query::HoverTarget;
use gpui::{
    AnyElement, App, Font, FontWeight, Global, InteractiveText, IntoElement, ParentElement as _,
    SharedString, StrikethroughStyle, StyleRefinement, Styled as _, StyledText, TextRun,
    UnderlineStyle, Window, div, px,
};
use gpui_component::{ActiveTheme as _, input::HoverRenderer, v_flex};

/// Go to a hover link's target. The studio decides where that is: the
/// manuscript in Write mode, a tab in Script.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = hover, no_json)]
pub struct GoToHoverTarget {
    pub path: String,
    pub start: usize,
    pub end: usize,
}

/// The targets of the hover most recently answered, beside its content.
#[derive(Default)]
struct HoverLinks {
    content: String,
    links: Vec<Option<HoverTarget>>,
}

impl Global for HoverLinks {}

/// Record the targets behind `content`'s `#N` links, for the card that is
/// about to draw it.
pub(crate) fn remember(content: &str, links: Vec<Option<HoverTarget>>, cx: &mut App) {
    cx.set_global(HoverLinks {
        content: content.to_owned(),
        links,
    });
}

/// Draw every editor's hover with this card from now on.
pub(crate) fn install(cx: &mut App) {
    let card = StyleRefinement::default()
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .max_w(px(480.));
    gpui_component::input::set_hover_renderer(
        HoverRenderer {
            render: Rc::new(render),
            card,
        },
        cx,
    );
}

/// One piece of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Code(String),
    Strong(String),
    Em(String),
    /// `[label](#N)`; the label's backticks are kept out, and `code` says
    /// whether it had them.
    Link {
        label: String,
        code: bool,
        index: usize,
    },
}

/// The web's inline subset — `` `code` ``, `**strong**`, `*em*`,
/// `[label](#N)` — and plain text between. Anything that does not close
/// stays text, as the web's regex leaves it.
pub(crate) fn pieces(line: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = line;
    let flush = |text: &mut String, out: &mut Vec<Piece>| {
        if !text.is_empty() {
            out.push(Piece::Text(std::mem::take(text)));
        }
    };
    while let Some(ch) = rest.chars().next() {
        let taken = match ch {
            '[' => link(rest).map(|(piece, len)| {
                flush(&mut text, &mut out);
                out.push(piece);
                len
            }),
            '`' => closed(rest, "`").map(|(inner, len)| {
                flush(&mut text, &mut out);
                out.push(Piece::Code(inner.to_owned()));
                len
            }),
            '*' if rest.starts_with("**") => closed(rest, "**").map(|(inner, len)| {
                flush(&mut text, &mut out);
                out.push(Piece::Strong(inner.to_owned()));
                len
            }),
            '*' => closed(rest, "*").map(|(inner, len)| {
                flush(&mut text, &mut out);
                out.push(Piece::Em(inner.to_owned()));
                len
            }),
            _ => None,
        };
        match taken {
            Some(len) => rest = &rest[len..],
            None => {
                text.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    flush(&mut text, &mut out);
    out
}

/// `rest` opens with `mark`; the text up to the next `mark`, and the
/// length consumed. `None` when it never closes or is empty.
fn closed<'a>(rest: &'a str, mark: &str) -> Option<(&'a str, usize)> {
    let body = &rest[mark.len()..];
    let end = body.find(mark)?;
    (end > 0).then(|| (&body[..end], mark.len() + end + mark.len()))
}

/// `[label](#N)` at the start of `rest`.
fn link(rest: &str) -> Option<(Piece, usize)> {
    let close = rest.find("](#")?;
    let label = &rest[1..close];
    let after = &rest[close + 3..];
    let end = after.find(')')?;
    let index = after[..end].parse().ok()?;
    let (label, code) = match label.strip_prefix('`').and_then(|l| l.strip_suffix('`')) {
        Some(inner) => (inner.to_owned(), true),
        None => (label.to_owned(), false),
    };
    Some((Piece::Link { label, code, index }, close + 3 + end + 1))
}

/// The card's content.
fn render(hover: &lsp_types::Hover, _window: &mut Window, cx: &mut App) -> AnyElement {
    let content = match &hover.contents {
        lsp_types::HoverContents::Markup(markup) => markup.value.clone(),
        lsp_types::HoverContents::Scalar(lsp_types::MarkedString::String(s)) => s.clone(),
        lsp_types::HoverContents::Scalar(lsp_types::MarkedString::LanguageString(s)) => {
            s.value.clone()
        }
        lsp_types::HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                lsp_types::MarkedString::String(s) => s.clone(),
                lsp_types::MarkedString::LanguageString(s) => s.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    let links = cx
        .try_global::<HoverLinks>()
        .filter(|held| held.content == content)
        .map(|held| held.links.clone())
        .unwrap_or_default();

    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let hsla = brink_gpui_shell::theme::hsla;
    let colours = Colours {
        fg: hsla(tokens.fg),
        muted: hsla(tokens.fg_muted),
        accent: hsla(tokens.accent),
        info: hsla(tokens.info),
        chip: hsla(tokens.surface_bg),
    };
    let border = hsla(tokens.border).opacity(0.55);
    let theme = cx.theme();
    let ui = gpui::font(theme.font_family.clone());
    let mono = gpui::font(theme.mono_font_family.clone());

    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("```"))
        .collect();
    let rows = lines.iter().enumerate().map(|(ix, line)| {
        let first = ix == 0;
        let base = if first { &mono } else { &ui };
        let (text, runs, targets) = layout(line, base, &mono, &colours, &links);
        let styled = StyledText::new(text).with_runs(runs);
        let clickable: Vec<Range<usize>> = targets.iter().map(|(r, _)| r.clone()).collect();
        let line_el = InteractiveText::new(SharedString::from(format!("hover-line-{ix}")), styled)
            .on_click(clickable, move |which, window, cx| {
                if let Some((_, target)) = targets.get(which) {
                    window.dispatch_action(
                        Box::new(GoToHoverTarget {
                            path: target.path.clone(),
                            start: target.start as usize,
                            end: target.end as usize,
                        }),
                        cx,
                    );
                }
            });
        div()
            .when_first(first, border)
            .child(line_el)
            .into_any_element()
    });
    v_flex()
        .gap(px(5.))
        .text_sm()
        .line_height(gpui::relative(1.55))
        .children(rows)
        .into_any_element()
}

/// A chip's padding: a thin space either side, on the chip's colour.
const PAD: &str = "\u{2009}";

/// The colours a card draws with, from the studio theme.
struct Colours {
    fg: gpui::Hsla,
    muted: gpui::Hsla,
    accent: gpui::Hsla,
    info: gpui::Hsla,
    chip: gpui::Hsla,
}

/// A line ready to draw: its characters, a style per stretch, and the
/// byte ranges that are links with where they go.
type LaidOutLine = (String, Vec<TextRun>, Vec<(Range<usize>, HoverTarget)>);

/// A line as one run of text: its characters, a style per stretch, and the
/// byte ranges that are links with where they go.
fn layout(
    line: &str,
    base: &Font,
    mono: &Font,
    colours: &Colours,
    links: &[Option<HoverTarget>],
) -> LaidOutLine {
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut targets = Vec::new();
    let run = |len: usize, font: Font, color: gpui::Hsla| TextRun {
        len,
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None::<StrikethroughStyle>,
    };
    // A chip: the code in the monospace face on the surface colour, with a
    // thin space either side standing in for its padding.
    let chip = |s: &str| format!("{PAD}{s}{PAD}");
    for piece in pieces(line) {
        match piece {
            Piece::Text(s) => {
                runs.push(run(s.len(), base.clone(), colours.fg));
                text.push_str(&s);
            }
            Piece::Code(s) => {
                let s = chip(&s);
                let mut r = run(s.len(), mono.clone(), colours.fg);
                r.background_color = Some(colours.chip);
                runs.push(r);
                text.push_str(&s);
            }
            Piece::Strong(s) => {
                let font = Font {
                    weight: FontWeight::SEMIBOLD,
                    ..base.clone()
                };
                runs.push(run(s.len(), font, colours.accent));
                text.push_str(&s);
            }
            Piece::Em(s) => {
                // The web's `em` is muted, not slanted: "Defined in" is a
                // caption, not emphasis.
                runs.push(run(s.len(), base.clone(), colours.muted));
                text.push_str(&s);
            }
            Piece::Link { label, code, index } => {
                let target = links.get(index).cloned().flatten();
                let font = if code { mono.clone() } else { base.clone() };
                // No target the studio can open: plain, as the web draws
                // an unresolved link.
                let color = if target.is_some() {
                    colours.info
                } else {
                    colours.fg
                };
                let pad = |r: &mut Vec<TextRun>, text: &mut String| {
                    let mut space = run(PAD.len(), mono.clone(), color);
                    space.background_color = Some(colours.chip);
                    r.push(space);
                    text.push_str(PAD);
                };
                // The chip's padding is not underlined: only the path is.
                if code {
                    pad(&mut runs, &mut text);
                }
                let mut r = run(label.len(), font, color);
                if code {
                    r.background_color = Some(colours.chip);
                }
                let start = text.len();
                if let Some(target) = target {
                    r.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(colours.info.opacity(0.45)),
                        wavy: false,
                    });
                    targets.push((start..start + label.len(), target));
                }
                runs.push(r);
                text.push_str(&label);
                if code {
                    pad(&mut runs, &mut text);
                }
            }
        }
    }
    (text, runs, targets)
}

/// The first line's rule, as the web draws it: a border under it.
trait FirstLine {
    fn when_first(self, first: bool, border: gpui::Hsla) -> Self;
}

impl FirstLine for gpui::Div {
    fn when_first(self, first: bool, border: gpui::Hsla) -> Self {
        if first {
            self.pb(px(6.)).mb(px(1.)).border_b_1().border_color(border)
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inline_subset_reads_as_the_web_reads_it() {
        assert_eq!(
            pieces("**label** `clue_case_file.examine`"),
            vec![
                Piece::Strong("label".into()),
                Piece::Text(" ".into()),
                Piece::Code("clue_case_file.examine".into()),
            ]
        );
        assert_eq!(
            pieces("*Defined in* [`clues/clue.ink`](#0)"),
            vec![
                Piece::Em("Defined in".into()),
                Piece::Text(" ".into()),
                Piece::Link {
                    label: "clues/clue.ink".into(),
                    code: true,
                    index: 0
                },
            ]
        );
        assert_eq!(
            pieces("a * b and `unclosed"),
            vec![Piece::Text("a * b and `unclosed".into())],
            "what does not close stays text"
        );
    }
}
