//! The editor's right-click menu, ours in place of the kit's: the web
//! studio's (`docs/editor-context-menu-spec.md`, `EditorTextMenuHost.tsx`)
//! in the OS's own menu, the one the kit shows for an editor.
//!
//! On a knot's or stitch's header line it is the shared knot/stitch menu
//! ([`symbol_menu`]) with the line's breakpoint verb after it — that
//! line's gutter press plays, so the menu is where its breakpoint lives.
//! Anywhere else it is the text menu, in the web's four groups:
//!
//! 1. **Fixes** for the diagnostic under the pointer, each naming its tier,
//!    and "Fix all safe in this file" when there is at least one.
//! 2. **Identity**, when the word resolves to a definition: Go to
//!    Definition, Find References, and Rename '…'… when it can be renamed.
//! 3. **Context**: Open an `INCLUDE`'s file, Fold / Unfold, Show in TODOs
//!    Panel on a `TODO:` line.
//! 4. **Text**: Cut, Copy, Paste · Select All · Hide / Show Gutters.
//!
//! The groups need the worker's answers — what the word resolves to, which
//! fixes apply — so the kit is handed an empty menu (which it does not
//! show) and the real one opens at the pointer once they land, exactly the
//! web's "one landing later". The kit has already put the caret at the
//! click, so every item is an action that acts at the caret.

use brink_gpui_model::fixes::{FixPlan, Tier};
use brink_gpui_model::query::{QueryKind, QueryResult};
use brink_ir::SymbolKind;
use gpui::{App, Pixels, Point, Window};
use gpui_component::input::Editor;
use gpui_component::native_menu::NativeMenu;

use crate::binder::BinderEvent;
use crate::navigation::EditorSite;
use crate::symbol_menu;
use crate::todos::TODO_CODE;

/// Run a menu item that the Binder's vocabulary already names — the
/// shared knot/stitch menu's items, from the editor.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = editor_menu, no_json)]
pub struct Outline {
    pub event: BinderEvent,
}

/// Apply one offered fix.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = editor_menu, no_json)]
pub struct ApplyFix {
    pub plan: FixPlan,
}

/// Open a file by its project path — an `INCLUDE` line's target.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = editor_menu, no_json)]
pub struct OpenFile {
    pub path: String,
}

/// Break on writes to a global, or stop — the editor menu's and the State
/// view's "Break on write".
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = editor_menu, no_json)]
pub struct ToggleWatch {
    pub name: String,
}

/// Fold or unfold the region starting on a 0-based line of the focused
/// editor.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = editor_menu, no_json)]
pub struct ToggleFold {
    pub line: usize,
}

gpui::actions!(
    editor_menu,
    [
        /// Show the TODOs tool window.
        ShowTodos,
        /// Show or hide the editor's gutters — the Appearance setting.
        ToggleGutters,
    ]
);

/// Give `editor` our menu.
pub(crate) fn install(editor: Editor, site: EditorSite) -> Editor {
    editor.context_menu(move |_, window, cx| {
        // The kit calls this while it still holds the editor, and `open`
        // reads and focuses that editor — so it runs once the kit lets go.
        // Run here, it is a double lease: a panic on every right-click.
        let site = site.clone();
        let position = window.mouse_position();
        window.defer(cx, move |window, cx| open(&site, position, window, cx));
        NativeMenu::new()
    })
}

/// What the menu is built from, read at the click.
struct Click {
    offset: usize,
    line: usize,
    line_text: String,
    has_selection: bool,
    fold: Option<bool>,
    todo: bool,
    breakpoint: bool,
    gutters: bool,
}

