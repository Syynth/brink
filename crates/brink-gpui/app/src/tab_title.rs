//! A centre tab's title, with the close affordance the tab strip lacks.
//!
//! The kit's strip draws no ✕, and its only Close sits in the `…` menu,
//! refused outright for the last tab of a lone group (base's rule for a
//! dock that must not be emptied — wrong for an editor, where no file open
//! is a perfectly good state). So the close lives in the one part of a tab
//! a panel draws itself, its title: a ✕ revealed on hover, and a middle
//! click anywhere on the title.
//!
//! Neither closes anything here. Both dispatch [`CloseTabById`], which the
//! studio handles beside `cmd-w`'s `CloseTab` — one path for every way a
//! tab is closed, so the unsaved-edits prompt and `CodeView`'s bookkeeping
//! cannot be skipped by whichever way the author happened to use.

use gpui::prelude::*;
use gpui::{App, EntityId, IntoElement, MouseButton, SharedString, div, px};
use gpui_component::{ActiveTheme as _, h_flex};

/// Close the centre tab whose panel is entity `id` — a document, or one of
/// the Player, Compiled Output and the Story Graph. Not a palette command:
/// it names a tab, and only a tab's own title knows which.
#[derive(Clone, Copy, PartialEq, Eq, Debug, gpui::Action)]
#[action(namespace = brink, no_json)]
pub struct CloseTabById {
    pub id: EntityId,
}

/// The `group` the ✕ reveals itself on: hovering the title, not the ✕.
const GROUP: &str = "brink-tab-title";

/// A document tab's label: its file name, marked `name •` while unsaved.
/// The ✕ beside it stays a ✕, so what it does never depends on state.
pub fn document_label(name: &str, dirty: bool) -> SharedString {
    if dirty {
        SharedString::from(format!("{name} \u{2022}"))
    } else {
        SharedString::from(name.to_owned())
    }
}

/// `label` — a document's name, or a surface's icon and name
/// (`brink_gpui_shell::tool_window::tab_title`) — then the ✕.
pub fn closable_tab(id: EntityId, label: impl IntoElement, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, fg, hover) = (
        theme.muted_foreground,
        theme.foreground,
        theme.secondary_hover,
    );
    h_flex()
        .id(("tab-title", id))
        .group(GROUP)
        .gap_1()
        .items_center()
        // Middle-click closes, as in every tabbed editor. On release, so a
        // press that turns into something else closes nothing.
        .on_mouse_up(MouseButton::Middle, move |_, window, cx| {
            cx.stop_propagation();
            window.dispatch_action(Box::new(CloseTabById { id }), cx);
        })
        .child(label)
        .child(
            div()
                .id(("tab-close", id))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .text_xs()
                .text_color(muted)
                // Hidden rather than absent: the tab keeps its width, so
                // the strip does not shuffle under the pointer.
                .invisible()
                .group_hover(GROUP, |style| style.visible())
                .hover(|style| style.bg(hover).text_color(fg))
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    // Before the tab's own click, which would select the
                    // tab first.
                    cx.stop_propagation();
                    window.dispatch_action(Box::new(CloseTabById { id }), cx);
                })
                .child("\u{2715}"),
        )
}

/// Closing tabs end to end, on the real `Studio` (see `crate::harness`).
#[cfg(test)]
mod driven {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use gpui::{AnyWindowHandle, AppContext as _, Entity, Focusable as _};

    use super::CloseTabById;
    use crate::harness::{Harness, scratch_project};
    use crate::{CloseTab, FocusEditor, Studio};

    const FIXTURE: &str = "tests/tier1-native/conventions-cross-file";

    /// A window with its entry open in a tab, the editor focused. Answers
    /// the window, the studio, the project root and the open file.
    fn with_a_tab(h: &mut Harness) -> (AnyWindowHandle, Entity<Studio>, PathBuf, String) {
        let root = scratch_project(FIXTURE);
        let window = h.open(&root);
        h.dispatch(window, FocusEditor);
        let studio = h.studio(window).expect("the window just opened");
        let path = h.read(|cx| studio.read(cx).code.read(cx).active_path(cx));
        let path = path.expect("the entry is open in a tab");
        (window, studio, root, path)
    }

    fn open_paths(h: &mut Harness, studio: &Entity<Studio>) -> Vec<String> {
        h.read(|cx| studio.read(cx).code.read(cx).open_paths(cx))
    }

    fn make_dirty(h: &mut Harness, studio: &Entity<Studio>, path: &str) {
        h.update(|cx| {
            let project = studio.read(cx).project.clone();
            project.update(cx, |project, cx| {
                let text = format!(
                    "{}\n// an unsaved line\n",
                    project.loaded_source(path).unwrap_or_default()
                );
                assert!(project.edit(path, text, None, cx), "the edit took");
            });
        });
    }

    fn on_disk(root: &Path, path: &str) -> String {
        std::fs::read_to_string(root.join(path)).expect("the scratch file exists")
    }

