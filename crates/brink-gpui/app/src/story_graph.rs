//! The Story Graph — knots and stitches as boxes, diverts and choices as
//! lines (`docs/studio-shell-spec.md` §4's last session document).
//!
//! **Edges are painted; nodes are elements.** A `canvas` under the nodes
//! draws every line with `paint_path`, and the nodes themselves are
//! ordinary absolutely-positioned divs on top — so they carry text the
//! text system shaped, they hover, and they can be clicked. Painting the
//! boxes too would mean laying out text by hand for no gain.
//!
//! **Pan by dragging the background; zoom with the buttons.** No inertia,
//! no minimap: what an author needs from this is "show me what leads
//! where", and a graph you cannot get lost in beats one that behaves like
//! a map application.
//!
//! Clicking a node opens its declaration; clicking an edge's label would
//! be the next thing and is not built — the edge knows its site, which is
//! the half that would have been hard.
//!
//! Compile-bound like the Program Explorer: it re-asks after an analysis
//! while it is the shown tab, and marks itself stale while hidden.

use std::ops::Range;

use brink_gpui_model::graph::{GraphEdge, GraphNode, StoryGraphReport};
use brink_gpui_model::query::{QueryKind, QueryResult};
use gpui::prelude::*;
use gpui::{
    App, Bounds, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString,
    Subscription, Window, canvas, div, point, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, PanelId, TabGroup};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::graph_layout::{self, Layout};
use crate::project::{Project, ProjectEvent};
use brink_gpui_shell::tool_window::{TabSlot, select_tab};
use gpui::WeakEntity;

/// A node box, in layout units scaled by the zoom.
const NODE_W: f32 = 168.;
const NODE_H: f32 = 34.;
/// The gap between columns and between rows.
const GAP_X: f32 = 96.;
const GAP_Y: f32 = 18.;
/// Zoom bounds — far enough out to see a big story, close enough in to
/// read a name.
const MIN_ZOOM: f32 = 0.35;
const MAX_ZOOM: f32 = 1.6;

pub enum StoryGraphEvent {
    /// Open a node's declaration.
    Navigate { path: String, span: Range<usize> },
}

