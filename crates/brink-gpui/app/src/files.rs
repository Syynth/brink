//! Creating, renaming and deleting a file from the Binder.
//!
//! The three operations an author needs to shape a project and could not
//! do from the studio at all: the Binder's menu carried "Rename…" and
//! "Delete" as no-ops, which is worse than not carrying them.
//!
//! Each one goes through [`Project`], which owns both the mirror and the
//! disk, and each writes to disk immediately rather than leaving the file
//! dirty — a file that exists in the Binder and not on disk is a file the
//! next `INCLUDE` cannot find, and nothing on screen would say why.
//!
//! **Renaming a FILE is not renaming a knot.** `f2` is the cross-file,
//! safe-by-default rename; this moves a path. A move rewrites the
//! `INCLUDE`s it affects — the ones pointing at what moved, and the moved
//! files' own relative ones — as the web studio's does (#3656, reversing
//! the earlier native rule that left them for the analysis to report).
//! It is safe-by-default like every structural change: a move that would
//! still break something shows what, with "Move anyway". `brink.toml`'s
//! `entry` is NOT rewritten — which file the story starts from is the
//! author's to decide, and the analysis says when it names nothing.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, Entity, SharedString, Window, div, px};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputEvent, InputState};

use brink_gpui_model::query::{MoveOutcome, MovePlan, QueryKind, QueryResult};

use crate::project::Project;
use brink_gpui_shell::notify::{Severity, notify};

/// What a prompt does when it is confirmed. Shared by all three, which is
/// why it has a name rather than being spelled out at each call.
pub(crate) type Confirm = Rc<dyn Fn(&mut Window, &mut App)>;

/// The text a brand-new `.ink` file starts with: nothing.
///
/// Not a knot header, not a `-> DONE`. A template would be this crate
/// deciding how a story starts, which is the author's business and the
/// prose dialect's — and an empty file analyses cleanly.
const NEW_FILE_TEMPLATE: &str = "";

/// Ask for a name and create the file in `folder`.
pub fn new_file(project: Entity<Project>, folder: String, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        InputState::new(window, cx).placeholder(if folder.is_empty() {
            "scene.ink".to_owned()
        } else {
            format!("{folder}/scene.ink")
        })
    });
    let confirm = Rc::new({
        let project = project.clone();
        let input = input.clone();
        let folder = folder.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            window.close_dialog(cx);
            if name.is_empty() {
                return;
            }
            // A bare name goes in the row's folder; a name with a slash in
            // it is taken as written, so the box can also say where.
            let path = if name.contains('/') || folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            let path = with_ink_suffix(&path);
            let created = project.update(cx, |project, cx| {
                project.create_file(&path, NEW_FILE_TEMPLATE, cx)
            });
            match created {
                Ok(()) => notify(
                    Severity::Success,
                    "files",
                    format!("Created {path}."),
                    window,
                    cx,
                ),
                Err(err) => notify(Severity::Error, "files", format!("{err}"), window, cx),
            }
        }
    });
    prompt("New file", "Create", input, confirm, window, cx);
}

/// Ask for a name and make an empty folder in `folder`.
pub fn new_folder(project: Entity<Project>, folder: String, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        InputState::new(window, cx).placeholder(if folder.is_empty() {
            "chapter".to_owned()
        } else {
            format!("{folder}/chapter")
        })
    });
    let confirm = Rc::new({
        let project = project.clone();
        let input = input.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            window.close_dialog(cx);
            if name.is_empty() {
                return;
            }
            let path = if name.contains('/') || folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            let made = project.update(cx, |project, cx| project.create_folder(&path, cx));
            match made {
                Ok(()) => notify(
                    Severity::Success,
                    "files",
                    format!("Created {}/.", path.trim_end_matches('/')),
                    window,
                    cx,
                ),
                Err(err) => notify(Severity::Error, "files", format!("{err}"), window, cx),
            }
        }
    });
    prompt("New folder", "Create", input, confirm, window, cx);
}

/// Ask for a new path for folder `folder` and move it, with every file in
/// it, there. `INCLUDE` lines are not rewritten — the same rule as a
/// single file's move.
pub fn rename_folder(project: Entity<Project>, folder: String, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder("New path");
        state.set_value(folder.clone(), window, cx);
        state
    });
    let confirm = Rc::new({
        let project = project.clone();
        let input = input.clone();
        let from = folder.clone();
        move |window: &mut Window, cx: &mut App| {
            let to = input
                .read(cx)
                .value()
                .trim()
                .trim_end_matches('/')
                .to_owned();
            window.close_dialog(cx);
            if to.is_empty() || to == from {
                return;
            }
            move_path(project.clone(), from.clone(), to, true, window, cx);
        }
    });
    prompt(
        &format!("Rename {folder}/"),
        "Rename",
        input,
        confirm,
        window,
        cx,
    );
}

