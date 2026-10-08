//! Go to — `cmd-k`, the studio's one jump to anywhere in the project:
//! files, knots, functions, stitches and labels, in one ranked list
//! (decision log 2026-10-08, "⌘K go-to replaces cmd-p quick-open"; the
//! design is the canvas's direction B).
//!
//! ## Why this is not the command palette
//!
//! The shell's palette is over the command registry: its items are
//! actions, its ranking is over command titles, and confirming one
//! dispatches it. Go to is over the *project*, and confirming one opens a
//! place. The overlay behaviour is the same, but the shell must not learn
//! what a knot is (the one-way edge the three-crate split exists for), so
//! this lives in the feature crate and the app root paints it, the way it
//! already paints the dialog and notification layers.
//!
//! ## Where the items come from
//!
//! Files come from the mirror. Everything else comes from one
//! [`QueryKind::GoToIndex`]: each file's symbol manifest, which already
//! names a stitch `knot.stitch` and a label `knot.stitch.label`. Each
//! carries its name's span, so a row reveals the declaration rather than
//! only opening its file.
//!
//! The index is asked for when the overlay opens, not held between
//! openings. It is one query over the analysis that already exists, and a
//! cached list would be a second thing to keep in step with every edit for
//! no gain a person could perceive.
//!
//! ## A row
//!
//! Two lines: the place's own name, its matched letters bold in the
//! accent, over a monospace breadcrumb — `file › knot › stitch :line` —
//! that tells two labels of the same name apart. The kind's icon (the
//! Binder's own, so a knot reads the same in both places) is centred on
//! the whole row, and a chip names the kind at the right edge.

use std::ops::Range;

use brink_gpui_model::query::{GoToKind, GoToSymbol, QueryKind, QueryResult};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, HighlightStyle, Hsla,
    IntoElement, KeyDownEvent, Render, SharedString, StyledText, Subscription, Window, div, px,
    uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use crate::project::Project;
use brink_gpui_shell::icons;

/// Rows drawn before the list scrolls, and the overlay's width.
const MAX_VISIBLE_ROWS: usize = 8;
const ROW_HEIGHT: f32 = 50.0;
const WIDTH: f32 = 620.0;
/// The row's icon: one size up from the Binder's, since it marks two lines.
const ICON: f32 = 17.0;

/// The most rows kept after filtering — a project with thousands of
/// passages must not build thousands of elements for a one-letter query.
const CAP: usize = 200;

/// What matching in a place's own name is worth over matching its whole
/// address — enough to put every own-name hit above a parent-only one.
const NAME_BONUS: i32 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickOpenEvent {
    /// Open this file, revealing `span` when there is one.
    Open {
        path: String,
        span: Option<Range<usize>>,
    },
    Dismiss,
}

/// What an item is, which decides its icon, its colour and its chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Knot,
    Function,
    Stitch,
    Label,
}

impl Kind {
    /// The Binder's icon for it: the outline forms, since those say what a
    /// thing IS — the filled and entry variants say something about its
    /// role, which this list does not.
    #[must_use]
    pub fn icon(self) -> icons::BrinkIcon {
        match self {
            Self::File => icons::BrinkIcon::Drop,
            Self::Knot => icons::BrinkIcon::Knot,
            Self::Function => icons::BrinkIcon::Function,
            Self::Stitch => icons::BrinkIcon::Stitch,
            Self::Label => icons::BrinkIcon::Label,
        }
    }

    #[must_use]
    pub fn chip(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Knot => "Knot",
            Self::Function => "Function",
            Self::Stitch => "Stitch",
            Self::Label => "Label",
        }
    }

    /// The theme's colour for the kind — the symbol colours the outline
    /// already uses, and the label colour the editor paints `(label)` in.
    fn colour(self, cx: &App) -> Hsla {
        let tokens = brink_gpui_shell::theme::current(cx).tokens;
        brink_gpui_shell::theme::hsla(match self {
            Self::File => tokens.symbol_file,
            Self::Knot => tokens.symbol_knot,
            Self::Function => tokens.symbol_function,
            Self::Stitch => tokens.symbol_stitch,
            Self::Label => tokens.syn_label,
        })
    }
}