pub struct StoryGraphView {
    project: Entity<Project>,
    report: Option<StoryGraphReport>,
    layout: Layout,
    busy: bool,
    stale: bool,
    shown: bool,
    generation: u64,
    /// Pan, in pixels, applied to the whole drawing.
    pan: Point<Pixels>,
    zoom: f32,
    /// Where a drag started, and the pan it started from.
    drag: Option<(Point<Pixels>, Point<Pixels>)>,
    focus: FocusHandle,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for StoryGraphView {}
impl EventEmitter<StoryGraphEvent> for StoryGraphView {}

impl StoryGraphView {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let on_project = cx.subscribe(&project, |this: &mut Self, _, event: &ProjectEvent, cx| {
            if matches!(event, ProjectEvent::Analyzed) {
                if this.shown {
                    this.refresh(cx);
                } else {
                    this.stale = true;
                }
            }
        });
        Self {
            project,
            report: None,
            layout: Layout::default(),
            busy: false,
            stale: true,
            shown: false,
            generation: 0,
            pan: point(px(24.), px(24.)),
            zoom: 1.,
            drag: None,
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_project],
        }
    }

    /// Whether the panel currently sits in a dock.
    #[must_use]
    pub fn is_docked(&self) -> bool {
        self.tab.group().is_some()
    }

    /// Make this the shown tab of its group.
    pub fn activate(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        if let Some(group) = this.read(cx).tab.group() {
            select_tab(&group, PanelId::from(this.entity_id()), window, cx);
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
        let query = self.project.read(cx).query(QueryKind::StoryGraph, cx);
        cx.spawn(async move |this, cx| {
            let result = query.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.busy = false;
                if let Ok(QueryResult::StoryGraph(report)) = result {
                    this.layout = graph_layout::layout(&report);
                    this.report = Some(*report);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The top-left of a node's box, in the drawing's own pixels (before
    /// pan, after zoom).
    fn node_origin(&self, layer: usize, row: usize) -> Point<Pixels> {
        point(
            px(layer as f32 * (NODE_W + GAP_X) * self.zoom),
            px(row as f32 * (NODE_H + GAP_Y) * self.zoom),
        )
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        cx.notify();
    }

    fn reset_view(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.;
        self.pan = point(px(24.), px(24.));
        cx.notify();
    }

    /// The colour an edge is drawn in — its kind, in the palette the rest
    /// of the studio already uses for the same distinctions.
    fn edge_colour(kind: &str, cx: &App) -> Hsla {
        let theme = cx.theme();
        match kind {
            "choice" => theme.primary.opacity(0.75),
            "tunnel" => theme.warning.opacity(0.7),
            "thread" => theme.danger.opacity(0.6),
            _ => theme.muted_foreground.opacity(0.55),
        }
    }

    /// The lines, painted under the nodes.
    fn render_edges(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(report) = self.report.clone() else {
            return div().into_any_element();
        };
        let layout = self.layout.clone();
        let (zoom, pan) = (self.zoom, self.pan);
        let colours: Vec<(GraphEdge, Hsla)> = report
            .edges
            .iter()
            .map(|e| (e.clone(), Self::edge_colour(&e.kind, cx)))
            .collect();
        div()
            .absolute()
            .size_full()
            .child(canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, (), window, _cx| {
                    for (edge, colour) in &colours {
                        let (Some(from), Some(to)) =
                            (layout.find(&edge.from), layout.find(&edge.to))
                        else {
                            continue;
                        };
                        let origin = |layer: usize, row: usize| {
                            point(
                                bounds.origin.x
                                    + pan.x
                                    + px(layer as f32 * (NODE_W + GAP_X) * zoom),
                                bounds.origin.y + pan.y + px(row as f32 * (NODE_H + GAP_Y) * zoom),
                            )
                        };
                        // Out of the right edge of the source, into the left
                        // edge of the target — the direction the story runs.
                        let a = origin(from.layer, from.row);
                        let b = origin(to.layer, to.row);
                        let start = point(a.x + px(NODE_W * zoom), a.y + px(NODE_H * zoom / 2.));
                        let end = point(b.x, b.y + px(NODE_H * zoom / 2.));
                        let thickness = px((1.5 * zoom).max(1.));
                        // A thin quad per segment rather than a stroked path:
                        // `paint_path` fills, and a filled 1px line needs a
                        // rectangle anyway. Two segments and a joint give the
                        // orthogonal look a story graph reads best in.
                        let mid_x = (start.x + end.x) / 2.;
                        let seg = |from: Point<Pixels>, to: Point<Pixels>| {
                            let (x0, x1) = (from.x.min(to.x), from.x.max(to.x));
                            let (y0, y1) = (from.y.min(to.y), from.y.max(to.y));
                            Bounds {
                                origin: point(x0, y0),
                                size: size((x1 - x0).max(thickness), (y1 - y0).max(thickness)),
                            }
                        };
                        for bounds in [
                            seg(start, point(mid_x, start.y)),
                            seg(point(mid_x, start.y), point(mid_x, end.y)),
                            seg(point(mid_x, end.y), end),
                        ] {
                            window.paint_quad(gpui::fill(bounds, *colour));
                        }
                    }
                },
            ))
            .into_any_element()
    }

    fn render_nodes(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let Some(report) = self.report.as_ref() else {
            return Vec::new();
        };
        let theme = cx.theme();
        let (fg, muted, border, surface, accent) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.sidebar,
            theme.primary,
        );
        let entry = report.entry.clone();
        report
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, node)| {
                let placed = self.layout.find(&node.id)?;
                let origin = self.node_origin(placed.layer, placed.row);
                Some(self.render_node(
                    i,
                    node,
                    origin,
                    &entry,
                    (fg, muted, border, surface, accent),
                    cx,
                ))
            })
            .collect()
    }

    fn render_node(
        &self,
        ix: usize,
        node: &GraphNode,
        origin: Point<Pixels>,
        entry: &Option<String>,
        colours: (Hsla, Hsla, Hsla, Hsla, Hsla),
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (fg, muted, border, surface, accent) = colours;
        let is_entry = entry.as_deref() == Some(node.id.as_str());
        let pseudo = node.kind == "end" || node.kind == "done";
        let target = node.file.clone().zip(node.range);
        let label: SharedString = node.name.clone().into();
        let kind: SharedString = node.kind.clone().into();
        div()
            .id(("graph-node", ix))
            .absolute()
            .left(self.pan.x + origin.x)
            .top(self.pan.y + origin.y)
            .w(px(NODE_W * self.zoom))
            .h(px(NODE_H * self.zoom))
            .px_2()
            .rounded_md()
            .bg(surface)
            .border_1()
            .border_color(if is_entry { accent } else { border })
            .when(pseudo, |el| el.border_dashed())
            .overflow_hidden()
            .child(
                h_flex()
                    .size_full()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(if pseudo { muted } else { fg })
                            .child(label),
                    )
                    .child(div().text_xs().text_color(muted).child(kind)),
            )
            .when(target.is_some(), |el| {
                el.cursor_pointer()
                    .hover(move |s| s.border_color(accent))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        if let Some((path, (start, end))) = target.clone() {
                            cx.emit(StoryGraphEvent::Navigate {
                                path,
                                span: start as usize..end as usize,
                            });
                        }
                    }))
            })
            .into_any_element()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let summary: SharedString = match (&self.report, self.busy) {
            (Some(report), _) => format!(
                "{} nodes · {} edges · {} layers",
                report.nodes.len(),
                report.edges.len(),
                self.layout.layers
            )
            .into(),
            (None, true) => "reading…".into(),
            (None, false) => "not analyzed yet".into(),
        };
        h_flex()
            .w_full()
            .h(px(brink_gpui_shell::tool_window::HEADER_HEIGHT))
            .gap_2()
            .px_2()
            .items_center()
            .border_b_1()
            .border_color(border)
            .text_xs()
            .child(div().flex_1().text_color(muted).child(summary))
            .child(
                Button::new("graph-zoom-out")
                    .ghost()
                    .xsmall()
                    .label("\u{2212}")
                    .tooltip("Zoom out")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(0.8, cx))),
            )
            .child(
                div()
                    .text_color(muted)
                    .child(format!("{:.0}%", self.zoom * 100.)),
            )
            .child(
                Button::new("graph-zoom-in")
                    .ghost()
                    .xsmall()
                    .label("+")
                    .tooltip("Zoom in")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(1.25, cx))),
            )
            .child(
                Button::new("graph-reset")
                    .ghost()
                    .xsmall()
                    .label("Fit")
                    .tooltip("Back to 100% at the top left")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reset_view(cx))),
            )
            .into_any_element()
    }
}

