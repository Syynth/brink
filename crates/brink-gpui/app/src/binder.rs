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
//! The drag order persists to the `.binder.json` sidecar
//! (`brink_gpui_model::binder_order`), which the PROJECT owns — it owns
//! the disk, so a rename re-keys the arrangement and a delete drops it
//! however the operation was asked for. This panel only says what moved.
//!
//! Everything the spike deliberately skipped has since landed: the
//! Library section, multi-select, creating a knot inline, and the undo
//! stack (`Project::undo_file_op`, reached from File ▸ Undo File
//! Operation — the operations are the Project's, since the Project owns
//! the disk).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, DragMoveEvent, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};

use crate::file_menu;
use crate::project::{Project, ProjectEvent};
use crate::symbol_menu;
use brink_gpui_shell::icons;

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
/// A row's icon, square. Its centre is where a child's guide line falls.
const ICON: f32 = 13.0;

/// Where the guide line under the ancestor at `depth` falls, from the row's
/// left edge: centred under that ancestor's icon. A row at `depth` starts
/// its icon at `PAD_X + depth × INDENT` — strictly, the web studio's
/// "depth is pad + n × indent" (`binder.css`, maintainer 2026-08-23).
fn guide_x(depth: usize) -> f32 {
    (PAD_X + depth as f32 * INDENT + ICON / 2.).floor()
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinderEvent {
    /// Open a file, optionally revealing a byte offset within it.
    Open {
        path: String,
        offset: Option<usize>,
    },
    /// Start the story at a knot or `knot.stitch`.
    Play {
        path: String,
    },
    /// A file operation the studio runs: it owns the prompts and the
    /// dialogs, and the panel owns only the rows they were asked from.
    NewFile {
        folder: String,
    },
    RenameFile {
        path: String,
    },
    /// Delete these files — the row's own, or the whole selection when
    /// the row is part of one.
    DeleteFile {
        paths: Vec<String>,
    },
    /// Make an empty folder in `folder`.
    NewFolder {
        folder: String,
    },
    /// Move folder `folder` (no trailing slash) and everything in it.
    RenameFolder {
        folder: String,
    },
    /// Delete folder `folder` — `paths`, every file in it.
    DeleteFolder {
        folder: String,
        paths: Vec<String>,
    },
    /// Write a new knot at the end of `path`.
    NewKnot {
        path: String,
    },
    /// Write a new stitch at the end of the knot ending at `full_end`.
    NewStitch {
        path: String,
        full_end: usize,
    },
    /// Lift a stitch out of its knot and make it a knot of its own.
    Promote {
        path: String,
        knot: String,
        stitch: String,
    },
    /// Fold a knot into `into` (or the knot above it), as a stitch.
    Demote {
        path: String,
        knot: String,
        into: Option<String>,
    },
    /// Move a stitch out of its knot into `dest`.
    MoveStitch {
        path: String,
        knot: String,
        stitch: String,
        dest: String,
    },
    /// Move a knot, or one of its stitches, one place up or down.
    Reorder {
        path: String,
        knot: String,
        stitch: Option<String>,
        up: bool,
    },
    /// Rename the knot or stitch whose name starts at `offset`, now `name`.
    RenameSymbol {
        path: String,
        offset: usize,
        name: String,
    },
}

/// The row menu's "Play from here": the knot or `knot.stitch` path, as the
/// runtime addresses it. Dispatched with the Binder as the action context,
/// so it lands here whichever editor had focus when the menu opened.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = binder, no_json)]
pub struct PlayFromHere {
    pub path: String,
}

/// A row's menu, shared by its right-click and its `⋯`.
type RowMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

impl Row {
    /// What a structural move would act on, read off the row's key
    /// (`file::knot[::stitch]`). `None` for a file or a folder, which have
    /// no shape to change.
    fn structural(&self) -> Option<Structural> {
        let mut parts = self.key.split("::").skip(1);
        let knot = parts.next()?.to_owned();
        match (self.kind, parts.next()) {
            (RowKind::Stitch, Some(stitch)) => Some(Structural::Stitch {
                knot,
                stitch: stitch.to_owned(),
            }),
            (RowKind::Knot, None) => Some(Structural::Knot { knot }),
            _ => None,
        }
    }
}

/// The rows between two indices, the anchor left out — it is already
/// selected, and holding it in both places would double-count it.
#[must_use]
fn marked_range(
    rows: &[Row],
    anchor: usize,
    index: usize,
    anchor_key: Option<&SharedString>,
) -> BTreeSet<SharedString> {
    let (lo, hi) = (anchor.min(index), anchor.max(index));
    let Some(slice) = rows.get(lo..=hi) else {
        return BTreeSet::new();
    };
    slice
        .iter()
        .map(|r| r.key.clone())
        .filter(|key| Some(key) != anchor_key)
        .collect()
}

/// Every selected FILE row's path, in row order. A folder row and a
/// symbol row are not files and are left out — a delete acts on files.
#[must_use]
fn files_in(
    rows: &[Row],
    selected: Option<&SharedString>,
    marked: &BTreeSet<SharedString>,
) -> Vec<String> {
    rows.iter()
        .filter(|row| {
            row.kind == RowKind::File && (selected == Some(&row.key) || marked.contains(&row.key))
        })
        .map(|row| row.path.clone())
        .collect()
}

/// Which structural move a row offers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Structural {
    Knot { knot: String },
    Stitch { knot: String, stitch: String },
}

