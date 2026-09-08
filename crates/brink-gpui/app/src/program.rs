//! The Program Explorer: the compiled program, read four ways
//! (`docs/studio-shell-spec.md` §4, `ProgramView.tsx`).
//!
//! - **Structure** — globals, lists, externals, and the knot → stitch tree
//!   with size bars (bytecode as the track, lines as the fill).
//! - **Lines** — the compiled line tables, scoped as the compiler scopes
//!   them; a template line reads as prose with its slots named.
//! - **Disassembly** — every container, scope and anonymous `c-N` alike,
//!   and its name-resolved bytecode with provenance.
//! - **Size** — real on-disk bytes: sections, per-scope line tables, and
//!   per-knot bytecode.
//!
//! Compile-bound: the worker compiles on request (`QueryKind::Program`,
//! memoized), and the panel asks again after each analysis **only while it
//! is the shown tab** — otherwise it marks itself stale and asks when
//! shown. A row that knows its source opens it. The session overlay (the
//! executing instruction) and `stepi` wait on the debugger.

use std::collections::BTreeSet;
use std::ops::Range;

use brink_gpui_model::program::{Program, ProgramReport, ProgramStatus};
use brink_gpui_model::query::{QueryKind, QueryResult};
use brink_gpui_shell::tool_window::{TabSlot, ToolWindow};
use brink_ide::program_model::{KnotNodeJs, ProgramModel};
use brink_intl::{ContentJson, LinesJson, PartJson, ScopeJson};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle,
    Window, div, px, relative, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};

/// A row with provenance was clicked: open it.
#[derive(Debug, Clone)]
pub enum ProgramEvent {
    Navigate {
        path: String,
        span: Range<usize>,
    },
    /// The `.inkt` toolbar button: show the dump of this same compile.
    /// The panel raises it rather than opening the tab itself — a tab is
    /// the host's to open, as it is for a navigation.
    OpenCompiledOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Structure,
    Lines,
    Disasm,
    Size,
}

impl View {
    const ALL: [Self; 4] = [Self::Structure, Self::Lines, Self::Disasm, Self::Size];

    fn label(self) -> &'static str {
        match self {
            Self::Structure => "Structure",
            Self::Lines => "Lines",
            Self::Disasm => "Disasm",
            Self::Size => "Size",
        }
    }
}

/// Source provenance on a row, in the compiler's file keys.
/// A jump from one view of the program to another — the web studio's
/// cross-view targeting. A row that names something another view holds
/// says so, and clicking it takes you there and puts the row under a
/// highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Jump {
    /// Lines view, a scope's table, at one line index. `scope` is the
    /// scope's NAME (empty for the root), because that is what a disasm
    /// node and a size row both know; the scope id is looked up from it.
    Line { scope: String, index: u16 },
    /// Lines view, a whole scope's table.
    Scope { scope: String },
    /// Disasm view, one knot's bytecode, by knot path.
    Knot { path: String },
}

#[derive(Debug, Clone)]
struct Src {
    path: String,
    start: u32,
    end: u32,
}

/// One rendered row — every view flattens to these, so one uniform list
/// draws all four.
#[derive(Debug, Clone)]
enum Item {
    /// A collapsible section header (Globals, Lists, …).
    Section {
        key: String,
        title: SharedString,
        collapsed: bool,
    },
    /// `key  mid  val` — a global, an external, a fact.
    Pair {
        key: SharedString,
        mid: SharedString,
        val: SharedString,
        indent: usize,
    },
    /// Plain text, optionally muted.
    Text {
        text: SharedString,
        dim: bool,
        indent: usize,
    },
    /// A knot or stitch in the Structure tree.
    Knot {
        key: String,
        name: SharedString,
        stitch: bool,
        flags: SharedString,
        depth: usize,
        expanded: bool,
        /// Size bars, top-level knots only.
        bar: Option<Bar>,
    },
    /// An expandable row whose children follow it (a container, a scope).
    Group {
        key: String,
        label: SharedString,
        facts: SharedString,
        depth: usize,
        expanded: bool,
        /// The container the running story is inside. Marked on the
        /// GROUP as well as the instruction, because a story at rest is
        /// parked one past its container's last instruction — the row
        /// the position names does not exist — while "the story is in
        /// here" is true and useful at every yield point.
        current: bool,
    },
    /// One instruction.
    Instr {
        /// The container this instruction belongs to, so a running
        /// position can be matched against the row.
        container: u32,
        offset: u32,
        text: SharedString,
        src: Option<Src>,
        /// The line this instruction emits, when it emits one — the
        /// `emit_line #N` operand, paired with the scope it is in.
        line_ref: Option<Jump>,
    },
    /// One compiled line.
    Line {
        index: u16,
        text: SharedString,
        template: bool,
        src: Option<Src>,
    },
    /// A size row with a proportional bar.
    Size {
        label: SharedString,
        bytes: usize,
        ratio: f32,
        indent: usize,
        /// The view that holds what this row measures, if one does: a
        /// line-table row is a scope, a bytecode row is a knot. Section
        /// rows measure something no other view lists, and carry none.
        jump: Option<Jump>,
    },
    Error(SharedString),
}

#[derive(Debug, Clone)]
struct Bar {
    bytes_ratio: f32,
    lines_ratio: f32,
    label: SharedString,
    containers: SharedString,
}

