//! Creating a knot or a stitch from the Binder.
//!
//! The one structural creation the Binder was missing: a file row makes a
//! knot at the end of its file, a knot or stitch row makes a stitch at the
//! end of the knot it belongs to. Both are ordinary TEXT edits through the
//! shared buffer (`Project::edit`) — the same road a fix or a rename takes
//! — so undo, the dirty marker and the next analysis all behave as they do
//! for anything the author typed.
//!
//! What this deliberately does not do: choose where in the file a knot
//! goes. A knot appended at the end is where ink itself puts the next one
//! and where an author reading top-to-bottom expects to find it; anything
//! cleverer (beside the selection, in the Binder's authored order) would
//! be guessing at an arrangement the sidecar already owns.

use std::ops::Range;
use std::rc::Rc;

use gpui::{App, AppContext as _, Entity, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::InputState;

use crate::files::prompt;
use crate::project::Project;
use brink_gpui_shell::notify::{Severity, notify};

/// What a knot or stitch may be called: ink's own identifier shape. A name
/// that is not one is refused with the reason rather than written into the
/// file, where it would come back as a parse error the author did not ask
/// for.
#[must_use]
pub fn name_error(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("A name is required.".to_owned());
    }
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Some(format!("`{name}` starts with a digit."));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some(format!(
            "`{name}` has a character a knot name cannot hold — letters, digits and `_` only."
        ));
    }
    None
}

/// What a new knot writes and what it writes over: the end of the file,
/// separated from whatever precedes it by exactly one blank line.
///
/// The range REPLACES the trailing blank space rather than inserting
/// before it — a file that already ended in three blank lines would
/// otherwise grow them, and the separation an author sees would depend on
/// how the file happened to end.
#[must_use]
pub fn knot_insertion(source: &str, name: &str) -> (Range<usize>, String) {
    let trimmed = source.trim_end_matches(['\n', '\r', ' ', '\t']);
    let lead = if trimmed.is_empty() { "" } else { "\n\n" };
    (
        trimmed.len()..source.len(),
        format!("{lead}=== {name} ===\n\n"),
    )
}

/// The text a new stitch adds, and where it goes: the end of the knot that
/// owns it (`full_end`), on its own line and separated by a blank one.
///
/// `full_end` is where the knot's own content stops, which is the line the
/// NEXT knot header starts on — so the offset is walked back over trailing
/// blank lines first. Otherwise a stitch would be written under the
/// following knot's header and belong to it instead.
#[must_use]
pub fn stitch_insertion(source: &str, full_end: usize, name: &str) -> (Range<usize>, String) {
    let until = full_end.min(source.len());
    let at = source[..until]
        .trim_end_matches(['\n', '\r', ' ', '\t'])
        .len();
    let lead = if at == 0 { "" } else { "\n\n" };
    (at..until, format!("{lead}= {name}\n\n"))
}

/// `source` with `range` replaced by `text`.
#[must_use]
fn splice(source: &str, range: Range<usize>, text: &str) -> String {
    let start = range.start.min(source.len());
    let end = range.end.clamp(start, source.len());
    format!("{}{text}{}", &source[..start], &source[end..])
}

/// How the studio reveals what was just written — the panel cannot open a
/// document itself, and the studio should not have to know how a knot's
/// text was assembled.
pub type Reveal = Rc<dyn Fn(&str, usize, &mut Window, &mut App)>;

/// Ask for a knot name and write it at the end of `path`.
pub fn new_knot(
    project: Entity<Project>,
    path: String,
    reveal: Reveal,
    window: &mut Window,
    cx: &mut App,
) {
    create(project, path, None, "New knot", reveal, window, cx);
}

/// Ask for a stitch name and write it at the end of the knot ending at
/// `full_end` in `path`.
pub fn new_stitch(
    project: Entity<Project>,
    path: String,
    full_end: usize,
    reveal: Reveal,
    window: &mut Window,
    cx: &mut App,
) {
    create(
        project,
        path,
        Some(full_end),
        "New stitch",
        reveal,
        window,
        cx,
    );
}