// ── Tree ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<String>,
}

/// The tree of folders and files. `empty` are folders made in the app
/// with nothing in them yet (`BinderOrder::folders`, trailing slash): a
/// tree read off files alone would not show them.
fn build_folder_tree(files: &[String], empty: &[String]) -> Folder {
    let mut root = Folder::default();
    for folder in empty {
        let mut cursor = &mut root;
        for segment in folder.trim_end_matches('/').split('/') {
            cursor = cursor.folders.entry(segment.to_owned()).or_default();
        }
    }
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
    order: &BTreeMap<String, Vec<String>>,
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
    selected: Option<SharedString>,
    /// Rows selected ALONGSIDE `selected` — a shift-range or a
    /// cmd-clicked scatter. `selected` stays the anchor a range extends
    /// from and the row the keyboard moves; this is everything else that
    /// is lit up, and it is what a delete acts on when it is not empty.
    marked: BTreeSet<SharedString>,
    rows: Vec<Row>,
    filter: Entity<InputState>,
    filter_open: bool,
    filter_text: String,
    drop: Option<DropTarget>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// The dock tab this panel sits in, for the rail to select.
    tab: brink_gpui_shell::tool_window::TabSlot,
    /// Files only: no Files/Structure switch, and `mode` stays `Files`.
    /// Writing mode's sidebar is one of these, beside its own structure
    /// column (`crate::write_view`).
    files_only: bool,
    /// The header's label.
    title: SharedString,
    /// The host's own control at the header's end — Writing mode's
    /// structure-column toggle (W5).
    accessory: Option<HeaderAccessory>,
    _subs: Vec<gpui::Subscription>,
}

