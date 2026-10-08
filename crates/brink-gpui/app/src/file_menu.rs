//! The file and folder menu — one list, built here, for the Binder's file
//! and folder rows (right-click and `⋯`) and the manuscript's file header.
//! The web studio's `BinderContextMenu` is the reference: what it offers,
//! in its order, and what it disables rather than hides.
//!
//! A file: New file here… · New folder here… · Rename… · Delete…. A folder:
//! New file here… · New folder here… · Rename folder… · Delete folder (N)…,
//! the last two disabled while it holds no files. A library file or folder
//! is not the author's: none of it applies.
//!
//! Surfaces put their own items first — the Binder's Open and New Knot…,
//! the manuscript's Play and Open in Script — and this list after them.

use gpui::{App, ClickEvent, Window};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::binder::BinderEvent;
use crate::symbol_menu::Emit;

/// What the menu was opened on.
#[derive(Clone, Debug)]
pub(crate) enum Target {
    File {
        path: String,
    },
    /// `path` without its trailing slash; `files` everything under it.
    Folder {
        path: String,
        files: Vec<String>,
    },
}

/// One entry, before it is drawn.
#[derive(Clone, Debug)]
pub(crate) enum Item {
    Action {
        label: String,
        event: BinderEvent,
        disabled: bool,
    },
    Separator,
}

/// The folder a file sits in, root-relative and possibly empty.
fn folder_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(folder, _)| folder.to_owned())
        .unwrap_or_default()
}

impl Target {
    /// What this target's menu offers, in order.
    pub(crate) fn items(&self) -> Vec<Item> {
        let action = |label: String, event: BinderEvent, disabled: bool| Item::Action {
            label,
            event,
            disabled,
        };
        let here = match self {
            Self::File { path } => folder_of(path),
            Self::Folder { path, .. } => path.clone(),
        };
        let mut items = vec![
            action(
                "New file here\u{2026}".to_owned(),
                BinderEvent::NewFile {
                    folder: here.clone(),
                },
                false,
            ),
            action(
                "New folder here\u{2026}".to_owned(),
                BinderEvent::NewFolder { folder: here },
                false,
            ),
        ];
        match self {
            Self::File { path } => {
                items.push(action(
                    "Rename\u{2026}".to_owned(),
                    BinderEvent::RenameFile { path: path.clone() },
                    false,
                ));
                items.push(Item::Separator);
                items.push(action(
                    "Delete\u{2026}".to_owned(),
                    BinderEvent::DeleteFile {
                        paths: vec![path.clone()],
                    },
                    false,
                ));
            }
            Self::Folder { path, files } => {
                let empty = files.is_empty();
                items.push(action(
                    "Rename folder\u{2026}".to_owned(),
                    BinderEvent::RenameFolder {
                        folder: path.clone(),
                    },
                    empty,
                ));
                items.push(Item::Separator);
                items.push(action(
                    format!("Delete folder ({})\u{2026}", files.len()),
                    BinderEvent::DeleteFolder {
                        folder: path.clone(),
                        paths: files.clone(),
                    },
                    empty,
                ));
            }
        }
        items
    }
}

/// Add `target`'s items to `menu`.
pub(crate) fn build(mut menu: PopupMenu, target: &Target, emit: &Emit) -> PopupMenu {
    for item in target.items() {
        menu = match item {
            Item::Separator => menu.separator(),
            Item::Action {
                label,
                event,
                disabled,
            } => {
                let emit = emit.clone();
                menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        emit(event.clone(), window, cx);
                    },
                ))
            }
        };
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(target: &Target) -> Vec<String> {
        target
            .items()
            .iter()
            .map(|item| match item {
                Item::Action {
                    label, disabled, ..
                } if *disabled => format!("({label})"),
                Item::Action { label, .. } => label.clone(),
                Item::Separator => "─".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_files_menu_makes_things_beside_it() {
        let target = Target::File {
            path: "acts/one.ink".to_owned(),
        };
        assert_eq!(
            labels(&target),
            [
                "New file here…",
                "New folder here…",
                "Rename…",
                "─",
                "Delete…"
            ]
        );
        assert!(
            matches!(
                target.items().first(),
                Some(Item::Action {
                    event: BinderEvent::NewFile { folder },
                    ..
                }) if folder == "acts"
            ),
            "beside the file, in its folder"
        );
    }

    #[test]
    fn a_folders_menu_makes_things_inside_it_and_counts_its_files() {
        let target = Target::Folder {
            path: "acts".to_owned(),
            files: vec!["acts/one.ink".to_owned(), "acts/two.ink".to_owned()],
        };
        assert_eq!(
            labels(&target),
            [
                "New file here…",
                "New folder here…",
                "Rename folder…",
                "─",
                "Delete folder (2)…"
            ]
        );
        assert!(
            matches!(
                target.items().first(),
                Some(Item::Action {
                    event: BinderEvent::NewFile { folder },
                    ..
                }) if folder == "acts"
            ),
            "inside the folder, not beside it"
        );
    }

    #[test]
    fn an_empty_folder_cannot_be_renamed_or_deleted() {
        let target = Target::Folder {
            path: "drafts".to_owned(),
            files: Vec::new(),
        };
        assert_eq!(
            labels(&target),
            [
                "New file here…",
                "New folder here…",
                "(Rename folder…)",
                "─",
                "(Delete folder (0)…)"
            ]
        );
    }
}