impl From<GoToKind> for Kind {
    fn from(kind: GoToKind) -> Self {
        match kind {
            GoToKind::Knot => Self::Knot,
            GoToKind::Function => Self::Function,
            GoToKind::Stitch => Self::Stitch,
            GoToKind::Label => Self::Label,
        }
    }
}

/// One place to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What is typed against: a file's path, or a place's dotted address —
    /// so `harbour.wat` narrows to the harbour's watch.
    pub title: SharedString,
    /// What the row's first line shows: the file's name, or the last part
    /// of the address. Always a suffix of `title`.
    pub name: SharedString,
    /// The second line: `file › knot › stitch`, or a file's whole path.
    pub crumb: SharedString,
    /// 1-based, for the breadcrumb's `:line`.
    pub line: Option<u32>,
    pub kind: Kind,
    pub path: String,
    pub span: Option<Range<usize>>,
}

impl Item {
    fn file(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        Self {
            title: SharedString::from(path.to_owned()),
            name: SharedString::from(name.to_owned()),
            crumb: SharedString::from(path.to_owned()),
            line: None,
            kind: Kind::File,
            path: path.to_owned(),
            span: None,
        }
    }

    fn place(symbol: &GoToSymbol) -> Self {
        let name = symbol
            .qualified
            .rsplit('.')
            .next()
            .unwrap_or(&symbol.qualified);
        let crumb = std::iter::once(symbol.file.as_str())
            .chain(symbol.qualified.split('.'))
            .collect::<Vec<_>>()
            .join(" \u{203a} ");
        Self {
            title: SharedString::from(symbol.qualified.clone()),
            name: SharedString::from(name.to_owned()),
            crumb: SharedString::from(crumb),
            line: Some(symbol.line),
            kind: symbol.kind.into(),
            path: symbol.file.clone(),
            span: Some(symbol.span.clone()),
        }
    }

    /// The byte ranges of `name` to draw bold: the matched characters
    /// that fall in it (`title` ends with `name`, so they are the tail of
    /// the alignment), merged into runs.
    fn name_highlights(&self, positions: &[usize]) -> Vec<Range<usize>> {
        let title_chars = self.title.chars().count();
        let name_chars = self.name.chars().count();
        let offset = title_chars.saturating_sub(name_chars);
        let bytes: Vec<(usize, usize)> = self
            .name
            .char_indices()
            .map(|(at, c)| (at, at + c.len_utf8()))
            .collect();
        let mut runs: Vec<Range<usize>> = Vec::new();
        for p in positions.iter().filter(|p| **p >= offset) {
            let Some((start, end)) = bytes.get(p - offset).copied() else {
                continue;
            };
            match runs.last_mut() {
                Some(run) if run.end == start => run.end = end,
                _ => runs.push(start..end),
            }
        }
        runs
    }
}

/// One ranked row: the item, and the characters of its title that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub item: usize,
    pub positions: Vec<usize>,
}

/// Rank `items` against `query`, keeping the input's order when it is
/// empty. Subsequence matching with a bonus for a prefix hit, which is
/// what makes `sh` find `shore` before `harbour_scene`.
#[must_use]
pub fn rank(items: &[Item], query: &str) -> Vec<Hit> {
    if query.trim().is_empty() {
        return (0..items.len().min(CAP))
            .map(|item| Hit {
                item,
                positions: Vec::new(),
            })
            .collect();
    }
    let needle = query.trim().to_lowercase();
    let mut scored: Vec<(i32, Hit)> = Vec::new();
    for (ix, item) in items.iter().enumerate() {
        let hay = item.title.to_lowercase();
        let Some(whole) = score(&hay, &needle) else {
            continue;
        };
        // A match in the place's OWN name beats one that only threads
        // through its parents: `wat` means `keep_watch` before `oath`,
        // which matches only by way of `night_watch`. And its letters are
        // the ones lit, not the parent's.
        let own = (item.name.len() < item.title.len())
            .then(|| score(&item.name.to_lowercase(), &needle))
            .flatten()
            .map(|(points, positions)| {
                let offset = item.title.chars().count() - item.name.chars().count();
                (
                    points + NAME_BONUS,
                    positions.into_iter().map(|p| p + offset).collect(),
                )
            });
        let (points, positions) = match own {
            Some(own) if own.0 >= whole.0 => own,
            _ => whole,
        };
        scored.push((
            points,
            Hit {
                item: ix,
                positions,
            },
        ));
    }
    // Best first; ties keep the project's own order, which is file order
    // then declaration order — stable, so a list does not reshuffle as you
    // type a character that changes nothing.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.item.cmp(&b.1.item)));
    scored.truncate(CAP);
    scored.into_iter().map(|(_, hit)| hit).collect()
}

