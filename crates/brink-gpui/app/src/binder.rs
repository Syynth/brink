//! SPIKE — the studio's Binder, rebuilt natively.
//!
//! The reference is `packages/studio-ui/src/Binder.tsx` (2,271 lines of TSX)
//! plus `studio-store/src/slices/binder.ts` and `binder-order.ts`. This is
//! the same widget against the same rules, sourcing its data from
//! `IdeSession` directly instead of through wasm:
//!
//! - **Two modes** (#3036): Files, and Structure (files open into knots,
//!   knots into stitches).
//! - **The fill rule** (ruled 2026-08-23): the icon IS the expander — no
//!   chevrons. Filled = collapsed over content; outline = expanded or a
//!   leaf. Folders additionally swap to the open silhouette.
//! - **The entry file** carries the brand mark rather than a text badge
//!   (#3014/#3021); a file outside the compile closure is dimmed.
//! - **Diagnostic marks** (#3041): error/warning counts, zero-suppressed,
//!   summed over the file and, in Structure mode, over each symbol's own
//!   body range.
//! - **Drag to reorder**, with an insertion line between rows and a
//!   drop-into highlight on folders — the feature whose HTML5 equivalent
//!   needed two WebKit-specific fixes (#3351, and the `-webkit-user-drag`
//!   cascade bug its follow-up found).
//! - Filter box, collapse/expand all, keyboard navigation, hover row
//!   actions, right-click menu.
//!
//! Deliberately skipped (not what the spike is asking): the undo stack, the
//! Library section, multi-select, inline create, and persistence of the
//! drag order to a `.binder.json` sidecar — reordering here lives in
//! memory, which is enough to feel it.

use std::collections::{BTreeMap, HashMap, HashSet};

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, DragMoveEvent, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, Point, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle, Window, anchored,
    deferred, div, point, prelude::FluentBuilder as _, px, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, h_flex,
    input::{Input, InputEvent, InputState},
    menu::ContextMenuExt as _,
    v_flex,
};

use crate::icons;
use crate::project::{Project, ProjectEvent};

/// One knot (with its stitches) for Structure mode — the worker's
/// [`brink_gpui_model::query::Symbol`] with offsets widened to the `usize`
/// the row model uses.
#[derive(Clone, Debug)]
pub struct SymbolNode {
    pub name: String,
    pub start: usize,
    pub full_start: usize,
    pub full_end: usize,
    pub is_function: bool,
    pub children: Vec<SymbolNode>,
}

fn convert_symbol(symbol: &brink_gpui_model::query::Symbol) -> SymbolNode {
    SymbolNode {
        name: symbol.name.clone(),
        start: symbol.start as usize,
        full_start: symbol.full_start as usize,
        full_end: symbol.full_end as usize,
        is_function: symbol.is_function,
        children: symbol.children.iter().map(convert_symbol).collect(),
    }
}

// ── Metrics, from `studio-ui/src/styles/binder.css` ──────────────────

const ROW_HEIGHT: f32 = 26.0;
const INDENT: f32 = 18.0;
const PAD_X: f32 = 12.0;

// ── Model ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Files,
    Structure,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Folder,
    File,
    Knot,
    Stitch,
}

/// Per-row diagnostic counts. Info/Hint never mark (the Structure artboard's
/// roll-up rule).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Marks {
    pub errors: u32,
    pub warnings: u32,
}

impl Marks {
    fn is_empty(self) -> bool {
        self.errors == 0 && self.warnings == 0
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub key: SharedString,
    pub kind: RowKind,
    pub depth: usize,
    pub label: SharedString,
    /// The file this row belongs to (a folder row's own key for folders).
    pub path: String,
    /// Byte offset to reveal when the row is opened (symbol rows only).
    pub offset: Option<usize>,
    pub expandable: bool,
    pub expanded: bool,
    pub entry: bool,
    /// Matched a `[project] drafts` glob and is outside the compile closure
    /// ("reachability wins", 2026-08-27). Drawn dashed.
    pub draft: bool,
    pub dimmed: bool,
    pub is_function: bool,
    pub marks: Marks,
    /// Parent container key — `""` for the root level. Reordering is scoped
    /// to a parent, exactly as the sidecar's order is.
    pub parent: String,
}

/// What a drag is currently over.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DropTarget {
    /// Into a container (a folder, or a file/knot in Structure mode).
    Into(SharedString),
    /// Between two rows — the insertion line.
    Between { key: SharedString, after: bool },
}

