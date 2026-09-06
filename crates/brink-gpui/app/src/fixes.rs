//! Fixes in the editor and the Problems panel — `docs/autofix-spec.md` §7.
//!
//! The editor's code-action menu (`cmd-.`) lists the fixes for the
//! diagnostics under the caret, every tier, plus the whole-source refactors;
//! one click applies one fix, and a Placeholder fix moves the caret to the
//! hole it left. The Problems panel gets a per-row **Fix** and a header
//! **Fix all safe (N)**. All of it is `Project::apply_edits` /
//! `Project::edit` at the end, so every editor over a touched file follows
//! and undo sees an ordinary edit.
//!
//! The provider keeps what it last offered and hands the toolkit an index
//! into that list as the action's `data`, rather than serializing a plan
//! through `lsp_types::CodeAction` and back: the plan is plain data the app
//! already holds, and the round trip would only be a second copy of it.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use brink_gpui_model::fixes::{FixAllReport, FixPlan, FixScope, Refactor, Tier};
use brink_gpui_model::query::{QueryKind, QueryResult};
use gpui::{App, Entity, EntityId, SharedString, Task, WeakEntity, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{CodeActionProvider, EditorState, RopeExt as _};
use gpui_component::notification::Notification;
use lsp_types as lsp;

use crate::document::seed_edit;
use crate::project::Project;

/// What one menu entry does when chosen.
#[derive(Clone)]
enum Offered {
    Fix(FixPlan),
    Refactor(Refactor),
}

pub struct BrinkCodeActions {
    project: WeakEntity<Project>,
    path: SharedString,
    origin: EntityId,
    /// The last list offered, which the chosen action's `data` indexes.
    offered: Rc<RefCell<Vec<Offered>>>,
}

impl BrinkCodeActions {
    pub fn new(project: WeakEntity<Project>, path: SharedString, origin: EntityId) -> Self {
        Self {
            project,
            path,
            origin,
            offered: Rc::default(),
        }
    }
}

fn entry(ix: usize, title: String, kind: lsp::CodeActionKind) -> lsp::CodeAction {
    lsp::CodeAction {
        title,
        kind: Some(kind),
        data: Some(serde_json::Value::from(ix)),
        ..Default::default()
    }
}

/// The menu label: the fixer's own wording, with the tier appended when it
/// is not the one the author would assume.
fn fix_title(plan: &FixPlan) -> String {
    match plan.tier {
        Tier::Safe => plan.title.clone(),
        Tier::Suggested | Tier::Placeholder => format!("{} ({})", plan.title, plan.tier.label()),
    }
}

impl CodeActionProvider for BrinkCodeActions {
    fn id(&self) -> SharedString {
        SharedString::from("brink")
    }

    fn code_actions(
        &self,
        state: Entity<EditorState>,
        range: std::ops::Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<lsp::CodeAction>>> {
        let Some(project) = self.project.upgrade() else {
            return Task::ready(Ok(Vec::new()));
        };
        let text = state.read(cx).text().clone();
        seed_edit(&project, &self.path, &text, self.origin, cx);
        let offset = u32::try_from(range.start).unwrap_or(u32::MAX);
        let fixes = project.read(cx).query(
            QueryKind::FixesAt {
                path: self.path.to_string(),
                offset,
            },
            cx,
        );
        let refactors = project.read(cx).query(
            QueryKind::Refactors {
                path: self.path.to_string(),
                offset,
            },
            cx,
        );
        let offered = self.offered.clone();
        cx.spawn(async move |_| {
            let mut list: Vec<Offered> = Vec::new();
            if let Ok(QueryResult::FixesAt(fixes)) = fixes.await {
                list.extend(fixes.into_iter().map(Offered::Fix));
            }
            if let Ok(QueryResult::Refactors(found)) = refactors.await {
                list.extend(found.into_iter().map(Offered::Refactor));
            }
            let actions = list
                .iter()
                .enumerate()
                .map(|(ix, item)| match item {
                    Offered::Fix(plan) => entry(ix, fix_title(plan), lsp::CodeActionKind::QUICKFIX),
                    Offered::Refactor(r) => {
                        entry(ix, r.title.clone(), lsp::CodeActionKind::REFACTOR)
                    }
                })
                .collect();
            *offered.borrow_mut() = list;
            Ok(actions)
        })
    }

    fn perform_code_action(
        &self,
        state: Entity<EditorState>,
        action: lsp::CodeAction,
        _push_to_history: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let Some(project) = self.project.upgrade() else {
            return Task::ready(Ok(()));
        };
        let chosen = action
            .data
            .as_ref()
            .and_then(serde_json::Value::as_u64)
            .and_then(|ix| {
                self.offered
                    .borrow()
                    .get(usize::try_from(ix).ok()?)
                    .cloned()
            });
        match chosen {
            Some(Offered::Fix(plan)) => {
                apply_fix(&project, &plan, Some((&state, &self.path)), window, cx);
                Task::ready(Ok(()))
            }
            Some(Offered::Refactor(refactor)) => {
                let path = self.path.to_string();
                let query = project.read(cx).query(
                    QueryKind::ResolveRefactor {
                        path: path.clone(),
                        data: refactor.data,
                    },
                    cx,
                );
                window.spawn(cx, async move |cx| {
                    let Ok(QueryResult::ResolvedRefactor(Some(text))) = query.await else {
                        return Ok(());
                    };
                    let _ = cx.update(|_, cx| {
                        project.update(cx, |project, cx| {
                            project.edit(&path, text, None, cx);
                        });
                    });
                    Ok(())
                })
            }
            None => Task::ready(Ok(())),
        }
    }
}

/// Apply one fix. `editor` is the editor the fix was chosen from, if any:
/// a Placeholder fix moves its caret to the hole when the hole is in that
/// file.
pub fn apply_fix(
    project: &Entity<Project>,
    plan: &FixPlan,
    editor: Option<(&Entity<EditorState>, &SharedString)>,
    window: &mut Window,
    cx: &mut App,
) {
    let files = project.update(cx, |project, cx| project.apply_edits(&plan.edits, cx));
    if files == 0 {
        window.push_notification(
            Notification::warning(format!("`{}` no longer applies.", plan.title)),
            cx,
        );
        return;
    }
    if let (Some(caret), Some((state, path))) = (&plan.caret, editor)
        && caret.path == path.as_ref()
    {
        state.update(cx, |state, cx| {
            let position = state.text().offset_to_position(caret.start as usize);
            state.set_cursor_position(position, window, cx);
        });
    }
}

/// Run the safe batch over `scope` and write what it produced.
pub fn fix_all(project: &Entity<Project>, scope: FixScope, window: &mut Window, cx: &mut App) {
    let query = project.read(cx).query(QueryKind::FixAll { scope }, cx);
    let project = project.clone();
    window
        .spawn(cx, async move |cx| {
            let Ok(QueryResult::FixAll(report)) = query.await else {
                return;
            };
            let _ = cx.update(|window, cx| {
                write_report(&project, report, window, cx);
            });
        })
        .detach();
}

fn write_report(
    project: &Entity<Project>,
    report: FixAllReport,
    window: &mut Window,
    cx: &mut App,
) {
    if report.files.is_empty() {
        window.push_notification(Notification::info("Nothing to fix."), cx);
        return;
    }
    let files = report.files.len();
    project.update(cx, |project, cx| {
        for (path, text) in report.files {
            project.edit(&path, text, None, cx);
        }
    });
    let mut message = format!(
        "Fixed {} problem{} in {files} file{}.",
        report.applied,
        if report.applied == 1 { "" } else { "s" },
        if files == 1 { "" } else { "s" }
    );
    if report.cap_hit {
        message.push_str(&format!(
            " {} remain after {} rounds — run again.",
            report.remaining, report.rounds
        ));
    }
    window.push_notification(Notification::success(message), cx);
}