pub struct ProgramExplorer {
    project: Entity<Project>,
    report: Option<ProgramReport>,
    view: View,
    /// Collapsed sections, by key.
    collapsed: BTreeSet<String>,
    /// Expanded tree rows, by key — everything starts folded.
    expanded: BTreeSet<String>,
    items: Vec<Item>,
    /// The row a jump landed on, by index into `items`. Cleared by any
    /// relayout that is not the jump's own, so it can never point at a
    /// row that has since become a different one.
    highlight: Option<usize>,
    scroll: UniformListScrollHandle,
    /// Whether the panel is the shown tab, from the dock's `set_active` and
    /// from being rendered. Tracked rather than asked: asking the tab group
    /// reads this entity back while it is being updated (a panic).
    shown: bool,
    /// An analysis landed while the panel was not shown.
    stale: bool,
    /// The instruction the running story is about to execute, as
    /// `(container_idx, offset)` — the Program Explorer's half of the
    /// debugger, keyed the way the model's rows are.
    executing: Option<(u32, usize)>,
    /// A story is running on a program older than this one (the web's
    /// `sessionDegraded`). Read from the Player, which is the only thing
    /// that knows a session exists: what it compiled from is gone from
    /// the mirror, so a checksum comparison would be comparing the
    /// current program with itself.
    session_degraded: bool,
    busy: bool,
    generation: u64,
    focus: FocusHandle,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProgramEvent> for ProgramExplorer {}
impl EventEmitter<PanelEvent> for ProgramExplorer {}

impl ProgramExplorer {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let on_project = cx.subscribe(&project, |this, _, event: &ProjectEvent, cx| {
            if matches!(event, ProjectEvent::Analyzed) {
                this.refresh_if_shown(cx);
            }
        });
        Self {
            project,
            report: None,
            view: View::Structure,
            collapsed: BTreeSet::new(),
            expanded: BTreeSet::new(),
            items: Vec::new(),
            highlight: None,
            scroll: UniformListScrollHandle::new(),
            shown: false,
            stale: true,
            executing: None,
            session_degraded: false,
            busy: false,
            generation: 0,
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_project],
        }
    }

    /// A click on a row with provenance opens it.
    fn on_src(
        src: &Src,
        cx: &mut Context<Self>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let src = src.clone();
        cx.listener(move |_, _: &ClickEvent, _, cx| {
            cx.emit(ProgramEvent::Navigate {
                path: src.path.clone(),
                span: src.start as usize..src.end as usize,
            });
        })
    }

    /// Ask again if shown; otherwise remember to.
    fn refresh_if_shown(&mut self, cx: &mut Context<Self>) {
        if self.shown {
            self.refresh(cx);
        } else {
            self.stale = true;
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.project.read(cx).has_analyzed() {
            self.stale = true;
            return;
        }
        self.stale = false;
        self.busy = true;
        self.generation += 1;
        let generation = self.generation;
        let query = self.project.read(cx).query(QueryKind::Program, cx);
        cx.spawn(async move |this, cx| {
            let result = query.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.busy = false;
                if let Ok(QueryResult::Program(report)) = result {
                    this.report = Some(*report);
                    this.relayout();
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            self.relayout();
            cx.notify();
        }
    }

    fn toggle_section(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        self.relayout();
        cx.notify();
    }

    fn toggle_expanded(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        self.relayout();
        cx.notify();
    }

    /// Watch the Player for a degraded session — a story still running on
    /// a program this panel no longer shows.
    pub fn watch_player(&mut self, player: &Entity<crate::player::Player>, cx: &mut Context<Self>) {
        let subscription = cx.observe(player, |this: &mut Self, player, cx| {
            let degraded = player.read(cx).is_stale();
            if this.session_degraded != degraded {
                this.session_degraded = degraded;
                cx.notify();
            }
            this.read_position(cx);
        });
        self._subscriptions.push(subscription);
    }

    /// The container the running story is inside, if any.
    fn executing_container(&self) -> Option<u32> {
        self.executing.map(|(container, _)| container)
    }

    /// Ask the play session where it is. Cheap (it advances nothing), and
    /// asked only when the Player moves — a story waiting for a choice is
    /// not going anywhere, so nothing polls.
    fn read_position(&mut self, cx: &mut Context<Self>) {
        let query = self
            .project
            .read(cx)
            .play(brink_gpui_model::play::PlayCommand::Snapshot, cx);
        cx.spawn(async move |this, cx| {
            let outcome = query.await;
            let _ = this.update(cx, |this, cx| {
                let next = outcome.ok().and_then(|o| o.state).and_then(|s| s.position);
                if this.executing != next {
                    this.executing = next;
                    // The marks are laid at layout time, so the rows have
                    // to be rebuilt for the new position to show.
                    this.relayout();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Take a row's cross-reference: switch to the view that holds it,
    /// open what has to be open, and put the row under a highlight.
    fn jump(&mut self, jump: &Jump, cx: &mut Context<Self>) {
        let Some(program) = self.program() else {
            return;
        };
        let landing = match jump {
            Jump::Line { scope, index } => {
                let Some(id) = scope_id_named(&program.lines, scope) else {
                    return;
                };
                self.view = View::Lines;
                self.expanded.insert(format!("lines:{id}"));
                Some(Landing::Line(*index))
            }
            Jump::Scope { scope } => {
                let Some(id) = scope_id_named(&program.lines, scope) else {
                    return;
                };
                self.view = View::Lines;
                let key = format!("lines:{id}");
                self.expanded.insert(key.clone());
                Some(Landing::Group(key))
            }
            Jump::Knot { path } => {
                self.view = View::Disasm;
                let key = format!("disasm:{path}");
                self.expanded.insert(key.clone());
                Some(Landing::Group(key))
            }
        };
        self.relayout();
        // After the relayout, which clears any previous highlight.
        self.highlight = landing.and_then(|landing| landing.find(&self.items));
        if let Some(ix) = self.highlight {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn program(&self) -> Option<&Program> {
        match &self.report {
            Some(ProgramReport {
                status: ProgramStatus::Ready(program),
                ..
            }) => Some(program),
            _ => None,
        }
    }

    // ── Layout ───────────────────────────────────────────────────────

    fn relayout(&mut self) {
        // The rows are about to be different ones; a highlight by index
        // would land on whatever now sits there.
        self.highlight = None;
        self.items = match &self.report {
            None => Vec::new(),
            Some(ProgramReport {
                status: ProgramStatus::NoEntry,
                ..
            }) => vec![Item::Text {
                text: "Nothing names the story's start — set [project] entry in brink.toml.".into(),
                dim: true,
                indent: 0,
            }],
            Some(ProgramReport {
                status: ProgramStatus::Errors(errors),
                ..
            }) => {
                let mut items = vec![Item::Text {
                    text: format!("No program: the story has {} error(s).", errors.len()).into(),
                    dim: true,
                    indent: 0,
                }];
                items.extend(errors.iter().map(|e| Item::Error(e.clone().into())));
                items
            }
            Some(ProgramReport {
                status: ProgramStatus::Ready(program),
                ..
            }) => match self.view {
                View::Structure => layout_structure(program, &self.collapsed, &self.expanded),
                View::Lines => layout_lines(&program.lines, &self.expanded),
                View::Disasm => {
                    layout_disasm(&program.model, &self.expanded, self.executing_container())
                }
                View::Size => layout_size(program, &self.collapsed),
            },
        };
    }

    // ── Rendering ────────────────────────────────────────────────────

    /// The bytecode-by-knot treemap, above the list in the Size view.
    ///
    /// A treemap rather than a row of bars (the web's Size view, #3339):
    /// with forty knots a bar chart says which is biggest and almost
    /// nothing about the shape of the whole, while area is comparable at
    /// a glance across the map. Laid out in a normalised 1×1 box and
    /// placed with relative lengths, so it fills whatever width the dock
    /// happens to give it without the panel measuring itself.
    fn render_treemap(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let program = self.program()?;
        let knots = &program.model.knots;
        let values: Vec<(String, f64)> = knots
            .iter()
            .map(|k| (k.path.clone(), f64::from(subtree_bytes(k))))
            .collect();
        let tiles = crate::treemap::squarify(&values, 0., 0., 1., 1.);
        if tiles.is_empty() {
            return None;
        }
        let bytes: std::collections::HashMap<&str, u32> = knots
            .iter()
            .map(|k| (k.path.as_str(), subtree_bytes(k)))
            .collect();
        let theme = cx.theme();
        let (primary, muted, border) = (theme.primary, theme.muted_foreground, theme.border);
        let blocks: Vec<AnyElement> = tiles
            .iter()
            .enumerate()
            .map(|(i, tile)| {
                let size = bytes.get(tile.key.as_str()).copied().unwrap_or(0) as usize;
                let share = f64::from(tile.w) * f64::from(tile.h);
                // Bigger blocks read stronger; the scale is the block's
                // own share of the map, so the colour says the same thing
                // the area does rather than a second, different thing.
                let fill = primary.opacity((0.22 + share * 1.6).min(0.85) as f32);
                let jump = Jump::Knot {
                    path: tile.key.clone(),
                };
                let label = tile.key.clone();
                // A label only where it fits: a name clipped to two
                // letters is noise, and the tooltip carries it anyway.
                let roomy = tile.w > 0.18 && tile.h > 0.16;
                div()
                    .id(("treemap-tile", i))
                    .absolute()
                    .left(relative(tile.x))
                    .top(relative(tile.y))
                    .w(relative(tile.w))
                    .h(relative(tile.h))
                    .p_0p5()
                    .child(
                        div()
                            .size_full()
                            .rounded_sm()
                            .bg(fill)
                            .border_1()
                            .border_color(border)
                            .overflow_hidden()
                            .px_1()
                            .when(roomy, |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.foreground)
                                        .truncate()
                                        .child(label.clone()),
                                )
                                .child(div().text_xs().text_color(muted).child(fmt_bytes(size)))
                            }),
                    )
                    .tooltip({
                        let text = format!("{} · {}", tile.key, fmt_bytes(size));
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
                        }
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.jump(&jump, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            v_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("Bytecode by knot ({})", knots.len())),
                )
                .child(div().relative().w_full().h(px(180.)).children(blocks))
                .into_any_element(),
        )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, warn, primary) = (theme.muted_foreground, theme.warning, theme.primary);
        let name: SharedString = self
            .report
            .as_ref()
            .and_then(|r| r.entry.clone())
            .map_or_else(
                || "program".into(),
                |entry| {
                    entry
                        .rsplit('/')
                        .next()
                        .unwrap_or(&entry)
                        .trim_end_matches(".ink")
                        .trim_end_matches(".brink")
                        .to_owned()
                        .into()
                },
            );
        let (checksum, counts): (SharedString, SharedString) = match self.program() {
            Some(program) => {
                let model = &program.model;
                let stitches: usize = model.knots.iter().map(|k| k.children.len()).sum();
                let containers: u32 = model
                    .knots
                    .iter()
                    .map(|k| {
                        k.container_count
                            + k.children.iter().map(|c| c.container_count).sum::<u32>()
                    })
                    .sum();
                let lines: usize = program.lines.scopes.iter().map(|s| s.lines.len()).sum();
                (
                    model.checksum.clone().into(),
                    format!(
                        "{} knots · {stitches} stitches · {containers} containers · {lines} lines",
                        model.knots.len()
                    )
                    .into(),
                )
            }
            None => ("".into(), "".into()),
        };
        let status: Option<(SharedString, gpui::Hsla)> = if self.busy {
            Some(("compiling…".into(), muted))
        } else if self.stale {
            Some(("stale".into(), warn))
        } else if self.session_degraded {
            // What is on screen is the CURRENT program; the story running
            // in the Player is not this one. Saying so is the difference
            // between reading a disassembly and reading the right one.
            Some(("the running story is on an older program".into(), warn))
        } else {
            None
        };
        let view = self.view;
        v_flex()
            .w_full()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_baseline()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                    .child(div().text_xs().text_color(muted).child(checksum))
                    .when_some(status, |el, (text, color)| {
                        el.child(div().text_xs().text_color(color).child(text))
                    }),
            )
            .child(div().text_xs().text_color(muted).child(counts))
            .child(
                h_flex()
                    .w_full()
                    .gap_0p5()
                    .child(h_flex().gap_0p5().children(View::ALL.iter().map(|&v| {
                        let on = v == view;
                        Button::new(v.label())
                            .ghost()
                            .compact()
                            .toggled(on)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if on { primary } else { muted })
                                    .child(v.label()),
                            )
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_view(v, cx);
                            }))
                    })))
                    .child(div().flex_1())
                    // The dump is a fifth reading of this same compile, and
                    // it belongs in the editor rather than the dock — so the
                    // button opens a tab (the web's `.inkt` toolbar button,
                    // §4 "Program Explorer").
                    .child(
                        Button::new("open-inkt")
                            .ghost()
                            .compact()
                            .child(div().text_xs().text_color(muted).child(".inkt"))
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                cx.emit(ProgramEvent::OpenCompiledOutput);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.view != View::Structure {
            return None;
        }
        let program = self.program()?;
        let theme = cx.theme();
        let model = &program.model;
        let bytes: u32 = model.knots.iter().map(subtree_bytes).sum();
        let tables = program.lines.scopes.len();
        let lines: usize = program.lines.scopes.iter().map(|s| s.lines.len()).sum();
        let templates: usize = program
            .lines
            .scopes
            .iter()
            .flat_map(|s| &s.lines)
            .filter(|l| matches!(l.content, Some(ContentJson::Template { .. })))
            .count();
        let mut facts = vec![format!("{} bytecode", fmt_bytes(bytes as usize))];
        if tables > 0 {
            facts.push(format!("{lines} lines in {tables} tables"));
        }
        if templates > 0 {
            facts.push(format!("{templates} templates"));
        }
        facts.push(format!("{} externals", model.externals.len()));
        Some(
            div()
                .w_full()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(facts.join(" · "))
                .into_any_element(),
        )
    }

    #[expect(clippy::too_many_lines, reason = "one match arm per row kind")]
    fn render_item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, hover, primary, danger, track) = (
            theme.foreground,
            theme.muted_foreground,
            theme.muted.opacity(0.5),
            theme.primary,
            theme.danger,
            theme.muted,
        );
        let Some(item) = self.items.get(ix) else {
            return div().into_any_element();
        };
        // The row a jump landed on keeps a wash of the accent until the
        // next relayout — a scroll alone leaves you hunting for which row
        // of forty you were sent to.
        let landed = self.highlight == Some(ix);
        let row = move |id: usize| {
            h_flex()
                .id(("program-row", id))
                .w_full()
                .h(px(22.))
                .px_2()
                .gap_2()
                .items_center()
                .rounded_sm()
                .text_xs()
                .when(landed, |el| el.bg(primary.opacity(0.18)))
        };
        let chevron = |open: bool| {
            div()
                .w(px(10.))
                .text_color(muted)
                .child(if open { "\u{25BE}" } else { "\u{25B8}" })
        };
        match item {
            Item::Section {
                key,
                title,
                collapsed,
            } => {
                let key = key.clone();
                row(ix)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(chevron(!collapsed))
                    .child(div().text_color(fg).child(title.clone()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_section(&key, cx);
                    }))
                    .into_any_element()
            }
            Item::Pair {
                key,
                mid,
                val,
                indent,
            } => row(ix)
                .pl(px(8. + *indent as f32 * 12.))
                .child(div().text_color(fg).child(key.clone()))
                .child(div().text_color(muted).child(mid.clone()))
                .child(div().flex_1().text_color(fg).truncate().child(val.clone()))
                .into_any_element(),
            Item::Text { text, dim, indent } => row(ix)
                .pl(px(8. + *indent as f32 * 12.))
                .child(
                    div()
                        .text_color(if *dim { muted } else { fg })
                        .truncate()
                        .child(text.clone()),
                )
                .into_any_element(),
            Item::Error(text) => row(ix)
                .child(div().text_color(danger).truncate().child(text.clone()))
                .into_any_element(),
            Item::Knot {
                key,
                name,
                stitch,
                flags,
                depth,
                expanded,
                bar,
            } => {
                let key = key.clone();
                let mut el = row(ix)
                    .pl(px(8. + *depth as f32 * 12.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(chevron(*expanded))
                    .child(
                        div()
                            .text_color(if *stitch { muted } else { fg })
                            .child(name.clone()),
                    )
                    .when(!flags.is_empty(), |el| {
                        el.child(div().text_color(muted).child(flags.clone()))
                    });
                if let Some(bar) = bar {
                    el = el
                        .child(div().flex_1())
                        .child(
                            div().w(px(64.)).h(px(6.)).rounded_sm().bg(track).child(
                                div()
                                    .w(relative(bar.bytes_ratio))
                                    .h_full()
                                    .rounded_sm()
                                    .bg(muted)
                                    .child(
                                        div()
                                            .w(relative(bar.lines_ratio))
                                            .h_full()
                                            .rounded_sm()
                                            .bg(primary),
                                    ),
                            ),
                        )
                        .child(div().text_color(muted).child(bar.label.clone()))
                        .child(div().text_color(muted).child(bar.containers.clone()));
                }
                el.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_expanded(&key, cx);
                }))
                .into_any_element()
            }
            Item::Group {
                key,
                label,
                facts,
                depth,
                expanded,
                current,
            } => {
                let key = key.clone();
                row(ix)
                    .pl(px(8. + *depth as f32 * 12.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .when(*current, |el| el.bg(primary.opacity(0.14)))
                    .child(chevron(*expanded))
                    .child(
                        div()
                            .text_color(if *current { primary } else { fg })
                            .child(label.clone()),
                    )
                    .child(div().text_color(muted).child(facts.clone()))
                    // "The story is in here" — true at every yield point,
                    // unlike the instruction marker, which names a
                    // position one past the container's last row while a
                    // story waits.
                    .when(*current, |el| {
                        el.child(div().text_color(primary).child("\u{25B8} here"))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_expanded(&key, cx);
                    }))
                    .into_any_element()
            }
            Item::Instr {
                container,
                offset,
                text,
                src,
                line_ref,
            } => {
                // The instruction the running story is about to execute
                // (D9/#3187): a caret in the offset column and the row in
                // the accent. Nothing marks it when no story is running,
                // which is most of the time.
                let executing = self.executing == Some((*container, *offset as usize));
                let el = row(ix)
                    .when(executing, |el| el.bg(primary.opacity(0.18)))
                    .pl(px(28.))
                    .font_family("monospace")
                    .child(
                        div()
                            .w(px(40.))
                            .text_color(if executing { primary } else { muted })
                            .child(if executing {
                                format!("\u{25B8}{offset:03x}")
                            } else {
                                format!("{offset:04x}")
                            }),
                    )
                    .child(div().flex_1().truncate().child(opcode_spans(text, fg, cx)))
                    // The chip is its own click target, so the row keeps
                    // meaning "open the source" and the cross-reference
                    // does not have to fight it for the same gesture.
                    .children(line_ref.clone().map(|jump| {
                        let label = match &jump {
                            Jump::Line { index, .. } => format!("line #{index}"),
                            _ => String::new(),
                        };
                        Button::new(("instr-line", ix))
                            .ghost()
                            .xsmall()
                            .label(label)
                            .tooltip("Show this line in Lines")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.jump(&jump, cx);
                            }))
                    }))
                    .when(src.is_some(), |el| {
                        el.child(div().text_color(muted).child("\u{203A}"))
                    });
                match src {
                    Some(src) => el
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(Self::on_src(src, cx))
                        .into_any_element(),
                    None => el.into_any_element(),
                }
            }
            Item::Line {
                index,
                text,
                template,
                src,
            } => {
                let el = row(ix)
                    .pl(px(28.))
                    .child(
                        div()
                            .w(px(28.))
                            .text_color(muted)
                            .child(format!("#{index}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(if *template { primary } else { fg })
                            .truncate()
                            .child(text.clone()),
                    )
                    .when(src.is_some(), |el| {
                        el.child(div().text_color(muted).child("\u{203A}"))
                    });
                match src {
                    Some(src) => el
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(Self::on_src(src, cx))
                        .into_any_element(),
                    None => el.into_any_element(),
                }
            }
            Item::Size {
                label,
                bytes,
                ratio,
                indent,
                jump,
            } => {
                let el = row(ix)
                    .pl(px(8. + *indent as f32 * 12.))
                    .child(
                        div()
                            .w(px(120.))
                            .text_color(fg)
                            .truncate()
                            .child(label.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(6.))
                            .rounded_sm()
                            .bg(track)
                            .child(div().w(relative(*ratio)).h_full().rounded_sm().bg(primary)),
                    )
                    .child(div().w(px(64.)).text_color(muted).child(fmt_bytes(*bytes)));
                match jump {
                    Some(jump) => {
                        let jump = jump.clone();
                        el.cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .child(div().text_color(muted).child("\u{203A}"))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.jump(&jump, cx);
                            }))
                            .into_any_element()
                    }
                    None => el.into_any_element(),
                }
            }
        }
    }
}