/// The drag payload. GPUI carries this as a typed value, so `on_drop`
/// receives it already downcast — there is no `dataTransfer` string to
/// encode into and no `dragenter`/`dragover` contract to satisfy.
#[derive(Clone, Debug)]
struct DraggedRow {
    key: SharedString,
    label: SharedString,
    kind: RowKind,
}

pub enum BinderEvent {
    /// Open a file, optionally revealing a byte offset within it.
    Open { path: String, offset: Option<usize> },
}

// ── Tree ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<String>,
}

fn build_folder_tree(files: &[String]) -> Folder {
    let mut root = Folder::default();
    for path in files {
        let mut cursor = &mut root;
        let segments: Vec<&str> = path.split('/').collect();
        for segment in &segments[..segments.len().saturating_sub(1)] {
            cursor = cursor
                .folders
                .entry((*segment).to_owned())
                .or_insert_with(Folder::default);
        }
        cursor.files.push(path.clone());
    }
    root
}

/// A level's children in display order: the authored order first (what the
/// `.binder.json` sidecar persists — here, whatever dragging has done), then
/// the fallback the studio uses when nothing is authored — entry first,
/// folders before files, alphabetical. Folders and files interleave;
/// placement is authorship.
fn ordered_children(
    folder: &Folder,
    parent_key: &str,
    entry: Option<&str>,
    order: &HashMap<String, Vec<String>>,
) -> Vec<Child> {
    let mut children: Vec<Child> = Vec::new();
    for (name, sub) in &folder.folders {
        children.push(Child::Folder {
            key: format!("{parent_key}{name}/"),
            name: name.clone(),
        });
        let _ = sub;
    }
    for path in &folder.files {
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        children.push(Child::File {
            path: path.clone(),
            name,
        });
    }

    children.sort_by(|a, b| {
        let rank = |c: &Child| match c {
            Child::File { path, .. } if Some(path.as_str()) == entry => 0,
            Child::Folder { .. } => 1,
            Child::File { .. } => 2,
        };
        rank(a).cmp(&rank(b)).then_with(|| {
            a.sort_name()
                .to_lowercase()
                .cmp(&b.sort_name().to_lowercase())
        })
    });

    if let Some(authored) = order.get(parent_key) {
        let position = |c: &Child| authored.iter().position(|k| *k == c.key());
        children.sort_by(|a, b| match (position(a), position(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            // A child the authored order has never seen keeps its fallback
            // place relative to the rest, but sorts after everything placed.
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
    }
    children
}

enum Child {
    Folder { key: String, name: String },
    File { path: String, name: String },
}

impl Child {
    fn key(&self) -> &str {
        match self {
            Child::Folder { key, .. } => key,
            Child::File { path, .. } => path,
        }
    }
    fn sort_name(&self) -> &str {
        match self {
            Child::Folder { name, .. } | Child::File { name, .. } => name,
        }
    }
}

// ── The view ─────────────────────────────────────────────────────────

pub struct Binder {
    project: Entity<Project>,
    /// Per-file knots and stitches, filled asynchronously.
    ///
    /// Symbols are a per-file query, not part of the analysis broadcast:
    /// shipping them for every file on every keystroke would be O(project)
    /// for the sake of rows that are collapsed. Structure mode requests the
    /// files it is about to draw and renders whatever has landed, so a first
    /// expand shows the file row immediately and its knots a moment later
    /// rather than blocking the frame.
    symbols: HashMap<String, Vec<SymbolNode>>,
    /// Requests in flight, so a rebuild during one does not fire a second.
    pending_symbols: HashSet<String>,
    mode: Mode,
    collapsed: HashSet<SharedString>,
    order: HashMap<String, Vec<String>>,
    selected: Option<SharedString>,
    rows: Vec<Row>,
    filter: Entity<InputState>,
    filter_open: bool,
    filter_text: String,
    drop: Option<DropTarget>,
    /// The hover-revealed ⋯ menu: which row, and where to anchor it.
    /// gpui-component 0.6.0 publishes no click-triggered popup (only the
    /// right-click `ContextMenu`), so the row-actions affordance the studio
    /// has is built here directly — `anchored` + `deferred`, which is what
    /// that component does internally anyway.
    row_menu: Option<(SharedString, Point<Pixels>)>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// The dock tab this panel sits in, for the rail to select.
    tab: brink_gpui_shell::tool_window::TabSlot,
    _subs: Vec<gpui::Subscription>,
}

impl Binder {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter…"));
        let sub = cx.subscribe(&filter, |this: &mut Self, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.filter_text = state.read(cx).value().to_string();
                this.rebuild(cx);
            }
        });
        // Observing the project is what replaces the spike's hand-called
        // `rebuild()` after every mutation: the panel hears that analysis
        // moved and redraws itself.
        let watch = cx.subscribe(&project, |this: &mut Self, _, event: &ProjectEvent, cx| {
            match event {
                ProjectEvent::Opened { .. } => {
                    this.symbols.clear();
                    this.pending_symbols.clear();
                    this.rebuild(cx);
                }
                ProjectEvent::Analyzed => {
                    // Structure is derived from the analysis that just
                    // moved, so what is cached is now stale by definition.
                    this.symbols.clear();
                    this.rebuild(cx);
                }
                ProjectEvent::OpenFailed(_) => {}
            }
        });
        let mut this = Self {
            project,
            symbols: HashMap::new(),
            pending_symbols: HashSet::new(),
            mode: Mode::Files,
            collapsed: HashSet::new(),
            order: HashMap::new(),
            selected: None,
            rows: Vec::new(),
            filter,
            filter_open: false,
            filter_text: String::new(),
            drop: None,
            row_menu: None,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            tab: brink_gpui_shell::tool_window::TabSlot::default(),
            _subs: vec![sub, watch],
        };
        this.rebuild(cx);
        this
    }

    /// Rebuild the flat row list. Called on every input that can change it —
    /// mode, collapse, filter, order, or the project's own analysis.
    pub fn rebuild(&mut self, cx: &mut Context<Self>) {
        let (files, entry, closure, diagnostics, drafts) = {
            let project = self.project.read(cx);
            (
                project.files().to_vec(),
                project.entry().map(str::to_owned),
                project
                    .files()
                    .iter()
                    .filter(|p| project.in_story(p))
                    .cloned()
                    .collect::<HashSet<String>>(),
                project.diagnostic_points(),
                project
                    .files()
                    .iter()
                    .filter(|p| project.is_draft(p))
                    .cloned()
                    .collect::<HashSet<String>>(),
            )
        };
        if self.mode == Mode::Structure {
            self.request_symbols(&files, cx);
        }
        let symbols = self.symbols.clone();

        let mut file_marks: HashMap<&str, Marks> = HashMap::new();
        for (path, _, is_error) in &diagnostics {
            let entry = file_marks.entry(path.as_str()).or_default();
            if *is_error {
                entry.errors += 1;
            } else {
                entry.warnings += 1;
            }
        }

        let filter = self.filter_text.trim().to_lowercase();
        let matches = |label: &str, path: &str| {
            filter.is_empty()
                || label.to_lowercase().contains(&filter)
                || path.to_lowercase().contains(&filter)
        };

        let tree = build_folder_tree(&files);
        let mut rows = Vec::new();
        self.walk(
            &tree,
            "",
            0,
            entry.as_deref(),
            &closure,
            &drafts,
            &file_marks,
            &symbols,
            &diagnostics,
            &matches,
            &mut rows,
        );

        // A filter hides folders that ended up with nothing under them.
        if !filter.is_empty() {
            let mut kept: Vec<Row> = Vec::with_capacity(rows.len());
            for (i, row) in rows.iter().enumerate() {
                let has_children = rows.get(i + 1).is_some_and(|next| next.depth > row.depth);
                if row.kind == RowKind::Folder && !has_children {
                    continue;
                }
                kept.push(row.clone());
            }
            rows = kept;
        }

        self.rows = rows;
        cx.notify();
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a tree walk carrying every per-row input; splitting it would only \
                  move the same arguments into a struct nothing else uses"
    )]
    fn walk(
        &self,
        folder: &Folder,
        parent_key: &str,
        depth: usize,
        entry: Option<&str>,
        closure: &HashSet<String>,
        drafts: &HashSet<String>,
        file_marks: &HashMap<&str, Marks>,
        symbols: &HashMap<String, Vec<SymbolNode>>,
        diagnostics: &[(String, usize, bool)],
        matches: &dyn Fn(&str, &str) -> bool,
        out: &mut Vec<Row>,
    ) {
        for child in ordered_children(folder, parent_key, entry, &self.order) {
            match child {
                Child::Folder { key, name } => {
                    let Some(sub) = folder.folders.get(&name) else {
                        continue;
                    };
                    let shared: SharedString = key.clone().into();
                    let expanded = !self.collapsed.contains(&shared);
                    out.push(Row {
                        key: shared,
                        kind: RowKind::Folder,
                        depth,
                        label: name.clone().into(),
                        path: key.clone(),
                        offset: None,
                        expandable: true,
                        expanded,
                        entry: false,
                        draft: false,
                        dimmed: false,
                        is_function: false,
                        marks: Marks::default(),
                        parent: parent_key.to_owned(),
                    });
                    if expanded {
                        self.walk(
                            sub,
                            &key,
                            depth + 1,
                            entry,
                            closure,
                            drafts,
                            file_marks,
                            symbols,
                            diagnostics,
                            matches,
                            out,
                        );
                    }
                }
                Child::File { path, name } => {
                    let file_symbols = symbols.get(&path).map(Vec::as_slice).unwrap_or(&[]);
                    let structure = self.mode == Mode::Structure;
                    let shared: SharedString = path.clone().into();
                    let expanded = !self.collapsed.contains(&shared);
                    let self_matches = matches(&name, &path);
                    let child_matches = structure
                        && file_symbols.iter().any(|k| {
                            matches(&k.name, &path)
                                || k.children.iter().any(|s| matches(&s.name, &path))
                        });
                    if !self_matches && !child_matches {
                        continue;
                    }
                    out.push(Row {
                        key: shared,
                        kind: RowKind::File,
                        depth,
                        label: name.into(),
                        path: path.clone(),
                        offset: None,
                        expandable: structure && !file_symbols.is_empty(),
                        expanded,
                        entry: Some(path.as_str()) == entry,
                        draft: drafts.contains(path.as_str()),
                        // "closure empty means nothing to contradict": before
                        // the first analysis nothing is known to be out of
                        // scope, so no row is dimmed.
                        dimmed: !closure.is_empty() && !closure.contains(&path),
                        is_function: false,
                        marks: file_marks.get(path.as_str()).copied().unwrap_or_default(),
                        parent: parent_key.to_owned(),
                    });
                    if !structure || !expanded {
                        continue;
                    }
                    for knot in file_symbols {
                        let knot_key: SharedString = format!("{path}::{}", knot.name).into();
                        let knot_expanded = !self.collapsed.contains(&knot_key);
                        let knot_matches = matches(&knot.name, &path);
                        let stitch_matches = knot.children.iter().any(|s| matches(&s.name, &path));
                        if !self_matches && !knot_matches && !stitch_matches {
                            continue;
                        }
                        out.push(Row {
                            key: knot_key.clone(),
                            kind: RowKind::Knot,
                            depth: depth + 1,
                            label: knot.name.clone().into(),
                            path: path.clone(),
                            offset: Some(knot.start),
                            expandable: !knot.children.is_empty(),
                            expanded: knot_expanded,
                            entry: false,
                            draft: false,
                            dimmed: false,
                            is_function: knot.is_function,
                            marks: symbol_marks(diagnostics, &path, knot.full_start, knot.full_end),
                            parent: path.clone(),
                        });
                        if !knot_expanded {
                            continue;
                        }
                        for stitch in &knot.children {
                            if !self_matches && !knot_matches && !matches(&stitch.name, &path) {
                                continue;
                            }
                            out.push(Row {
                                key: format!("{path}::{}::{}", knot.name, stitch.name).into(),
                                kind: RowKind::Stitch,
                                depth: depth + 2,
                                label: stitch.name.clone().into(),
                                path: path.clone(),
                                offset: Some(stitch.start),
                                expandable: false,
                                expanded: false,
                                entry: false,
                                draft: false,
                                dimmed: false,
                                is_function: false,
                                marks: symbol_marks(
                                    diagnostics,
                                    &path,
                                    stitch.full_start,
                                    stitch.full_end,
                                ),
                                parent: knot_key.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // ── Interaction ─────────────────────────────────────────────────

    /// Ask the worker for any expanded file's symbols we do not hold yet.
    fn request_symbols(&mut self, files: &[String], cx: &mut Context<Self>) {
        for path in files {
            if self.symbols.contains_key(path) || self.pending_symbols.contains(path) {
                continue;
            }
            self.pending_symbols.insert(path.clone());
            let query = self.project.read(cx).query(
                brink_gpui_model::query::QueryKind::DocumentSymbols { path: path.clone() },
                cx,
            );
            let path = path.clone();
            cx.spawn(async move |this, cx| {
                let answer = query.await;
                let _ = this.update(cx, |this, cx| {
                    this.pending_symbols.remove(&path);
                    if let Ok(brink_gpui_model::query::QueryResult::DocumentSymbols(found)) = answer
                    {
                        this.symbols
                            .insert(path, found.iter().map(convert_symbol).collect());
                        this.rebuild(cx);
                    }
                });
            })
            .detach();
        }
    }

    fn toggle(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        if self.collapsed.contains(key) {
            self.collapsed.remove(key);
        } else {
            self.collapsed.insert(key.clone());
        }
        self.rebuild(cx);
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        self.selected = Some(row.key.clone());
        if row.expandable {
            self.toggle(&row.key, cx);
        }
        if row.kind != RowKind::Folder {
            cx.emit(BinderEvent::Open {
                path: row.path.clone(),
                offset: row.offset,
            });
        }
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().position(|r| &r.key == selected)
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            self.selected = Some(row.key.clone());
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
            cx.notify();
        }
    }

    /// Arrow-key navigation, with the tree semantics the studio uses:
    /// Left collapses (or steps to the parent when already collapsed), Right
    /// expands (or steps into the first child).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let current = self.selected_index();
        match key {
            "down" => {
                let next = current.map_or(0, |i| (i + 1).min(self.rows.len().saturating_sub(1)));
                self.select_index(next, cx);
            }
            "up" => {
                let next = current.map_or(0, |i| i.saturating_sub(1));
                self.select_index(next, cx);
            }
            "right" => {
                if let Some(i) = current {
                    let row = self.rows[i].clone();
                    if row.expandable && !row.expanded {
                        self.toggle(&row.key, cx);
                    } else if i + 1 < self.rows.len() && self.rows[i + 1].depth > row.depth {
                        self.select_index(i + 1, cx);
                    }
                }
            }
            "left" => {
                if let Some(i) = current {
                    let row = self.rows[i].clone();
                    if row.expandable && row.expanded {
                        self.toggle(&row.key, cx);
                    } else if let Some(parent) =
                        self.rows[..i].iter().rposition(|r| r.depth < row.depth)
                    {
                        self.select_index(parent, cx);
                    }
                }
            }
            "enter" => {
                if let Some(i) = current {
                    self.activate(i, cx);
                }
            }
            _ => {}
        }
    }

    /// Apply a drop. Reordering is scoped to the target's parent, mirroring
    /// the sidecar's per-level order; a drop INTO a folder re-parents.
    fn apply_drop(&mut self, dragged: &DraggedRow, cx: &mut Context<Self>) {
        let Some(target) = self.drop.take() else {
            return;
        };
        if dragged.kind == RowKind::Knot || dragged.kind == RowKind::Stitch {
            // Symbol reordering is a structural edit (it rewrites source),
            // which this spike does not do — the drag still runs, it just
            // declines at the end rather than pretending.
            cx.notify();
            return;
        }
        match target {
            DropTarget::Between { key, after } => {
                let Some(target_row) = self.rows.iter().find(|r| r.key == key).cloned() else {
                    return;
                };
                let parent = target_row.parent.clone();
                let mut siblings: Vec<String> = self
                    .rows
                    .iter()
                    .filter(|r| r.parent == parent)
                    .map(|r| r.key.to_string())
                    .collect();
                siblings.retain(|k| k != dragged.key.as_ref());
                let at = siblings
                    .iter()
                    .position(|k| k.as_str() == key.as_ref())
                    .map_or(siblings.len(), |i| if after { i + 1 } else { i });
                siblings.insert(at, dragged.key.to_string());
                self.order.insert(parent, siblings);
            }
            DropTarget::Into(key) => {
                let mut siblings: Vec<String> = self
                    .rows
                    .iter()
                    .filter(|r| r.parent == key)
                    .map(|r| r.key.to_string())
                    .collect();
                siblings.retain(|k| k != dragged.key.as_ref());
                siblings.push(dragged.key.to_string());
                self.order.insert(key.to_string(), siblings);
                self.collapsed.remove(&key);
            }
        }
        self.rebuild(cx);
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn icon_for(row: &Row) -> &'static str {
        // The fill rule: filled = collapsed over content, outline =
        // expanded or a leaf.
        let filled = row.expandable && !row.expanded;
        match row.kind {
            RowKind::Folder => {
                if row.expandable && row.expanded {
                    icons::FOLDER_OPEN
                } else if filled {
                    icons::FOLDER_FILLED
                } else {
                    // An empty folder: a leaf, so the outline.
                    icons::FOLDER
                }
            }
            RowKind::File => {
                if row.path.ends_with(".toml") {
                    icons::DOC
                } else if row.draft {
                    // Dashed, whether or not the row is selected: being a
                    // draft is a property of the file, not of the selection.
                    icons::FILE_DRAFT
                } else if row.entry {
                    if filled {
                        icons::FILE_ENTRY
                    } else {
                        icons::FILE_ENTRY_OUTLINE
                    }
                } else if filled {
                    icons::FILE_FILLED
                } else {
                    icons::FILE
                }
            }
            RowKind::Knot => {
                if row.is_function {
                    icons::FUNCTION
                } else if filled {
                    icons::KNOT_FILLED
                } else {
                    icons::KNOT
                }
            }
            RowKind::Stitch => icons::STITCH,
        }
    }

    fn icon_tint(row: &Row, cx: &App) -> Hsla {
        let theme = cx.theme();
        match row.kind {
            RowKind::Folder => theme.muted_foreground,
            RowKind::File => {
                if row.entry {
                    theme.primary
                } else {
                    theme.muted_foreground
                }
            }
            RowKind::Knot | RowKind::Stitch => theme.primary.opacity(0.75),
        }
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let selected = self.selected.as_ref() == Some(&row.key);
        let drop_into = self.drop == Some(DropTarget::Into(row.key.clone()));
        let line_before = self.drop
            == Some(DropTarget::Between {
                key: row.key.clone(),
                after: false,
            });
        let line_after = self.drop
            == Some(DropTarget::Between {
                key: row.key.clone(),
                after: true,
            });

        let text_color = if row.dimmed {
            theme.muted_foreground.opacity(0.65)
        } else {
            theme.foreground
        };
        let icon_color = Self::icon_tint(&row, cx);
        let dragged = DraggedRow {
            key: row.key.clone(),
            label: row.label.clone(),
            kind: row.kind,
        };
        let key_for_move = row.key.clone();
        let menu_key = row.key.clone();
        let kind_for_move = row.kind;

        // Indent guides: one hairline under each ancestor's icon column.
        let guides = (0..row.depth).map(|_| {
            div()
                .w(px(INDENT))
                .h_full()
                .border_l_1()
                .border_color(theme.border.opacity(0.5))
        });

        let marks = (!row.marks.is_empty()).then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .when(row.marks.errors > 0, |el| {
                    el.child(icons::icon(icons::ERROR_MARK, px(8.), theme.danger))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.danger)
                                .child(row.marks.errors.to_string()),
                        )
                })
                .when(row.marks.warnings > 0, |el| {
                    el.child(icons::icon(icons::WARNING_MARK, px(10.), theme.warning))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.warning)
                                .child(row.marks.warnings.to_string()),
                        )
                })
        });

        div()
            .id(("binder-row", index))
            .relative()
            .w_full()
            .h(px(ROW_HEIGHT))
            .child(
                h_flex()
                    .size_full()
                    .pl(px(PAD_X))
                    .pr_2()
                    .items_center()
                    .gap_2()
                    .when(selected, |el| el.bg(theme.accent))
                    .when(drop_into, |el| el.bg(theme.primary.opacity(0.16)))
                    .when(!selected && !drop_into, |el| {
                        el.hover(|s| s.bg(theme.muted.opacity(0.5)))
                    })
                    .children(guides)
                    .child(icons::icon(Self::icon_for(&row), px(13.), icon_color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(text_color)
                            .when(row.entry, |el| el.font_semibold())
                            .child(row.label.clone()),
                    )
                    .children(marks)
                    .child(
                        div()
                            .id(("row-actions", index))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(18.))
                            .rounded_sm()
                            .invisible()
                            .group_hover("", |s| s.visible())
                            .hover(|s| s.bg(theme.muted))
                            .child(icons::icon(icons::DOTS, px(12.), theme.muted_foreground))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener({
                                    let key = menu_key.clone();
                                    move |this, event: &MouseDownEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.row_menu = Some((key.clone(), event.position));
                                        cx.notify();
                                    }
                                }),
                            ),
                    ),
            )
            .group("")
            // The insertion line — 2px, drawn at the row edge the pointer is
            // nearer, exactly like the studio's `.brink-binder-drop-line`.
            .when(line_before, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(theme.primary),
                )
            })
            .when(line_after, |el| {
                el.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(theme.primary),
                )
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.activate(index, cx);
            }))
            // GPUI's own drag system: a typed payload and a real preview
            // view. No `dataTransfer`, no `dragenter` contract to satisfy.
            .on_drag(dragged, |dragged, _offset, _window, cx| {
                let label = dragged.label.clone();
                cx.new(|_| DragPreview { label })
            })
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<DraggedRow>, _window, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let dragged: &DraggedRow = event.drag(cx);
                    if dragged.key == key_for_move {
                        return;
                    }
                    let middle = event.bounds.center().y;
                    let third = event.bounds.size.height * 0.3;
                    let y = event.event.position.y;
                    // A container takes a drop INTO it in its middle band,
                    // and a BETWEEN line near either edge.
                    let container = kind_for_move == RowKind::Folder;
                    let next = if container && (y - middle).abs() < third {
                        DropTarget::Into(key_for_move.clone())
                    } else {
                        DropTarget::Between {
                            key: key_for_move.clone(),
                            after: y > middle,
                        }
                    };
                    if this.drop.as_ref() != Some(&next) {
                        this.drop = Some(next);
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(move |this, dragged: &DraggedRow, _window, cx| {
                let dragged = dragged.clone();
                this.apply_drop(&dragged, cx);
            }))
            .context_menu(move |menu, _window, _cx| {
                menu.label(row.label.clone())
                    .separator()
                    .menu("Open", Box::new(NoopAction))
                    .menu("Play from here", Box::new(NoopAction))
                    .separator()
                    .menu("Rename…", Box::new(NoopAction))
                    .menu("Delete", Box::new(NoopAction))
            })
            .into_any_element()
    }

    /// A header affordance: our own SVG, tinted, with an active state.
    fn tool(
        id: &'static str,
        src: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let theme = cx.theme();
        let color = if active {
            theme.primary
        } else {
            theme.muted_foreground
        };
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(22.))
            .rounded_sm()
            .when(active, |el| el.bg(theme.accent))
            .hover(|s| s.bg(theme.muted.opacity(0.6)))
            .cursor_pointer()
            .child(icons::icon(src, px(14.), color))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                on_click(this, window, cx);
            }))
            .into_any_element()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let mode = self.mode;
        let filter_open = self.filter_open;
        h_flex()
            .w_full()
            .h(px(32.))
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("BINDER"),
            )
            .child(Self::tool(
                "mode-files",
                icons::DOC,
                mode == Mode::Files,
                cx,
                |this, _, cx| {
                    this.mode = Mode::Files;
                    this.rebuild(cx);
                },
            ))
            .child(Self::tool(
                "mode-structure",
                icons::KNOT,
                mode == Mode::Structure,
                cx,
                |this, _, cx| {
                    this.mode = Mode::Structure;
                    this.rebuild(cx);
                },
            ))
            .child(Self::tool(
                "collapse-all",
                icons::COLLAPSE_ALL,
                false,
                cx,
                |this, _, cx| {
                    let keys: Vec<SharedString> = this
                        .rows
                        .iter()
                        .filter(|r| r.expandable)
                        .map(|r| r.key.clone())
                        .collect();
                    this.collapsed.extend(keys);
                    this.rebuild(cx);
                },
            ))
            .child(Self::tool(
                "expand-all",
                icons::EXPAND_ALL,
                false,
                cx,
                |this, _, cx| {
                    this.collapsed.clear();
                    this.rebuild(cx);
                },
            ))
            .child(Self::tool(
                "filter",
                icons::SEARCH,
                filter_open,
                cx,
                |this, window, cx| {
                    this.filter_open = !this.filter_open;
                    if this.filter_open {
                        this.filter.update(cx, |state, cx| state.focus(window, cx));
                    }
                    cx.notify();
                },
            ))
            .into_any_element()
    }
}