    #[test]
    fn cmd_w_closes_a_clean_tab_without_asking_and_keys_still_work() {
        let mut h = Harness::new();
        let (window, studio, _, path) = with_a_tab(&mut h);
        h.press(window, "cmd-w");
        assert_eq!(h.prompt(), None, "nothing to ask about");
        assert!(
            !open_paths(&mut h, &studio).contains(&path),
            "{path} closed"
        );
        // The editor that held the keyboard is gone; focus must land
        // somewhere still drawn, or every shortcut goes dead until a click.
        let code = h.read(|cx| studio.read(cx).code.clone());
        let held = h
            .update(|cx| {
                window.update(cx, |_, window, cx| {
                    code.focus_handle(cx).contains_focused(window, cx)
                })
            })
            .expect("the window is open");
        assert!(held, "focus stays in the Code view after the close");
    }

    #[test]
    fn the_tab_s_own_close_names_its_document() {
        let mut h = Harness::new();
        let (window, studio, _, path) = with_a_tab(&mut h);
        let id = h.read(|cx| {
            studio
                .read(cx)
                .code
                .read(cx)
                .active_document()
                .map(Entity::entity_id)
        });
        let id = id.expect("a document is active");
        h.dispatch(window, CloseTabById { id });
        assert!(!open_paths(&mut h, &studio).contains(&path));
    }

    #[test]
    fn a_dirty_tab_asks_and_cancel_keeps_it() {
        let mut h = Harness::new();
        let (window, studio, root, path) = with_a_tab(&mut h);
        let before = on_disk(&root, &path);
        make_dirty(&mut h, &studio, &path);
        h.dispatch(window, CloseTab);
        let prompt = h.prompt().expect("closing a dirty tab asks");
        assert!(prompt.message.contains(&path), "{prompt:?}");
        assert_eq!(prompt.buttons, ["Save", "Don't Save", "Cancel"]);
        h.answer("Cancel");
        assert!(open_paths(&mut h, &studio).contains(&path), "still open");
        assert!(h.read(|cx| studio.read(cx).project.read(cx).is_dirty(&path)));
        assert_eq!(on_disk(&root, &path), before, "nothing written");
        // Answered, so the next close asks again.
        h.dispatch(window, CloseTab);
        assert!(h.prompt().is_some());
    }

    #[test]
    fn dont_save_reverts_the_buffer_and_closes() {
        let mut h = Harness::new();
        let (window, studio, root, path) = with_a_tab(&mut h);
        let before = on_disk(&root, &path);
        make_dirty(&mut h, &studio, &path);
        h.dispatch(window, CloseTab);
        h.answer("Don't Save");
        assert!(!open_paths(&mut h, &studio).contains(&path), "closed");
        assert_eq!(on_disk(&root, &path), before, "the disk is untouched");
        let (dirty, text) = h.read(|cx| {
            let project = studio.read(cx).project.read(cx);
            (
                project.is_dirty(&path),
                project.loaded_source(&path).map(str::to_owned),
            )
        });
        // The buffer is shared with the manuscript, so declining the edits
        // has to take them out of it too.
        assert!(!dirty, "the buffer went back to the disk");
        assert_eq!(text.as_deref(), Some(before.as_str()));
        // And the window is free for its own prompts again.
        assert!(!h.read(|cx| studio.read(cx).close.asking));
    }

    #[test]
    fn save_writes_the_file_then_closes() {
        let mut h = Harness::new();
        let (window, studio, root, path) = with_a_tab(&mut h);
        make_dirty(&mut h, &studio, &path);
        h.dispatch(window, CloseTab);
        h.answer("Save");
        assert!(
            h.settle_until(Duration::from_secs(5), |h| !open_paths(h, &studio)
                .contains(&path)),
            "the tab closes once the save lands"
        );
        assert!(on_disk(&root, &path).contains("// an unsaved line"));
    }

    /// The menu bar is generated from the registry: every command the
    /// window registered is in it, the ☰'s replacement included.
    #[test]
    fn the_installed_menu_bar_holds_every_command() {
        let mut h = Harness::new();
        let (window, studio, _, _) = with_a_tab(&mut h);
        let _ = window;
        let (titles, missing) = h.read(|cx| {
            let menus = gpui_component::GlobalState::global(cx).app_menus().to_vec();
            let mut actions = Vec::new();
            fn walk(items: &[gpui::OwnedMenuItem], out: &mut Vec<Box<dyn gpui::Action>>) {
                for item in items {
                    match item {
                        gpui::OwnedMenuItem::Action { action, .. } => {
                            out.push(action.boxed_clone());
                        }
                        gpui::OwnedMenuItem::Submenu(menu) => walk(&menu.items, out),
                        _ => {}
                    }
                }
            }
            for menu in &menus {
                walk(&menu.items, &mut actions);
            }
            let workspace = studio.read(cx).workspace.read(cx);
            let missing: Vec<String> = workspace
                .commands()
                .commands()
                .iter()
                .filter(|c| !actions.iter().any(|a| a.partial_eq(c.action.as_ref())))
                .map(brink_gpui_shell::commands::Command::full_title)
                .collect();
            let titles: Vec<String> = menus.iter().map(|m| m.name.to_string()).collect();
            (titles, missing)
        });
        assert!(titles.contains(&"File".to_owned()), "{titles:?}");
        assert!(missing.is_empty(), "not in the menu bar: {missing:?}");
    }
}
