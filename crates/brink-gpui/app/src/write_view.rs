//! **Write mode's occupant** — the sidebar, the manuscript, and the Player
//! beside it (`docs/gpui-writing-scripting-modes.md` §3).
//!
//! **The Player.** In Script mode it is a centre tab. In Write mode it
//! slides in from the right and pushes the manuscript, so a passage can be
//! read and fixed in the same glance (W7, which supersedes the parked "swap,
//! not split" direction). It is the same `Player` entity either way: only
//! its home changes, and only one mode is on screen at a time, so it is
//! never drawn twice.
//!
//! **The sidebar** (W4–W6). Two columns, Inky-style: the project's files,
//! then the structure of the file the author is in — its knots and
//! stitches, its functions, its globals. Its own component for now, not the
//! Binder (W11): migrating it into Script mode, replacing the Binder, comes
//! afterwards. What it asks of the studio — a new knot, play from here, a
//! promote — goes out as the Binder's own events, so both surfaces run the
//! same flows.

use std::collections::{BTreeMap, BTreeSet};

use brink_gpui_model::query::{QueryKind, QueryResult, Symbol};
use brink_ir::SymbolKind;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ClickEvent, Context, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};

use crate::binder::{Binder, BinderEvent};
use crate::continuous::{ContinuousView, ManuscriptEvent};
use crate::player::Player;
use crate::project::{Project, ProjectEvent};
use brink_gpui_shell::icons;

/// The Player panel's width once it has slid in.
pub(crate) const PLAYER_WIDTH: f32 = 400.;

/// How long a slide takes — the Player's and the sidebar's, and the
/// title bar's strip above the sidebar, which must move with it.
use brink_gpui_shell::workspace::{SLIDE, SidebarStrip};

/// The sidebar's two columns. Files is a Binder, whose header carries more
/// tools than a plain list's would, hence the wider of the two.
const FILES_WIDTH: f32 = 240.;
const STRUCTURE_WIDTH: f32 = 240.;

/// A sidebar row's height, and its header's.
const ROW_HEIGHT: f32 = 24.;
const HEADER_HEIGHT: f32 = 30.;

/// What Write mode asks of the studio.
#[derive(Debug, Clone)]
pub(crate) enum WriteEvent {
    /// A structural operation, as the Binder would ask for it.
    Outline(BinderEvent),
}

/// Where the sidebar is: out, sliding away, or gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sidebar {
    Closed,
    Open,
    /// Still drawn while it slides out; `Closed` once the slide is over.
    Closing,
}

pub(crate) struct WriteView {
    project: Entity<Project>,
    manuscript: Entity<ContinuousView>,
    player: Entity<Player>,
    player_open: bool,
    /// Bumped on every opening, so the slide's animation id is new and it
    /// plays again rather than resuming at its end.
    openings: usize,
    sidebar: Sidebar,
    /// Bumped per slide, in or out, so each animates from its start.
    slide: usize,
    /// Column 1: a files-only Binder — every file interaction the Binder
    /// has, with its own expansion and selection. Its events are the
    /// studio's to handle, as the Binder's are.
    files: Entity<Binder>,
    /// The structure column (W5) — its toggle is in the Files header. Off
    /// until asked for: the files are what a writer reaches for first.
    structure_open: bool,
    /// Each file's outline, as last answered. Cleared on every analysis:
    /// an edit moves offsets, and a stale outline would put the caret in
    /// the wrong stitch.
    symbols: BTreeMap<String, Vec<Symbol>>,
    pending: BTreeSet<String>,
    /// The story's prose word count, as of the last analysis — for the
    /// bare page's chip (W9). Cached: counting walks every file's prose.
    words: usize,
    me: WeakEntity<Self>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WriteEvent> for WriteView {}

impl WriteView {
    pub(crate) fn new(
        project: Entity<Project>,
        manuscript: Entity<ContinuousView>,
        player: Entity<Player>,
        files: Entity<Binder>,
        cx: &mut Context<Self>,
    ) -> Self {
        let on_caret = cx.subscribe(&manuscript, |this, _, event: &ManuscriptEvent, cx| {
            let ManuscriptEvent::Caret { path, .. } = event;
            this.request_symbols(&path.clone(), cx);
            cx.notify();
        });
        let on_project = cx.subscribe(&project, |this, _, event: &ProjectEvent, cx| {
            if matches!(
                event,
                ProjectEvent::Analyzed | ProjectEvent::FilesChanged | ProjectEvent::Opened { .. }
            ) {
                this.symbols.clear();
                this.words = this.project.read(cx).prose_word_count();
                if let Some(path) = this.current_file(cx) {
                    this.request_symbols(&path, cx);
                }
                cx.notify();
            }
        });
        Self {
            project,
            manuscript,
            player,
            player_open: false,
            openings: 0,
            sidebar: Sidebar::Closed,
            slide: 0,
            files,
            structure_open: false,
            symbols: BTreeMap::new(),
            pending: BTreeSet::new(),
            words: 0,
            me: cx.weak_entity(),
            _subscriptions: vec![on_caret, on_project],
        }
    }

