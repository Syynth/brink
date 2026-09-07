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
//! Each action carries its own payload in `lsp_types::CodeAction::data`. A
//! first cut kept the last-offered list and handed the toolkit an index into
//! it — and the toolkit asks for actions again whenever the caret moves, so
//! a menu built from one list could be confirmed against another: choosing
//! "Format knot" applied a different file's E031 fix. The payload rides
//! with the entry it belongs to.

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

/// What one menu entry does when chosen — the action's `data`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum Offered {
    Fix(FixPlan),
    Refactor(Refactor),
}

pub struct BrinkCodeActions {
    project: WeakEntity<Project>,
    path: SharedString,
    origin: EntityId,
}

impl BrinkCodeActions {
    pub fn new(project: WeakEntity<Project>, path: SharedString, origin: EntityId) -> Self {
        Self {
            project,
            path,
            origin,
        }
    }
}

fn entry(offered: &Offered, title: String, kind: lsp::CodeActionKind) -> Option<lsp::CodeAction> {
    Some(lsp::CodeAction {
        title,
        kind: Some(kind),
        data: Some(serde_json::to_value(offered).ok()?),
        ..Default::default()
    })
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
        cx.spawn(async move |_| {
            let mut list: Vec<Offered> = Vec::new();
            if let Ok(QueryResult::FixesAt(fixes)) = fixes.await {
                list.extend(fixes.into_iter().map(Offered::Fix));
            }
            if let Ok(QueryResult::Refactors(found)) = refactors.await {
                list.extend(found.into_iter().map(Offered::Refactor));
            }
            Ok(list
                .iter()
                .filter_map(|item| match item {
                    Offered::Fix(plan) => {
                        entry(item, fix_title(plan), lsp::CodeActionKind::QUICKFIX)
                    }
                    Offered::Refactor(r) => {
                        entry(item, r.title.clone(), lsp::CodeActionKind::REFACTOR)
                    }
                })
                .collect())
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
        let chosen: Option<Offered> = action
            .data
            .and_then(|data| serde_json::from_value(data).ok());
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

/// Apply every Safe fix in `scope` and answer how many landed — the same
/// engine as [`fix_all`] with none of its talk.
///
/// Fix-on-save runs on every `cmd-s`, and a "Nothing to fix." toast each
/// time would be noise about the thing that did NOT happen. The count
/// comes back so a caller can say something once if it wants to.
pub fn fix_all_quietly(
    project: &Entity<Project>,
    scope: FixScope,
    cx: &mut App,
) -> gpui::Task<usize> {
    let query = project.read(cx).query(QueryKind::FixAll { scope }, cx);
    let project = project.clone();
    cx.spawn(async move |cx| {
        let Ok(QueryResult::FixAll(report)) = query.await else {
            return 0;
        };
        let applied = report.applied;
        project.update(cx, |project, cx| {
            for (path, text) in report.files {
                project.edit(&path, text, None, cx);
            }
        });
        applied
    })
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
