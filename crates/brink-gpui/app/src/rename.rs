//! The rename prompt and its breakage report — ruled 2026-06-20 (#305 and
//! its F2 follow-up): one cross-file, safe-by-default pipeline for every
//! entry point.
//!
//! The prompt asks for the new name and nothing else — no "unsafe" checkbox
//! to tick before the author can see whether anything would break. If the
//! plan is safe it applies. If not, the prompt is replaced by the **report**:
//! what would break, where, and one button inside it — Force rename — which
//! applies anyway and still says what it broke, so breakage is never
//! silent. An `EXTERNAL` is always on the report side (ruled 2026-08-24): its
//! name is the story↔engine contract.

use std::rc::Rc;

use brink_gpui_model::query::RenamePlan;
use gpui::prelude::*;
use gpui::{App, Entity, SharedString, Window, div, px};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::notification::Notification;
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use crate::navigation::{EditorSite, rename};
use crate::project::Project;

/// Ask for a new name for the symbol at `offset`, currently `current`.
pub fn prompt(site: EditorSite, offset: usize, current: String, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder("New name");
        state.set_value(current.clone(), window, cx);
        state
    });
    // Enter in the box is the OK button — the dialog's own Confirm does not
    // reach an input that holds focus, and a rename prompt you have to
    // click is a prompt nobody uses.
    let confirm = Rc::new({
        let site = site.clone();
        let input = input.clone();
        move |window: &mut Window, cx: &mut App| {
            let new_name = input.read(cx).value().trim().to_owned();
            window.close_dialog(cx);
            if new_name.is_empty() {
                return;
            }
            run(&site, offset, new_name, window, cx);
        }
    });
    let on_enter = {
        let confirm = confirm.clone();
        let handle = window.window_handle();
        cx.subscribe(&input, move |_, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let confirm = confirm.clone();
                let _ = handle.update(cx, move |_, window, cx| confirm(window, cx));
            }
        })
    };
    let title = format!("Rename `{current}`");
    let focus_input = input.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        // Keep the subscription alive for as long as the dialog is built.
        let _keep = &on_enter;
        focus_input.update(cx, |state, cx| state.focus(window, cx));
        let input = input.clone();
        let confirm = confirm.clone();
        dialog
            .title(SharedString::from(title.clone()))
            .w(px(420.))
            .content(move |content, _window, _cx| {
                content.child(div().py_1().child(Input::new(&input)))
            })
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Rename")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                confirm(window, cx);
                false
            })
    });
}

/// Compute the plan; apply it when safe, else show the report.
fn run(site: &EditorSite, offset: usize, new_name: String, window: &mut Window, cx: &mut App) {
    let plan = rename(site, offset, new_name, cx);
    let project = site.project.clone();
    window
        .spawn(cx, async move |cx| {
            let Some(plan) = plan.await else {
                let _ = cx.update(|window, cx| {
                    window.push_notification(Notification::warning("Nothing to rename here."), cx);
                });
                return;
            };
            let _ = cx.update(|window, cx| {
                if plan.is_safe() {
                    apply(&project, &plan, window, cx);
                } else {
                    report(project.clone(), plan, window, cx);
                }
            });
        })
        .detach();
}

fn apply(project: &Entity<Project>, plan: &RenamePlan, window: &mut Window, cx: &mut App) {
    let files = project.update(cx, |project, cx| project.apply_edits(&plan.edits, cx));
    let places = plan.edits.len();
    let message = format!(
        "Renamed `{}` → `{}` in {places} place{} across {files} file{}.",
        plan.old_name,
        plan.new_name,
        if places == 1 { "" } else { "s" },
        if files == 1 { "" } else { "s" }
    );
    window.push_notification(Notification::success(message), cx);
    // Force never hides what it broke.
    if !plan.introduced.is_empty() {
        let n = plan.introduced.len();
        window.push_notification(
            Notification::warning(format!(
                "That rename introduced {n} diagnostic{} — see Problems.",
                if n == 1 { "" } else { "s" }
            )),
            cx,
        );
    }
}

/// The breakage report, with Force rename inside it.
fn report(project: Entity<Project>, plan: RenamePlan, window: &mut Window, cx: &mut App) {
    let plan = Rc::new(plan);
    let title = if plan.external {
        format!("`{}` is an EXTERNAL", plan.old_name)
    } else {
        format!(
            "Renaming `{}` → `{}` would break {} place{}",
            plan.old_name,
            plan.new_name,
            plan.introduced.len(),
            if plan.introduced.len() == 1 { "" } else { "s" }
        )
    };
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
                if plan.external {
                    body = body.child(div().text_color(muted).child(
                        "Its name is the contract between the story and the engine; \\
                         renaming it here does not rename the host's binding.",
                    ));
                }
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
                    .ok_text("Force rename")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                apply(&project, &plan_for_ok, window, cx);
                true
            })
    });
}
