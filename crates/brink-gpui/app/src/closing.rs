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
