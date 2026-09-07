//! Quick-open — `cmd-p`, the studio's "binder files/knots/stitches"
//! picker (`docs/studio-shell-spec.md` §5.2; deferred in
//! `docs/gpui-studio-spec.md` §4.5 until now).
//!
//! ## Why this is not the command palette
//!
//! The shell's palette is over the command registry: its items are
//! actions, its ranking is over command titles, and confirming one
//! dispatches it. Quick-open is over the *project* — files, knots and
//! stitches — and confirming one opens a place. The overlay behaviour is
//! the same, but the shell must not learn what a knot is (the one-way edge
//! the three-crate split exists for), so this lives in the feature crate
//! and the app root paints it, the way it already paints the dialog and
//! notification layers.
//!
//! ## Where the items come from
//!
//! Files come from the mirror. Knots and stitches come from one
//! [`QueryKind::PassageIndex`] — the same query the Conventions editor's
//! picker uses, which is why it carries a span: a passage row reveals its
//! declaration rather than only opening its file.
//!
//! The index is asked for when the overlay opens, not held between
//! openings. It is one query over the analysis that already exists, and a
//! cached list would be a second thing to keep in step with every edit for
//! no gain a person could perceive.

use std::ops::Range;

use brink_gpui_model::query::{PassageSymbol, QueryKind, QueryResult};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render,
    SharedString, Subscription, Window, div, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use crate::project::Project;

/// Rows drawn before the list scrolls, and the overlay's width.
const MAX_VISIBLE_ROWS: usize = 12;
const ROW_HEIGHT: f32 = 30.0;
const WIDTH: f32 = 520.0;

/// The most rows kept after filtering — a project with thousands of
/// passages must not build thousands of elements for a one-letter query.
const CAP: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickOpenEvent {
    /// Open this file, revealing `span` when there is one.
    Open {
        path: String,
        span: Option<Range<usize>>,
    },
    Dismiss,
}

/// What an item is, which decides its icon. The Binder's icon language
/// (`crate::icons`), so a knot reads the same in both places — the row is
/// scanned by shape before it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Knot,
    Stitch,
}

impl Kind {
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            // The outline file, not the entry or draft variants: those say
            // something about the file's ROLE, which this list does not.
            Self::File => crate::icons::FILE,
            // Outline, by the Binder's fill rule: filled means collapsed
            // over content, and nothing here is collapsed.
            Self::Knot => crate::icons::KNOT,
            Self::Stitch => crate::icons::STITCH,
        }
    }
}

/// One openable place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What is typed against and shown: a file name, or a knot path.
    pub title: SharedString,
    /// The file it lives in, shown subdued when it differs from the title.
    pub detail: Option<SharedString>,
    pub kind: Kind,
    pub path: String,
    pub span: Option<Range<usize>>,
}

impl Item {
    fn file(path: &str) -> Self {
        Self {
            title: SharedString::from(path.to_owned()),
            detail: None,
            kind: Kind::File,
            path: path.to_owned(),
            span: None,
        }
    }

    fn passage(symbol: &PassageSymbol) -> Self {
        Self {
            title: SharedString::from(symbol.path.clone()),
            detail: Some(SharedString::from(symbol.file.clone())),
            kind: if symbol.is_stitch {
                Kind::Stitch
            } else {
                Kind::Knot
            },
            path: symbol.file.clone(),
            span: Some(symbol.span.clone()),
        }
    }
}

/// Rank `items` against `query`, keeping the input's order when it is
/// empty. Subsequence matching with a bonus for a prefix hit, which is
/// what makes `sh` find `shore` before `harbour_scene`.
#[must_use]
pub fn rank(items: &[Item], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..items.len().min(CAP)).collect();
    }
    let needle = query.trim().to_lowercase();
    let mut scored: Vec<(i32, usize)> = Vec::new();
    for (ix, item) in items.iter().enumerate() {
        let hay = item.title.to_lowercase();
        let Some(score) = score(&hay, &needle) else {
            continue;
        };
        scored.push((score, ix));
    }
    // Best first; ties keep the project's own order, which is file order
    // then declaration order — stable, so a list does not reshuffle as you
    // type a character that changes nothing.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.truncate(CAP);
    scored.into_iter().map(|(_, ix)| ix).collect()
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
fn score(hay: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = hay.chars().collect();
    let leftmost = align(&hay, needle, false)?;
    let boundary = align(&hay, needle, true).unwrap_or(leftmost);
    Some(leftmost.max(boundary))
}