/// A name with no extension gets `.ink` — the surface a new file in a
/// story project is overwhelmingly going to be. An explicit extension
/// (`.brink`, `.toml`, anything) is left exactly as typed.
#[must_use]
pub fn with_ink_suffix(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.contains('.') {
        path.to_owned()
    } else {
        format!("{path}.ink")
    }
}

/// Ask for a new path for `path` and move it there.
pub fn rename_file(project: Entity<Project>, path: String, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder("New path");
        state.set_value(path.clone(), window, cx);
        state
    });
    let confirm = Rc::new({
        let project = project.clone();
        let input = input.clone();
        let from = path.clone();
        move |window: &mut Window, cx: &mut App| {
            let to = input.read(cx).value().trim().to_owned();
            window.close_dialog(cx);
            if to.is_empty() || to == from {
                return;
            }
            let to = with_ink_suffix(&to);
            move_path(project.clone(), from.clone(), to, false, window, cx);
        }
    });
    prompt(
        &format!("Rename {path}"),
        "Rename",
        input,
        confirm,
        window,
        cx,
    );
}

/// Move a file, or a folder, `INCLUDE`s and all (#3656): the worker plans
/// it — the moved files' new text, the rewrites in the files that include
/// them, what it would break — and a safe plan applies; one that would
/// break something shows what, with "Move anyway".
pub(crate) fn move_path(
    project: Entity<Project>,
    from: String,
    to: String,
    folder: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let kind = if folder {
        QueryKind::MoveFolder {
            from: from.clone(),
            to: to.clone(),
        }
    } else {
        QueryKind::MoveFile {
            from: from.clone(),
            to: to.clone(),
        }
    };
    let query = project.read(cx).query(kind, cx);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let outcome = query.await;
        let _ = handle.update(cx, move |_, window, cx| match outcome {
            Ok(QueryResult::Move(MoveOutcome::Plan(plan))) => {
                let span = folder.then_some((from, to));
                if plan.is_safe() {
                    apply_move(&project, &plan, span.as_ref(), window, cx);
                } else {
                    let title = format!(
                        "{} would break {} place{}",
                        plan.summary,
                        plan.introduced.len(),
                        if plan.introduced.len() == 1 { "" } else { "s" }
                    );
                    let introduced = plan.introduced.clone();
                    let plan = Rc::new(*plan);
                    crate::structural::breakage_report(
                        title,
                        introduced,
                        Rc::new(move |window, cx| {
                            apply_move(&project, &plan, span.as_ref(), window, cx);
                        }),
                        window,
                        cx,
                    );
                }
            }
            Ok(QueryResult::Move(MoveOutcome::Refused(why))) => {
                notify(Severity::Error, "files", why, window, cx);
            }
            _ => notify(
                Severity::Error,
                "files",
                "That move could not be computed.",
                window,
                cx,
            ),
        });
    })
    .detach();
}

fn apply_move(
    project: &Entity<Project>,
    plan: &MovePlan,
    folder: Option<&(String, String)>,
    window: &mut Window,
    cx: &mut App,
) {
    let folder = folder.map(|(a, b)| (a.as_str(), b.as_str()));
    let applied = project.update(cx, |project, cx| project.apply_move(plan, folder, cx));
    match applied {
        Ok(_) => {
            let rewrote = plan.edits.len();
            let tail = match rewrote {
                0 => String::new(),
                1 => " — 1 INCLUDE rewritten".to_owned(),
                n => format!(" — {n} INCLUDEs rewritten"),
            };
            notify(
                Severity::Success,
                "files",
                format!("{}{tail}.", plan.summary),
                window,
                cx,
            );
            // Force never hides what it broke.
            if !plan.introduced.is_empty() {
                let n = plan.introduced.len();
                notify(
                    Severity::Warning,
                    "files",
                    format!(
                        "That move introduced {n} diagnostic{} — see Problems.",
                        if n == 1 { "" } else { "s" }
                    ),
                    window,
                    cx,
                );
            }
        }
        Err(err) => notify(Severity::Error, "files", format!("{err}"), window, cx),
    }
}

