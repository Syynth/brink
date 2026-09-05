//! What every Project section shares: reading `brink.toml` out of the
//! mirror, writing it back through the shared buffer, and the notice for a
//! project that has no config to write to.
//!
//! A section never rewrites the file. It parses the current text into a
//! `ConfigDocument`, changes the one key it was asked to, and writes the
//! result through `Project::edit` — so an open `brink.toml` tab, the Binder
//! and the analysis all follow, and the author's comments survive
//! (`brink-project-config`'s `edit` module is the round-trip seam).

use brink_project_config::edit::{ConfigDocument, EditError};
use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, div};
use gpui_component::{ActiveTheme as _, v_flex};

use crate::project::Project;

/// The config's path and current text, or `None` for a project without one.
pub fn config_text(project: &Entity<Project>, cx: &App) -> Option<(String, String)> {
    let project = project.read(cx);
    let path = project.config_path()?.to_owned();
    let text = project.loaded_source(&path)?.to_owned();
    Some((path, text))
}

/// Parse the config, let `f` change it, and write it back if it changed.
/// `f` reports whether it changed anything, so a no-op (removing a key
/// that was never there) writes nothing and echoes nothing.
pub fn edit_config(
    project: &Entity<Project>,
    cx: &mut App,
    f: impl FnOnce(&mut ConfigDocument) -> Result<bool, EditError>,
) {
    let Some((path, text)) = config_text(project, cx) else {
        return;
    };
    let mut doc = match ConfigDocument::parse(&text) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("{path}: {err}");
            return;
        }
    };
    match f(&mut doc) {
        Ok(true) => {
            let next = doc.to_toml_string();
            if next != text {
                project.update(cx, |project, cx| {
                    project.edit(&path, next, None, cx);
                });
            }
        }
        Ok(false) => {}
        Err(err) => eprintln!("{path}: {err}"),
    }
}

/// Set `table.key` to `value`, or remove it for `None`. Reports whether the
/// document changed, for [`edit_config`].
pub fn set_or_remove(
    doc: &mut ConfigDocument,
    table: &str,
    key: &str,
    value: Option<&str>,
) -> Result<bool, EditError> {
    match value {
        Some(value) => {
            doc.set_string(table, key, value)?;
            Ok(true)
        }
        None => doc.remove_key(table, key),
    }
}

/// The section body for a project with no `brink.toml`: says so, and what
/// would be written where.
pub fn no_config(what: &str, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "This project has no brink.toml, so there is nothing to write {what} to yet. Add one beside the entry file and reopen the project."
                )),
        )
        .into_any_element()
}

/// Whether the parsed config can be read at all — the sections show the
/// reason and stop, since a form over a text that does not parse would be
/// a form over nothing.
pub fn parse_error(text: &str) -> Option<String> {
    ConfigDocument::parse(text).err().map(|e| e.to_string())
}

/// The line a section shows when the file does not parse.
pub fn broken_notice(reason: &str, cx: &App) -> AnyElement {
    div()
        .pb_1()
        .text_xs()
        .text_color(cx.theme().danger)
        .child(format!("The form is off until brink.toml parses: {reason}"))
        .into_any_element()
}