impl Binder {
    /// The ⋯ menu, anchored where it was opened. Same items as the
    /// right-click menu — one list, two affordances, as in the studio
    /// (`BinderContextMenu.tsx` is shared by both).
    fn render_row_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (key, position) = self.row_menu.clone()?;
        let row = self.rows.iter().find(|r| r.key == key)?.clone();
        let theme = cx.theme();
        let (fg, muted, accent, popover, border) = (
            theme.foreground,
            theme.muted_foreground,
            theme.accent,
            theme.popover,
            theme.border,
        );
        let item = |label: &'static str, cx: &mut Context<Self>, row: Row| {
            div()
                .id(label)
                .px_3()
                .py_1()
                .text_sm()
                .text_color(fg)
                .hover(move |s| s.bg(accent))
                .cursor_pointer()
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.row_menu = None;
                    if label == "Open" && row.kind != RowKind::Folder {
                        cx.emit(BinderEvent::Open {
                            path: row.path.clone(),
                            offset: row.offset,
                        });
                    }
                    cx.notify();
                }))
        };
        Some(
            deferred(
                anchored().position(point(position.x, position.y)).child(
                    v_flex()
                        .min_w(px(160.))
                        .py_1()
                        .rounded_md()
                        .bg(popover)
                        .border_1()
                        .border_color(border)
                        .shadow_md()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .child(row.label.clone()),
                        )
                        .child(item("Open", cx, row.clone()))
                        .child(item("Play from here", cx, row.clone()))
                        .child(item("Rename…", cx, row.clone()))
                        .child(item("Delete", cx, row)),
                ),
            )
            .into_any_element(),
        )
    }
}

