//! The Problems panel — every diagnostic in the project, as the studio's
//! Problems view shows them (`packages/studio-ui/src/ProblemsView.tsx`;
//! `docs/studio-shell-spec.md` §4, §5.1, §6.1).
//!
//! It reads the mirror rather than the session, so it needs no query of its
//! own: diagnostics arrive with the analysis, already project-wide.
//!
//! ## What is ported
//!
//! - **Canonical order**: file, then offset, errors first at one offset.
//! - **Grouped by file by default** (ruled — "a flat list of every
//!   diagnostic in a project reads as noise; per-file sections are how you
//!   actually scan it"), with collapsible headings carrying a count summary,
//!   and a flat list a toggle away.
//! - **Severity buckets as toggles**, each showing its count over the
//!   UNFILTERED list, so a muted bucket still says what turning it back on
//!   would restore. Info and Hint share one bucket; TODO notes (`E189`)
//!   are their own bucket and off by default (ruled 2026-08-29: they belong
//!   to the TODOs panel, and Problems shows them only when asked).
//! - **A text filter** over the message and the location.
//! - **Click-to-reveal** (§6.1's `editor.reveal`): open the file, put the
//!   caret on the span, select it, focus the editor.
//! - **The rail badge** (§5.1): the error count, hidden when clean.
//!
//! ## Not ported yet
//!
//! The prose bucket (there is no native prose checker), Fix buttons (the
//! worker offers no fixes), and the suppress context menu (#3148).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use brink_gpui_model::worker::Diagnostic;
use brink_gpui_shell::tool_window::{Badge, BadgeTone, TabSlot, ToolWindow};
use brink_ir::{LineIndex, Severity};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, SharedString, Subscription, Window, div, px, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use rowan::TextSize;

use brink_gpui_model::fixes::{FixPlan, FixScope};
use brink_gpui_model::query::{QueryKind, QueryResult};

use crate::fixes;
use crate::icons;
use crate::project::{Project, ProjectEvent};

/// How a row finds its fixes in the one-per-analysis offers map: its
/// diagnostic's `(path, start, end, code)`.
type OfferKey = (String, u32, u32, String);

/// Activating a row opens its file with the span selected.
#[derive(Debug, Clone)]
pub struct OpenProblem {
    pub path: String,
    pub span: Range<usize>,
}

/// The lint code TODO notes carry (decision log 2026-08-23: emitted at HIR
/// lowering as Info-severity diagnostics).
const TODO_CODE: &str = "E189";

/// Which toggle a diagnostic belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Error,
    Warning,
    /// Info and Hint together: the rows render them identically.
    Info,
    Todo,
}

impl Bucket {
    pub const ALL: [Self; 4] = [Self::Error, Self::Warning, Self::Info, Self::Todo];

    /// Source before severity: a TODO note is Info-severity, and letting it
    /// fall through to the `Info` bucket would make "off by default"
    /// impossible to express, since Info is on.
    fn of(d: &Diagnostic) -> Self {
        if d.code == TODO_CODE {
            return Self::Todo;
        }
        match d.severity {
            Severity::Error => Self::Error,
            Severity::Warning => Self::Warning,
            _ => Self::Info,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Info => 2,
            Self::Todo => 3,
        }
    }

    /// The glyph a row shows, reused on its toggle so the toggle reads as
    /// "this kind of row".
    const fn glyph(self) -> &'static str {
        match self {
            Self::Error => "\u{25CF}",
            Self::Warning => "\u{25B2}",
            Self::Info => "\u{2139}",
            Self::Todo => "\u{2611}",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Error => "errors",
            Self::Warning => "warnings",
            Self::Info => "info and hints",
            Self::Todo => "TODO notes",
        }
    }

    /// Off by default for TODO notes only — see the module doc.
    const fn on_by_default(self) -> bool {
        !matches!(self, Self::Todo)
    }

    /// Errors first at one offset.
    const fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Info => 2,
            Self::Todo => 3,
        }
    }
}

