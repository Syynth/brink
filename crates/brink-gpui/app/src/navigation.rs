//! Editor navigation — go to definition, references, rename — over any
//! `EditorState` holding a brink file, wherever it is hosted.
//!
//! INVENTORY §0 item 1. Two hosts show a file's editor: a `Document` tab in
//! Code view, and a section of the manuscript in Continuous view. Both must
//! navigate the same way, and the only thing that differs between them is
//! **where a target is shown** — a tab opens, a manuscript scrolls. So the
//! host hands over one callback, [`Navigate`], and everything else here is
//! shared. A feature that worked only in Code view would be the two-
//! implementations split the layering ruling exists to prevent, one layer
//! up.
//!
//! Positions cross the worker boundary in bytes; the LSP-shaped provider
//! traits want line/column. The mapping happens here, once, against the
//! text the UI already holds.

use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use brink_gpui_model::query::{Location, QueryKind, QueryResult, Reference, RenamePlan};
use brink_ir::LineIndex;
use gpui::{App, AppContext as _, Entity, EntityId, SharedString, Task, WeakEntity, Window};
use gpui_component::input::{DefinitionProvider, EditorState, Rope, ShowDocumentHandler};
use lsp_types as lsp;

use crate::document::{position, seed_edit};
use crate::project::Project;

/// How a host shows a target: `(path, byte span)`.
pub type Navigate = Rc<dyn Fn(&str, Range<usize>, &mut Window, &mut App)>;

/// The offset a command acts on. With a selection that is its START, not
/// the caret: after a jump the target's name is selected and the caret
/// sits one past it, where nothing resolves — so F12-then-Shift-F12 found
/// no references and said nothing.
fn caret(state: &EditorState) -> usize {
    let selection = state.selected_range();
    if selection.is_empty() {
        state.cursor()
    } else {
        selection.start
    }
}

/// One editor over one file — what a keyboard command resolves to before
/// asking anything of the worker. Showing a *target* is not part of it:
/// that is the `show_document` hook installed on the editor itself, so
/// go-to-definition needs no site at all.
#[derive(Clone)]
pub struct EditorSite {
    pub editor: Entity<EditorState>,
    pub project: Entity<Project>,
    pub path: SharedString,
}

/// Install the definition provider and the `show_document` hook on an
/// editor. Hover and completion are installed by the host alongside, since
/// they predate this module; the three together are what "a brink editor"
/// means.
pub(crate) fn install(
    state: &mut EditorState,
    project: &Entity<Project>,
    path: SharedString,
    origin: EntityId,
    navigate: Navigate,
) {
    let lsp = state.lsp_mut();
    lsp.definition_provider = Some(Rc::new(BrinkDefinition {
        project: project.downgrade(),
        path: path.clone(),
        origin,
    }));
    // Fixes and refactors — the code-action menu (`cmd-.`).
    lsp.code_action_providers
        .push(Rc::new(crate::fixes::BrinkCodeActions::new(
            project.downgrade(),
            path,
            origin,
        )));
    // gpui-base consults this BEFORE its own fallback, which is `open_url`
    // for an http(s) target and a same-buffer caret move otherwise —
    // neither of which can open another file. Returning `true` claims the
    // navigation.
    let handler: ShowDocumentHandler = Rc::new(move |params, window, cx| {
        let Some((path, span)) = parse_target(&params.uri) else {
            return false;
        };
        navigate(&path, span, window, cx);
        true
    });
    lsp.show_document = Some(handler);
}

// ── Target URIs ──────────────────────────────────────────────────────
//
// A `LocationLink` names its target by URI. `lsp_types` 0.97 carries
// `fluent_uri`, not `url`, so there are no file-path helpers: the URI is
// built and read here, under a private `brink:` scheme. That scheme is
// also what keeps gpui-base's own fallback out of the way — it treats only
// http(s) as external, and consults the `show_document` hook first for
// everything else. The path is percent-encoded; the byte span rides in the
// query so the hook does not have to re-derive it from a line/column
// against the target's text.

const SCHEME: &str = "brink:///";

fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = encoded.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn target_uri(loc: &Location) -> Option<lsp::Uri> {
    format!(
        "{SCHEME}{}?s={}&e={}",
        encode(&loc.path),
        loc.start,
        loc.end
    )
    .parse()
    .ok()
}

fn parse_target(uri: &lsp::Uri) -> Option<(String, Range<usize>)> {
    let text = uri.as_str();
    let rest = text.strip_prefix(SCHEME)?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let path = decode(path)?;
    let mut start = 0;
    let mut end = 0;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("s", v)) => start = v.parse().ok()?,
            Some(("e", v)) => end = v.parse().ok()?,
            _ => {}
        }
    }
    Some((path, start..end))
}

/// A `LocationLink` for a target, with line/column computed against the
/// text the project holds for that file.
fn location_link(source: &str, loc: &Location) -> Option<lsp::LocationLink> {
    let index = LineIndex::new(source);
    let range = lsp::Range {
        start: position(&index, loc.start),
        end: position(&index, loc.end),
    };
    Some(lsp::LocationLink {
        origin_selection_range: None,
        target_uri: target_uri(loc)?,
        target_range: range,
        target_selection_range: range,
    })
}

// ── Definition ───────────────────────────────────────────────────────

struct BrinkDefinition {
    project: WeakEntity<Project>,
    path: SharedString,
    origin: EntityId,
}