impl EventEmitter<BinderEvent> for Binder {}

impl Focusable for Binder {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Binder {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows.len();
        let header = self.render_header(cx);
        let theme = cx.theme();
        let (sidebar, border, muted) = (theme.sidebar, theme.border, theme.muted_foreground);
        v_flex()
            .id("binder")
            .track_focus(&self.focus)
            .size_full()
            .bg(sidebar)
            .border_r_1()
            .border_color(border)
            .on_key_down(cx.listener(Self::on_key))
            .child(header)
            .when(self.filter_open, |el| {
                el.child(div().px_2().py_1().child(Input::new(&self.filter).xsmall()))
            })
            .child(
                uniform_list(
                    "binder-rows",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                        range.map(|i| this.render_row(i, cx)).collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1(),
            )
            .child(
                h_flex()
                    .h(px(22.))
                    .px_3()
                    .items_center()
                    .border_t_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{count} rows")),
            )
            .children(self.render_row_menu(cx))
            // Dropping anywhere clears the highlight even if no row took it.
            .on_drop(cx.listener(|this, _: &DraggedRow, _window, cx| {
                this.drop = None;
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    if this.row_menu.take().is_some() {
                        cx.notify();
                    }
                }),
            )
    }
}

/// What follows the pointer during a drag.
struct DragPreview {
    label: SharedString,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .rounded_md()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .text_sm()
            .text_color(theme.foreground)
            .child(self.label.clone())
    }
}