// ── Layouts ──────────────────────────────────────────────────────────

fn subtree_bytes(node: &KnotNodeJs) -> u32 {
    node.byte_size + node.children.iter().map(subtree_bytes).sum::<u32>()
}

fn scope_lines(lines: &LinesJson, path: &str) -> usize {
    lines
        .scopes
        .iter()
        .find(|s| s.name.as_deref() == Some(path))
        .map_or(0, |s| s.lines.len())
}

fn subtree_lines(node: &KnotNodeJs, lines: &LinesJson) -> usize {
    scope_lines(lines, &node.path)
        + node
            .children
            .iter()
            .map(|c| subtree_lines(c, lines))
            .sum::<usize>()
}

fn fmt_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{:.1} KB", n as f64 / 1024.)
    }
}

fn section(key: &str, title: String, collapsed: &BTreeSet<String>) -> (Item, bool) {
    let is_collapsed = collapsed.contains(key);
    (
        Item::Section {
            key: key.to_owned(),
            title: title.into(),
            collapsed: is_collapsed,
        },
        is_collapsed,
    )
}

fn layout_structure(
    program: &Program,
    collapsed: &BTreeSet<String>,
    expanded: &BTreeSet<String>,
) -> Vec<Item> {
    let model = &program.model;
    let mut items = Vec::new();

    let (header, folded) = section(
        "globals",
        format!("Globals ({})", model.globals.len()),
        collapsed,
    );
    items.push(header);
    if !folded {
        if model.globals.is_empty() {
            items.push(Item::Text {
                text: "none".into(),
                dim: true,
                indent: 1,
            });
        }
        for g in &model.globals {
            items.push(Item::Pair {
                key: g.name.clone().into(),
                mid: g.ty.clone().into(),
                val: g.default.clone().into(),
                indent: 1,
            });
        }
    }

    if !model.lists.is_empty() {
        let (header, folded) =
            section("lists", format!("Lists ({})", model.lists.len()), collapsed);
        items.push(header);
        if !folded {
            for l in &model.lists {
                let names: Vec<String> = l
                    .items
                    .iter()
                    .map(|it| format!("{}·{}", it.name, it.ordinal))
                    .collect();
                items.push(Item::Pair {
                    key: l.name.clone().into(),
                    mid: "".into(),
                    val: names.join("  ").into(),
                    indent: 1,
                });
            }
        }
    }

    if !model.externals.is_empty() {
        let (header, folded) = section(
            "externals",
            format!("Externals ({})", model.externals.len()),
            collapsed,
        );
        items.push(header);
        if !folded {
            for e in &model.externals {
                let args = if e.arg_count == 1 {
                    "1 arg".to_owned()
                } else {
                    format!("{} args", e.arg_count)
                };
                items.push(Item::Pair {
                    key: e.name.clone().into(),
                    mid: args.into(),
                    val: if e.fallback.is_some() {
                        "fallback"
                    } else {
                        "host"
                    }
                    .into(),
                    indent: 1,
                });
            }
        }
    }

    let (header, folded) = section("knots", format!("Knots ({})", model.knots.len()), collapsed);
    items.push(header);
    if !folded {
        let max_bytes = model
            .knots
            .iter()
            .map(subtree_bytes)
            .max()
            .unwrap_or(1)
            .max(1);
        let max_lines = model
            .knots
            .iter()
            .map(|k| subtree_lines(k, &program.lines))
            .max()
            .unwrap_or(1)
            .max(1);
        if model.knots.is_empty() {
            items.push(Item::Text {
                text: "none".into(),
                dim: true,
                indent: 1,
            });
        }
        for knot in &model.knots {
            push_knot(
                &mut items,
                knot,
                1,
                program,
                expanded,
                (max_bytes, max_lines),
            );
        }
    }
    items
}