impl DefinitionProvider for BrinkDefinition {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<lsp::LocationLink>>> {
        let Some(project) = self.project.upgrade() else {
            return Task::ready(Ok(Vec::new()));
        };
        seed_edit(&project, &self.path, text, self.origin, cx);
        let query = project.read(cx).query(
            QueryKind::Definition {
                path: self.path.to_string(),
                offset: u32::try_from(offset).unwrap_or(u32::MAX),
            },
            cx,
        );
        cx.spawn(async move |cx| {
            let QueryResult::Definition(Some(loc)) = query.await? else {
                return Ok(Vec::new());
            };
            // The target's text lives on the project, so the link is built
            // back on the main thread.
            let link: Option<lsp::LocationLink> = project.read_with(cx, |project, _| {
                project
                    .loaded_source(&loc.path)
                    .and_then(|source| location_link(source, &loc))
            });
            Ok(link.into_iter().collect())
        })
    }
}

/// The definition of whatever is under the caret, for the host to show.
///
/// gpui-base's own `GoToDefinition` action only follows a target the
/// editor has ALREADY resolved by a Cmd-hover (`on_action_go_to_definition`
/// reads `hover_definition.last_location` and does nothing otherwise), so
/// a cold F12 through it is a no-op. This asks the worker directly.
pub fn definition(site: &EditorSite, cx: &mut App) -> Task<Option<Location>> {
    let (text, offset) = {
        let state = site.editor.read(cx);
        (state.text().clone(), caret(state))
    };
    seed_edit(
        &site.project,
        &site.path,
        &text,
        site.editor.entity_id(),
        cx,
    );
    let query = site.project.read(cx).query(
        QueryKind::Definition {
            path: site.path.to_string(),
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
        },
        cx,
    );
    cx.background_spawn(async move {
        match query.await {
            Ok(QueryResult::Definition(loc)) => loc,
            _ => None,
        }
    })
}

// ── References ───────────────────────────────────────────────────────

/// Every use of the symbol under the caret, with the symbol's name — the
/// name is what the Search panel titles the list with.
pub fn find_references(site: &EditorSite, cx: &mut App) -> Task<Option<(String, Vec<Reference>)>> {
    let (text, offset) = {
        let state = site.editor.read(cx);
        (state.text().clone(), caret(state))
    };
    seed_edit(
        &site.project,
        &site.path,
        &text,
        site.editor.entity_id(),
        cx,
    );
    let source = text.to_string();
    let path = site.path.to_string();
    let query = site.project.read(cx).query(
        QueryKind::References {
            path: path.clone(),
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
            include_declaration: true,
        },
        cx,
    );
    cx.background_spawn(async move {
        let Ok(QueryResult::References(refs)) = query.await else {
            return None;
        };
        if refs.is_empty() {
            return None;
        }
        // The symbol's name is the text of the site the caret is on — the
        // one reference in this file covering the offset — or, failing
        // that, of the first one here.
        let name = refs
            .iter()
            .filter(|r| r.location.path == path)
            .find(|r| (r.location.start as usize..=r.location.end as usize).contains(&offset))
            .or_else(|| refs.iter().find(|r| r.location.path == path))
            .and_then(|r| source.get(r.location.start as usize..r.location.end as usize))
            .unwrap_or("symbol")
            .to_owned();
        Some((name, refs))
    })
}

// ── Rename ───────────────────────────────────────────────────────────

/// Whether the symbol under the caret can be renamed: its range and its
/// current name, for the prompt to seed from.
pub fn prepare_rename(site: &EditorSite, cx: &mut App) -> Task<Option<(Range<usize>, String)>> {
    let (text, offset) = {
        let state = site.editor.read(cx);
        (state.text().clone(), caret(state))
    };
    seed_edit(
        &site.project,
        &site.path,
        &text,
        site.editor.entity_id(),
        cx,
    );
    let source = text.to_string();
    let query = site.project.read(cx).query(
        QueryKind::PrepareRename {
            path: site.path.to_string(),
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
        },
        cx,
    );
    cx.background_spawn(async move {
        let Ok(QueryResult::PrepareRename(Some((start, end)))) = query.await else {
            return None;
        };
        let range = start as usize..end as usize;
        let name = source.get(range.clone())?.to_owned();
        Some((range, name))
    })
}

/// The rename plan for the symbol at `offset` — computed and gated, not
/// applied. Applying is [`Project::apply_edits`], the host's act.
pub fn rename(
    site: &EditorSite,
    offset: usize,
    new_name: String,
    cx: &mut App,
) -> Task<Option<RenamePlan>> {
    let query = site.project.read(cx).query(
        QueryKind::Rename {
            path: site.path.to_string(),
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
            new_name,
        },
        cx,
    );
    cx.background_spawn(async move {
        match query.await {
            Ok(QueryResult::Rename(plan)) => plan,
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_round_trips_through_its_uri_with_its_span() {
        let loc = Location {
            path: "scenes/act one — \u{e9}.ink".to_owned(),
            start: 12,
            end: 17,
        };
        let uri = target_uri(&loc).expect("a uri");
        assert!(
            uri.as_str().starts_with("brink:///scenes/act%20one%20"),
            "{uri:?}"
        );
        assert!(
            uri.scheme().is_some_and(|s| s.as_str() == "brink"),
            "not http(s), so gpui-base consults the show_document hook"
        );
        assert_eq!(
            parse_target(&uri),
            Some((loc.path.clone(), 12..17)),
            "the path decodes and the span survives the query"
        );
    }

    #[test]
    fn a_foreign_uri_is_not_a_target() {
        let http: lsp::Uri = "https://example.com/x?s=0&e=1".parse().expect("a uri");
        assert_eq!(parse_target(&http), None);
        let file: lsp::Uri = "file:///tmp/x.ink".parse().expect("a uri");
        assert_eq!(parse_target(&file), None);
    }
}