/// One diagnostic, decorated for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub span: Range<usize>,
    pub bucket: Bucket,
    pub code: String,
    pub message: String,
    /// 1-based, when the file's source was available to resolve it.
    pub line_col: Option<(u32, u32)>,
}

impl Row {
    /// "file.ink:12:5", or "file.ink@offset" when the source is unknown.
    fn location(&self) -> String {
        match self.line_col {
            Some((line, col)) => format!("{}:{line}:{col}", self.path),
            None => format!("{}@{}", self.path, self.span.start),
        }
    }

    /// Inside a file group the path is redundant: "12:5".
    fn location_in_file(&self) -> String {
        match self.line_col {
            Some((line, col)) => format!("{line}:{col}"),
            None => format!("@{}", self.span.start),
        }
    }
}

/// Every diagnostic in canonical order, located against `source_of`.
///
/// `source_of` is consulted once per file; `None` means the source is not
/// loaded, and the row falls back to its offset.
pub fn build_rows<'a>(
    diagnostics: impl IntoIterator<Item = (&'a String, &'a Vec<Diagnostic>)>,
    mut source_of: impl FnMut(&str) -> Option<String>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (path, found) in diagnostics {
        let index = source_of(path).map(|source| LineIndex::new(&source));
        let mut file_rows: Vec<Row> = found
            .iter()
            .map(|d| Row {
                path: path.clone(),
                span: d.start as usize..d.end as usize,
                bucket: Bucket::of(d),
                code: d.code.clone(),
                message: d.message.clone(),
                line_col: index.as_ref().map(|index| {
                    let (line, col) = index.line_col(TextSize::from(d.start));
                    (line + 1, col + 1)
                }),
            })
            .collect();
        file_rows.sort_by_key(|row| (row.span.start, row.bucket.rank()));
        rows.extend(file_rows);
    }
    rows
}

/// Per-bucket totals, indexed by [`Bucket::index`].
pub fn count_by_bucket<'a>(rows: impl IntoIterator<Item = &'a Row>) -> [usize; 4] {
    let mut counts = [0; 4];
    for row in rows {
        counts[row.bucket.index()] += 1;
    }
    counts
}

/// Case-insensitive match over the message and the display location.
pub fn matches_filter(row: &Row, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || row.message.to_lowercase().contains(&q)
        || row.location().to_lowercase().contains(&q)
}

/// The toggles and the filter applied, order preserved.
pub fn visible_rows<'a>(rows: &'a [Row], enabled: &[bool; 4], query: &str) -> Vec<&'a Row> {
    rows.iter()
        .filter(|row| enabled[row.bucket.index()] && matches_filter(row, query))
        .collect()
}

/// "2 errors · 1 warning · 1 info · 1 todo", omitting empty buckets.
pub fn summarize(counts: &[usize; 4]) -> String {
    let mut parts = Vec::new();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    if counts[0] > 0 {
        parts.push(plural(counts[0], "error", "errors"));
    }
    if counts[1] > 0 {
        parts.push(plural(counts[1], "warning", "warnings"));
    }
    if counts[2] > 0 {
        parts.push(format!("{} info", counts[2]));
    }
    if counts[3] > 0 {
        parts.push(format!("{} todo", counts[3]));
    }
    parts.join(" \u{B7} ")
}

/// What the list draws, top to bottom, after grouping and collapsing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Heading {
        path: String,
        summary: String,
        collapsed: bool,
    },
    /// An index into [`Problems::rows`], and whether it sits under a heading
    /// (which already names the file).
    Row { index: usize, in_group: bool },
}

