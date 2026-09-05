//! The TODOs tool window — ink's `TODO:` author notes, as the studio's
//! TODOs view shows them (`packages/studio-ui/src/TodosView.tsx`; ruled
//! 2026-08-23, `docs/decision-log.md` "TODO feature").
//!
//! ## Data source
//!
//! No query of its own: lowering emits one `E189` Info diagnostic per
//! `AUTHOR_WARNING`, and the mirror already holds every diagnostic in the
//! project. The panel derives the rest — the note's text and `(tag)`, its
//! line, the knot or stitch it sits in — from that plus the sources it has.
//! **Lifetime is existence in source** (ruled): a note that is gone from
//! the next analysis lingers struck through for a moment and drops out;
//! nothing is persisted and there is no done state.
//!
//! ## What is ported
//!
//! - **Grouped by file → containing knot/stitch** (ruled: "grouping by file
//!   and knot/content mirrors how authors think about where work remains"),
//!   with a flat list a toggle away; file headers carry a count.
//! - **Filter**: a text filter over note, file and container, plus **tag
//!   chips** — `TODO(audio): …` lifts `audio` out of the text into a chip
//!   the list can be narrowed by. Closing the filter row clears both, so a
//!   closed row never hides notes silently.
//! - **Click-to-navigate**: a row reveals the note; a file header opens
//!   the file.
//! - **The rail badge**: the count, in the theme's TODO amber (advisory,
//!   not an error).
//! - **The leaving row**: a removed note stays 1.4 s, struck through and
//!   muted, keyed by LOCATION (file + line) so rewording a note updates
//!   its row in place rather than striking it.
//!
//! The editor's side of the feature — the amber band on a `TODO:` line —
//! is `document.rs`.

use std::collections::BTreeSet;
use std::ops::Range;
use std::time::{Duration, Instant};

use brink_gpui_model::worker::Diagnostic;
use brink_gpui_shell::tool_window::{Badge, BadgeTone, TabSlot, ToolWindow};
use brink_ir::LineIndex;
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

use crate::icons;
use crate::project::{Project, ProjectEvent};
use crate::search::container_at;

/// The lint code lowering assigns to `TODO:` author notes.
pub const TODO_CODE: &str = "E189";

/// How long a removed note lingers before it drops out.
pub const LEAVE: Duration = Duration::from_millis(1400);

/// Activating a row reveals the note; a file header opens the file.
#[derive(Debug, Clone)]
pub struct OpenTodo {
    pub path: String,
    pub span: Range<usize>,
}

/// One note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub path: String,
    pub span: Range<usize>,
    /// The `TODO(tag):` tag, when the note carries one.
    pub tag: Option<String>,
    /// The note's text, minus `TODO:` and any leading `(tag)`.
    pub text: String,
    /// 1-based, when the file's source is at hand.
    pub line: Option<u32>,
    /// `knot` / `knot.stitch`; `None` at file level.
    pub container: Option<String>,
}

impl TodoItem {
    /// Identity for the leaving diff: file + line (the offset when the
    /// source is missing). Ruled: by location, not text — rewording a note
    /// never churns the panel; only deleting the line counts as removal.
    #[must_use]
    pub fn key(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path),
            None => format!("{}@{}", self.path, self.span.start),
        }
    }
}

/// Split a leading `(tag)` off a note. `()` and `(   )` are not tags: an
/// empty chip would be unlabelled and unfilterable, so they stay as text.
#[must_use]
pub fn split_tag(text: &str) -> (Option<String>, String) {
    let Some(rest) = text.strip_prefix('(') else {
        return (None, text.to_owned());
    };
    let Some(close) = rest.find(')') else {
        return (None, text.to_owned());
    };
    let tag = rest[..close].trim();
    if tag.is_empty() {
        return (None, text.to_owned());
    }
    let after = rest[close + 1..].trim_start();
    let after = after.strip_prefix(':').unwrap_or(after).trim_start();
    (Some(tag.to_owned()), after.to_owned())
}

/// The note's text from an `E189` message (`TODO: …` or a bare `TODO`).
fn note_text(message: &str) -> &str {
    let rest = message.strip_prefix("TODO").unwrap_or(message);
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    rest.trim()
}

