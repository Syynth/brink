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

use brink_gpui_model::prose::ProseFix;
use brink_gpui_model::query::HoverTarget;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Font, FontWeight, Global, InteractiveText, SharedString, StrikethroughStyle,
    StyleRefinement, StyledText, TextRun, UnderlineStyle, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{HoverCard, HoverRenderer},
    v_flex,
};

/// Go to a hover link's target. The studio decides where that is: the
/// manuscript in Write mode, a tab in Script.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = hover, no_json)]
pub struct GoToHoverTarget {
    pub path: String,
    pub start: usize,
    pub end: usize,
}

/// Add a misspelled word to the project's `[prose] dictionary` — the
/// spelling card's "Add to dictionary". The studio writes `brink.toml`.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = hover, no_json)]
pub struct AddToDictionary {
    pub word: String,
}

/// A prose lint's fixes, as a squiggle's `data` carries them.
pub(crate) fn fixes_to_data(fixes: &[ProseFix]) -> serde_json::Value {
    serde_json::Value::Array(
        fixes
            .iter()
            .map(|fix| match fix {
                ProseFix::Replace(text) => serde_json::json!({ "replace": text }),
                ProseFix::Remove => serde_json::json!({ "remove": true }),
            })
            .collect(),
    )
}

/// The fixes back out of a squiggle's `data`; none when it carries none.
pub(crate) fn prose_fixes(data: Option<&serde_json::Value>) -> Vec<ProseFix> {
    data.and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if let Some(text) = item.get("replace").and_then(serde_json::Value::as_str) {
                        Some(ProseFix::Replace(text.to_owned()))
                    } else {
                        item.get("remove").map(|_| ProseFix::Remove)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A prose fix, applied: the lint's span becomes `text` (empty removes it),
/// as an edit the author made — so it is undoable, and the section or tab
/// reports it to the project like any keystroke.
pub(crate) fn apply_fix(
    editor: &gpui::Entity<gpui_component::input::EditorState>,
    range: Range<usize>,
    text: &str,
    window: &mut Window,
    cx: &mut App,
) {
    editor.update(cx, |state, cx| {
        state.set_selected_range(range, cx);
        state.replace(text, window, cx);
    });
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
    // Each section pads itself, so the rules between them run edge to edge.
    let card = StyleRefinement::default()
        .p_0()
        .rounded(px(8.))
        .overflow_hidden()
        .max_w(px(460.));
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

/// The whole card: the hover section if the provider answered, then a
/// section listing every problem under the pointer — one card, as the web
/// stacks them (`.cm-tooltip-section`), with a rule between.
fn render(card: &HoverCard, window: &mut Window, cx: &mut App) -> AnyElement {
    let rule = palette(cx).border;
    // The card sits inside the editor, whose text style is the code face;
    // the web's card is in the UI face, with code (and fixes) in mono.
    let ui = cx.theme().font_family.clone();
    let mut sections: Vec<AnyElement> = Vec::new();
    if let Some(hover) = &card.hover {
        sections.push(hover_section(hover, cx));
    }
    if !card.diagnostics.is_empty() {
        sections.push(problems_section(card, window, cx));
    }
    v_flex()
        .font_family(ui)
        .children(sections.into_iter().enumerate().map(|(ix, section)| {
            div()
                .when(ix > 0, |el| el.border_t_1().border_color(rule))
                .child(section)
        }))
        .into_any_element()
}

/// The theme's colours, as the card names them.
fn palette(cx: &App) -> Colours {
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let hsla = brink_gpui_shell::theme::hsla;
    Colours {
        fg: hsla(tokens.fg),
        muted: hsla(tokens.fg_muted),
        accent: hsla(tokens.accent),
        info: hsla(tokens.info),
        chip: hsla(tokens.surface_bg),
        error: hsla(tokens.error),
        warning: hsla(tokens.warning),
        panel: hsla(tokens.panel_bg),
        border: hsla(tokens.border).opacity(0.55),
        solid_border: hsla(tokens.border),
    }
}

/// The symbol's card: one row per line of `brink_ide`'s markdown.
fn hover_section(hover: &lsp_types::Hover, cx: &mut App) -> AnyElement {
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

    let colours = palette(cx);
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
            .when_first(first, colours.border)
            .child(line_el)
            .into_any_element()
    });
    v_flex()
        .px(px(12.))
        .py(px(9.))
        .gap(px(5.))
        .text_sm()
        .line_height(gpui::relative(1.55))
        .children(rows)
        .into_any_element()
}

/// Every problem under the pointer, one row each, ruled between
/// (`.cm-tooltip-lint` > `li.cm-diagnostic`).
fn problems_section(card: &HoverCard, window: &mut Window, cx: &mut App) -> AnyElement {
    let colours = palette(cx);
    let rows: Vec<AnyElement> = card
        .diagnostics
        .iter()
        .enumerate()
        .map(|(ix, entry)| problem_row(ix, entry, &card.editor, &colours, window, cx))
        .collect();
    v_flex()
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            div()
                .when(ix > 0, |el| el.border_t_1().border_color(colours.border))
                .child(row)
        }))
        .into_any_element()
}