fn push_knot(
    items: &mut Vec<Item>,
    node: &KnotNodeJs,
    depth: usize,
    program: &Program,
    expanded: &BTreeSet<String>,
    (max_bytes, max_lines): (u32, usize),
) {
    let key = format!("knot:{}", node.path);
    let is_expanded = expanded.contains(&key);
    let bar = (depth == 1).then(|| {
        let bytes = subtree_bytes(node);
        let lines = subtree_lines(node, &program.lines);
        let containers =
            node.container_count + node.children.iter().map(|c| c.container_count).sum::<u32>();
        let mut label = fmt_bytes(bytes as usize);
        if lines > 0 {
            label.push_str(&format!(" · {lines} lines"));
        }
        Bar {
            bytes_ratio: bytes as f32 / max_bytes as f32,
            lines_ratio: lines as f32 / max_lines as f32,
            label: label.into(),
            containers: format!("{containers} cont.").into(),
        }
    });
    items.push(Item::Knot {
        key,
        name: node.name.clone().into(),
        stitch: node.kind == "stitch",
        flags: node.flags.join(" ").into(),
        depth,
        expanded: is_expanded,
        bar,
    });
    if !is_expanded {
        return;
    }
    items.push(Item::Pair {
        key: "path".into(),
        mid: node.path.clone().into(),
        val: if node.path_hash == 0 {
            String::new()
        } else {
            format!("hash {}", node.path_hash)
        }
        .into(),
        indent: depth + 1,
    });
    for child in &node.children {
        push_knot(
            items,
            child,
            depth + 1,
            program,
            expanded,
            (max_bytes, max_lines),
        );
    }
}

