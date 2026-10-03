//! Unsaved work on the way out (decision log 2026-10-03, "The native
//! studio asks before unsaved work is lost").
//!
//! Closing a project window, or quitting, while a file is dirty asks
//! **Save / Don't Save / Cancel**. This module is the part of that with no
//! window in it — what the prompt says and what each answer means — so it
//! can be tested without one. The asking itself lives on `Studio`, which
//! owns the window and the project.
//!
//! What it does NOT cover, stated rather than implied: macOS Dock Quit.
//! gpui's macOS backend only implements `applicationWillTerminate`, which
//! runs after the quit is decided, so nothing here is asked first — the
//! same hole the Tauri shell has (#2400).

use gpui::PromptButton;

/// What the author answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Write every dirty file, then close. Stays open if a write fails.
    Save,
    /// Close without writing; the edits are gone.
    Discard,
    /// Do nothing: the window stays, the edits stay unsaved.
    Cancel,
}

/// The buttons, in the order [`choice`] reads them back. `Cancel` is a
/// cancel button so Escape answers it.
pub fn answers() -> [PromptButton; 3] {
    [
        PromptButton::ok("Save"),
        PromptButton::new("Don't Save"),
        PromptButton::cancel("Cancel"),
    ]
}

/// The answer behind the index the prompt returns. Anything else — a
/// prompt dismissed some way that names no button — is a Cancel: the one
/// reading that can never lose work.
pub fn choice(index: usize) -> Choice {
    match index {
        0 => Choice::Save,
        1 => Choice::Discard,
        _ => Choice::Cancel,
    }
}

/// How many dirty paths the detail names before it summarizes the rest.
const LISTED: usize = 8;

/// The prompt's message and detail for `dirty` (root-relative paths) in
/// the project called `project`. The detail names the files, because
/// "save your changes?" means nothing until you know which ones.
pub fn prompt_text(project: &str, dirty: &[String]) -> (String, String) {
    let message = match dirty {
        [one] => format!("Save the changes to {one} in {project}?"),
        _ => format!("Save the changes to {} files in {project}?", dirty.len()),
    };
    let mut lines: Vec<String> = dirty.iter().take(LISTED).cloned().collect();
    if dirty.len() > LISTED {
        lines.push(format!("and {} more", dirty.len() - LISTED));
    }
    let detail = format!(
        "{}\n\nIf you don't save, your changes will be lost.",
        lines.join("\n")
    );
    (message, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("ch{i}.ink")).collect()
    }

    #[test]
    fn one_file_is_named_in_the_question() {
        let (message, detail) = prompt_text("harbour", &paths(1));
        assert_eq!(message, "Save the changes to ch1.ink in harbour?");
        assert!(detail.starts_with("ch1.ink\n\n"), "{detail}");
    }

    #[test]
    fn several_files_are_counted_and_listed() {
        let (message, detail) = prompt_text("harbour", &paths(3));
        assert_eq!(message, "Save the changes to 3 files in harbour?");
        assert!(
            detail.starts_with("ch1.ink\nch2.ink\nch3.ink\n\n"),
            "{detail}"
        );
    }

    #[test]
    fn a_long_list_is_cut_and_the_rest_counted() {
        let (_, detail) = prompt_text("harbour", &paths(11));
        assert!(detail.contains("ch8.ink\nand 3 more"), "{detail}");
        assert!(!detail.contains("ch9.ink"), "{detail}");
    }

    #[test]
    fn answers_read_back_in_order_and_unknown_is_cancel() {
        assert!(answers()[2].is_cancel(), "Escape must answer Cancel");
        assert_eq!(choice(0), Choice::Save);
        assert_eq!(choice(1), Choice::Discard);
        assert_eq!(choice(2), Choice::Cancel);
        assert_eq!(choice(7), Choice::Cancel);
    }
}

/// The flows end to end, on the real `Studio` (see `crate::harness`).
#[cfg(test)]
mod driven {
    use std::time::Duration;

    use brink_gpui_shell::commands::CloseWindow;
    use gpui::AnyWindowHandle;

    use crate::Quit;
    use crate::harness::{Harness, scratch_project};

    const FIXTURE: &str = "tests/tier1-native/conventions-cross-file";
    const FILE: &str = "story.brink";

