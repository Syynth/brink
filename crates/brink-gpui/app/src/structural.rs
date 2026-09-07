//! The Binder's structural moves — promote a stitch to a knot, demote a
//! knot into the one above it — and their breakage report.
//!
//! The same safe-by-default shape the symbol rename has (ruled
//! 2026-06-20, #305): the plan is computed first, and if it introduces
//! nothing it applies. If it would break something, the author sees WHAT
//! would break, where, and one button — Force — which applies anyway and
//! still says what it broke. A move the engine refuses outright (a name
//! collision, a knot that has stitches of its own, the first knot in a
//! file with nothing above it) is reported with its reason rather than
//! silently doing nothing.
//!
//! The arithmetic is `brink-ide`'s (`structural_move`), and so is the gate
//! (`structural_result::gate_with_source`, run by the worker query). This
//! module only asks, applies and reports.

use std::rc::Rc;

use brink_gpui_model::query::{QueryKind, QueryResult, StructuralOutcome, StructuralPlan};
use gpui::prelude::*;
use gpui::{App, Entity, SharedString, Window, div, px};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use crate::project::Project;
use brink_gpui_shell::notify::{Severity, notify};

/// Ask for a name, then lift `start..end` of `path` into a new knot (or
/// function) and leave a call behind.
pub fn extract(
    project: Entity<Project>,
    path: String,
    span: std::ops::Range<usize>,
    function: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let what = if function { "function" } else { "knot" };
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("name"));
    let confirm: crate::files::Confirm = Rc::new({
        let project = project.clone();
        let input = input.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            window.close_dialog(cx);
            if name.is_empty() {
                return;
            }
            if let Some(why) = crate::knots::name_error(&name) {
                notify(Severity::Error, "refactor", why, window, cx);
                return;
            }
            run(
                project.clone(),
                QueryKind::Extract {
                    path: path.clone(),
                    start: u32::try_from(span.start).unwrap_or(0),
                    end: u32::try_from(span.end).unwrap_or(0),
                    name,
                    function,
                },
                window,
                cx,
            );
        }
    });
    crate::files::prompt(
        &format!("Extract to {what}"),
        "Extract",
        input,
        confirm,
        window,
        cx,
    );
}

/// Promote `knot.stitch` in `path` to a knot of its own.
pub fn promote(
    project: Entity<Project>,
    path: String,
    knot: String,
    stitch: String,
    window: &mut Window,
    cx: &mut App,
) {
    run(
        project,
        QueryKind::Promote { path, knot, stitch },
        window,
        cx,
    );
}

/// Demote `knot` in `path` into the knot above it.
pub fn demote(
    project: Entity<Project>,
    path: String,
    knot: String,
    window: &mut Window,
    cx: &mut App,
) {
    run(project, QueryKind::Demote { path, knot }, window, cx);
}

fn run(project: Entity<Project>, kind: QueryKind, window: &mut Window, cx: &mut App) {
    let query = project.read(cx).query(kind, cx);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let outcome = query.await;
        let _ = handle.update(cx, move |_, window, cx| match outcome {
            Ok(QueryResult::Structural(StructuralOutcome::Plan(plan))) => {
                if plan.is_safe() {
                    apply(&project, &plan, window, cx);
                } else {
                    report(project, *plan, window, cx);
                }
            }
            Ok(QueryResult::Structural(StructuralOutcome::Refused(why))) => {
                notify(Severity::Warning, "binder", why, window, cx);
            }
            _ => notify(
                Severity::Error,
                "binder",
                "That move could not be computed.",
                window,
                cx,
            ),
        });
    })
    .detach();
}

/// Write the plan: the primary file wholesale, the reference rewrites in
/// their own files.
fn apply(project: &Entity<Project>, plan: &StructuralPlan, window: &mut Window, cx: &mut App) {
    let files = project.update(cx, |project, cx| {
        let wrote = project.edit(&plan.path, plan.new_source.clone(), None, cx);
        let others = project.apply_edits(&plan.edits, cx);
        usize::from(wrote) + others
    });
    notify(
        Severity::Success,
        "binder",
        format!(
            "{} in {files} file{}.",
            plan.summary,
            if files == 1 { "" } else { "s" }
        ),
        window,
        cx,
    );
    // Force never hides what it broke.
    if !plan.introduced.is_empty() {
        let n = plan.introduced.len();
        notify(
            Severity::Warning,
            "binder",
            format!(
                "That move introduced {n} diagnostic{} — see Problems.",
                if n == 1 { "" } else { "s" }
            ),
            window,
            cx,
        );
    }
}

/// The breakage report, with Force inside it.
fn report(project: Entity<Project>, plan: StructuralPlan, window: &mut Window, cx: &mut App) {
    let plan = Rc::new(plan);
    let title = format!(
        "{} would break {} place{}",
        plan.summary,
        plan.introduced.len(),
        if plan.introduced.len() == 1 { "" } else { "s" }
    );
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let (muted, danger, warning, fg) = (
            theme.muted_foreground,
            theme.danger,
            theme.warning,
            theme.foreground,
        );
        let plan_for_content = plan.clone();
        let plan_for_ok = plan.clone();
        let project = project.clone();
        dialog
            .title(SharedString::from(title.clone()))
            .w(px(560.))
            .content(move |content, _window, _cx| {
                let plan = plan_for_content.clone();
                let mut body = v_flex().gap_1().text_xs();
                for d in &plan.introduced {
                    let colour = match d.severity {
                        brink_ir::Severity::Error => danger,
                        brink_ir::Severity::Warning => warning,
                        _ => muted,
                    };
                    body = body.child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(colour))
                            .child(
                                div()
                                    .text_color(muted)
                                    .child(format!("{}:{}:{}", d.path, d.line, d.col)),
                            )
                            .child(div().text_color(fg).child(d.message.clone())),
                    );
                }
                content.child(body)
            })
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Move anyway")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                apply(&project, &plan_for_ok, window, cx);
                true
            })
    });
}