/// Ask the worker, then open the menu at `position`.
fn open(site: &EditorSite, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    // The OS menu sends its choice to whatever has focus, and every item
    // acts on this editor — so this editor has it.
    site.editor.update(cx, |state, cx| state.focus(window, cx));
    let click = {
        let state = site.editor.read(cx);
        let text = state.text().to_string();
        let offset = crate::navigation::caret(state);
        let line = text.get(..offset).map_or(0, |b| b.matches('\n').count());
        let line_start = text
            .get(..offset)
            .and_then(|b| b.rfind('\n'))
            .map_or(0, |n| n + 1);
        let line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |n| line_start + n);
        let project = site.project.read(cx);
        let todo = project.diagnostics_for(&site.path).iter().any(|d| {
            d.code == TODO_CODE && (d.start as usize) < line_end && (d.end as usize) >= line_start
        });
        let breakpoint = u32::try_from(line + 1).is_ok_and(|n| {
            project
                .breakpoints_in(&site.path)
                .iter()
                .any(|(l, _)| *l == n)
        });
        Click {
            offset,
            line,
            line_text: text[line_start..line_end].to_owned(),
            has_selection: !state.selected_range().is_empty(),
            fold: state.fold_at(line),
            todo,
            breakpoint,
            gutters: brink_gpui_shell::settings::AppSettings::get(cx).show_gutters,
        }
    };
    let rope = site.editor.read(cx).text().clone();
    crate::document::seed_edit(
        &site.project,
        &site.path,
        &rope,
        site.editor.entity_id(),
        cx,
    );
    let path = site.path.to_string();
    let at = u32::try_from(click.offset).unwrap_or(u32::MAX);
    let ask = |kind: QueryKind, cx: &App| site.project.read(cx).query(kind, cx);
    let symbols = ask(QueryKind::DocumentSymbols { path: path.clone() }, cx);
    let definition = ask(
        QueryKind::Definition {
            path: path.clone(),
            offset: at,
        },
        cx,
    );
    let rename = ask(
        QueryKind::PrepareRename {
            path: path.clone(),
            offset: at,
        },
        cx,
    );
    let fixes = ask(
        QueryKind::FixesAt {
            path: path.clone(),
            offset: at,
        },
        cx,
    );
    let library = site.project.read(cx).is_library(&path);
    let project = site.project.clone();
    let source = site.editor.read(cx).text().to_string();
    window
        .spawn(cx, async move |cx| {
            let symbols = match symbols.await {
                Ok(QueryResult::DocumentSymbols(found)) => found,
                _ => Vec::new(),
            };
            let line_of = |at: u32| {
                source
                    .get(..at as usize)
                    .map_or(0, |b| b.matches('\n').count())
            };
            // The header line the click is on, if it is one.
            let header = symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Knot)
                .find_map(|knot| {
                    if line_of(knot.start) == click.line {
                        return Some((knot.name.clone(), None));
                    }
                    knot.children
                        .iter()
                        .filter(|s| s.kind == SymbolKind::Stitch)
                        .find(|s| line_of(s.start) == click.line)
                        .map(|s| (knot.name.clone(), Some(s.name.clone())))
                });
            let menu = if let Some((knot, stitch)) = header {
                let target = symbol_menu::Target {
                    path,
                    knot,
                    stitch,
                    library,
                    outline: std::rc::Rc::new(symbol_menu::outline_from_symbols(&symbols)),
                };
                header_menu(&target, click.breakpoint)
            } else {
                let definition = match definition.await {
                    Ok(QueryResult::Definition(found)) => found,
                    _ => None,
                };
                // A global's definition: its file's outline says so. Only
                // a global can be watched for writes.
                let global = match &definition {
                    Some(loc) => {
                        let outline = cx
                            .update(|_, cx| {
                                project.read(cx).query(
                                    QueryKind::DocumentSymbols {
                                        path: loc.path.clone(),
                                    },
                                    cx,
                                )
                            })
                            .ok();
                        let found = match outline {
                            Some(task) => match task.await {
                                Ok(QueryResult::DocumentSymbols(found)) => found,
                                _ => Vec::new(),
                            },
                            None => Vec::new(),
                        };
                        found
                            .into_iter()
                            .find(|s| {
                                matches!(s.kind, SymbolKind::Variable | SymbolKind::List)
                                    && s.start == loc.start
                            })
                            .map(|s| s.name)
                    }
                    None => None,
                };
                let global = global.map(|name| {
                    let watched = cx
                        .update(|_, cx| project.read(cx).is_watched(&name))
                        .unwrap_or(false);
                    (name, watched)
                });
                let definition = definition.is_some();
                let rename = match rename.await {
                    Ok(QueryResult::PrepareRename(Some((start, end)))) => {
                        source.get(start as usize..end as usize).map(str::to_owned)
                    }
                    _ => None,
                };
                let fixes = match fixes.await {
                    Ok(QueryResult::FixesAt(found)) => found,
                    _ => Vec::new(),
                };
                text_menu(&click, &fixes, definition, rename.as_deref(), global)
            };
            let menu = native(menu);
            let _ = cx.update(|window, cx| menu.show(position, window, cx));
        })
        .detach();
}