/// Draws the host's control at the end of a Binder's header.
pub type HeaderAccessory = std::rc::Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

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
                // A file was created, renamed or deleted: the tree is a
                // different tree now.
                ProjectEvent::FilesChanged => {
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
                // Text moving inside a file changes no row; a save changes
                // no row either.
                ProjectEvent::OpenFailed(_)
                | ProjectEvent::SourceChanged { .. }
                | ProjectEvent::BreakpointsChanged
                | ProjectEvent::ProseChanged
                | ProjectEvent::ProseOptionsChanged
                | ProjectEvent::DiskChanged(_)
                | ProjectEvent::Saved
                | ProjectEvent::SaveFailed { .. } => {}
            }
        });
        let mut this = Self {
            project,
            symbols: HashMap::new(),
            pending_symbols: HashSet::new(),
            mode: Mode::Files,
            collapsed: HashSet::new(),
            selected: None,
            marked: BTreeSet::new(),
            rows: Vec::new(),
            filter,
            filter_open: false,
            filter_text: String::new(),
            drop: None,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            tab: brink_gpui_shell::tool_window::TabSlot::default(),
            files_only: false,
            title: "BINDER".into(),
            accessory: None,
            _subs: vec![sub, watch],
        };
        this.rebuild(cx);
        this
    }

    /// A Binder that shows files only, under `title` — every file
    /// interaction the Binder has (folders, drag to reorder or move,
    /// multi-select, New/Rename/Delete, the row menus, the keyboard), and
    /// no Files/Structure switch.
    #[must_use]
    pub fn files_only(mut self, title: impl Into<SharedString>) -> Self {
        self.files_only = true;
        self.mode = Mode::Files;
        self.title = title.into();
        self
    }

    /// Put the host's control at the end of the header.
    pub fn set_header_accessory(&mut self, accessory: HeaderAccessory, cx: &mut Context<Self>) {
        self.accessory = Some(accessory);
        cx.notify();
    }

    /// Rebuild the flat row list. Called on every input that can change it —
    /// mode, collapse, filter, order, or the project's own analysis.
    /// The selected row's key, for the tests.
    #[cfg(test)]
    pub(crate) fn selected_key(&self) -> Option<SharedString> {
        self.selected.clone()
    }

    /// Show a file's row: open every folder above it, select it, and
    /// scroll it into view — a manuscript separator's "Reveal in Files".
    pub fn reveal_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let mut folder = String::new();
        for part in path
            .split('/')
            .collect::<Vec<_>>()
            .split_last()
            .map_or(&[][..], |(_, dirs)| dirs)
        {
            folder.push_str(part);
            folder.push('/');
            self.collapsed.remove(&SharedString::from(folder.clone()));
        }
        self.rebuild(cx);
        if let Some(index) = self.rows.iter().position(|r| r.key.as_ref() == path) {
            self.select_index(index, cx);
        }
    }

    pub fn rebuild(&mut self, cx: &mut Context<Self>) {
        let (sources, config, artifacts, library, entry, closure, diagnostics, drafts) = {
            let project = self.project.read(cx);
            (
                project.files().to_vec(),
                project.config_path().map(str::to_owned),
                project.artifacts().to_vec(),
                project.library(),
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
            self.request_symbols(&sources, cx);
        }
        let symbols = self.symbols.clone();

        // `brink.toml` is listed beside the sources (its click opens
        // Settings — ruled 2026-08-27, "brink.toml opens in the Settings
        // takeover, in every view"), but it is not a source: it has no
        // symbols to ask for and is never "outside the story".
        let mut files = sources;
        if let Some(config) = &config {
            files.push(config.clone());
        }
        // The config's artifacts (`dialect.json`) list beside it: the
        // Conventions section writes one, and until now nothing in the
        // studio could open what it had written.
        files.extend(artifacts.iter().cloned());
        // The Library — the mounted stdlib (ruled 2026-08-06). Listed
        // last and under its own folder, since `std/` is the key prefix
        // the session mounts them at, so the tree builder puts them in a
        // folder of that name with no special case here.
        files.extend(library.iter().map(|(key, _)| (*key).to_owned()));

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

        // Read once per rebuild: the authored order lives in the project,
        // which owns the sidecar on disk.
        let (order, empty) = {
            let sidecar = self.project.read(cx).binder_order();
            (sidecar.order.clone(), sidecar.folders.clone())
        };
        let tree = build_folder_tree(&files, &empty);
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
            &order,
            &mut rows,
        );

        for row in &mut rows {
            if config.as_deref() == Some(row.path.as_str()) {
                row.dimmed = false;
            }
        }

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
        // The authored order, from the project's `.binder.json`.
        order: &BTreeMap<String, Vec<String>>,
        out: &mut Vec<Row>,
    ) {
        for child in ordered_children(folder, parent_key, entry, order) {
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
                            order,
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
                        // Knots (functions among them) and their stitches
                        // only: the outline also carries `VAR`/`CONST`/`LIST`
                        // declarations, which drew here as knot rows —
                        // offering "Play from here" and "Demote" on a
                        // variable. Writing mode's sidebar shows them as
                        // what they are.
                        this.symbols.insert(
                            path,
                            found
                                .iter()
                                .filter(|s| s.kind == brink_ir::SymbolKind::Knot)
                                .map(convert_symbol)
                                .collect(),
                        );
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

    /// A plain click: the anchor moves, the scatter is dropped, the row
    /// opens.
    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        self.marked.clear();
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

    /// A shift-click: everything from the anchor to here. With no anchor
    /// it is a plain click, since a range needs two ends.
    fn extend_to(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(anchor) = self.selected_index() else {
            self.activate(index, cx);
            return;
        };
        self.marked = marked_range(&self.rows, anchor, index, self.selected.as_ref());
        cx.notify();
    }

    /// A cmd-click: this row joins or leaves the selection, and nothing
    /// opens. Clicking the ANCHOR itself moves the anchor to another
    /// marked row rather than leaving a selection with no anchor.
    fn toggle_marked(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        if self.selected.as_ref() == Some(&row.key) {
            self.selected = self.marked.iter().next().cloned();
            if let Some(next) = self.selected.clone() {
                self.marked.remove(&next);
            }
        } else if !self.marked.remove(&row.key) {
            if self.selected.is_none() {
                self.selected = Some(row.key.clone());
            } else {
                self.marked.insert(row.key.clone());
            }
        }
        cx.notify();
    }

    /// Every selected FILE, anchor included, in row order. Empty when the
    /// selection holds no files — a folder or a symbol row is not one.
    fn selected_files(&self) -> Vec<String> {
        files_in(&self.rows, self.selected.as_ref(), &self.marked)
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().position(|r| &r.key == selected)
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            self.marked.clear();
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
                self.project.update(cx, |project, cx| {
                    project.reorder_binder(&parent, siblings, cx)
                });
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
                self.project.update(cx, |project, cx| {
                    project.reorder_binder(key.as_ref(), siblings, cx);
                });
                self.collapsed.remove(&key);
            }
        }
        self.rebuild(cx);
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn icon_for(row: &Row) -> icons::BrinkIcon {
        // The fill rule: filled = collapsed over content, outline =
        // expanded or a leaf.
        let filled = row.expandable && !row.expanded;
        match row.kind {
            RowKind::Folder => {
                if row.expandable && row.expanded {
                    icons::BrinkIcon::TreeFolderOpen
                } else if filled {
                    icons::BrinkIcon::TreeFolderFilled
                } else {
                    // An empty folder: a leaf, so the outline.
                    icons::BrinkIcon::TreeFolder
                }
            }
            RowKind::File => {
                // The config and its artifacts are documents ABOUT the
                // story, not part of it — the ink drop is for story text.
                if row.path.ends_with(".toml") || row.path.ends_with(".json") {
                    icons::BrinkIcon::Doc
                } else if row.draft {
                    // Dashed, whether or not the row is selected: being a
                    // draft is a property of the file, not of the selection.
                    icons::BrinkIcon::DropDraft
                } else if row.entry {
                    if filled {
                        icons::BrinkIcon::DropEntry
                    } else {
                        icons::BrinkIcon::DropEntryOutline
                    }
                } else if filled {
                    icons::BrinkIcon::DropFilled
                } else {
                    icons::BrinkIcon::Drop
                }
            }
            RowKind::Knot => {
                if row.is_function {
                    icons::BrinkIcon::Function
                } else if filled {
                    icons::BrinkIcon::KnotFilled
                } else {
                    icons::BrinkIcon::Knot
                }
            }
            RowKind::Stitch => icons::BrinkIcon::Stitch,
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
        // One menu, two ways to open it: right-click and the `⋯`.
        let row_menu = self.menu_for(&row, cx);
        let dots_menu = row_menu.clone();
        let theme = cx.theme();
        let selected = self.selected.as_ref() == Some(&row.key) || self.marked.contains(&row.key);
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
        let kind_for_move = row.kind;

        // Indent guides, Zed's placement (the web studio's `binder.css`,
        // maintainer 2026-08-23): one hairline per ancestor, centred under
        // that ancestor's icon. An overlay rather than spacer boxes in the
        // row: as flex children they picked up the row's gap after each
        // one, so a level was 26px instead of 18, and each line sat at its
        // box's left edge — under the edge of the parent's icon, not its
        // middle.
        let guides = (0..row.depth).map(|ancestor| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(guide_x(ancestor)))
                .w(px(1.))
                .bg(theme.border.opacity(0.5))
        });

        let marks = (!row.marks.is_empty()).then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .when(row.marks.errors > 0, |el| {
                    el.child(icons::icon(
                        icons::BrinkIcon::ErrorMark,
                        px(8.),
                        theme.danger,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.danger)
                            .child(row.marks.errors.to_string()),
                    )
                })
                .when(row.marks.warnings > 0, |el| {
                    el.child(icons::icon(
                        icons::BrinkIcon::WarningMark,
                        px(10.),
                        theme.warning,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.warning)
                            .child(row.marks.warnings.to_string()),
                    )
                })
        });

        let element = div()
            .id(("binder-row", index))
            .relative()
            .w_full()
            .h(px(ROW_HEIGHT))
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .pl(px(PAD_X + row.depth as f32 * INDENT))
                    .pr_2()
                    .items_center()
                    .gap_2()
                    .when(selected, |el| el.bg(theme.accent))
                    .when(drop_into, |el| el.bg(theme.primary.opacity(0.16)))
                    .when(!selected && !drop_into, |el| {
                        el.hover(|s| s.bg(theme.muted.opacity(0.5)))
                    })
                    .children(guides)
                    .child(icons::icon(Self::icon_for(&row), px(ICON), icon_color))
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
                    .children(dots_menu.map(|dots_menu| {
                        div().invisible().group_hover("", |s| s.visible()).child(
                            Button::new(("row-actions", index))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Ellipsis)
                                .dropdown_menu(move |menu, window, cx| dots_menu(menu, window, cx)),
                        )
                    })),
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
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let modifiers = event.modifiers();
                if modifiers.shift {
                    this.extend_to(index, cx);
                } else if modifiers.secondary() {
                    this.toggle_marked(index, cx);
                } else {
                    this.activate(index, cx);
                }
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
            }));
        match row_menu {
            Some(row_menu) => element
                .context_menu(move |menu, window, cx| row_menu(menu, window, cx))
                .into_any_element(),
            None => element.into_any_element(),
        }
    }

    /// The menu a row opens. A knot or stitch row's is the shared symbol
    /// menu ([`symbol_menu`]); a file's and a folder's are the file
    /// operations.
    fn menu_for(&self, row: &Row, cx: &mut Context<Self>) -> Option<RowMenu> {
        let me = cx.entity().downgrade();
        let emit: symbol_menu::Emit = Rc::new(move |event, _, cx| {
            let _ = me.update(cx, |this, cx| {
                // A menu opened on a row that is part of a selection deletes
                // the SELECTION: the rows are lit up, and deleting one of
                // them while the rest stayed would be a surprise.
                let event = match event {
                    BinderEvent::DeleteFile { paths } => {
                        let selected = this.selected_files();
                        BinderEvent::DeleteFile {
                            paths: if paths.iter().all(|p| selected.contains(p)) {
                                selected
                            } else {
                                paths
                            },
                        }
                    }
                    event => event,
                };
                cx.emit(event);
            });
        });
        let library = self.project.read(cx).is_library(&row.path);
        if let Some(structural) = row.structural() {
            let (knot, stitch) = match structural {
                Structural::Knot { knot } => (knot, None),
                Structural::Stitch { knot, stitch } => (knot, Some(stitch)),
            };
            let outline = self
                .symbols
                .get(&row.path)
                .map(|nodes| symbol_menu::outline_from_nodes(nodes))
                .unwrap_or_default();
            let target = symbol_menu::Target {
                path: row.path.clone(),
                knot,
                stitch,
                library,
                outline: Rc::new(outline),
            };
            return Some(Rc::new(move |menu, window, cx| {
                symbol_menu::build(menu, &target, &emit, window, cx)
            }));
        }
        let project = self.project.read(cx);
        let target = match row.kind {
            RowKind::File => file_menu::Target::File {
                path: row.path.clone(),
            },
            RowKind::Folder => {
                let path = row.path.trim_end_matches('/').to_owned();
                // The mounted library's folder: nothing in it is the
                // author's, so it offers nothing at all.
                let mounted = project
                    .library()
                    .iter()
                    .any(|(key, _)| key.starts_with(&row.path));
                let files = project.files_under(&path);
                if mounted && files.is_empty() {
                    return None;
                }
                file_menu::Target::Folder { path, files }
            }
            RowKind::Knot | RowKind::Stitch => return None,
        };
        let open = (row.kind == RowKind::File).then(|| row.path.clone());
        Some(Rc::new(move |menu, _, _| {
            let click = |event: BinderEvent| {
                let emit = emit.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    emit(event.clone(), window, cx);
                }
            };
            // The Binder's own items first: Open, and — the Binder's only
            // way to make one — New Knot….
            let menu = match &open {
                Some(path) => {
                    let menu = menu.item(PopupMenuItem::new("Open").on_click(click(
                        BinderEvent::Open {
                            path: path.clone(),
                            offset: None,
                        },
                    )));
                    if library {
                        return menu;
                    }
                    menu.item(
                        PopupMenuItem::new("New Knot\u{2026}")
                            .on_click(click(BinderEvent::NewKnot { path: path.clone() })),
                    )
                    .separator()
                }
                None => menu,
            };
            file_menu::build(menu, &target, &emit)
        }))
    }

    /// A header affordance: our own SVG, tinted, with an active state.
    fn tool(
        id: &'static str,
        src: icons::BrinkIcon,
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

    fn render_header(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Copied out of the theme before the chain: `Self::tool` takes
        // `&mut cx`, and a live `&Theme` read between two of them keeps an
        // immutable borrow across it.
        let (border, muted) = {
            let theme = cx.theme();
            (theme.border, theme.muted_foreground)
        };
        let mode = self.mode;
        let filter_open = self.filter_open;
        h_flex()
            .w_full()
            .h(px(brink_gpui_shell::tool_window::HEADER_HEIGHT))
            .flex_none()
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(muted)
                    .child(self.title.clone()),
            )
            .child(Self::tool(
                "new-file",
                icons::BrinkIcon::Add,
                false,
                cx,
                |_, _, cx| {
                    // At the root: the header belongs to the whole tree,
                    // and a row's own menu is where "beside this one"
                    // lives.
                    cx.emit(BinderEvent::NewFile {
                        folder: String::new(),
                    });
                },
            ))
            // Files and Structure are ALTERNATIVES, and sitting loose in
            // a row of actions they read as two more buttons to press.
            // One border around the pair is what says "pick one" — the
            // active state itself was never the problem here, `tool`
            // already tints and fills it.
            .when(!self.files_only, |el| {
                el.child(
                    h_flex()
                        .rounded_sm()
                        .border_1()
                        .border_color(border)
                        .child(Self::tool(
                            "mode-files",
                            icons::BrinkIcon::Doc,
                            mode == Mode::Files,
                            cx,
                            |this, _, cx| {
                                this.mode = Mode::Files;
                                this.rebuild(cx);
                            },
                        ))
                        .child(Self::tool(
                            "mode-structure",
                            icons::BrinkIcon::Knot,
                            mode == Mode::Structure,
                            cx,
                            |this, _, cx| {
                                this.mode = Mode::Structure;
                                this.rebuild(cx);
                            },
                        )),
                )
            })
            .child(Self::tool(
                "collapse-all",
                icons::BrinkIcon::CollapseAll,
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
                icons::BrinkIcon::ExpandAll,
                false,
                cx,
                |this, _, cx| {
                    this.collapsed.clear();
                    this.rebuild(cx);
                },
            ))
            .child(Self::tool(
                "filter",
                icons::BrinkIcon::Find,
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
            .children(self.accessory.as_ref().map(|draw| draw(window, cx)))
            .into_any_element()
    }
}

impl EventEmitter<BinderEvent> for Binder {}

impl Focusable for Binder {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Binder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows.len();
        let header = self.render_header(window, cx);
        let theme = cx.theme();
        let (sidebar, border, muted) = (theme.sidebar, theme.border, theme.muted_foreground);
        v_flex()
            .id("binder")
            .track_focus(&self.focus)
            .key_context(brink_gpui_shell::tool_window::TOOL_WINDOW_CONTEXT)
            .on_action(cx.listener(|_, action: &PlayFromHere, _, cx| {
                cx.emit(BinderEvent::Play {
                    path: action.path.clone(),
                });
            }))
            .size_full()
            .bg(sidebar)
            // A docked Binder draws its own edge. A files-only one is a
            // column inside a host that already rules its columns apart, and
            // drawing one here too doubled the line to 2px.
            .when(!self.files_only, |el| el.border_r_1().border_color(border))
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
            // Dropping anywhere clears the highlight even if no row took it.
            .on_drop(cx.listener(|this, _: &DraggedRow, _window, cx| {
                this.drop = None;
                cx.notify();
            }))
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

    /// The Binder draws its own header edge to edge, like every other tool
    /// window here. Without this it took the skin's default `pt_2` — the
    /// kit applies it to the active panel of any group holding more than
    /// one, which the left dock does (Binder + Search) — and since Search
    /// already opted out, the gap appeared above one tab and not the
    /// other.
    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

/// The picture: nested folders in the Binder, for checking the guides by
/// eye. Written beside the test output.
#[cfg(test)]
mod driven {
    use crate::harness::{Harness, scratch_dir};

    /// A folder made in the app, with nothing in it yet, is still a
    /// folder in the tree — and a file-derived one is not doubled.
    #[test]
    fn an_empty_folder_from_the_sidecar_is_in_the_tree() {
        let tree = super::build_folder_tree(
            &["acts/one.ink".to_owned()],
            &["acts/drafts/".to_owned(), "acts/".to_owned()],
        );
        let acts = tree.folders.get("acts").expect("acts");
        assert_eq!(acts.files, ["acts/one.ink"]);
        assert!(acts.folders.contains_key("drafts"));
        assert_eq!(tree.folders.len(), 1);
    }

    #[test]
    fn nested_folders_render_with_their_guides() {
        let dir = scratch_dir("binder");
        let files = [
            (
                "main.ink",
                "INCLUDE clues/clue_case_file.ink\nINCLUDE lib/functions.ink\n-> END\n",
            ),
            ("clues/clue_case_file.ink", "A case file.\n"),
            ("clues/clue_parents_letter.ink", "A letter.\n"),
            ("lib/functions.ink", "=== function f ===\n~ return 1\n"),
            ("lib/lists.ink", "LIST colours = red, blue\n"),
            ("lib/deep/more.ink", "More.\n"),
        ];
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("has a folder")).expect("folders");
            std::fs::write(path, text).expect("writing");
        }
        let mut h = Harness::new();
        let window = h.open(&dir.join("main.ink"));
        let shot = scratch_dir("shot").join("binder.png");
        h.screenshot(window, &shot);
        eprintln!("binder screenshot: {}", shot.display());
    }
}