    #[must_use]
    pub(crate) fn is_player_open(&self) -> bool {
        self.player_open
    }

    /// Slide the Player in, or leave it where it is if it already is.
    pub(crate) fn open_player(&mut self, cx: &mut Context<Self>) {
        if !self.player_open {
            self.player_open = true;
            self.openings += 1;
            cx.notify();
        }
    }

    /// Put the Player away. The session keeps running; Play or the toggle
    /// brings it back where it was.
    pub(crate) fn close_player(&mut self, cx: &mut Context<Self>) {
        if self.player_open {
            self.player_open = false;
            cx.notify();
        }
    }

    /// Out, or on its way out — not while sliding away.
    #[must_use]
    pub(crate) fn is_sidebar_open(&self) -> bool {
        self.sidebar == Sidebar::Open
    }

    /// Slide the sidebar out, or away.
    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.slide += 1;
        if self.sidebar == Sidebar::Open {
            // Drawn until the slide is over, then gone. A toggle back in
            // meanwhile bumps `slide`, and this finish is then stale.
            self.sidebar = Sidebar::Closing;
            let slide = self.slide;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SLIDE).await;
                let _ = this.update(cx, |this, cx| {
                    if this.slide == slide && this.sidebar == Sidebar::Closing {
                        this.sidebar = Sidebar::Closed;
                        cx.notify();
                    }
                });
            })
            .detach();
        } else {
            self.sidebar = Sidebar::Open;
            if let Some(path) = self.current_file(cx) {
                self.request_symbols(&path, cx);
            }
        }
        cx.notify();
    }

    /// The chip's two numbers: prose words, and problems.
    #[must_use]
    pub(crate) fn counts(&self, cx: &App) -> (usize, usize) {
        (self.words, self.project.read(cx).problem_count())
    }

    #[must_use]
    pub(crate) fn is_structure_open(&self) -> bool {
        self.structure_open
    }

    pub(crate) fn toggle_structure(&mut self, cx: &mut Context<Self>) {
        self.structure_open = !self.structure_open;
        if self.structure_open
            && let Some(path) = self.current_file(cx)
        {
            self.request_symbols(&path, cx);
        }
        // The toggle is drawn in the Binder's header.
        self.files.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// The Files column's Binder.
    #[cfg(test)]
    pub(crate) fn files(&self) -> &Entity<Binder> {
        &self.files
    }

    /// The sidebar's full width.
    fn sidebar_width(&self) -> Pixels {
        px(FILES_WIDTH
            + if self.structure_open {
                STRUCTURE_WIDTH
            } else {
                0.
            })
    }

    /// What the title bar's strip should do: slide with the sidebar.
    #[must_use]
    pub(crate) fn sidebar_strip(&self) -> Option<SidebarStrip> {
        let opening = match self.sidebar {
            Sidebar::Closed => return None,
            Sidebar::Open => true,
            Sidebar::Closing => false,
        };
        Some(SidebarStrip {
            width: self.sidebar_width(),
            opening,
            slide: self.slide,
        })
    }

    /// The file the author is in — the manuscript's say.
    fn current_file(&self, cx: &App) -> Option<String> {
        self.manuscript.read(cx).current_file().map(str::to_owned)
    }

    /// `knot › stitch` at the caret, for the title bar.
    #[must_use]
    pub(crate) fn crumb(&self, cx: &App) -> Option<SharedString> {
        let (path, offset) = self.manuscript.read(cx).caret()?;
        let (knot, stitch) = at_caret(self.symbols.get(path)?, offset)?;
        Some(match stitch {
            Some(stitch) => format!("{} \u{203a} {}", knot.name, stitch.name).into(),
            None => knot.name.clone().into(),
        })
    }

    /// Ask the worker for a file's outline, unless it is held or asked for.
    fn request_symbols(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.symbols.contains_key(path) || !self.pending.insert(path.to_owned()) {
            return;
        }
        let query = self.project.read(cx).query(
            QueryKind::DocumentSymbols {
                path: path.to_owned(),
            },
            cx,
        );
        let path = path.to_owned();
        cx.spawn(async move |this, cx| {
            let answer = query.await;
            let _ = this.update(cx, |this, cx| {
                this.pending.remove(&path);
                if let Ok(QueryResult::DocumentSymbols(found)) = answer {
                    this.symbols.insert(path, found);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn reveal_at(&mut self, path: &str, offset: usize, cx: &mut Context<Self>) {
        self.manuscript
            .update(cx, |m, cx| m.reveal_span(path, offset..offset, cx));
    }
}

/// The knot (functions are knots) whose ownership range holds `offset`,
/// and the stitch in it that does.
fn at_caret(symbols: &[Symbol], offset: usize) -> Option<(&Symbol, Option<&Symbol>)> {
    let holds = |s: &&Symbol| (s.full_start as usize) <= offset && offset < s.full_end as usize;
    let knot = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Knot)
        .find(holds)?;
    Some((knot, knot.children.iter().find(holds)))
}

impl Focusable for WriteView {
    /// The manuscript's: switching to Write puts the caret in the text.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.manuscript.read(cx).focus_handle(cx)
    }
}

// ── The sidebar ─────────────────────────────────────────────────────

impl WriteView {
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.current_file(cx);
        let border = cx.theme().sidebar_border;
        let width = f32::from(self.sidebar_width());
        let opening = self.sidebar == Sidebar::Open;
        h_flex()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .child(
                div()
                    .w(px(FILES_WIDTH))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(border)
                    .child(self.files.clone()),
            )
            .when(self.structure_open, |el| {
                el.child(self.render_structure(current.as_deref(), cx))
            })
            // The slide: the width runs from nothing to full (or back), so
            // the manuscript is pushed, not covered; the columns keep their
            // own widths and are clipped meanwhile. The title bar's strip
            // runs the same animation (`SidebarStrip`).
            .with_animation(
                SharedString::from(format!("write-sidebar-{}", self.slide)),
                Animation::new(SLIDE).with_easing(gpui::ease_out_quint()),
                move |el, delta| el.w(px(width * if opening { delta } else { 1. - delta })),
            )
            .into_any_element()
    }

    /// Column 2: the current file's knots and stitches, functions and
    /// globals (W6).
    fn render_structure(&self, current: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (sidebar, border, muted) =
            (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
        let caret = self
            .manuscript
            .read(cx)
            .caret()
            .map(|(p, o)| (p.to_owned(), o));
        let symbols = current.and_then(|p| self.symbols.get(p));
        let path = current.unwrap_or_default().to_owned();
        let (here_knot, here_stitch) = match (&caret, symbols) {
            (Some((p, offset)), Some(symbols)) if *p == path => match at_caret(symbols, *offset) {
                Some((knot, stitch)) => (Some(knot.full_start), stitch.map(|s| s.full_start)),
                None => (None, None),
            },
            _ => (None, None),
        };

        let mut body: Vec<AnyElement> = Vec::new();
        if let Some(symbols) = symbols {
            let knots: Vec<&Symbol> = symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Knot && !s.is_function)
                .collect();
            let functions: Vec<&Symbol> = symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Knot && s.is_function)
                .collect();
            let globals: Vec<&Symbol> = symbols
                .iter()
                .filter(|s| {
                    matches!(
                        s.kind,
                        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::List
                    )
                })
                .collect();

            let new_knot = {
                let path = path.clone();
                let me = self.me.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let path = path.clone();
                    let _ = me.update(cx, |_, cx| {
                        cx.emit(WriteEvent::Outline(BinderEvent::NewKnot { path }));
                    });
                }
            };
            body.push(self.section_header("Knots", Some(Box::new(new_knot)), cx));
            for knot in knots {
                body.push(self.symbol_row(
                    &path,
                    knot,
                    None,
                    here_knot == Some(knot.full_start) && here_stitch.is_none(),
                    cx,
                ));
                for stitch in &knot.children {
                    body.push(self.symbol_row(
                        &path,
                        stitch,
                        Some(knot),
                        here_stitch == Some(stitch.full_start),
                        cx,
                    ));
                }
            }
            if !functions.is_empty() {
                body.push(self.section_header("Functions", None, cx));
                for function in functions {
                    body.push(self.symbol_row(
                        &path,
                        function,
                        None,
                        here_knot == Some(function.full_start),
                        cx,
                    ));
                }
            }
            if !globals.is_empty() {
                body.push(self.section_header("Globals", None, cx));
                for global in globals {
                    body.push(self.symbol_row(&path, global, None, false, cx));
                }
            }
        }

        v_flex()
            .w(px(STRUCTURE_WIDTH))
            .h_full()
            .flex_none()
            .bg(sidebar)
            .border_r_1()
            .border_color(border)
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .px_3()
                    .items_center()
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(file_name(&path)),
                    ),
            )
            .child(
                v_flex()
                    .id("write-structure")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_1()
                    .pb_2()
                    .children(body),
            )
            .into_any_element()
    }

    /// A section's title, with a `+` where the section has a creation flow.
    fn section_header(
        &self,
        title: &'static str,
        add: Option<OnClick>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .h(px(ROW_HEIGHT))
            .mt_2()
            .pl_2()
            .pr_1()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(muted)
                    .child(title.to_uppercase()),
            )
            .when_some(add, |el, add| {
                el.child(
                    Button::new(SharedString::from(format!("write-add-{title}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .tooltip(format!("New {}\u{2026}", title.trim_end_matches('s')))
                        .on_click(add),
                )
            })
            .into_any_element()
    }

    /// One knot, stitch, function or global. Click reveals it; hover shows
    /// `+` (a new stitch, on a knot) and the `⋯` menu.
    fn symbol_row(
        &self,
        path: &str,
        symbol: &Symbol,
        knot: Option<&Symbol>,
        here: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, accent) = (theme.foreground, theme.muted_foreground, theme.accent);
        let faint = muted.opacity(0.7);
        let group = SharedString::from(format!("write-row-{path}-{}", symbol.start));
        let (icon, indent): (AnyElement, Pixels) = match symbol.kind {
            SymbolKind::Stitch => (
                icons::icon(icons::BrinkIcon::Stitch, px(12.), muted).into_any_element(),
                px(22.),
            ),
            SymbolKind::Knot if symbol.is_function => (
                icons::icon(icons::BrinkIcon::Function, px(12.), muted).into_any_element(),
                px(8.),
            ),
            SymbolKind::Knot => (
                icons::icon(icons::BrinkIcon::Knot, px(12.), muted).into_any_element(),
                px(8.),
            ),
            // The studio's set has no glyph for a variable; the kit's
            // asterisk reads as "a declared value" well enough.
            _ => (
                gpui_component::Icon::new(IconName::Asterisk)
                    .with_size(px(11.))
                    .text_color(muted)
                    .into_any_element(),
                px(8.),
            ),
        };
        let is_knot = symbol.kind == SymbolKind::Knot && !symbol.is_function;
        let at = symbol.start as usize;
        let reveal_path = path.to_owned();

        // The `⋯` menu: what the Binder offers on the same row.
        let menu =
            {
                let me = self.me.clone();
                let path = path.to_owned();
                let name = symbol.name.clone();
                let kind = symbol.kind;
                let is_function = symbol.is_function;
                let full_end = symbol.full_end as usize;
                let knot_name = knot.map(|k| k.name.clone());
                let knot_end = knot.map(|k| k.full_end as usize);
                Button::new(SharedString::from(format!("write-more-{path}-{at}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu(move |menu, _, _| {
                        let emit = |event: WriteEvent| {
                            let me = me.clone();
                            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                let event = event.clone();
                                let _ = me.update(cx, |_, cx| cx.emit(event));
                            }
                        };
                        let reveal = {
                            let me = me.clone();
                            let path = path.clone();
                            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                let _ = me.update(cx, |this, cx| this.reveal_at(&path, at, cx));
                            }
                        };
                        let mut menu = menu.item(PopupMenuItem::new("Go to").on_click(reveal));
                        let play_path = match (kind, &knot_name) {
                            (SymbolKind::Stitch, Some(k)) => Some(format!("{k}.{name}")),
                            (SymbolKind::Knot, _) if !is_function => Some(name.clone()),
                            _ => None,
                        };
                        if let Some(play) = play_path {
                            menu = menu.item(PopupMenuItem::new("Play from here").on_click(emit(
                                WriteEvent::Outline(BinderEvent::Play { path: play }),
                            )));
                        }
                        match (kind, &knot_name) {
                            (SymbolKind::Knot, _) if !is_function => menu
                                .item(PopupMenuItem::new("New Stitch\u{2026}").on_click(emit(
                                    WriteEvent::Outline(BinderEvent::NewStitch {
                                        path: path.clone(),
                                        full_end,
                                    }),
                                )))
                                .separator()
                                .item(PopupMenuItem::new("Demote to Stitch\u{2026}").on_click(
                                    emit(WriteEvent::Outline(BinderEvent::Demote {
                                        path: path.clone(),
                                        knot: name.clone(),
                                    })),
                                )),
                            (SymbolKind::Stitch, Some(k)) => menu
                                .item(PopupMenuItem::new("New Stitch\u{2026}").on_click(emit(
                                    WriteEvent::Outline(BinderEvent::NewStitch {
                                        path: path.clone(),
                                        full_end: knot_end.unwrap_or(full_end),
                                    }),
                                )))
                                .separator()
                                .item(PopupMenuItem::new("Promote to Knot\u{2026}").on_click(
                                    emit(WriteEvent::Outline(BinderEvent::Promote {
                                        path: path.clone(),
                                        knot: k.clone(),
                                        stitch: name.clone(),
                                    })),
                                )),
                            _ => menu,
                        }
                    })
            };

        let new_stitch = is_knot.then(|| {
            let me = self.me.clone();
            let path = path.to_owned();
            let full_end = symbol.full_end as usize;
            Button::new(SharedString::from(format!("write-stitch-{path}-{at}")))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .tooltip("New Stitch\u{2026}")
                .on_click(move |_: &ClickEvent, _, cx| {
                    let path = path.clone();
                    let _ = me.update(cx, |_, cx| {
                        cx.emit(WriteEvent::Outline(BinderEvent::NewStitch {
                            path,
                            full_end,
                        }));
                    });
                })
        });

        h_flex()
            .id(group.clone())
            .group(group.clone())
            .h(px(ROW_HEIGHT))
            .pl(indent)
            .pr_1()
            .gap_2()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .when(here, |el| el.bg(accent))
            .hover(|s| s.bg(accent.opacity(0.6)))
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(fg)
                    .child(symbol.name.clone()),
            )
            .when_some(symbol.value.clone(), |el, value| {
                el.child(
                    div()
                        .max_w(px(90.))
                        .truncate()
                        .text_xs()
                        .text_color(faint)
                        .child(value),
                )
            })
            // Shown on hover only, so a resting outline is just names.
            .child(
                h_flex()
                    .invisible()
                    .group_hover(group, |s| s.visible())
                    .children(new_stitch)
                    .child(menu),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.reveal_at(&reveal_path, at, cx);
            }))
            .into_any_element()
    }
}