fn layout_disasm(
    model: &ProgramModel,
    expanded: &BTreeSet<String>,
    executing: Option<u32>,
) -> Vec<Item> {
    fn push(
        items: &mut Vec<Item>,
        node: &KnotNodeJs,
        depth: usize,
        expanded: &BTreeSet<String>,
        executing: Option<u32>,
    ) {
        let key = format!("disasm:{}", node.path);
        let is_expanded = expanded.contains(&key);
        items.push(Item::Group {
            key,
            label: if depth == 0 {
                node.path.clone()
            } else {
                format!("= {}", node.name)
            }
            .into(),
            facts: format!(
                "{} instr · {}",
                node.disasm.len(),
                fmt_bytes(node.byte_size as usize)
            )
            .into(),
            depth,
            expanded: is_expanded,
            current: executing == Some(node.container_idx),
        });
        if is_expanded {
            push_instrs(items, &node.disasm, &node.path, node.container_idx);
        }
        for anon in &node.anon {
            let key = format!("disasm:{}.{}", node.path, anon.label);
            let is_expanded = expanded.contains(&key);
            items.push(Item::Group {
                key,
                label: anon.label.clone().into(),
                facts: format!(
                    "{} instr · {}",
                    anon.disasm.len(),
                    fmt_bytes(anon.byte_size as usize)
                )
                .into(),
                depth: depth + 1,
                expanded: is_expanded,
                current: executing == Some(anon.container_idx),
            });
            if is_expanded {
                // An anonymous container's lines belong to the scope that
                // owns it, which is the knot this container hangs under.
                push_instrs(items, &anon.disasm, &node.path, anon.container_idx);
            }
        }
        for child in &node.children {
            push(items, child, depth + 1, expanded, executing);
        }
    }
    let mut items = Vec::new();
    if model.knots.is_empty() {
        items.push(Item::Text {
            text: "no containers".into(),
            dim: true,
            indent: 0,
        });
    }
    for knot in &model.knots {
        push(&mut items, knot, 0, expanded, executing);
    }
    items
}