#[cfg(test)]
mod guide_tests {
    use super::{ICON, INDENT, PAD_X, guide_x};

    #[test]
    fn a_guide_falls_under_the_middle_of_its_ancestors_icon() {
        for depth in 0..4 {
            let icon_left = PAD_X + depth as f32 * INDENT;
            let x = guide_x(depth);
            assert!(
                x >= icon_left + ICON / 2. - 1. && x <= icon_left + ICON / 2.,
                "depth {depth}: line at {x}, icon {icon_left}..{}",
                icon_left + ICON
            );
        }
        // Levels are exactly one indent apart: no gap rides along.
        assert_eq!(guide_x(1) - guide_x(0), INDENT);
    }
}

#[cfg(test)]
mod tests {
    use super::{Marks, Row, RowKind, files_in, marked_range};
    use gpui::SharedString;
    use std::collections::BTreeSet;

    fn row(key: &str, kind: RowKind) -> Row {
        Row {
            key: key.into(),
            kind,
            depth: 0,
            label: key.into(),
            path: key.to_owned(),
            offset: None,
            expandable: false,
            expanded: false,
            entry: false,
            draft: false,
            dimmed: false,
            is_function: false,
            marks: Marks::default(),
            parent: String::new(),
        }
    }

    fn rows() -> Vec<Row> {
        vec![
            row("a.ink", RowKind::File),
            row("acts", RowKind::Folder),
            row("b.ink", RowKind::File),
            row("c.ink", RowKind::File),
        ]
    }