/// Lay the visible rows out as headings and rows. Files keep their
/// first-appearance order, which is the canonical one.
fn layout(
    rows: &[Row],
    visible: &[&Row],
    grouped: bool,
    collapsed: &BTreeSet<String>,
) -> Vec<Item> {
    let index_of = |row: &Row| {
        rows.iter()
            .position(|r| std::ptr::eq(r, row))
            .unwrap_or_default()
    };
    if !grouped {
        return visible
            .iter()
            .map(|row| Item::Row {
                index: index_of(row),
                in_group: false,
            })
            .collect();
    }
    let mut items = Vec::new();
    let mut i = 0;
    while i < visible.len() {
        let path = &visible[i].path;
        let end = visible[i..]
            .iter()
            .position(|row| &row.path != path)
            .map_or(visible.len(), |n| i + n);
        let group = &visible[i..end];
        let is_collapsed = collapsed.contains(path);
        items.push(Item::Heading {
            path: path.clone(),
            summary: summarize(&count_by_bucket(group.iter().copied())),
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            items.extend(group.iter().map(|row| Item::Row {
                index: index_of(row),
                in_group: true,
            }));
        }
        i = end;
    }
    items
}

pub struct Problems {
    project: Entity<Project>,
    focus: FocusHandle,
    /// Every diagnostic, canonical order, rebuilt when an analysis lands.
    rows: Vec<Row>,
    /// Totals over `rows`, for the toggles.
    counts: [usize; 4],
    /// What the list draws — `rows` after the toggles, filter and grouping.
    items: Vec<Item>,
    enabled: [bool; 4],
    grouped: bool,
    collapsed: BTreeSet<String>,
    filter: Entity<InputState>,
    filter_text: String,
    filter_open: bool,
    tab: TabSlot,
    /// The fixes offered for each visible diagnostic — ONE query per
    /// analysis (`docs/autofix-spec.md` §7), each row looking itself up.
    offers: BTreeMap<OfferKey, Vec<FixPlan>>,
    /// What "Fix all safe" would take: the batch's own count.
    batchable: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenProblem> for Problems {}
impl EventEmitter<PanelEvent> for Problems {}

impl Problems {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter problems…"));
        let on_filter = cx.subscribe(&filter, |this: &mut Self, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.filter_text = state.read(cx).value().to_string();
                this.relayout(cx);
            }
        });
        let on_project = cx.subscribe(&project, |this, _, event: &ProjectEvent, cx| {
            if matches!(event, ProjectEvent::Analyzed) {
                this.rebuild(cx);
            }
        });
        Self {
            project,
            focus: cx.focus_handle(),
            rows: Vec::new(),
            counts: [0; 4],
            items: Vec::new(),
            enabled: [
                Bucket::Error.on_by_default(),
                Bucket::Warning.on_by_default(),
                Bucket::Info.on_by_default(),
                Bucket::Todo.on_by_default(),
            ],
            grouped: true,
            collapsed: BTreeSet::new(),
            filter,
            filter_text: String::new(),
            filter_open: false,
            tab: TabSlot::default(),
            offers: BTreeMap::new(),
            batchable: 0,
            _subscriptions: vec![on_filter, on_project],
        }
    }

    /// Errors only — the rail badge's number (§5.1).
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.counts[Bucket::Error.index()]
    }

    /// Re-read the mirror. Locations are resolved here, once per analysis,
    /// rather than on every frame.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let project = self.project.read(cx);
        self.rows = build_rows(project.all_diagnostics(), |path| {
            project.loaded_source(path).map(str::to_owned)
        });
        self.counts = count_by_bucket(&self.rows);
        self.relayout(cx);
        self.refresh_offers(cx);
    }

    /// Ask once for every offered fix; rows look themselves up when drawn.
    fn refresh_offers(&mut self, cx: &mut Context<Self>) {
        let query = self.project.read(cx).query(QueryKind::FixOffers, cx);
        cx.spawn(async move |this, cx| {
            let Ok(QueryResult::FixOffers(offers)) = query.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.offers.clear();
                for offer in offers.offers {
                    this.offers
                        .entry((offer.path, offer.start, offer.end, offer.code))
                        .or_default()
                        .push(offer.fix);
                }
                this.batchable = offers.batchable;
                cx.notify();
            });
        })
        .detach();
    }

    fn fix_row(&mut self, plan: FixPlan, window: &mut Window, cx: &mut Context<Self>) {
        fixes::apply_fix(&self.project, &plan, None, window, cx);
    }

    /// Re-apply the toggles, filter and grouping to the rows already built.
    fn relayout(&mut self, cx: &mut Context<Self>) {
        let visible = visible_rows(&self.rows, &self.enabled, &self.filter_text);
        self.items = layout(&self.rows, &visible, self.grouped, &self.collapsed);
        cx.notify();
    }

    fn toggle_bucket(&mut self, bucket: Bucket, cx: &mut Context<Self>) {
        self.enabled[bucket.index()] = !self.enabled[bucket.index()];
        self.relayout(cx);
    }

    fn toggle_collapsed(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_owned());
        }
        self.relayout(cx);
    }

    /// A header affordance, the Binder's idiom: our own SVG, tinted, with
    /// an active state.
    fn tool(
        id: &'static str,
        src: &'static str,
        active: bool,
        tooltip: &'static str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let theme = cx.theme();
        let color = if active {
            theme.primary
        } else {
            theme.muted_foreground
        };
        Button::new(id)
            .ghost()
            .compact()
            .toggled(active)
            .tooltip(tooltip)
            .child(icons::icon(src, px(14.), color))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                on_click(this, window, cx);
            }))
            .into_any_element()
    }

    fn render_toggle(&self, bucket: Bucket, cx: &mut Context<Self>) -> AnyElement {
        let on = self.enabled[bucket.index()];
        let count = self.counts[bucket.index()];
        let theme = cx.theme();
        let colour = if on {
            match bucket {
                Bucket::Error => theme.danger,
                Bucket::Warning => theme.warning,
                _ => theme.foreground,
            }
        } else {
            theme.muted_foreground
        };
        Button::new(SharedString::from(format!("problems-{}", bucket.label())))
            .ghost()
            .compact()
            .toggled(on)
            .tooltip(format!(
                "{} {}",
                if on { "Hide" } else { "Show" },
                bucket.label()
            ))
            .child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(colour)
                    .child(bucket.glyph())
                    .child(count.to_string()),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.toggle_bucket(bucket, cx);
            }))
            .into_any_element()
    }

    fn render_item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, hover) = (
            theme.foreground,
            theme.muted_foreground,
            theme.muted.opacity(0.5),
        );
        match &self.items[ix] {
            Item::Heading {
                path,
                summary,
                collapsed,
            } => {
                let path = path.clone();
                let target = path.clone();
                h_flex()
                    .id(("problems-file", ix))
                    .w_full()
                    .h(px(24.))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .text_xs()
                    .child(div().w(px(10.)).text_color(muted).child(if *collapsed {
                        "\u{25B8}"
                    } else {
                        "\u{25BE}"
                    }))
                    .child(div().text_color(fg).child(path))
                    .child(div().text_color(muted).child(summary.clone()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_collapsed(&target, cx);
                    }))
                    .into_any_element()
            }
            Item::Row { index, in_group } => {
                let row = &self.rows[*index];
                let colour = match row.bucket {
                    Bucket::Error => theme.danger,
                    Bucket::Warning => theme.warning,
                    _ => muted,
                };
                let location = if *in_group {
                    row.location_in_file()
                } else {
                    row.location()
                };
                let open = OpenProblem {
                    path: row.path.clone(),
                    span: row.span.clone(),
                };
                let key: OfferKey = (
                    row.path.clone(),
                    u32::try_from(row.span.start).unwrap_or(u32::MAX),
                    u32::try_from(row.span.end).unwrap_or(u32::MAX),
                    row.code.clone(),
                );
                // The row's first offered fix; the rest are one `cmd-.`
                // away in the editor once the row is opened.
                let fix = self.offers.get(&key).and_then(|f| f.first()).cloned();
                h_flex()
                    .id(("problem", ix))
                    // Full width and clipped, or a long message pushes the
                    // location off the right edge instead of truncating.
                    .w_full()
                    .overflow_hidden()
                    .h(px(24.))
                    .px_2()
                    .when(*in_group, |el| el.pl_6())
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .text_xs()
                    .child(
                        div()
                            .w(px(10.))
                            .text_color(colour)
                            .child(row.bucket.glyph()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(fg)
                            .child(SharedString::from(row.message.clone())),
                    )
                    .children(fix.map(|plan| {
                        let tooltip = SharedString::from(plan.title.clone());
                        Button::new(("problem-fix", ix))
                            .ghost()
                            .compact()
                            .xsmall()
                            .label("Fix")
                            .tooltip(tooltip)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                this.fix_row(plan.clone(), window, cx);
                            }))
                    }))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(muted)
                            .child(SharedString::from(location)),
                    )
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(open.clone());
                    }))
                    .into_any_element()
            }
        }
    }
}