gpui::actions!(binder, [NoopAction]);

/// A symbol's own counts: diagnostics whose start falls inside its full body
/// range (`symbolMarks` in `Binder.tsx`).
fn symbol_marks(
    diagnostics: &[(String, usize, bool)],
    file: &str,
    full_start: usize,
    full_end: usize,
) -> Marks {
    let mut marks = Marks::default();
    for (path, start, is_error) in diagnostics {
        if path != file || *start < full_start || *start >= full_end {
            continue;
        }
        if *is_error {
            marks.errors += 1;
        } else {
            marks.warnings += 1;
        }
    }
    marks
}

// ── The Binder as a dock panel ───────────────────────────────────────

impl EventEmitter<gpui_component::dock::PanelEvent> for Binder {}

/// No badge: the Binder counts nothing the rail should shout about.
impl brink_gpui_shell::tool_window::ToolWindow for Binder {
    fn tab_slot(&self) -> Option<&brink_gpui_shell::tool_window::TabSlot> {
        Some(&self.tab)
    }
}

impl gpui_component::dock::BasePanel for Binder {
    fn panel_name(&self) -> &'static str {
        "Binder"
    }

    fn on_added_to(
        &mut self,
        group: gpui::WeakEntity<gpui_component::dock::TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tab.added_to(group);
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tab.removed();
    }
}

impl gpui_component::dock::Panel for Binder {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Binder")
    }
}