/// One alignment of `needle` onto `hay`, scored. With `prefer_word_start`,
/// each character takes the next occurrence that begins a word when there
/// is one, rather than simply the next.
fn align(hay: &[char], needle: &str, prefer_word_start: bool) -> Option<i32> {
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
        at = found + 1;
    }
    let first = first_at.unwrap_or(0);
    // An early first match beats a late one, and a whole-prefix hit beats
    // both.
    points += 40 - i32::try_from(first.min(40)).unwrap_or(0);
    if hay.iter().collect::<String>().starts_with(needle) {
        points += 60;
    }
    Some(points)
}

pub struct QuickOpen {
    project: Entity<Project>,
    items: Vec<Item>,
    /// Indices into `items`, best first.
    rows: Vec<usize>,
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
            InputState::new(window, cx).placeholder("Go to file, knot or stitch\u{2026}")
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

    /// The files now, the passages when the worker answers. Files first so
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

        let query = self.project.read(cx).query(QueryKind::PassageIndex, cx);
        cx.spawn(async move |this, cx| {
            let result = query.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(QueryResult::PassageIndex(symbols)) = result {
                    this.items.extend(symbols.iter().map(Item::passage));
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

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self
            .rows
            .get(self.selected)
            .and_then(|ix| self.items.get(*ix))
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

    fn render_row(&self, row: usize, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let Some(item) = self.rows.get(row).and_then(|ix| self.items.get(*ix)) else {
            return div();
        };
        let selected = row == self.selected;
        div().child(
            h_flex()
                .w_full()
                .h(px(ROW_HEIGHT))
                .px_2()
                .gap_2()
                .items_center()
                .when(selected, |el| el.bg(theme.accent))
                // A fixed slot, so the titles line up whatever the icon:
                // a ragged left edge is harder to scan than no icon at all.
                // 13px is the Binder's size, since these are its icons.
                .child(div().w(px(16.)).flex_none().child(crate::icons::icon(
                    item.kind.icon(),
                    px(13.),
                    theme.muted_foreground,
                )))
                .child(div().child(item.title.clone()))
                .children(item.detail.as_ref().map(|d| {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(d.clone())
                })),
        )
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
        let summary: SharedString = format!("{count} result(s)").into();
        // The scrim: a click outside dismisses, and it occludes what it
        // covers — a click that also lands on the Binder underneath is the
        // defect the Settings modal already had once.
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(QuickOpenEvent::Dismiss)),
            )
            .child(
                v_flex()
                    .absolute()
                    .top(px(80.))
                    .left_1_2()
                    .ml(px(-WIDTH / 2.0))
                    .w(px(WIDTH))
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {})
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key))
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .shadow_lg()
                    .child(div().p_1().child(Input::new(&self.input)))
                    .when(count > 0, |el| {
                        el.child(
                            uniform_list(
                                "quick-open-rows",
                                count,
                                cx.processor(|this, range: Range<usize>, _window, cx| {
                                    range
                                        .map(|i| this.render_row(i, cx).into_any_element())
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .h(px(visible as f32 * ROW_HEIGHT))
                            .pb_1(),
                        )
                    })
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .border_t_1()
                            .border_color(theme.border)
                            .child(summary),
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
            .map(|ix| items[ix].title.to_string())
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
    fn a_passage_carries_its_file_and_span() {
        let symbol = PassageSymbol {
            path: "lighthouse.top".to_owned(),
            is_stitch: true,
            file: "story.ink".to_owned(),
            span: 10..24,
        };
        let item = Item::passage(&symbol);
        assert_eq!(item.title.as_ref(), "lighthouse.top");
        assert_eq!(item.detail.as_deref(), Some("story.ink"));
        assert_eq!(item.path, "story.ink");
        assert_eq!(
            item.span,
            Some(10..24),
            "so the row reveals, not just opens"
        );
    }
}