/// Every note in the project, in canonical (file, offset) order.
pub fn collect<'a>(
    diagnostics: impl IntoIterator<Item = (&'a String, &'a Vec<Diagnostic>)>,
    source_of: impl Fn(&str) -> Option<String>,
) -> Vec<TodoItem> {
    let mut files: Vec<(&String, &Vec<Diagnostic>)> = diagnostics.into_iter().collect();
    files.sort_by(|a, b| a.0.cmp(b.0));
    let mut items = Vec::new();
    for (path, diags) in files {
        let source = source_of(path);
        let index = source.as_deref().map(LineIndex::new);
        let mut in_file: Vec<&Diagnostic> = diags.iter().filter(|d| d.code == TODO_CODE).collect();
        in_file.sort_by_key(|d| d.start);
        for d in in_file {
            let (tag, text) = split_tag(note_text(&d.message));
            let start = d.start as usize;
            items.push(TodoItem {
                path: path.clone(),
                span: start..d.end as usize,
                tag,
                text,
                line: index
                    .as_ref()
                    .map(|ix| ix.line_col(TextSize::from(d.start)).0 + 1),
                container: source.as_deref().and_then(|s| container_at(s, start)),
            });
        }
    }
    items
}

/// Case-insensitive over note text, file and container.
#[must_use]
pub fn matches_filter(item: &TodoItem, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || item.text.to_lowercase().contains(&q)
        || item.path.to_lowercase().contains(&q)
        || item
            .container
            .as_ref()
            .is_some_and(|c| c.to_lowercase().contains(&q))
}

/// Every tag present, in first-appearance order — the chip row.
#[must_use]
pub fn tags_of(items: &[TodoItem]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for item in items {
        if let Some(tag) = &item.tag
            && !seen.contains(tag)
        {
            seen.push(tag.clone());
        }
    }
    seen
}

/// What the list draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A file heading with its note count.
    File { path: String, count: usize },
    /// A containing knot/stitch heading within a file.
    Container(String),
    /// A note: index into the visible items, and whether it sits under
    /// headings (grouped) or shows its own file (flat).
    Row { index: usize, in_group: bool },
}

/// Group `(file, container)` runs into headings. Items are expected in
/// canonical order, so a run is a group.
#[must_use]
pub fn layout(items: &[TodoItem], grouped: bool) -> Vec<Item> {
    let mut out = Vec::new();
    if !grouped {
        out.extend((0..items.len()).map(|index| Item::Row {
            index,
            in_group: false,
        }));
        return out;
    }
    let mut ix = 0;
    while ix < items.len() {
        let path = &items[ix].path;
        let end = ix + items[ix..].iter().take_while(|i| &i.path == path).count();
        out.push(Item::File {
            path: path.clone(),
            count: end - ix,
        });
        let mut at = ix;
        while at < end {
            let container = &items[at].container;
            let run = at
                + items[at..end]
                    .iter()
                    .take_while(|i| &i.container == container)
                    .count();
            if let Some(name) = container {
                out.push(Item::Container(name.clone()));
            }
            out.extend((at..run).map(|index| Item::Row {
                index,
                in_group: true,
            }));
            at = run;
        }
        ix = end;
    }
    out
}

/// A note that has left the source and is on its way out of the panel.
struct Leaving {
    item: TodoItem,
    expires: Instant,
}