/// `None` when `needle` is not a subsequence of `hay`; otherwise a score
/// where bigger is better.
///
/// Three things earn points, in the order a person would name them: a
/// **prefix**, characters that are **contiguous**, and characters that
/// start a **word** (the segment after a `.`, `_`, `-` or `/`, which is how
/// both file paths and knot paths are built).
///
/// Subsequence alone cannot rank these: `lhtop` matches both
/// `lighthouse.approach` and `lighthouse.top` equally, and only the
/// word-start bonus separates them. But finding those word starts needs a
/// different *alignment* than the leftmost one — the `t` of `.top` is not
/// the first `t` in the string — so this scores TWICE and takes the better:
/// once taking each character's leftmost occurrence, once preferring an
/// occurrence that starts a word.
///
/// The leftmost pass is what decides whether it matches at all. Preferring
/// a word start can walk past the only alignment that works (`ab` in
/// `ab.a` takes the `a` after the dot and then finds no `b`), so a failed
/// preferring pass is not an answer — only the leftmost one can say "no".
fn score(hay: &str, needle: &str) -> Option<(i32, Vec<usize>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }
    let hay: Vec<char> = hay.chars().collect();
    let leftmost = align(&hay, needle, false)?;
    let boundary = align(&hay, needle, true).unwrap_or_else(|| leftmost.clone());
    // The better alignment wins — and its positions are what the row
    // highlights, so the bold letters are the ones that earned the rank.
    Some(if boundary.0 > leftmost.0 {
        boundary
    } else {
        leftmost
    })
}

/// One alignment of `needle` onto `hay`, scored, with the char positions
/// it used. With `prefer_word_start`, each character takes the next
/// occurrence that begins a word when there is one, rather than simply the
/// next.
fn align(hay: &[char], needle: &str, prefer_word_start: bool) -> Option<(i32, Vec<usize>)> {
    let starts_word = |at: usize| -> bool {
        at == 0
            || hay
                .get(at - 1)
                .is_some_and(|c| matches!(c, '.' | '_' | '-' | '/'))
    };
    let mut at = 0usize;
    let mut first_at: Option<usize> = None;
    let mut previous: Option<usize> = None;
    let mut points = 0i32;
    let mut positions = Vec::with_capacity(needle.len());
    for target in needle.chars() {
        let next = hay[at..].iter().position(|c| *c == target)? + at;
        let found = if prefer_word_start {
            hay[at..]
                .iter()
                .enumerate()
                .find(|(i, c)| **c == target && starts_word(at + i))
                .map_or(next, |(i, _)| at + i)
        } else {
            next
        };
        if first_at.is_none() {
            first_at = Some(found);
        }
        if previous.is_some_and(|p| found == p + 1) {
            points += 6;
        }
        if starts_word(found) {
            points += 8;
        }
        previous = Some(found);
        positions.push(found);
        at = found + 1;
    }
    let first = first_at.unwrap_or(0);
    // An early first match beats a late one, and a whole-prefix hit beats
    // both.
    points += 40 - i32::try_from(first.min(40)).unwrap_or(0);
    if hay.iter().collect::<String>().starts_with(needle) {
        points += 60;
    }
    Some((points, positions))
}