impl Focusable for Problems {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for Problems {
    fn panel_name(&self) -> &'static str {
        "Problems"
    }

    fn on_added_to(
        &mut self,
        group: gpui::WeakEntity<gpui_component::dock::TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tab.added_to(group);
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tab.removed();
    }
}

impl Panel for Problems {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Problems")
    }

    /// The controls, in the panel's own title strip: the bucket toggles,
    /// the filter, and grouping — the studio's header actions.
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let toggles: Vec<AnyElement> = Bucket::ALL
            .iter()
            .map(|&bucket| self.render_toggle(bucket, cx))
            .collect();
        let batchable = self.batchable;
        Some(
            h_flex()
                .gap_0p5()
                .items_center()
                .when(batchable > 0, |el| {
                    el.child(
                        Button::new("problems-fix-all")
                            .ghost()
                            .compact()
                            .xsmall()
                            .label(format!("Fix all safe ({batchable})"))
                            .tooltip("Apply every safe fix in the compilation")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                fixes::fix_all(&this.project, FixScope::Project, window, cx);
                            })),
                    )
                    .child(div().w(px(6.)))
                })
                .children(toggles)
                .child(div().w(px(6.)))
                .child(Self::tool(
                    "problems-filter",
                    icons::SEARCH,
                    self.filter_open,
                    "Filter problems",
                    cx,
                    |this, _, cx| {
                        this.filter_open = !this.filter_open;
                        cx.notify();
                    },
                ))
                .child(Self::tool(
                    "problems-group",
                    icons::GROUP_BY_FILE,
                    self.grouped,
                    "Group by file",
                    cx,
                    |this, _, cx| {
                        this.grouped = !this.grouped;
                        this.relayout(cx);
                    },
                )),
        )
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl ToolWindow for Problems {
    fn tab_slot(&self) -> Option<&TabSlot> {
        Some(&self.tab)
    }

    /// The error count, hidden when clean (§5.1).
    fn badge(&self, _cx: &App) -> Option<Badge> {
        Badge::count(self.error_count(), BadgeTone::Danger)
    }
}