pub struct Todos {
    project: Entity<Project>,
    focus: FocusHandle,
    /// The notes in the source right now.
    items: Vec<TodoItem>,
    leaving: Vec<Leaving>,
    /// Live and leaving notes together, after the filter — what `layout`
    /// draws from.
    visible: Vec<(TodoItem, bool)>,
    rows: Vec<Item>,
    grouped: bool,
    filter: Entity<InputState>,
    filter_text: String,
    filter_open: bool,
    tags: Vec<String>,
    selected_tags: BTreeSet<String>,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenTodo> for Todos {}
impl EventEmitter<PanelEvent> for Todos {}

const ROW_HEIGHT: f32 = 24.0;

impl Todos {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter TODOs\u{2026}"));
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
            items: Vec::new(),
            leaving: Vec::new(),
            visible: Vec::new(),
            rows: Vec::new(),
            grouped: true,
            filter,
            filter_text: String::new(),
            filter_open: false,
            tags: Vec::new(),
            selected_tags: BTreeSet::new(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_filter, on_project],
        }
    }

    /// Notes in the source — the badge's number.
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Re-read the mirror. A note present before and absent now starts
    /// leaving; one that came back (undo) stops.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let fresh = {
            let project = self.project.read(cx);
            collect(project.all_diagnostics(), |path| {
                project.loaded_source(path).map(str::to_owned)
            })
        };
        let now = Instant::now();
        let keys: BTreeSet<String> = fresh.iter().map(TodoItem::key).collect();
        let mut leaving: Vec<Leaving> = std::mem::take(&mut self.leaving)
            .into_iter()
            .filter(|l| !keys.contains(&l.item.key()))
            .collect();
        for old in &self.items {
            let key = old.key();
            if !keys.contains(&key) && !leaving.iter().any(|l| l.item.key() == key) {
                leaving.push(Leaving {
                    item: old.clone(),
                    expires: now + LEAVE,
                });
            }
        }
        let any_leaving = !leaving.is_empty();
        self.items = fresh;
        self.leaving = leaving;
        self.relayout(cx);
        if any_leaving {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(LEAVE).await;
                _ = this.update(cx, |this, cx| this.prune_leaving(cx));
            })
            .detach();
        }
    }

    /// Drop the leaving notes whose time is up.
    fn prune_leaving(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let before = self.leaving.len();
        self.leaving.retain(|l| l.expires > now);
        if self.leaving.len() != before {
            self.relayout(cx);
        }
    }

    /// Re-apply the filter, the tags and the grouping.
    fn relayout(&mut self, cx: &mut Context<Self>) {
        let mut merged: Vec<(TodoItem, bool)> = self
            .items
            .iter()
            .map(|i| (i.clone(), false))
            .chain(self.leaving.iter().map(|l| (l.item.clone(), true)))
            .collect();
        merged.sort_by(|a, b| {
            a.0.path
                .cmp(&b.0.path)
                .then(a.0.span.start.cmp(&b.0.span.start))
        });
        merged.retain(|(item, _)| matches_filter(item, &self.filter_text));
        // Chips come from the set BEFORE the tag filter, so selecting one
        // cannot make the others disappear and strand the selection.
        let unfiltered: Vec<TodoItem> = merged.iter().map(|(i, _)| i.clone()).collect();
        self.tags = tags_of(&unfiltered);
        if !self.selected_tags.is_empty() {
            merged.retain(|(item, _)| {
                item.tag
                    .as_ref()
                    .is_some_and(|t| self.selected_tags.contains(t))
            });
        }
        let items: Vec<TodoItem> = merged.iter().map(|(i, _)| i.clone()).collect();
        self.rows = layout(&items, self.grouped);
        self.visible = merged;
        cx.notify();
    }

    fn toggle_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if !self.selected_tags.remove(tag) {
            self.selected_tags.insert(tag.to_owned());
        }
        self.relayout(cx);
    }

    /// Open or close the filter row. Closing clears the text and the tags:
    /// a closed row that was still narrowing the list would be a panel
    /// hiding notes with no visible cause.
    fn toggle_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_open = !self.filter_open;
        if !self.filter_open {
            self.filter_text.clear();
            self.selected_tags.clear();
            self.filter.update(cx, |state, cx| {
                state.set_value(String::new(), window, cx);
            });
            self.relayout(cx);
        } else {
            self.filter.update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

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

    fn render_chips(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.tags.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let (accent, muted, border) = (theme.primary, theme.muted_foreground, theme.border);
        let mono = theme.mono_font_family.clone();
        Some(
            h_flex()
                .flex_wrap()
                .gap_1()
                .pt_1()
                .children(self.tags.iter().enumerate().map(|(ix, tag)| {
                    let on = self.selected_tags.contains(tag);
                    let target = tag.clone();
                    div()
                        .id(("todo-tag", ix))
                        .px_2()
                        .rounded_full()
                        .border_1()
                        .border_color(if on { accent } else { border })
                        .when(on, |el| el.bg(accent.opacity(0.12)))
                        .text_xs()
                        .font_family(mono.clone())
                        .text_color(if on { accent } else { muted })
                        .cursor_pointer()
                        .child(tag.clone())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.toggle_tag(&target, cx);
                        }))
                }))
                .into_any_element(),
        )
    }

    fn render_item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, hover, surface) = (
            theme.foreground,
            theme.muted_foreground,
            theme.muted.opacity(0.5),
            theme.sidebar,
        );
        let todo = brink_gpui_shell::theme::hsla(brink_gpui_shell::theme::current(cx).tokens.todo);
        let mono = theme.mono_font_family.clone();
        match &self.rows[ix] {
            Item::File { path, count } => {
                let open = OpenTodo {
                    path: path.clone(),
                    span: 0..0,
                };
                h_flex()
                    .id(("todos-file", ix))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(SharedString::from(path.clone())),
                    )
                    .child(div().text_color(muted).child(count.to_string()))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(open.clone());
                    }))
                    .into_any_element()
            }
            Item::Container(name) => h_flex()
                .id(("todos-container", ix))
                .w_full()
                .h(px(ROW_HEIGHT))
                .pl_6()
                .pr_2()
                .items_center()
                .text_xs()
                .text_color(muted)
                .child(SharedString::from(name.clone()))
                .into_any_element(),
            Item::Row { index, in_group } => {
                let (item, leaving) = &self.visible[*index];
                let open = OpenTodo {
                    path: item.path.clone(),
                    span: item.span.clone(),
                };
                let text: SharedString = if item.text.is_empty() {
                    "TODO".into()
                } else {
                    item.text.clone().into()
                };
                let leaving = *leaving;
                h_flex()
                    .id(("todo", ix))
                    .w_full()
                    .overflow_hidden()
                    .h(px(ROW_HEIGHT))
                    .pr_2()
                    .pl(px(if *in_group { 40. } else { 8. }))
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .when(!leaving, |el| {
                        el.cursor_pointer().hover(move |s| s.bg(hover))
                    })
                    .text_xs()
                    // The mark: a small amber square, the note's flag.
                    .child(
                        div()
                            .flex_shrink_0()
                            .size(px(7.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color(if leaving { muted } else { todo }),
                    )
                    .children(item.tag.clone().map(|tag| {
                        div()
                            .flex_shrink_0()
                            .px_1()
                            .rounded_sm()
                            .bg(surface)
                            .font_family(mono.clone())
                            .text_color(muted)
                            .child(tag)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(if leaving { muted } else { fg })
                            .when(leaving, |el| el.line_through())
                            .child(text),
                    )
                    .when(!*in_group, |el| {
                        el.child(
                            div()
                                .flex_shrink_0()
                                .text_color(muted)
                                .child(SharedString::from(item.path.clone())),
                        )
                    })
                    .children(item.line.map(|line| {
                        div()
                            .flex_shrink_0()
                            .font_family(mono.clone())
                            .text_color(muted)
                            .child(format!(":{line}"))
                    }))
                    .when(!leaving, |el| {
                        el.on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(open.clone());
                        }))
                    })
                    .into_any_element()
            }
        }
    }
}