/// One entry of the menu, before it is the OS's — what is built and what
/// the tests read.
enum Entry {
    Item {
        label: String,
        disabled: bool,
        action: Box<dyn gpui::Action>,
    },
    Submenu {
        label: String,
        items: Vec<Entry>,
    },
    Separator,
}

fn item(label: impl Into<String>, action: impl gpui::Action) -> Entry {
    Entry::Item {
        label: label.into(),
        disabled: false,
        action: Box::new(action),
    }
}

fn native(entries: Vec<Entry>) -> NativeMenu {
    entries
        .into_iter()
        .fold(NativeMenu::new(), |menu, entry| match entry {
            Entry::Item {
                label,
                disabled,
                action,
            } => menu.menu_with_disabled(label, disabled, action),
            Entry::Submenu { label, items } => menu.submenu(label, native(items)),
            Entry::Separator => menu.separator(),
        })
}

/// A header line's menu: the shared knot/stitch items, then the line's
/// breakpoint verb.
fn header_menu(target: &symbol_menu::Target, breakpoint: bool) -> Vec<Entry> {
    let mut entries: Vec<Entry> = target
        .items()
        .into_iter()
        .map(|it| match it {
            symbol_menu::Item::Separator => Entry::Separator,
            symbol_menu::Item::Action {
                label,
                event,
                disabled,
            } => Entry::Item {
                label,
                disabled,
                action: Box::new(Outline {
                    event: target.event(&event),
                }),
            },
            symbol_menu::Item::Submenu { label, items } => Entry::Submenu {
                label,
                items: items
                    .into_iter()
                    .map(|(name, key)| {
                        item(
                            name,
                            Outline {
                                event: target.event(&key),
                            },
                        )
                    })
                    .collect(),
            },
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(item(
        if breakpoint {
            "Remove breakpoint"
        } else {
            "Set breakpoint here"
        },
        crate::ToggleBreakpoint,
    ));
    entries
}

/// The web's tier wording: what a fix will do before the click.
fn tier_label(tier: Tier) -> &'static str {
    match tier {
        Tier::Safe => "Safe",
        Tier::Suggested => "Suggested",
        Tier::Placeholder => "Needs input",
    }
}

/// The `INCLUDE` target on a line, as written.
fn include_target(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("INCLUDE")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let target = rest.trim();
    (!target.is_empty()).then_some(target)
}

/// Everywhere but a header: the four groups, each closed by a separator
/// when it has anything in it.
fn text_menu(
    click: &Click,
    fixes: &[FixPlan],
    definition: bool,
    rename: Option<&str>,
    global: Option<(String, bool)>,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    // 1. Fixes first: making the problem go away outranks going elsewhere.
    for plan in fixes {
        entries.push(item(
            format!("{} \u{2014} {}", plan.title, tier_label(plan.tier)),
            ApplyFix { plan: plan.clone() },
        ));
    }
    if !fixes.is_empty() {
        entries.push(item("Fix all safe in this file", crate::FixAllInFile));
        entries.push(Entry::Separator);
    }
    // 2. Identity, when the word resolves.
    if definition {
        entries.push(item("Go to Definition", crate::GoToDefinition));
        entries.push(item("Find References", crate::FindReferences));
        if let Some(name) = rename {
            entries.push(item(
                format!("Rename '{name}'\u{2026}"),
                crate::RenameSymbol,
            ));
        }
        // The State view's verb, where the global is written about.
        if let Some((name, watched)) = global {
            let label = if watched {
                format!("Remove Break on Write '{name}'")
            } else {
                format!("Break on Write '{name}'")
            };
            entries.push(item(label, ToggleWatch { name }));
        }
        entries.push(Entry::Separator);
    }
    // 3. The line's own: its include, its fold, its TODO.
    let before = entries.len();
    if let Some(target) = include_target(&click.line_text) {
        let name = target.rsplit('/').next().unwrap_or(target);
        entries.push(item(
            format!("Open {name}"),
            OpenFile {
                path: target.to_owned(),
            },
        ));
    }
    if let Some(folded) = click.fold {
        entries.push(item(
            if folded { "Unfold" } else { "Fold" },
            ToggleFold { line: click.line },
        ));
    }
    if click.todo {
        entries.push(item("Show in TODOs Panel", ShowTodos));
    }
    if entries.len() > before {
        entries.push(Entry::Separator);
    }
    // 4. Text.
    entries.extend([
        Entry::Item {
            label: "Cut".to_owned(),
            disabled: !click.has_selection,
            action: Box::new(gpui_component::input::Cut),
        },
        Entry::Item {
            label: "Copy".to_owned(),
            disabled: !click.has_selection,
            action: Box::new(gpui_component::input::Copy),
        },
        item("Paste", gpui_component::input::Paste),
        Entry::Separator,
        item("Select All", gpui_component::input::SelectAll),
        Entry::Separator,
        item(
            if click.gutters {
                "Hide Gutters"
            } else {
                "Show Gutters"
            },
            ToggleGutters,
        ),
    ]);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                Entry::Item {
                    label, disabled, ..
                } if *disabled => format!("({label})"),
                Entry::Item { label, .. } => label.clone(),
                Entry::Submenu { label, items } => {
                    format!("{label} ▸ {}", labels(items).join(","))
                }
                Entry::Separator => "─".to_owned(),
            })
            .collect()
    }

    fn click(line_text: &str) -> Click {
        Click {
            offset: 0,
            line: 3,
            line_text: line_text.to_owned(),
            has_selection: false,
            fold: None,
            todo: false,
            breakpoint: false,
            gutters: true,
        }
    }

    /// Plain prose: no identity, nothing on the line — only text editing.
    #[test]
    fn plain_text_offers_only_the_text_group() {
        assert_eq!(
            labels(&text_menu(
                &click("The lamp gutters."),
                &[],
                false,
                None,
                None
            )),
            [
                "(Cut)",
                "(Copy)",
                "Paste",
                "─",
                "Select All",
                "─",
                "Hide Gutters"
            ]
        );
    }

    /// The web's group order: fixes, identity, the line's own, text.
    #[test]
    fn every_group_in_the_webs_order() {
        let mut c = click("INCLUDE acts/one.ink");
        c.fold = Some(false);
        c.todo = true;
        c.has_selection = true;
        let fix = FixPlan {
            code: "E1".to_owned(),
            title: "Add the missing knot".to_owned(),
            tier: Tier::Suggested,
            edits: Vec::new(),
            caret: None,
        };
        assert_eq!(
            labels(&text_menu(
                &c,
                &[fix],
                true,
                Some("gold"),
                Some(("gold".to_owned(), false))
            )),
            [
                "Add the missing knot — Suggested",
                "Fix all safe in this file",
                "─",
                "Go to Definition",
                "Find References",
                "Rename 'gold'…",
                "Break on Write 'gold'",
                "─",
                "Open one.ink",
                "Fold",
                "Show in TODOs Panel",
                "─",
                "Cut",
                "Copy",
                "Paste",
                "─",
                "Select All",
                "─",
                "Hide Gutters"
            ]
        );
    }

    /// A header line is the knot/stitch menu, with its breakpoint verb.
    #[test]
    fn a_header_line_is_the_symbol_menu_and_its_breakpoint() {
        let target = symbol_menu::Target {
            path: "story.ink".to_owned(),
            knot: "start".to_owned(),
            stitch: None,
            library: false,
            outline: std::rc::Rc::new(vec![
                symbol_menu::KnotEntry {
                    name: "start".to_owned(),
                    start: 0,
                    full_end: 10,
                    is_function: false,
                    stitches: Vec::new(),
                },
                symbol_menu::KnotEntry {
                    name: "market".to_owned(),
                    start: 20,
                    full_end: 30,
                    is_function: false,
                    stitches: Vec::new(),
                },
            ]),
        };
        assert_eq!(
            labels(&header_menu(&target, true)),
            [
                "Play from here",
                "─",
                "Rename…",
                "(Move Up)",
                "Move Down",
                "Demote into ▸ market",
                "─",
                "New Stitch…",
                "─",
                "Remove breakpoint"
            ]
        );
    }

    #[test]
    fn an_include_line_names_its_file() {
        assert_eq!(include_target("INCLUDE acts/one.ink"), Some("acts/one.ink"));
        assert_eq!(include_target("  INCLUDE   two.ink  "), Some("two.ink"));
        assert_eq!(include_target("INCLUDEd is not a keyword"), None);
        assert_eq!(include_target("INCLUDE"), None);
        assert_eq!(include_target("The INCLUDE word"), None);
    }
}