/// `1234567` → `1,234,567`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A click handler, boxed.
type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The last segment of a root-relative path.
fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

impl WriteView {
    /// The bare page's chip (W9): word count and problem count, bottom
    /// right, while the sidebar is closed. Clicking opens the sidebar,
    /// whose Files column carries each file's problems.
    fn render_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        let (words, problems) = self.counts(cx);
        let theme = cx.theme();
        let (bg, border, muted, fg, danger) = (
            theme.secondary,
            theme.border,
            theme.muted_foreground,
            theme.foreground,
            theme.danger,
        );
        h_flex()
            .id("write-chip")
            .absolute()
            .bottom_3()
            .right_4()
            .h(px(22.))
            .px_2()
            .gap_2()
            .items_center()
            .rounded_full()
            .border_1()
            .border_color(border)
            .bg(bg.opacity(0.9))
            .text_xs()
            .text_color(muted)
            .cursor_pointer()
            .hover(move |s| s.text_color(fg))
            .child(format!(
                "{} {}",
                grouped(words),
                if words == 1 { "word" } else { "words" }
            ))
            .when(problems > 0, |el| {
                el.child(div().text_color(danger).child(match problems {
                    1 => "1 problem".to_owned(),
                    n => format!("{n} problems"),
                }))
            })
            .tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new("Open the sidebar").build(window, cx)
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.toggle_sidebar(cx);
            }))
            .into_any_element()
    }
}