/// What a jump is looking for once the rows have been rebuilt.
enum Landing {
    Group(String),
    Line(u16),
}

impl Landing {
    /// The row index, or `None` when the rebuilt rows do not hold it —
    /// which is not an error: a scope with no lines has no row to land on.
    fn find(&self, items: &[Item]) -> Option<usize> {
        items.iter().position(|item| match (self, item) {
            (Self::Group(want), Item::Group { key, .. }) => key == want,
            (Self::Line(want), Item::Line { index, .. }) => index == want,
            _ => false,
        })
    }
}

/// The id of the scope with this name — empty meaning the root scope,
/// which is the one with no name at all.
fn scope_id_named(lines: &LinesJson, name: &str) -> Option<String> {
    lines
        .scopes
        .iter()
        .find(|s| match &s.name {
            Some(n) => n == name,
            None => name.is_empty(),
        })
        .map(|s| s.id.clone())
}

fn push_instrs(
    items: &mut Vec<Item>,
    disasm: &[brink_ide::program_model::DisasmLineJs],
    scope: &str,
    container: u32,
) {
    for line in disasm {
        items.push(Item::Instr {
            container,
            offset: line.offset,
            text: line.text.clone().into(),
            src: line.src.as_ref().map(|s| Src {
                path: s.file.clone(),
                start: s.start,
                end: s.end,
            }),
            line_ref: emitted_line(&line.text).map(|index| Jump::Line {
                scope: scope.to_owned(),
                index,
            }),
        });
    }
}

/// One instruction's text, coloured by the same lexer Compiled Output's
/// `.inkt` tab uses (`inkt_highlight::lex_opcode`) and the same theme
/// tokens, so an opcode reads the same in both places. Whitespace and
/// anything the lexer does not claim are drawn in `fg`.
fn opcode_spans(text: &str, fg: gpui::Hsla, cx: &App) -> AnyElement {
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let mut row = h_flex().whitespace_nowrap();
    let mut at = 0usize;
    for (range, key) in crate::inkt_highlight::lex_opcode(text) {
        if range.start > at
            && let Some(gap) = text.get(at..range.start)
        {
            row = row.child(div().text_color(fg).child(gap.to_owned()));
        }
        let Some(piece) = text.get(range.clone()) else {
            continue;
        };
        let colour = brink_gpui_shell::theme::syntax_colour(key, &tokens).unwrap_or(fg);
        row = row.child(div().text_color(colour).child(piece.to_owned()));
        at = range.end;
    }
    if let Some(tail) = text.get(at..)
        && !tail.is_empty()
    {
        row = row.child(div().text_color(fg).child(tail.to_owned()));
    }
    row.into_any_element()
}

