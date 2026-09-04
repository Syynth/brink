//! The Problems panel — every diagnostic in the project, path-keyed.
//!
//! It reads the mirror rather than the session, so it needs no query of its
//! own: diagnostics arrive with the analysis, already project-wide.

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Subscription, Window, div, px,
};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};

/// Activating a row opens its file at the offending offset.
#[derive(Debug, Clone)]
pub struct OpenProblem {
    pub path: String,
    pub offset: usize,
}

pub struct Problems {
    project: Entity<Project>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenProblem> for Problems {}
impl EventEmitter<PanelEvent> for Problems {}

impl Problems {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let watch = cx.subscribe(&project, |_, _, event: &ProjectEvent, cx| {
            if matches!(event, ProjectEvent::Analyzed) {
                cx.notify();
            }
        });
        Self {
            project,
            focus: cx.focus_handle(),
            _subscriptions: vec![watch],
        }
    }
}

impl Focusable for Problems {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for Problems {
    fn panel_name(&self) -> &'static str {
        "Problems"
    }
}

impl Panel for Problems {
    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.project.read(cx).problem_count();
        SharedString::from(if count == 0 {
            "Problems".to_owned()
        } else {
            format!("Problems ({count})")
        })
    }
}

impl Render for Problems {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (error, warning, muted, fg) = (
            theme.danger,
            theme.warning,
            theme.muted_foreground,
            theme.foreground,
        );
        let project = self.project.read(cx);

        // Flattened here rather than grouped by file: the panel is a wide,
        // short surface at the bottom of the window, so a flat list shows
        // more of what is wrong than a tree of one-item groups would.
        let rows: Vec<_> = project
            .all_diagnostics()
            .flat_map(|(path, found)| found.iter().map(move |d| (path.clone(), d.clone())))
            .collect();

        if rows.is_empty() {
            return v_flex()
                .size_full()
                .p_3()
                .text_xs()
                .text_color(muted)
                .child(if project.closure_known() {
                    "No problems."
                } else {
                    // Distinct from "no problems": nothing has been analyzed
                    // yet, so nothing is known either way.
                    "Not analyzed yet."
                })
                .into_any_element();
        }

        v_flex()
            .size_full()
            .p_1()
            .gap_0p5()
            .text_xs()
            .overflow_y_scrollbar()
            .children(rows.into_iter().map(move |(path, d)| {
                let colour = match d.severity {
                    brink_ir::Severity::Error => error,
                    brink_ir::Severity::Warning => warning,
                    _ => muted,
                };
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(colour))
                    .child(div().text_color(muted).child(SharedString::from(d.code)))
                    .child(
                        div()
                            .text_color(fg)
                            .flex_1()
                            .child(SharedString::from(d.message)),
                    )
                    .child(div().text_color(muted).child(SharedString::from(path)))
            }))
            .into_any_element()
    }
}
