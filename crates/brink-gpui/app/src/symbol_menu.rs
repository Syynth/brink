//! The knot/stitch menu — one list, built here, for every surface that
//! offers one: the Binder's rows (right-click and `⋯`), Write's Structure
//! column, and the Story Graph's nodes. The web studio's `symbolMenuActions`
//! is the reference (#304): what it offers, in its order, and what it
//! disables rather than hides.
//!
//! A knot: Play from here · Rename… · Move Up · Move Down · Demote into ▸.
//! A stitch: Play from here · Rename… · Move Up · Move Down · Move to ▸ ·
//! Promote to Knot. Both add New Stitch… — the web has a `+` for that, the
//! native Binder has only its menu. A function is a knot that cannot be
//! played into or hold stitches, so it gets neither. A library file is not
//! the author's: Play is all it offers.
//!
//! The menu only asks. Each item is a [`BinderEvent`] handed to the
//! surface's `emit`, and the studio runs it — the moves through the
//! safe-by-default gate (`structural`), a rename through the rename prompt.

use std::rc::Rc;

use brink_gpui_model::query::Symbol;
use brink_ir::SymbolKind;
use gpui::{App, ClickEvent, Context, Window};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::binder::{BinderEvent, SymbolNode};

/// How a surface delivers what its menu asked for.
pub(crate) type Emit = Rc<dyn Fn(BinderEvent, &mut Window, &mut App)>;

/// One knot of a file, as the menu needs it: enough to say where a move can
/// go and whether it would collide.
#[derive(Clone, Debug)]
pub(crate) struct KnotEntry {
    pub name: String,
    /// The name's own offset — where a rename asks.
    pub start: usize,
    pub full_end: usize,
    pub is_function: bool,
    /// The knot's stitches, by name and name offset.
    pub stitches: Vec<(String, usize)>,
}

impl KnotEntry {
    fn has_stitch(&self, name: &str) -> bool {
        self.stitches.iter().any(|(s, _)| s == name)
    }
}

/// A file's knots, in order — from the Binder's tree.
pub(crate) fn outline_from_nodes(nodes: &[SymbolNode]) -> Vec<KnotEntry> {
    nodes
        .iter()
        .map(|k| KnotEntry {
            name: k.name.clone(),
            start: k.start,
            full_end: k.full_end,
            is_function: k.is_function,
            stitches: k
                .children
                .iter()
                .map(|s| (s.name.clone(), s.start))
                .collect(),
        })
        .collect()
}

/// A file's knots, in order — from the worker's document symbols, whose
/// top level also holds the globals.
pub(crate) fn outline_from_symbols(symbols: &[Symbol]) -> Vec<KnotEntry> {
    symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Knot)
        .map(|k| KnotEntry {
            name: k.name.clone(),
            start: k.start as usize,
            full_end: k.full_end as usize,
            is_function: k.is_function,
            stitches: k
                .children
                .iter()
                .filter(|s| s.kind == SymbolKind::Stitch)
                .map(|s| (s.name.clone(), s.start as usize))
                .collect(),
        })
        .collect()
}

/// What the menu was opened on.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub path: String,
    pub knot: String,
    pub stitch: Option<String>,
    /// A mounted library file: nothing here is the author's to change.
    pub library: bool,
    /// The file's knots, in order.
    pub outline: Rc<Vec<KnotEntry>>,
}

/// The items a target's menu holds, before they are drawn: what is built
/// and what is tested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Action {
        label: String,
        event: EventKey,
        disabled: bool,
    },
    Submenu {
        label: String,
        items: Vec<(String, EventKey)>,
    },
    Separator,
}

/// A [`BinderEvent`] with equality, for the items list. (The event itself
/// carries no `PartialEq`; the menu's only needs this much.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventKey {
    Play(String),
    Rename { offset: usize, name: String },
    Reorder { stitch: bool, up: bool },
    DemoteInto(String),
    MoveTo(String),
    Promote,
    NewStitch(usize),
}

impl Target {
    fn knot_entry(&self) -> Option<&KnotEntry> {
        self.outline.iter().find(|k| k.name == self.knot)
    }