/// The line index an `emit_line #N …` instruction emits.
///
/// Read off the rendered text rather than the opcode: the panel is handed
/// `DisasmLineJs`, which is already formatted, and the format is
/// `brink-ide`'s own (`format_opcode`). Only the two emit forms name a
/// line; every other operand `#N` would be a different table.
fn emitted_line(text: &str) -> Option<u16> {
    let rest = text
        .strip_prefix("emit_line_nl ")
        .or_else(|| text.strip_prefix("emit_line "))?;
    rest.strip_prefix('#')?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Root first, knots in appearance order, stitches under their knot.
fn ordered_scopes(lines: &LinesJson) -> Vec<&ScopeJson> {
    let mut out: Vec<&ScopeJson> = lines.scopes.iter().filter(|s| s.name.is_none()).collect();
    let knots = lines
        .scopes
        .iter()
        .filter(|s| s.name.as_ref().is_some_and(|n| !n.contains('.')));
    let mut placed = BTreeSet::new();
    for knot in knots {
        out.push(knot);
        let name = knot.name.as_deref().unwrap_or_default();
        for stitch in lines.scopes.iter().filter(|s| {
            s.name
                .as_deref()
                .is_some_and(|n| n.split('.').next() == Some(name) && n.contains('.'))
        }) {
            out.push(stitch);
            placed.insert(stitch.id.clone());
        }
    }
    // A stitch whose knot scope never appeared still gets listed.
    for scope in &lines.scopes {
        if scope.name.as_ref().is_some_and(|n| n.contains('.')) && !placed.contains(&scope.id) {
            out.push(scope);
        }
    }
    out
}

/// A line's text with its template structure spelled inline: `{slot}`
/// for a slot, `[…|default]` for a select, `<name>…</name>` for a span.
fn line_text(line: &brink_intl::LineJson) -> (String, bool) {
    fn part(out: &mut String, p: &PartJson, line: &brink_intl::LineJson) {
        match p {
            PartJson::Literal(s) => out.push_str(s),
            PartJson::Slot { slot } => {
                let name = line
                    .slots
                    .iter()
                    .find(|s| s.index == *slot)
                    .map_or_else(|| slot.to_string(), |s| s.name.clone());
                out.push('{');
                out.push_str(&name);
                out.push('}');
            }
            PartJson::Select { select } => {
                let keys: Vec<String> = select
                    .variants
                    .iter()
                    .flat_map(|v| v.keys().cloned())
                    .collect();
                out.push('[');
                out.push_str(&keys.join("|"));
                out.push('|');
                out.push_str(&select.default);
                out.push(']');
            }
            PartJson::Span { span } => {
                out.push('<');
                out.push_str(&span.name);
                out.push('>');
                for child in &span.children {
                    part(out, child, line);
                }
                out.push_str("</");
                out.push_str(&span.name);
                out.push('>');
            }
        }
    }
    match &line.content {
        None => (String::new(), false),
        Some(ContentJson::Plain(s)) => (s.trim_end().to_owned(), false),
        Some(ContentJson::Template { template }) => {
            let mut out = String::new();
            for p in template {
                part(&mut out, p, line);
            }
            (out.trim_end().to_owned(), true)
        }
    }
}

fn layout_lines(lines: &LinesJson, expanded: &BTreeSet<String>) -> Vec<Item> {
    let mut items = Vec::new();
    if lines.scopes.is_empty() {
        items.push(Item::Text {
            text: "no line tables".into(),
            dim: true,
            indent: 0,
        });
    }
    for scope in ordered_scopes(lines) {
        let label = scope.name.clone().unwrap_or_else(|| "(root)".to_owned());
        let key = format!("lines:{}", scope.id);
        let is_expanded = expanded.contains(&key);
        let templates = scope
            .lines
            .iter()
            .filter(|l| matches!(l.content, Some(ContentJson::Template { .. })))
            .count();
        let mut facts = format!("{} lines", scope.lines.len());
        if templates > 0 {
            facts.push_str(&format!(" · {templates} templates"));
        }
        let depth = usize::from(label.contains('.'));
        items.push(Item::Group {
            key,
            label: label.into(),
            facts: facts.into(),
            depth,
            expanded: is_expanded,
            current: false,
        });
        if !is_expanded {
            continue;
        }
        for line in &scope.lines {
            let (text, template) = line_text(line);
            items.push(Item::Line {
                index: line.index,
                text: text.into(),
                template,
                src: line.source.as_ref().map(|s| Src {
                    path: s.file.clone(),
                    start: s.range_start,
                    end: s.range_end,
                }),
            });
        }
    }
    items
}

fn layout_size(program: &Program, collapsed: &BTreeSet<String>) -> Vec<Item> {
    let size = &program.size;
    let total = size.total.max(1);
    let mut items = vec![
        Item::Pair {
            key: "total".into(),
            mid: fmt_bytes(size.total).into(),
            val: "".into(),
            indent: 0,
        },
        Item::Pair {
            key: "shipping".into(),
            mid: fmt_bytes(size.shipping).into(),
            val: "without DebugInfo — what an export writes".into(),
            indent: 0,
        },
        Item::Pair {
            key: "debug".into(),
            mid: fmt_bytes(size.debug).into(),
            val: "".into(),
            indent: 0,
        },
        Item::Pair {
            key: "header".into(),
            mid: fmt_bytes(size.header).into(),
            val: "".into(),
            indent: 0,
        },
    ];

    let (header, folded) = section(
        "sections",
        format!("Sections ({})", size.sections.len()),
        collapsed,
    );
    items.push(header);
    if !folded {
        for s in &size.sections {
            items.push(Item::Size {
                label: s.kind.clone().into(),
                bytes: s.bytes,
                ratio: s.bytes as f32 / total as f32,
                indent: 1,
                // A section is a region of the file, not a thing another
                // view lists.
                jump: None,
            });
        }
    }

    let (header, folded) = section(
        "line-scopes",
        format!("Line tables ({})", size.line_scopes.len()),
        collapsed,
    );
    items.push(header);
    if !folded {
        let max = size
            .line_scopes
            .iter()
            .map(|s| s.bytes)
            .max()
            .unwrap_or(1)
            .max(1);
        for s in &size.line_scopes {
            items.push(Item::Size {
                label: s.name.clone().unwrap_or_else(|| "(root)".to_owned()).into(),
                bytes: s.bytes,
                ratio: s.bytes as f32 / max as f32,
                indent: 1,
                jump: Some(Jump::Scope {
                    scope: s.name.clone().unwrap_or_default(),
                }),
            });
        }
    }

    // "Bytecode by knot" is the treemap, drawn above the list by
    // `render_treemap` — a uniform list gives every row the same height,
    // and a map is a block, not a row. Nothing for it is emitted here.
    items
}

// ── Panel plumbing ───────────────────────────────────────────────────

impl Focusable for ProgramExplorer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for ProgramExplorer {
    fn panel_name(&self) -> &'static str {
        "Program"
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.shown = active;
        if active && self.stale {
            self.refresh(cx);
        }
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
        self.shown = false;
        self.tab.removed();
    }
}

