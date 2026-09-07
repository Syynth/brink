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
//! safe-by-default rename; this moves a path. A file's own name appears
//! in `INCLUDE` lines and in `brink.toml`'s `entry`, and moving it does
//! NOT rewrite those — the analysis reports the break, in the same place
//! it reports every other unresolved path, rather than this quietly
//! rewriting text the author did not ask it to touch.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, Entity, SharedString, Window, div, px};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputEvent, InputState};

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
            let moved = project.update(cx, |project, cx| project.rename_file(&from, &to, cx));
            match moved {
                Ok(()) => notify(
                    Severity::Success,
                    "files",
                    format!("Moved {from} to {to}."),
                    window,
                    cx,
                ),
                Err(err) => notify(Severity::Error, "files", format!("{err}"), window, cx),
            }
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
    if paths.is_empty() {
        return;
    }
    let title = match paths.as_slice() {
        [only] => format!("Delete {only}?"),
        many => format!("Delete {} files?", many.len()),
    };
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