    /// What this target's menu offers, in order.
    pub(crate) fn items(&self) -> Vec<Item> {
        let Some(knot) = self.knot_entry() else {
            return Vec::new();
        };
        let action = |label: &str, event: EventKey, disabled: bool| Item::Action {
            label: label.to_owned(),
            event,
            disabled,
        };
        let mut items = Vec::new();
        if !knot.is_function {
            let play = match &self.stitch {
                Some(s) => format!("{}.{s}", knot.name),
                None => knot.name.clone(),
            };
            items.push(action("Play from here", EventKey::Play(play), false));
        }
        if self.library {
            return items;
        }
        if !items.is_empty() {
            items.push(Item::Separator);
        }
        let (name, offset) = match &self.stitch {
            Some(stitch) => knot
                .stitches
                .iter()
                .find(|(s, _)| s == stitch)
                .map_or((stitch.clone(), knot.start), |(s, at)| (s.clone(), *at)),
            None => (knot.name.clone(), knot.start),
        };
        items.push(action(
            "Rename\u{2026}",
            EventKey::Rename { offset, name },
            false,
        ));
        // Up and down among the target's own siblings: the file's knots,
        // or its knot's stitches. Disabled at the ends, not hidden — the
        // menu keeps its shape.
        let (index, count) = match &self.stitch {
            Some(stitch) => (
                knot.stitches.iter().position(|(s, _)| s == stitch),
                knot.stitches.len(),
            ),
            None => (
                self.outline.iter().position(|k| k.name == knot.name),
                self.outline.len(),
            ),
        };
        let index = index.unwrap_or(0);
        let stitch = self.stitch.is_some();
        items.push(action(
            "Move Up",
            EventKey::Reorder { stitch, up: true },
            index == 0,
        ));
        items.push(action(
            "Move Down",
            EventKey::Reorder { stitch, up: false },
            index + 1 >= count,
        ));
        match &self.stitch {
            // A knot folds into another knot as a stitch — only when it
            // has none of its own (a stitch cannot hold stitches), and
            // only into a knot with no stitch of its name already.
            None if !knot.is_function && knot.stitches.is_empty() => {
                let into = self.destinations(&knot.name, &knot.name);
                if !into.is_empty() {
                    items.push(Item::Submenu {
                        label: "Demote into".to_owned(),
                        items: into
                            .into_iter()
                            .map(|k| (k.clone(), EventKey::DemoteInto(k)))
                            .collect(),
                    });
                }
            }
            Some(stitch) => {
                let to = self.destinations(&knot.name, stitch);
                if !to.is_empty() {
                    items.push(Item::Submenu {
                        label: "Move to".to_owned(),
                        items: to
                            .into_iter()
                            .map(|k| (k.clone(), EventKey::MoveTo(k)))
                            .collect(),
                    });
                }
                let taken = self.outline.iter().any(|k| &k.name == stitch);
                items.push(action("Promote to Knot", EventKey::Promote, taken));
            }
            None => {}
        }
        if !knot.is_function {
            items.push(Item::Separator);
            items.push(action(
                "New Stitch\u{2026}",
                EventKey::NewStitch(knot.full_end),
                false,
            ));
        }
        items
    }

    /// The knots, other than `from`, that a stitch called `name` could move
    /// into: not functions, and holding no stitch of that name already.
    fn destinations(&self, from: &str, name: &str) -> Vec<String> {
        self.outline
            .iter()
            .filter(|k| k.name != from && !k.is_function && !k.has_stitch(name))
            .map(|k| k.name.clone())
            .collect()
    }

    /// The event an item asks for.
    pub(crate) fn event(&self, key: &EventKey) -> BinderEvent {
        let path = self.path.clone();
        let knot = self.knot.clone();
        match key.clone() {
            EventKey::Play(path) => BinderEvent::Play { path },
            EventKey::Rename { offset, name } => BinderEvent::RenameSymbol { path, offset, name },
            EventKey::Reorder { up, .. } => BinderEvent::Reorder {
                path,
                knot,
                stitch: self.stitch.clone(),
                up,
            },
            EventKey::DemoteInto(into) => BinderEvent::Demote {
                path,
                knot,
                into: Some(into),
            },
            EventKey::MoveTo(dest) => BinderEvent::MoveStitch {
                path,
                knot,
                stitch: self.stitch.clone().unwrap_or_default(),
                dest,
            },
            EventKey::Promote => BinderEvent::Promote {
                path,
                knot,
                stitch: self.stitch.clone().unwrap_or_default(),
            },
            EventKey::NewStitch(full_end) => BinderEvent::NewStitch { path, full_end },
        }
    }
}