impl Panel for ProgramExplorer {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Program")
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl ToolWindow for ProgramExplorer {
    fn tab_slot(&self) -> Option<&TabSlot> {
        Some(&self.tab)
    }
}

impl Render for ProgramExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being rendered is being shown — the dock renders only the active
        // tab of an open dock. First shown before any analysis landed: ask now.
        self.shown = true;
        if self.stale && !self.busy {
            self.refresh(cx);
        }
        let muted = cx.theme().muted_foreground;
        let header = self.render_header(cx);
        // The Size view's map goes between the header and the rows: the
        // sections and line tables stay rows, the knots become a map.
        let treemap = (self.view == View::Size)
            .then(|| self.render_treemap(cx))
            .flatten();
        let footer = self.render_footer(cx);
        let count = self.items.len();
        let empty: Option<SharedString> = if self.report.is_none() {
            Some(if self.project.read(cx).has_analyzed() {
                "Compiling…".into()
            } else {
                "Not analyzed yet.".into()
            })
        } else {
            None
        };
        v_flex()
            .id("program")
            .track_focus(&self.focus)
            .key_context(brink_gpui_shell::tool_window::TOOL_WINDOW_CONTEXT)
            .size_full()
            .text_xs()
            .child(header)
            .children(treemap)
            .when_some(empty, |el, text| {
                el.child(div().p_3().text_color(muted).child(text))
            })
            .when(count > 0, |el| {
                el.child(
                    uniform_list(
                        "program-rows",
                        count,
                        cx.processor(|this, range: Range<usize>, _window, cx| {
                            range.map(|i| this.render_item(i, cx)).collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .p_1()
                    .flex_1(),
                )
            })
            .children(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_format_like_the_web_view() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
    }

    #[test]
    fn only_the_emit_forms_name_a_line() {
        assert_eq!(emitted_line("emit_line #3 0"), Some(3));
        assert_eq!(emitted_line("emit_line_nl #12 1"), Some(12));
        // Another opcode's `#N` indexes a different table, and following
        // it would take the author to an unrelated line.
        assert_eq!(emitted_line("push_const #3"), None);
        assert_eq!(emitted_line("goto $01_abc"), None);
        assert_eq!(emitted_line("emit_line"), None);
        assert_eq!(emitted_line("emit_line x"), None);
    }

    #[test]
    fn a_landing_finds_its_row_and_admits_when_there_is_none() {
        let items = vec![
            Item::Group {
                key: "lines:s1".to_owned(),
                label: SharedString::from("shore"),
                facts: SharedString::default(),
                depth: 0,
                expanded: true,
                current: false,
            },
            Item::Line {
                index: 0,
                text: SharedString::from("first"),
                template: false,
                src: None,
            },
            Item::Line {
                index: 4,
                text: SharedString::from("fifth"),
                template: false,
                src: None,
            },
        ];
        assert_eq!(Landing::Group("lines:s1".to_owned()).find(&items), Some(0));
        assert_eq!(Landing::Line(4).find(&items), Some(2));
        // A scope with no such line lands nowhere — and on no wrong row.
        assert_eq!(Landing::Line(9).find(&items), None);
        assert_eq!(Landing::Group("disasm:x".to_owned()).find(&items), None);
    }

    #[test]
    fn a_scope_is_found_by_name_with_the_root_as_the_empty_one() {
        let scope = |name: Option<&str>, id: &str| ScopeJson {
            name: name.map(str::to_owned),
            id: id.to_owned(),
            lines: Vec::new(),
        };
        let lines = LinesJson {
            version: 1,
            source_checksum: String::new(),
            scopes: vec![
                scope(None, "0"),
                scope(Some("shore"), "1"),
                scope(Some("shore.linger"), "2"),
            ],
        };
        assert_eq!(scope_id_named(&lines, ""), Some("0".to_owned()), "root");
        assert_eq!(scope_id_named(&lines, "shore.linger"), Some("2".to_owned()));
        assert_eq!(scope_id_named(&lines, "nowhere"), None);
    }

    #[test]
    fn scopes_order_root_then_knots_with_their_stitches() {
        let scope = |name: Option<&str>, id: &str| ScopeJson {
            name: name.map(str::to_owned),
            id: id.to_owned(),
            lines: Vec::new(),
        };
        let lines = LinesJson {
            version: 1,
            source_checksum: "0".to_owned(),
            scopes: vec![
                scope(Some("b.x"), "3"),
                scope(Some("a"), "1"),
                scope(None, "0"),
                scope(Some("a.y"), "2"),
                scope(Some("b"), "4"),
            ],
        };
        let ids: Vec<&str> = ordered_scopes(&lines)
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, ["0", "1", "2", "4", "3"]);
    }
}