impl Render for WriteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = (self.sidebar != Sidebar::Closed).then(|| self.render_sidebar(cx));
        let chip = (self.sidebar == Sidebar::Closed).then(|| self.render_chip(cx));
        let theme = cx.theme();
        let (border, surface, muted) = (theme.border, theme.background, theme.muted_foreground);
        let panel = self.player_open.then(|| {
            v_flex()
                .h_full()
                .flex_none()
                .overflow_hidden()
                .border_l_1()
                .border_color(border)
                .bg(surface)
                .child(
                    h_flex()
                        .w(px(PLAYER_WIDTH))
                        .h(px(HEADER_HEIGHT))
                        .flex_none()
                        .px_2()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(border)
                        .child(div().text_xs().text_color(muted).child("Player"))
                        .child(
                            Button::new("write-player-close")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Close the Player")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.close_player(cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .w(px(PLAYER_WIDTH))
                        .flex_1()
                        .min_h_0()
                        .child(self.player.clone()),
                )
                // The slide: the panel's width grows from nothing, so the
                // manuscript beside it is pushed rather than covered. The
                // contents stay at full width and are clipped meanwhile, so
                // nothing re-wraps mid-slide.
                .with_animation(
                    SharedString::from(format!("write-player-{}", self.openings)),
                    Animation::new(SLIDE).with_easing(gpui::ease_out_quint()),
                    |panel, delta| panel.w(px(PLAYER_WIDTH * delta)),
                )
        });
        h_flex()
            .id("write-view")
            .size_full()
            .children(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    // The chip sits over the manuscript's bottom-right
                    // corner, so it stays beside the text when the Player
                    // is out.
                    .relative()
                    .child(self.manuscript.clone())
                    .children(chip),
            )
            .children(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, kind: SymbolKind, range: (u32, u32), children: Vec<Symbol>) -> Symbol {
        Symbol {
            name: name.to_owned(),
            kind,
            start: range.0,
            full_start: range.0,
            full_end: range.1,
            is_function: false,
            value: None,
            children,
        }
    }

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn the_caret_is_in_the_stitch_whose_range_holds_it() {
        let symbols = vec![
            symbol("gold", SymbolKind::Variable, (0, 10), vec![]),
            symbol(
                "start",
                SymbolKind::Knot,
                (10, 100),
                vec![
                    symbol("a", SymbolKind::Stitch, (30, 60), vec![]),
                    symbol("b", SymbolKind::Stitch, (60, 100), vec![]),
                ],
            ),
        ];
        let name = |found: Option<(&Symbol, Option<&Symbol>)>| {
            found.map(|(k, s)| (k.name.clone(), s.map(|s| s.name.clone())))
        };
        assert_eq!(name(at_caret(&symbols, 5)), None, "a global is not a place");
        assert_eq!(name(at_caret(&symbols, 20)), Some(("start".into(), None)));
        assert_eq!(
            name(at_caret(&symbols, 60)),
            Some(("start".into(), Some("b".into()))),
            "a boundary belongs to the stitch that starts there"
        );
        assert_eq!(name(at_caret(&symbols, 100)), None, "the end is exclusive");
    }
}