/// One problem: its label (the severity, or a prose lint's kind), its
/// message as written, its code underneath, and — for a prose lint — the
/// checker's fixes and, for a misspelling, "Add to dictionary".
fn problem_row(
    ix: usize,
    entry: &gpui_component::highlighter::DiagnosticEntry,
    editor: &gpui::Entity<gpui_component::input::EditorState>,
    colours: &Colours,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    use gpui_component::highlighter::DiagnosticSeverity as S;
    let code = entry
        .code
        .as_ref()
        .map(|c| c.to_string())
        .unwrap_or_default();
    let prose_kind = code.strip_prefix("prose.").map(str::to_owned);
    let (label, label_colour) = match (&prose_kind, entry.severity) {
        (Some(kind), _) => (kind.to_lowercase(), colours.muted),
        (None, S::Error) => ("error".to_owned(), colours.error),
        (None, S::Warning) => ("warning".to_owned(), colours.warning),
        // The web has three severities; an info and a hint read alike.
        (None, S::Info | S::Hint) => ("info".to_owned(), colours.info),
    };
    let source = match &prose_kind {
        Some(kind) => format!("prose:{kind}"),
        None => code.clone(),
    };
    let mono = gpui::font(cx.theme().mono_font_family.clone());
    // `.cm-prose-fix:hover` — the primary fix's accent at 14%, the rest
    // on the surface colour.
    let soft = colours.accent.opacity(0.14);
    let hover_bg = colours.chip;
    let hover_fg = colours.fg;

    let mut actions: Vec<AnyElement> = Vec::new();
    if prose_kind.is_some() {
        let range = entry.range.clone();
        for (n, fix) in prose_fixes(entry.data.as_ref()).into_iter().enumerate() {
            let (shown, text) = match &fix {
                ProseFix::Replace(text) => (text.clone(), text.clone()),
                ProseFix::Remove => ("Remove".to_owned(), String::new()),
            };
            let editor = editor.clone();
            let range = range.clone();
            let primary = n == 0;
            actions.push(
                div()
                    .id(SharedString::from(format!("prose-fix-{ix}-{n}")))
                    .px(px(11.))
                    .py(px(5.))
                    .min_h(px(26.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if primary {
                        colours.accent
                    } else {
                        colours.solid_border
                    })
                    .bg(colours.panel)
                    .text_color(if primary { colours.accent } else { colours.fg })
                    .font(mono.clone())
                    .text_xs()
                    .cursor_pointer()
                    .hover(move |s| s.bg(if primary { soft } else { hover_bg }))
                    .child(shown)
                    .on_click(move |_, window, cx| {
                        apply_fix(&editor, range.clone(), &text, window, cx);
                    })
                    .into_any_element(),
            );
        }
        if prose_kind.as_deref() == Some("Spelling") {
            let word = editor
                .read(cx)
                .value()
                .get(entry.range.clone())
                .map(str::to_owned);
            if let Some(word) = word.filter(|w| !w.trim().is_empty()) {
                actions.push(
                    div()
                        .id(SharedString::from(format!("prose-dict-{ix}")))
                        .px(px(11.))
                        .py(px(5.))
                        .min_h(px(26.))
                        .rounded(px(5.))
                        .border_1()
                        .border_dashed()
                        .border_color(colours.solid_border)
                        .text_color(colours.muted)
                        .text_xs()
                        .cursor_pointer()
                        .hover(move |s| s.text_color(hover_fg))
                        .child("Add to dictionary")
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(
                                Box::new(AddToDictionary { word: word.clone() }),
                                cx,
                            );
                        })
                        .into_any_element(),
                );
            }
        }
    }

    v_flex()
        .px(px(12.))
        .py(px(9.))
        .gap(px(6.))
        .child({
            // Label and message as ONE run of text — the web's
            // `.cm-diag-body`: the label inline, the message wrapping
            // under it. (The kit lays a pop-up out at its contents' least
            // width, so two boxes side by side wrapped the message to a
            // word a line.)
            let label_len = label.len();
            let message = entry.message.to_string();
            let text = format!("{label}  {message}");
            let ui = gpui::font(cx.theme().font_family.clone());
            let run = |len: usize, font: Font, color: gpui::Hsla| TextRun {
                len,
                font,
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = vec![
                run(
                    label_len,
                    Font {
                        weight: FontWeight::SEMIBOLD,
                        ..ui.clone()
                    },
                    label_colour,
                ),
                run(text.len() - label_len, ui, colours.fg),
            ];
            div()
                .text_sm()
                .line_height(gpui::relative(1.5))
                .child(StyledText::new(text).with_runs(runs))
        })
        .when(!source.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(10.))
                    .text_color(colours.muted.opacity(0.85))
                    .child(source),
            )
        })
        .when(!actions.is_empty(), |el| {
            el.child(h_flex().flex_wrap().gap(px(6.)).children(actions))
        })
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
    error: gpui::Hsla,
    warning: gpui::Hsla,
    panel: gpui::Hsla,
    /// The 55% rule between sections and rows.
    border: gpui::Hsla,
    solid_border: gpui::Hsla,
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
    fn a_lints_fixes_survive_the_trip_through_a_squiggle() {
        let fixes = vec![ProseFix::Replace("noir".to_owned()), ProseFix::Remove];
        assert_eq!(prose_fixes(Some(&fixes_to_data(&fixes))), fixes);
        assert!(prose_fixes(None).is_empty(), "a compiler squiggle has none");
    }

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