impl Render for Problems {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let has_analyzed = self.project.read(cx).has_analyzed();

        let empty: Option<&'static str> = if self.items.is_empty() {
            Some(if !has_analyzed {
                // Distinct from "no problems": nothing is known either way
                // until an analysis has landed. NOT keyed on the compile
                // closure, which is empty whenever `brink.toml` names no
                // entry however many times the project has analyzed.
                "Not analyzed yet."
            } else if self.rows.is_empty() {
                "No problems."
            } else {
                "No problems match."
            })
        } else {
            None
        };

        let count = self.items.len();
        v_flex()
            .id("problems")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .when(self.filter_open, |el| {
                el.child(div().px_2().py_1().child(Input::new(&self.filter).xsmall()))
            })
            .when_some(empty, |el, text| {
                el.child(div().p_3().text_color(muted).child(text))
            })
            .when(empty.is_none(), |el| {
                el.child(
                    uniform_list(
                        "problems-rows",
                        count,
                        cx.processor(|this, range: Range<usize>, _window, cx| {
                            range.map(|i| this.render_item(i, cx)).collect::<Vec<_>>()
                        }),
                    )
                    .p_1()
                    .flex_1(),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(start: u32, severity: Severity, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            start,
            end: start + 3,
            severity,
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    fn fixture() -> Vec<(String, Vec<Diagnostic>)> {
        vec![
            (
                "a.ink".to_owned(),
                vec![
                    diag(10, Severity::Warning, "W1", "late warning"),
                    diag(10, Severity::Error, "E1", "early error"),
                    diag(0, Severity::Info, TODO_CODE, "TODO: finish"),
                ],
            ),
            (
                "b.ink".to_owned(),
                vec![diag(4, Severity::Info, "I1", "a note")],
            ),
        ]
    }

    fn rows() -> Vec<Row> {
        let files = fixture();
        build_rows(files.iter().map(|(p, d)| (p, d)), |path| match path {
            "a.ink" => Some("line one\nline two\n".to_owned()),
            _ => None,
        })
    }

    #[test]
    fn rows_are_in_canonical_order_and_located() {
        let rows = rows();
        // File, then offset, errors first at one offset.
        let order: Vec<(&str, usize, Bucket)> = rows
            .iter()
            .map(|r| (r.path.as_str(), r.span.start, r.bucket))
            .collect();
        assert_eq!(
            order,
            [
                ("a.ink", 0, Bucket::Todo),
                ("a.ink", 10, Bucket::Error),
                ("a.ink", 10, Bucket::Warning),
                ("b.ink", 4, Bucket::Info),
            ]
        );
        // Offset 10 is line 2, col 2 (1-based) of "line one\nline two".
        assert_eq!(rows[1].location(), "a.ink:2:2");
        assert_eq!(rows[1].location_in_file(), "2:2");
        // No source for b.ink: the offset fallback.
        assert_eq!(rows[3].location(), "b.ink@4");
    }

    #[test]
    fn todo_notes_are_their_own_bucket_and_off_by_default() {
        let rows = rows();
        assert_eq!(count_by_bucket(&rows), [1, 1, 1, 1]);
        let defaults = [true, true, true, false];
        let visible = visible_rows(&rows, &defaults, "");
        assert!(visible.iter().all(|r| r.bucket != Bucket::Todo));
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn the_filter_matches_message_or_location() {
        let rows = rows();
        let all = [true; 4];
        assert_eq!(visible_rows(&rows, &all, "EARLY").len(), 1);
        assert_eq!(visible_rows(&rows, &all, "b.ink").len(), 1);
        assert_eq!(visible_rows(&rows, &all, "2:2").len(), 2);
        assert_eq!(visible_rows(&rows, &all, "  ").len(), 4);
    }

    #[test]
    fn grouping_makes_headings_with_summaries_and_collapses() {
        let rows = rows();
        let all = [true; 4];
        let visible = visible_rows(&rows, &all, "");
        let items = layout(&rows, &visible, true, &BTreeSet::new());
        assert!(matches!(
            &items[0],
            Item::Heading { path, summary, collapsed: false }
                if path == "a.ink" && summary == "1 error \u{B7} 1 warning \u{B7} 1 todo"
        ));
        assert_eq!(items.len(), 2 + 4, "two headings, four rows");
        assert!(matches!(
            items[1],
            Item::Row {
                index: 0,
                in_group: true
            }
        ));

        let collapsed = BTreeSet::from(["a.ink".to_owned()]);
        let items = layout(&rows, &visible, true, &collapsed);
        assert_eq!(items.len(), 2 + 1, "a.ink folded away, b.ink's row stays");

        let flat = layout(&rows, &visible, false, &BTreeSet::new());
        assert_eq!(flat.len(), 4);
        assert!(flat.iter().all(|item| matches!(
            item,
            Item::Row {
                in_group: false,
                ..
            }
        )));
    }

    #[test]
    fn summaries_pluralise_and_omit_empty_buckets() {
        assert_eq!(summarize(&[2, 1, 0, 0]), "2 errors \u{B7} 1 warning");
        assert_eq!(
            summarize(&[1, 0, 3, 1]),
            "1 error \u{B7} 3 info \u{B7} 1 todo"
        );
        assert_eq!(summarize(&[0; 4]), "");
    }
}