/// Confirm, then delete every path in `paths` from the project and from
/// disk. ONE confirmation, naming them: a multi-selection deletes
/// together or not at all, and a dialog per file is a dialog nobody
/// reads by the third one.
///
/// An **alert** dialog, not the plain one the prompts use: a plain
/// `Dialog` renders only what its content builder returns, so its
/// `button_props` draw nothing (the rename prompt gets away with it
/// because Enter in its input is the confirm). A confirmation with no
/// input has no such key, and a confirmation with no buttons is a dead
/// end — which is exactly what the first version of this was on screen.
pub fn delete_files(
    project: Entity<Project>,
    paths: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = match paths.as_slice() {
        [only] => format!("Delete {only}?"),
        many => format!("Delete {} files?", many.len()),
    };
    confirm_delete(project, title, paths, window, cx);
}

/// Confirm, then delete folder `folder` — every file in it. The same
/// confirmation as [`delete_files`], titled for the folder as the web's
/// is ("Delete scenes/ and its 3 files?").
pub fn delete_folder(
    project: Entity<Project>,
    folder: &str,
    paths: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let n = paths.len();
    let title = format!(
        "Delete {}/ and its {n} file{}?",
        folder.trim_end_matches('/'),
        if n == 1 { "" } else { "s" }
    );
    confirm_delete(project, title, paths, window, cx);
}

fn confirm_delete(
    project: Entity<Project>,
    title: String,
    paths: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) {
    if paths.is_empty() {
        return;
    }
    // Every name, so a selection is never deleted sight unseen; past a
    // handful the list is the count plus what would fit.
    let listed: String = if paths.len() == 1 {
        String::new()
    } else {
        let shown: Vec<&str> = paths.iter().take(8).map(String::as_str).collect();
        let more = paths.len().saturating_sub(shown.len());
        let tail = if more > 0 {
            format!(", and {more} more")
        } else {
            String::new()
        };
        format!("{}{tail}\n\n", shown.join(", "))
    };
    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        let project = project.clone();
        let paths = paths.clone();
        alert
            .title(SharedString::from(title.clone()))
            // Said plainly: the studio has no undo for this, and
            // pretending otherwise would be the lie.
            .description(format!(
                "{listed}Removed from the project and from disk. \
                 File ▸ Undo File Operation brings the last one back."
            ))
            .show_cancel(true)
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Delete")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                let mut deleted = 0;
                let mut failed: Vec<String> = Vec::new();
                project.update(cx, |project, cx| {
                    for path in &paths {
                        match project.delete_file(path, cx) {
                            Ok(()) => deleted += 1,
                            Err(err) => failed.push(format!("{path}: {err}")),
                        }
                    }
                });
                if deleted > 0 {
                    notify(
                        Severity::Success,
                        "files",
                        match paths.as_slice() {
                            [only] => format!("Deleted {only}."),
                            _ => format!("Deleted {deleted} files."),
                        },
                        window,
                        cx,
                    );
                }
                // Each failure named: a partial delete that only said
                // "some failed" leaves the author to work out which.
                for message in failed {
                    notify(Severity::Error, "files", message, window, cx);
                }
                true
            })
    });
}

/// One-line prompt with an OK that Enter also reaches — the rename
/// prompt's shape (`crate::rename`), which exists for the same reason: a
/// dialog's Confirm does not reach an input that holds focus, and a
/// prompt you have to click is a prompt nobody uses.
pub(crate) fn prompt(
    title: &str,
    ok: &'static str,
    input: Entity<InputState>,
    confirm: Confirm,
    window: &mut Window,
    cx: &mut App,
) {
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
    let title = title.to_owned();
    let focus_input = input.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
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
            .button_props(DialogButtonProps::default().ok_text(ok).show_cancel(true))
            .on_ok(move |_, window, cx| {
                confirm(window, cx);
                false
            })
    });
}

#[cfg(test)]
mod tests {
    use super::with_ink_suffix;

    #[test]
    fn a_bare_name_becomes_an_ink_file() {
        assert_eq!(with_ink_suffix("scene"), "scene.ink");
        assert_eq!(with_ink_suffix("acts/two"), "acts/two.ink");
    }

    #[test]
    fn an_explicit_extension_is_left_exactly_as_typed() {
        assert_eq!(with_ink_suffix("scene.brink"), "scene.brink");
        assert_eq!(with_ink_suffix("notes.md"), "notes.md");
        assert_eq!(with_ink_suffix("a.b/c.ink"), "a.b/c.ink");
        // A dot in a FOLDER is not an extension on the name.
        assert_eq!(with_ink_suffix("a.b/c"), "a.b/c.ink");
    }
}