/// The shared body: prompt, validate, splice, edit, reveal.
fn create(
    project: Entity<Project>,
    path: String,
    full_end: Option<usize>,
    title: &'static str,
    reveal: Reveal,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("name"));
    let confirm = Rc::new({
        let project = project.clone();
        let input = input.clone();
        let path = path.clone();
        let reveal = reveal.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            window.close_dialog(cx);
            if name.is_empty() {
                return;
            }
            if let Some(why) = name_error(&name) {
                notify(Severity::Error, "binder", why, window, cx);
                return;
            }
            let Some(source) = project.read(cx).loaded_source(&path).map(str::to_owned) else {
                notify(
                    Severity::Error,
                    "binder",
                    format!("{path} is not open in this project."),
                    window,
                    cx,
                );
                return;
            };
            let (range, text) = match full_end {
                Some(end) => stitch_insertion(&source, end, &name),
                None => knot_insertion(&source, &name),
            };
            // Where the header itself starts, so the caret lands on the new
            // knot rather than on the blank line that separates it.
            let at_header = range.start + text.len() - text.trim_start_matches('\n').len();
            let next = splice(&source, range, &text);
            let wrote = project.update(cx, |project, cx| project.edit(&path, next, None, cx));
            if wrote {
                // After the edit has reached the editors, not before: the
                // mirror broadcasts its change as an event, and revealing
                // an offset in text the editor has not applied yet lands
                // the caret wherever the OLD text put that byte.
                let reveal = reveal.clone();
                let path = path.clone();
                window.defer(cx, move |window, cx| {
                    reveal(&path, at_header, window, cx);
                });
            }
        }
    });
    prompt(title, "Create", input, confirm, window, cx);
}

#[cfg(test)]
mod tests {
    use super::{knot_insertion, name_error, splice, stitch_insertion};

    #[test]
    fn a_knot_is_appended_with_one_blank_line_before_it() {
        let source = "=== shore ===\nThe tide was out.\n";
        let (range, text) = knot_insertion(source, "lighthouse");
        let out = splice(source, range, &text);
        assert_eq!(
            out,
            "=== shore ===\nThe tide was out.\n\n=== lighthouse ===\n\n"
        );
        // However many blank lines the file ended with, exactly one
        // separates the new knot — not none, and not four.
        let padded = "=== shore ===\nText.\n\n\n\n";
        let (range, text) = knot_insertion(padded, "lighthouse");
        assert_eq!(
            splice(padded, range, &text),
            "=== shore ===\nText.\n\n=== lighthouse ===\n\n"
        );
    }

    #[test]
    fn the_first_knot_in_an_empty_file_gets_no_leading_blank_line() {
        let (range, text) = knot_insertion("", "start");
        assert_eq!((range, text.as_str()), (0..0, "=== start ===\n\n"));
        let (range, text) = knot_insertion("\n\n", "start");
        assert_eq!(splice("\n\n", range, &text), "=== start ===\n\n");
    }

    #[test]
    fn a_stitch_lands_at_the_end_of_its_own_knot_not_under_the_next_header() {
        // `full_end` is where the knot's content stops, which is the line
        // the NEXT header starts on — the trailing blank must be walked
        // back over or the stitch belongs to `lighthouse`.
        let source = "=== shore ===\nThe tide.\n\n=== lighthouse ===\nThe door.\n";
        let full_end = source.find("=== lighthouse").expect("the second knot");
        let (range, text) = stitch_insertion(source, full_end, "linger");
        let out = splice(source, range, &text);
        assert_eq!(
            out,
            "=== shore ===\nThe tide.\n\n= linger\n\n=== lighthouse ===\nThe door.\n"
        );
        assert!(
            out.find("= linger").expect("written") < out.find("=== lighthouse").expect("kept"),
            "the stitch stays inside its own knot"
        );
    }

    #[test]
    fn a_stitch_at_the_end_of_the_file_needs_no_next_header() {
        let source = "=== shore ===\nThe tide.\n";
        let (range, text) = stitch_insertion(source, source.len(), "linger");
        assert_eq!(
            splice(source, range, &text),
            "=== shore ===\nThe tide.\n\n= linger\n\n"
        );
    }

    #[test]
    fn a_name_that_is_not_an_identifier_is_refused_with_the_reason() {
        assert!(name_error("lighthouse").is_none());
        assert!(name_error("way_out_2").is_none());
        assert!(name_error("").is_some());
        assert!(name_error("2nd_floor").is_some_and(|m| m.contains("digit")));
        assert!(name_error("the door").is_some_and(|m| m.contains("letters")));
        assert!(name_error("shore.linger").is_some());
    }
}