    /// Open a scratch copy of the fixture and make `story.brink` dirty
    /// through the project, as an editor would.
    fn dirty_window(h: &mut Harness) -> (AnyWindowHandle, std::path::PathBuf) {
        let root = scratch_project(FIXTURE);
        let window = h.open(&root);
        let studio = h.studio(window).expect("the window just opened");
        h.update(|cx| {
            let project = studio.read(cx).project.clone();
            project.update(cx, |project, cx| {
                let text = format!(
                    "{}\n// an unsaved line\n",
                    project.loaded_source(FILE).unwrap_or_default()
                );
                assert!(project.edit(FILE, text, None, cx), "the edit took");
            });
        });
        assert!(
            h.read(|cx| studio.read(cx).project.read(cx).is_dirty(FILE)),
            "the file is dirty before closing"
        );
        (window, root)
    }

    fn on_disk(root: &std::path::Path) -> String {
        std::fs::read_to_string(root.join(FILE)).expect("the scratch file exists")
    }

    #[test]
    fn a_clean_window_closes_without_asking() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(FIXTURE));
        h.dispatch(window, CloseWindow);
        assert_eq!(h.prompt(), None);
        assert!(!h.is_open(window));
    }

    #[test]
    fn cancel_keeps_the_window_and_the_edit() {
        let mut h = Harness::new();
        let (window, root) = dirty_window(&mut h);
        let before = on_disk(&root);
        h.dispatch(window, CloseWindow);
        let prompt = h.prompt().expect("closing a dirty window asks");
        assert!(prompt.message.contains(FILE), "{prompt:?}");
        assert_eq!(prompt.buttons, ["Save", "Don't Save", "Cancel"]);
        h.answer("Cancel");
        assert!(h.is_open(window));
        assert_eq!(on_disk(&root), before, "nothing written");
        // And a second close asks again rather than going straight through.
        h.dispatch(window, CloseWindow);
        assert!(h.prompt().is_some());
    }

    #[test]
    fn dont_save_closes_and_leaves_the_disk_alone() {
        let mut h = Harness::new();
        let (window, root) = dirty_window(&mut h);
        let before = on_disk(&root);
        h.dispatch(window, CloseWindow);
        h.answer("Don't Save");
        assert!(!h.is_open(window));
        assert_eq!(on_disk(&root), before);
    }

    #[test]
    fn save_writes_then_closes() {
        let mut h = Harness::new();
        let (window, root) = dirty_window(&mut h);
        h.dispatch(window, CloseWindow);
        h.answer("Save");
        assert!(
            h.settle_until(Duration::from_secs(5), |h| !h.is_open(window)),
            "the window closes once the save lands"
        );
        assert!(
            on_disk(&root).contains("// an unsaved line"),
            "the edit was written"
        );
    }

    #[test]
    fn a_save_that_cannot_write_keeps_the_window() {
        let mut h = Harness::new();
        let (window, root) = dirty_window(&mut h);
        let path = root.join(FILE);
        let mut perms = std::fs::metadata(&path).expect("exists").permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).expect("making the file read-only");
        h.dispatch(window, CloseWindow);
        h.answer("Save");
        assert!(h.is_open(window), "a failed save must not close");
        let studio = h.studio(window).expect("still open");
        assert!(h.read(|cx| studio.read(cx).project.read(cx).is_dirty(FILE)));
    }

    #[test]
    fn quit_asks_each_dirty_window_and_cancel_stops_it() {
        let mut h = Harness::new();
        let (first, _) = dirty_window(&mut h);
        let (second, _) = dirty_window(&mut h);
        h.dispatch(first, Quit);
        h.answer("Don't Save");
        assert!(h.prompt().is_some(), "the second window is asked too");
        h.answer("Cancel");
        for window in [first, second] {
            assert!(h.is_open(window));
            let studio = h.studio(window).expect("open");
            assert!(
                !h.read(|cx| studio.read(cx).close.confirmed),
                "a cancelled quit confirms nothing"
            );
        }
        // The quit is over, so a second cmd-q asks again from the start.
        h.dispatch(second, Quit);
        assert!(h.prompt().is_some());
    }

    #[test]
    fn quit_goes_ahead_once_every_window_is_answered() {
        let mut h = Harness::new();
        let (first, first_root) = dirty_window(&mut h);
        let (second, _) = dirty_window(&mut h);
        h.dispatch(first, Quit);
        h.answer("Save");
        assert!(
            h.settle_until(Duration::from_secs(5), |h| h.prompt().is_some()),
            "after the first save, the second window is asked"
        );
        h.answer("Don't Save");
        assert!(on_disk(&first_root).contains("// an unsaved line"));
        for window in [first, second] {
            let studio = h
                .studio(window)
                .expect("the test platform does not really quit");
            assert!(
                h.read(|cx| studio.read(cx).close.confirmed),
                "the quit went ahead"
            );
        }
    }
}