    #[test]
    fn a_range_covers_both_ends_and_leaves_the_anchor_out_of_the_scatter() {
        let rows = rows();
        let anchor: SharedString = "a.ink".into();
        let marked = marked_range(&rows, 0, 2, Some(&anchor));
        assert_eq!(
            marked
                .iter()
                .map(SharedString::to_string)
                .collect::<Vec<_>>(),
            ["acts", "b.ink"],
            "the anchor is selected already; the range adds the rest"
        );
        // Dragging the range BACKWARDS covers the same rows.
        assert_eq!(marked_range(&rows, 2, 0, Some(&anchor)), marked);
        // An index past the end selects nothing rather than panicking.
        assert!(marked_range(&rows, 0, 99, Some(&anchor)).is_empty());
    }

    #[test]
    fn only_file_rows_are_what_a_delete_acts_on() {
        let rows = rows();
        let anchor: SharedString = "a.ink".into();
        let marked: BTreeSet<SharedString> = ["acts".into(), "c.ink".into()].into_iter().collect();
        assert_eq!(
            files_in(&rows, Some(&anchor), &marked),
            ["a.ink", "c.ink"],
            "the folder is selected but is not a file"
        );
        // A selection of nothing but a folder deletes nothing.
        let folder: SharedString = "acts".into();
        assert!(files_in(&rows, Some(&folder), &BTreeSet::new()).is_empty());
    }
}