pub struct QuickOpen {
    project: Entity<Project>,
    items: Vec<Item>,
    /// Best first.
    rows: Vec<Hit>,
    selected: usize,
    input: Entity<InputState>,
    query: String,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<QuickOpenEvent> for QuickOpen {}

impl QuickOpen {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Go to file, knot, stitch or label\u{2026}")
        });
        let on_input =
            cx.subscribe(
                &input,
                |this: &mut Self, state, event: &InputEvent, cx| match event {
                    InputEvent::Change => {
                        this.query = state.read(cx).value().to_string();
                        this.rebuild(cx);
                    }
                    InputEvent::PressEnter { .. } => this.confirm(cx),
                    _ => {}
                },
            );
        let mut this = Self {
            project: project.clone(),
            items: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            input,
            query: String::new(),
            focus: cx.focus_handle(),
            _subscriptions: vec![on_input],
        };
        this.reload(cx);
        this
    }

    /// The files now, the places when the worker answers. Files first so
    /// the overlay is useful on the frame it opens rather than a beat
    /// later — a picker that is empty when it appears trains you to wait.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let files: Vec<Item> = self
            .project
            .read(cx)
            .files()
            .iter()
            .map(|f| Item::file(f))
            .collect();
        self.items = files;
        self.rebuild(cx);

        let query = self.project.read(cx).query(QueryKind::GoToIndex, cx);
        cx.spawn(async move |this, cx| {
            let result = query.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(QueryResult::GoToIndex(places)) = result {
                    this.items.extend(places.iter().map(Item::place));
                    this.rebuild(cx);
                }
            });
        })
        .detach();
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.rows = rank(&self.items, &self.query);
        self.selected = 0;
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// The titles of the ranked rows, best first — for the tests.
    #[cfg(test)]
    pub(crate) fn ranked_titles(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|hit| self.items.get(hit.item))
            .map(|item| item.title.to_string())
            .collect()
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self
            .rows
            .get(self.selected)
            .and_then(|hit| self.items.get(hit.item))
        else {
            cx.emit(QuickOpenEvent::Dismiss);
            return;
        };
        cx.emit(QuickOpenEvent::Open {
            path: item.path.clone(),
            span: item.span.clone(),
        });
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len();
        let next = (self.selected as isize + delta).rem_euclid(len as isize);
        self.selected = usize::try_from(next).unwrap_or(0);
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.move_selection(1, cx),
            "up" => self.move_selection(-1, cx),
            "escape" => cx.emit(QuickOpenEvent::Dismiss),
            _ => {}
        }
    }

    fn render_row(&self, row: usize, cx: &App) -> gpui::AnyElement {
        let theme = cx.theme();
        let Some((hit, item)) = self
            .rows
            .get(row)
            .and_then(|hit| Some((hit, self.items.get(hit.item)?)))
        else {
            return div().into_any_element();
        };
        let selected = row == self.selected;
        let colour = item.kind.colour(cx);
        let bold = HighlightStyle {
            color: Some(theme.primary),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..HighlightStyle::default()
        };
        let name = StyledText::new(item.name.clone()).with_highlights(
            item.name_highlights(&hit.positions)
                .into_iter()
                .map(|range| (range, bold)),
        );
        div()
            .px(px(6.))
            .child(
                h_flex()
                    .h(px(ROW_HEIGHT))
                    .px(px(12.))
                    .gap(px(11.))
                    .items_center()
                    .rounded(px(8.))
                    .when(selected, |el| el.bg(theme.accent))
                    .child(
                        div()
                            .flex_none()
                            .child(icons::icon(item.kind.icon(), px(ICON), colour)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(theme.foreground)
                                    .truncate()
                                    .child(name),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_1()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child(div().min_w_0().truncate().child(item.crumb.clone()))
                                    .children(item.line.map(|line| {
                                        div()
                                            .flex_none()
                                            .text_color(theme.muted_foreground.opacity(0.6))
                                            .child(format!(":{line}"))
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px(px(7.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(colour.opacity(0.12))
                            .text_size(px(11.))
                            .text_color(colour)
                            .child(item.kind.chip()),
                    ),
            )
            .into_any_element()
    }

    /// The footer's key hints: a key, then what it does.
    fn hint(key: &'static str, does: &'static str, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_1()
            .child(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.foreground.opacity(0.75))
                    .child(key),
            )
            .child(does)
    }
}

impl Focusable for QuickOpen {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for QuickOpen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.rows.len();
        let visible = count.min(MAX_VISIBLE_ROWS);
        let summary: SharedString = format!("{count} of {}", self.items.len()).into();
        // The scrim: a click outside dismisses, and it occludes what it
        // covers — a click that also lands on the Binder underneath is the
        // defect the Settings modal already had once.
        div()
            .absolute()
            .inset_0()
            .occlude()
            // Dimmed, as the design has it: the panel is the one thing to
            // look at while it is up.
            .bg(gpui::black().opacity(0.35))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(QuickOpenEvent::Dismiss)),
            )
            .child(
                v_flex()
                    .absolute()
                    .top(px(64.))
                    .left_1_2()
                    .ml(px(-WIDTH / 2.0))
                    .w(px(WIDTH))
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {})
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(px(12.))
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .h(px(52.))
                            .px(px(16.))
                            .gap(px(10.))
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border.opacity(0.6))
                            .child(
                                div()
                                    .flex_none()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(theme.border)
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(px(12.))
                                    .text_color(theme.muted_foreground)
                                    .child("\u{2318}K"),
                            )
                            .child(
                                div().flex_1().text_size(px(16.)).child(
                                    Input::new(&self.input)
                                        .appearance(false)
                                        .bordered(false)
                                        .focus_bordered(false),
                                ),
                            ),
                    )
                    .when(count > 0, |el| {
                        el.child(
                            uniform_list(
                                "quick-open-rows",
                                count,
                                cx.processor(|this, range: Range<usize>, _window, cx| {
                                    range.map(|i| this.render_row(i, cx)).collect::<Vec<_>>()
                                }),
                            )
                            .h(px(visible as f32 * ROW_HEIGHT + 12.))
                            .py(px(6.)),
                        )
                    })
                    .child(
                        h_flex()
                            .h(px(32.))
                            .px(px(16.))
                            .gap(px(16.))
                            .items_center()
                            .border_t_1()
                            .border_color(theme.border.opacity(0.6))
                            .text_size(px(11.))
                            .text_color(theme.muted_foreground)
                            .child(Self::hint("\u{2191}\u{2193}", "move", cx))
                            .child(Self::hint("\u{21b5}", "go", cx))
                            .child(Self::hint("esc", "close", cx))
                            .child(div().ml_auto().child(summary)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(titles: &[&str]) -> Vec<Item> {
        titles.iter().map(|t| Item::file(t)).collect()
    }

    fn ranked(titles: &[&str], query: &str) -> Vec<String> {
        let items = items(titles);
        rank(&items, query)
            .into_iter()
            .map(|hit| items[hit.item].title.to_string())
            .collect()
    }

    #[test]
    fn an_empty_query_keeps_the_projects_own_order() {
        assert_eq!(
            ranked(&["story.ink", "harbour.ink", "a.ink"], "  "),
            ["story.ink", "harbour.ink", "a.ink"]
        );
    }

    #[test]
    fn a_prefix_beats_a_match_in_the_middle() {
        // Typing `sh` should find `shore` before `harbour_scene`, which
        // contains an `s` and an `h` but starts with neither.
        let out = ranked(&["harbour_scene", "shore"], "sh");
        assert_eq!(out.first().map(String::as_str), Some("shore"), "{out:?}");
    }

    #[test]
    fn matching_is_a_subsequence_not_a_substring() {
        // `lhtop` finds `lighthouse.top` — the point of a fuzzy picker.
        assert_eq!(ranked(&["lighthouse.top"], "lhtop"), ["lighthouse.top"]);
    }

    #[test]
    fn a_non_match_is_dropped_rather_than_ranked_last() {
        assert!(ranked(&["shore", "lighthouse"], "zzz").is_empty());
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(ranked(&["Shore.Ink"], "shore"), ["Shore.Ink"]);
    }

    #[test]
    fn a_word_start_decides_between_two_subsequence_matches() {
        // Both contain `l h t o p` as a subsequence; only `lighthouse.top`
        // has `top` starting a word, which is the one a person means.
        let out = ranked(&["lighthouse.approach", "lighthouse.top"], "lhtop");
        assert_eq!(
            out.first().map(String::as_str),
            Some("lighthouse.top"),
            "{out:?}"
        );
    }

    #[test]
    fn preferring_a_word_start_never_loses_a_match() {
        // `ab` is a subsequence of `ab.a` only at 0,1 — the word-start
        // preference would take the `a` after the dot and find no `b`, so
        // the leftmost pass has to be the one that decides.
        assert_eq!(ranked(&["ab.a"], "ab"), ["ab.a"]);
    }

    #[test]
    fn contiguous_beats_scattered() {
        // `abc` is contiguous in `abc_x` and scattered in `a_b_c_y`.
        let out = ranked(&["a_b_c_y", "abc_x"], "abc");
        assert_eq!(out.first().map(String::as_str), Some("abc_x"), "{out:?}");
    }

    #[test]
    fn the_result_list_is_capped() {
        let titles: Vec<String> = (0..CAP + 50).map(|i| format!("file{i}.ink")).collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        assert_eq!(ranked(&refs, "").len(), CAP, "an empty query is capped too");
        assert!(ranked(&refs, "file").len() <= CAP);
    }

    #[test]
    fn a_place_carries_its_file_span_and_breadcrumb() {
        let symbol = GoToSymbol {
            kind: GoToKind::Label,
            qualified: "harbour.night_watch.oath".to_owned(),
            file: "chapters/harbour.ink".to_owned(),
            span: 10..14,
            line: 7,
        };
        let item = Item::place(&symbol);
        assert_eq!(item.title.as_ref(), "harbour.night_watch.oath");
        assert_eq!(item.name.as_ref(), "oath");
        assert_eq!(
            item.crumb.as_ref(),
            "chapters/harbour.ink \u{203a} harbour \u{203a} night_watch \u{203a} oath"
        );
        assert_eq!(item.line, Some(7));
        assert_eq!(item.kind, Kind::Label);
        assert_eq!(item.path, "chapters/harbour.ink");
        assert_eq!(
            item.span,
            Some(10..14),
            "so the row reveals, not just opens"
        );
    }

    #[test]
    fn a_file_shows_its_name_over_its_path() {
        let item = Item::file("side/watchers.ink");
        assert_eq!(item.name.as_ref(), "watchers.ink");
        assert_eq!(item.crumb.as_ref(), "side/watchers.ink");
        assert_eq!(item.line, None);
    }

    /// `wat` puts the places whose own name has it first, and lights those
    /// letters; one that matches only through its parent comes after.
    #[test]
    fn a_match_in_the_own_name_outranks_one_through_the_parent() {
        let place = |q: &str| {
            Item::place(&GoToSymbol {
                kind: GoToKind::Label,
                qualified: q.to_owned(),
                file: "h.ink".to_owned(),
                span: 0..0,
                line: 1,
            })
        };
        let items = vec![
            place("harbour.night_watch.oath"),
            place("watchtower.keep_watch"),
        ];
        let hits = rank(&items, "wat");
        assert_eq!(hits[0].item, 1, "keep_watch before oath");
        let lit = items[1].name_highlights(&hits[0].positions);
        assert_eq!(lit.len(), 1, "{lit:?}");
        assert_eq!(lit[0], 5..8, "the `wat` of keep_watch is the one lit");
        assert_eq!(hits.len(), 2, "oath still matches, lower down");
    }

    /// The bold letters are the ones in the name that matched; a match in
    /// the address before it is not drawn on the name.
    #[test]
    fn the_name_highlights_the_matched_letters_in_runs() {
        let item = Item::place(&GoToSymbol {
            kind: GoToKind::Stitch,
            qualified: "harbour.night_watch".to_owned(),
            file: "harbour.ink".to_owned(),
            span: 0..0,
            line: 1,
        });
        let hits = rank(std::slice::from_ref(&item), "wat");
        let runs = item.name_highlights(&hits[0].positions);
        assert_eq!(runs.len(), 1, "one run: {runs:?}");
        assert_eq!(runs[0], 6..9, "`wat` of night_watch");

        let hits = rank(std::slice::from_ref(&item), "harnw");
        let runs = item.name_highlights(&hits[0].positions);
        assert_eq!(
            runs,
            [0..1, 6..7],
            "`har` matched in the knot, `n` and `w` in the name"
        );
    }
}