impl Focusable for StoryGraphView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for StoryGraphView {
    fn panel_name(&self) -> &'static str {
        "StoryGraph"
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tab.added_to(group);
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tab.removed();
    }
}

impl Panel for StoryGraphView {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        brink_gpui_shell::tool_window::tab_title(gpui_component::IconName::Network, "Story Graph")
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for StoryGraphView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being rendered is being shown — the dock renders only the shown
        // tab (the Program Explorer's rule).
        self.shown = true;
        if self.stale && !self.busy {
            self.refresh(cx);
        }
        let muted = cx.theme().muted_foreground;
        let empty = self.report.as_ref().is_some_and(|r| r.nodes.is_empty());
        let header = self.render_header(cx);
        let edges = self.render_edges(cx);
        let nodes = self.render_nodes(cx);
        v_flex()
            .id("story-graph")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .child(header)
            .child(
                div()
                    .id("story-graph-canvas")
                    .relative()
                    .flex_1()
                    .overflow_hidden()
                    // Dragging the background pans. The drag is tracked on
                    // this element rather than a gpui drag-and-drop, which
                    // is about carrying a payload somewhere.
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.drag = Some((event.position, this.pan));
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        let Some((from, pan)) = this.drag else {
                            return;
                        };
                        this.pan = point(
                            pan.x + event.position.x - from.x,
                            pan.y + event.position.y - from.y,
                        );
                        cx.notify();
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.drag = None;
                            cx.notify();
                        }),
                    )
                    .child(edges)
                    .children(nodes)
                    .when(empty, |el| {
                        el.child(
                            div()
                                .p_3()
                                .text_color(muted)
                                .child("Nothing to draw: the project declares no knots."),
                        )
                    }),
            )
    }
}