impl Focusable for Todos {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for Todos {
    fn panel_name(&self) -> &'static str {
        "TODOs"
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

impl Panel for Todos {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("TODOs")
    }

    /// The header actions: the filter, and grouping.
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        Some(
            h_flex()
                .gap_0p5()
                .items_center()
                .child(Self::tool(
                    "todos-filter",
                    icons::SEARCH,
                    self.filter_open,
                    "Filter TODOs",
                    cx,
                    |this, window, cx| this.toggle_filter(window, cx),
                ))
                .child(Self::tool(
                    "todos-group",
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

impl ToolWindow for Todos {
    fn tab_slot(&self) -> Option<&TabSlot> {
        Some(&self.tab)
    }

    /// The count, amber: advisory, not an error state.
    fn badge(&self, _cx: &App) -> Option<Badge> {
        Badge::count(self.count(), BadgeTone::Advisory)
    }
}

impl Render for Todos {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let has_analyzed = self.project.read(cx).has_analyzed();
        let empty: Option<&'static str> = if self.rows.is_empty() {
            Some(if !has_analyzed {
                "Not analyzed yet."
            } else if self.items.is_empty() {
                "No TODOs."
            } else {
                "No TODOs match."
            })
        } else {
            None
        };
        let chips = self.render_chips(cx);
        let count = self.rows.len();
        v_flex()
            .id("todos")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .when(self.filter_open, |el| {
                el.child(
                    v_flex()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(border)
                        .child(Input::new(&self.filter).xsmall())
                        .children(chips),
                )
            })
            .when_some(empty, |el, text| {
                el.child(div().p_3().text_color(muted).child(text))
            })
            .when(empty.is_none(), |el| {
                el.child(
                    uniform_list(
                        "todos-rows",
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
    use brink_ir::Severity;

    fn todo(start: u32, message: &str) -> Diagnostic {
        Diagnostic {
            start,
            end: start + 8,
            severity: Severity::Info,
            code: TODO_CODE.to_owned(),
            message: message.to_owned(),
        }
    }

    const INK: &str =
        "TODO: top of file\n=== start ===\nHello.\nTODO(audio): mix this down\n= inner\nTODO\n";

    fn items() -> Vec<TodoItem> {
        let diags = vec![
            todo(0, "TODO: top of file"),
            todo(
                INK.find("TODO(audio)").unwrap() as u32,
                "TODO: (audio): mix this down",
            ),
            todo(INK.rfind("TODO").unwrap() as u32, "TODO"),
        ];
        let mut other = Vec::new();
        other.push(Diagnostic {
            start: 0,
            end: 1,
            severity: Severity::Error,
            code: "E001".to_owned(),
            message: "not a todo".to_owned(),
        });
        other.extend(diags);
        let path = "story.ink".to_owned();
        let files = [(path, other)];
        collect(files.iter().map(|(p, d)| (p, d)), |_| Some(INK.to_owned()))
    }

    #[test]
    fn tags_split_off_and_empty_parens_do_not() {
        assert_eq!(
            split_tag("(audio): mix this down"),
            (Some("audio".to_owned()), "mix this down".to_owned())
        );
        assert_eq!(
            split_tag("( art ) sketch"),
            (Some("art".to_owned()), "sketch".to_owned())
        );
        assert_eq!(split_tag("() nothing"), (None, "() nothing".to_owned()));
        assert_eq!(split_tag("plain"), (None, "plain".to_owned()));
        assert_eq!(note_text("TODO: x"), "x");
        assert_eq!(note_text("TODO"), "");
    }

    #[test]
    fn notes_come_with_line_container_and_tag() {
        let items = items();
        assert_eq!(items.len(), 3, "only E189s: {items:?}");
        assert_eq!(items[0].text, "top of file");
        assert_eq!(items[0].line, Some(1));
        assert_eq!(items[0].container, None);
        assert_eq!(items[1].tag.as_deref(), Some("audio"));
        assert_eq!(items[1].text, "mix this down");
        assert_eq!(items[1].line, Some(4));
        assert_eq!(items[1].container.as_deref(), Some("start"));
        assert_eq!(items[2].text, "");
        assert_eq!(items[2].container.as_deref(), Some("start.inner"));
        assert_eq!(items[2].key(), "story.ink:6");
    }

    #[test]
    fn grouping_heads_files_and_containers_and_flat_does_not() {
        let items = items();
        let rows = layout(&items, true);
        assert_eq!(
            rows,
            vec![
                Item::File {
                    path: "story.ink".to_owned(),
                    count: 3
                },
                Item::Row {
                    index: 0,
                    in_group: true
                },
                Item::Container("start".to_owned()),
                Item::Row {
                    index: 1,
                    in_group: true
                },
                Item::Container("start.inner".to_owned()),
                Item::Row {
                    index: 2,
                    in_group: true
                },
            ]
        );
        let flat = layout(&items, false);
        assert_eq!(flat.len(), 3);
        assert!(flat.iter().all(|r| matches!(
            r,
            Item::Row {
                in_group: false,
                ..
            }
        )));
    }

    #[test]
    fn the_filter_reads_text_file_and_container_and_tags_collect() {
        let items = items();
        assert!(matches_filter(&items[1], "MIX"));
        assert!(matches_filter(&items[1], "story"));
        assert!(matches_filter(&items[2], "inner"));
        assert!(!matches_filter(&items[0], "inner"));
        assert!(matches_filter(&items[0], "  "));
        assert_eq!(tags_of(&items), vec!["audio".to_owned()]);
    }
}