/// Add `target`'s items to `menu`.
pub(crate) fn build(
    mut menu: PopupMenu,
    target: &Target,
    emit: &Emit,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let click = |event: BinderEvent| {
        let emit = emit.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| emit(event.clone(), window, cx)
    };
    for item in target.items() {
        menu = match item {
            Item::Separator => menu.separator(),
            Item::Action {
                label,
                event,
                disabled,
            } => menu.item(
                PopupMenuItem::new(label)
                    .disabled(disabled)
                    .on_click(click(target.event(&event))),
            ),
            Item::Submenu { label, items } => {
                let events: Vec<(String, BinderEvent)> = items
                    .iter()
                    .map(|(name, key)| (name.clone(), target.event(key)))
                    .collect();
                let emit = emit.clone();
                menu.submenu(label, window, cx, move |mut sub, _, _| {
                    for (name, event) in &events {
                        let emit = emit.clone();
                        let event = event.clone();
                        sub = sub.item(PopupMenuItem::new(name.clone()).on_click(
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                emit(event.clone(), window, cx);
                            },
                        ));
                    }
                    sub
                })
            }
        };
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knot(name: &str, stitches: &[&str]) -> KnotEntry {
        KnotEntry {
            name: name.to_owned(),
            start: 0,
            full_end: 0,
            is_function: false,
            stitches: stitches.iter().map(|s| ((*s).to_owned(), 0)).collect(),
        }
    }

    fn target(outline: Vec<KnotEntry>, knot: &str, stitch: Option<&str>) -> Target {
        Target {
            path: "main.ink".to_owned(),
            knot: knot.to_owned(),
            stitch: stitch.map(str::to_owned),
            library: false,
            outline: Rc::new(outline),
        }
    }

    fn labels(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Action {
                    label, disabled, ..
                } => {
                    if *disabled {
                        format!("({label})")
                    } else {
                        label.clone()
                    }
                }
                Item::Submenu { label, items } => format!(
                    "{label} ▸ {}",
                    items
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                Item::Separator => "─".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_first_knot_without_stitches_moves_down_and_demotes() {
        let t = target(
            vec![knot("a", &[]), knot("b", &[]), knot("c", &["a"])],
            "a",
            None,
        );
        assert_eq!(
            labels(&t.items()),
            [
                "Play from here",
                "─",
                "Rename…",
                "(Move Up)",
                "Move Down",
                // `c` already has a stitch called `a`.
                "Demote into ▸ b",
                "─",
                "New Stitch…",
            ]
        );
    }

    #[test]
    fn a_knot_with_stitches_cannot_be_demoted() {
        let t = target(vec![knot("a", &[]), knot("b", &["x"])], "b", None);
        assert_eq!(
            labels(&t.items()),
            [
                "Play from here",
                "─",
                "Rename…",
                "Move Up",
                "(Move Down)",
                "─",
                "New Stitch…",
            ]
        );
    }

    #[test]
    fn a_stitch_moves_among_its_siblings_and_to_other_knots() {
        let t = target(
            vec![knot("a", &["x", "y"]), knot("b", &["y"]), knot("x", &[])],
            "a",
            Some("y"),
        );
        assert_eq!(
            labels(&t.items()),
            [
                "Play from here",
                "─",
                "Rename…",
                "Move Up",
                "(Move Down)",
                // `b` already holds a `y`.
                "Move to ▸ x",
                "Promote to Knot",
                "─",
                "New Stitch…",
            ]
        );
        let promote = target(
            vec![knot("a", &["x", "y"]), knot("b", &["y"]), knot("x", &[])],
            "a",
            Some("x"),
        );
        assert!(
            labels(&promote.items()).contains(&"(Promote to Knot)".to_owned()),
            "a knot called `x` exists, so promoting `a.x` would collide"
        );
    }

    #[test]
    fn a_function_is_neither_played_nor_given_stitches() {
        let mut f = knot("f", &[]);
        f.is_function = true;
        let t = target(vec![knot("a", &[]), f], "f", None);
        assert_eq!(labels(&t.items()), ["Rename…", "Move Up", "(Move Down)"]);
    }

    #[test]
    fn a_library_file_only_plays() {
        let mut t = target(vec![knot("a", &["x"])], "a", Some("x"));
        t.library = true;
        assert_eq!(labels(&t.items()), ["Play from here"]);
    }
}
