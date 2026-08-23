import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Overlay } from "@brink/studio-shell";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isOutOfScope } from "./InkFileDocument.js";
import {
  BinderContextMenu,
  type ContextMenuAction,
  type ContextMenuTarget,
} from "./BinderContextMenu.js";
import { dispatchSymbolAction } from "./symbolMenuActions.js";
import type { FileOutline, DocumentSymbol } from "@brink/wasm-types";
import type { TabTarget } from "@brink/studio-store";

// ── Icons ──────────────────────────────────────────────────────────

const ICON_FILE = "\ud83d\udcc4";
const ICON_FOLDER = "\ud83d\udcc1"; // \ud83d\udcc1
const ICON_KNOT = "\u25c6";
const ICON_STITCH = "\u25c7";
const ICON_FUNCTION = "\u0192"; // \u0192 \u2014 a knot declared as a function
/** The Binder's "Library" section (mounted `std/` files, issue #2306/#2343):
 *  a closed-book glyph, visually distinct from the project's own open-book
 *  file/folder icons. */
const ICON_LIBRARY = "\ud83d\udcda";

/** Synthetic row key for the Library section's own collapse toggle \u2014 distinct
 *  from any real file/folder key (which never contain a NUL byte). */
export const LIBRARY_ROW_KEY = "\u0000library";

/** Prefix applied to a mounted folder's own key so it never collides with the
 *  identically-named real-project folder key in the shared `collapsed` set. */
function libraryFolderKey(folderKey: string): string {
  return `${LIBRARY_ROW_KEY}:${folderKey}`;
}

function iconChar(kind: string, isFunction = false): string {
  switch (kind) {
    case "folder":
      return ICON_FOLDER;
    case "file":
      return ICON_FILE;
    case "knot":
      return isFunction ? ICON_FUNCTION : ICON_KNOT;
    case "stitch":
      return ICON_STITCH;
    case "library":
      return ICON_LIBRARY;
    default:
      return "\u00b7";
  }
}

function iconClass(kind: string, isFunction = false): string {
  switch (kind) {
    case "folder":
      return "brink-binder-icon-folder";
    case "file":
      return "brink-binder-icon-file";
    case "knot":
      return isFunction ? "brink-binder-icon-function" : "brink-binder-icon-knot";
    case "stitch":
      return "brink-binder-icon-stitch";
    case "library":
      return "brink-binder-icon-library";
    default:
      return "";
  }
}

function displayName(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash >= 0 ? path.substring(slash + 1) : path;
}

/** Directory of a file path, with trailing slash; "" for a root-level file. */
function dirOf(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash >= 0 ? path.substring(0, slash + 1) : "";
}

/** Parent directory of a folder prefix ("a/b/" → "a/", "scenes/" → ""). */
function parentOf(prefix: string): string {
  const noTrail = prefix.replace(/\/$/, "");
  const slash = noTrail.lastIndexOf("/");
  return slash >= 0 ? noTrail.substring(0, slash + 1) : "";
}

// ── Inline rename input ────────────────────────────────────────────

interface RenameInputProps {
  initial: string;
  onCommit: (value: string) => void;
  onCancel: () => void;
}

/** In-row rename field: Enter (or blur) commits, Escape cancels. A guard ref
 *  prevents the blur fired by commit-driven unmount from double-firing. */
function RenameInput({ initial, onCommit, onCancel }: RenameInputProps) {
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);

  useEffect(() => {
    const input = ref.current;
    if (!input) return;
    input.focus();
    // Pre-select the basename (without extension) for a quick retype.
    const dot = initial.lastIndexOf(".");
    // SELECT-INVARIANT Binder.renameInput.preSelectBasename: runs synchronously
    // in this mount effect, right after input.focus() — no deferred frame, no
    // window in which the user could have typed before this selection lands.
    // `key={editing.initial}` on the call site (Binder.tsx below) guarantees
    // this effect only ever runs on a fresh mount for a given `initial`, so a
    // same-instance `initial` change can never re-run it over user-typed text.
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);
  }, [initial]);

  const commit = useCallback(() => {
    if (done.current) return;
    done.current = true;
    onCommit(ref.current?.value ?? initial);
  }, [onCommit, initial]);

  const cancel = useCallback(() => {
    if (done.current) return;
    done.current = true;
    onCancel();
  }, [onCancel]);

  return (
    <input
      ref={ref}
      className="brink-binder-rename-input"
      defaultValue={initial}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
        } else if (e.key === "Escape") {
          e.preventDefault();
          cancel();
        }
      }}
      onBlur={commit}
    />
  );
}

// ── Drag state types ────────────────────────────────────────────────

interface DragState {
  sourceKeys: string[];
  sourceKind: "knot" | "stitch" | "file";
  sourcePath: string;
  sourceParent?: string;
}

interface DropTarget {
  kind: "between" | "into";
  afterKey?: string;
  targetKey?: string;
}

// ── Row component ──────────────────────────────────────────────────

interface RowProps {
  rowKey: string;
  depth: number;
  kind: string;
  /** For knot rows: whether the knot is declared as a function. */
  isFunction?: boolean;
  label: string;
  expandable: boolean;
  isExpanded: boolean;
  isActive: boolean;
  isSelected: boolean;
  isFocused: boolean;
  isDragging: boolean;
  isDropInto: boolean;
  dropLinePosition: "before" | "after" | null;
  draggable: boolean;
  /** When set, the row renders an inline rename field instead of its label. */
  editing?: RenameInputProps;
  /** Scope badge after the label (#3014/#3021 — Binder.dc.html): the entry
   *  mark or the "not included" mark. */
  badge?: { text: string; tone: "entry" | "muted" };
  /** Dim the row (a file on disk that nothing INCLUDEs — outside the
   *  compile closure). */
  dimmed?: boolean;
  onChevronClick: () => void;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onDragStart: (e: React.DragEvent) => void;
  onDragEnd: () => void;
  onDragOver: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent) => void;
}

/**
 * Exported for `binder-seed-race.test.tsx` (#2571): the `key={editing.initial}`
 * remount below is only observable at this level. Inside the full `Binder` a
 * row's key and its `editing.initial` are both derived from the same path, so
 * they cannot diverge there — the row that would prove the guard works can
 * only be rendered directly.
 */
export function BinderRow({
  rowKey,
  depth,
  kind,
  isFunction = false,
  label,
  expandable,
  isExpanded,
  isActive,
  isSelected,
  isFocused,
  isDragging,
  isDropInto,
  dropLinePosition,
  draggable,
  editing,
  badge,
  dimmed = false,
  onChevronClick,
  onClick,
  onDoubleClick,
  onContextMenu,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDrop,
}: RowProps) {
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Cancel the pending single-click timer if the row unmounts (e.g. a
  // reorder/reparent drops it) so it can't fire onClick after teardown.
  useEffect(
    () => () => {
      if (clickTimer.current) clearTimeout(clickTimer.current);
    },
    [],
  );

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      if (clickTimer.current) clearTimeout(clickTimer.current);
      clickTimer.current = setTimeout(() => {
        clickTimer.current = null;
        onClick(e);
      }, 200);
    },
    [onClick],
  );

  const handleDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      if (clickTimer.current) {
        clearTimeout(clickTimer.current);
        clickTimer.current = null;
      }
      onDoubleClick();
    },
    [onDoubleClick],
  );

  const handleChevronClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onChevronClick();
    },
    [onChevronClick],
  );

  const rowClass =
    "brink-binder-row" +
    (kind === "folder" ? " brink-binder-folder-row" : "") +
    (kind === "file" ? " brink-binder-file-row" : "") +
    (kind === "knot" ? " brink-binder-knot" : "") +
    (kind === "stitch" ? " brink-binder-stitch" : "") +
    (isActive ? " brink-binder-active" : "") +
    (isSelected ? " brink-binder-selected" : "") +
    (isFocused ? " brink-binder-focused" : "") +
    (isDragging ? " brink-binder-dragging" : "") +
    (isDropInto ? " brink-binder-drop-into" : "") +
    (dimmed ? " brink-binder-dimmed" : "") +
    (badge?.tone === "entry" ? " brink-binder-entry" : "");

  const chevronClass =
    "brink-binder-chevron" +
    (expandable ? (isExpanded ? "" : " collapsed") : " leaf");

  return (
    <>
      {dropLinePosition === "before" && <div className="brink-binder-drop-line" />}
      <div
        className={rowClass}
        data-binder-row-key={rowKey}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
        onContextMenu={onContextMenu}
        draggable={draggable}
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
        onDragOver={onDragOver}
        onDrop={onDrop}
      >
        <div className="brink-binder-guides">
          {Array.from({ length: depth }, (_, i) => (
            <div key={i} className="brink-binder-guide" />
          ))}
        </div>
        <div className={chevronClass} onClick={handleChevronClick}>
          {expandable ? "\u25b6" : ""}
        </div>
        <span className={"brink-binder-icon " + iconClass(kind, isFunction)}>
          {iconChar(kind, isFunction)}
        </span>
        {editing ? (
          // `key={editing.initial}` forces a fresh RenameInput instance (and
          // thus a fresh mount effect) whenever `initial` changes, so the
          // SELECT-INVARIANT justification below — "no window in which the
          // user could have typed before this selection lands" — is true by
          // construction rather than merely true today: without the key, a
          // same-instance `initial` change would re-run the effect over
          // whatever the user had already typed into the uncontrolled
          // `defaultValue` input (React does not re-apply `defaultValue` on
          // rerender).
          <RenameInput key={editing.initial} {...editing} />
        ) : (
          <span className="brink-binder-label">{label}</span>
        )}
        {badge !== undefined && !editing && (
          <span className={"brink-binder-badge brink-binder-badge-" + badge.tone}>
            {badge.text}
          </span>
        )}
      </div>
      {dropLinePosition === "after" && <div className="brink-binder-drop-line" />}
    </>
  );
}

// ── Flat row key list builder ───────────────────────────────────────

interface FlatRow {
  key: string;
  kind: "folder" | "file" | "knot" | "stitch";
  path: string;
  knot?: string;
  stitch?: string;
  index: number;
  siblingCount: number;
}

/** A folder in the binder tree, derived from `/`-separated file paths. `key` is
 *  the directory path with a trailing slash (e.g. `"scenes/act1/"`) — distinct
 *  from any file path and used as the collapse key. */
export interface FolderNode {
  name: string;
  key: string;
  folders: FolderNode[];
  files: FileOutline[];
}

interface TreeLevel {
  folders: FolderNode[];
  files: FileOutline[];
}

/** Group files into a collapsible folder tree by splitting their paths on `/`.
 *  Purely presentational (no new data model); files with no `/` sit at root.
 *  Folders and files are sorted by name within each level for determinism. */
export function buildBinderTree(outline: FileOutline[]): TreeLevel {
  const root: TreeLevel = { folders: [], files: [] };
  for (const file of outline) {
    const slash = file.path.lastIndexOf("/");
    if (slash < 0) {
      root.files.push(file);
      continue;
    }
    let level: TreeLevel = root;
    let prefix = "";
    for (const segment of file.path.substring(0, slash).split("/")) {
      prefix += `${segment}/`;
      let child = level.folders.find((f) => f.key === prefix);
      if (!child) {
        child = { name: segment, key: prefix, folders: [], files: [] };
        level.folders.push(child);
      }
      level = child;
    }
    level.files.push(file);
  }
  const sortLevel = (lvl: TreeLevel): void => {
    lvl.folders.sort((a, b) => a.name.localeCompare(b.name));
    lvl.files.sort((a, b) => a.path.localeCompare(b.path));
    lvl.folders.forEach(sortLevel);
  };
  sortLevel(root);
  return root;
}

function buildFlatRows(outline: FileOutline[], collapsed: Set<string>): FlatRow[] {
  const rows: FlatRow[] = [];
  const pushFile = (file: FileOutline): void => {
    rows.push({ key: file.path, kind: "file", path: file.path, index: 0, siblingCount: 1 });
    if (collapsed.has(file.path)) return;
    const knots = file.symbols.filter((s) => s.kind === "knot");
    knots.forEach((knot, ki) => {
      const knotKey = `${file.path}::${knot.name}`;
      rows.push({
        key: knotKey,
        kind: "knot",
        path: file.path,
        knot: knot.name,
        index: ki,
        siblingCount: knots.length,
      });
      if (collapsed.has(knotKey)) return;
      const stitches = knot.children.filter((c) => c.kind === "stitch");
      stitches.forEach((stitch, si) => {
        rows.push({
          key: `${file.path}::${knot.name}::${stitch.name}`,
          kind: "stitch",
          path: file.path,
          knot: knot.name,
          stitch: stitch.name,
          index: si,
          siblingCount: stitches.length,
        });
      });
    });
  };
  const walk = (level: TreeLevel): void => {
    for (const folder of level.folders) {
      rows.push({ key: folder.key, kind: "folder", path: folder.key, index: 0, siblingCount: 1 });
      if (!collapsed.has(folder.key)) walk(folder);
    }
    for (const file of level.files) pushFile(file);
  };
  walk(buildBinderTree(outline));
  return rows;
}

// ── Drag-reorder helpers ────────────────────────────────────────────

/** Last `::`-separated segment of a row key (the knot or stitch name). */
function lastSegment(key: string): string {
  const parts = key.split("::");
  return parts[parts.length - 1]!;
}

/** Stitch names in a knot, in document order, from the outline. */
function stitchNamesIn(outline: FileOutline[], path: string, knot: string): string[] {
  const file = outline.find((f) => f.path === path);
  const k = file?.symbols.find((s) => s.kind === "knot" && s.name === knot);
  return (k?.children ?? []).filter((c) => c.kind === "stitch").map((c) => c.name);
}

/** Top-level knot names in a file, in document order. */
function knotNamesIn(outline: FileOutline[], path: string): string[] {
  const file = outline.find((f) => f.path === path);
  return (file?.symbols ?? []).filter((s) => s.kind === "knot").map((s) => s.name);
}

/**
 * Compute the new sibling order after dropping `dragged` (kept in their current
 * relative order) just before/after `refName`. Returns the order unchanged if
 * `refName` is itself dragged (drop onto self) or not found.
 */
export function computeReorder(
  siblings: string[],
  dragged: string[],
  refName: string,
  side: "before" | "after",
): string[] {
  const draggedSet = new Set(dragged);
  const orderedDragged = siblings.filter((n) => draggedSet.has(n));
  const without = siblings.filter((n) => !draggedSet.has(n));
  let idx = without.indexOf(refName);
  if (idx === -1) return siblings;
  if (side === "after") idx += 1;
  return [...without.slice(0, idx), ...orderedDragged, ...without.slice(idx)];
}

// ── Main Binder component ──────────────────────────────────────────

function BinderInner() {
  const rawOutline = useStudioStore((s) => s.outline);
  // The Binder's own file tree only ever shows real project files (issue
  // #2306/#2343): `outline` now lists mounted stdlib files alongside them
  // (flagged `mounted`, #2343's flag flip on `list_files`/`project_outline`/
  // `story_graph`), so every existing tree/drag/search/rename/delete code
  // path below keeps operating on exactly the same file set it always did —
  // the Library section (below) is the only new consumer of the mounted half.
  const outline = useMemo(() => rawOutline.filter((f) => !f.mounted), [rawOutline]);
  const libraryFiles = useMemo(() => rawOutline.filter((f) => f.mounted), [rawOutline]);
  const closureFiles = useStudioStore((s) => s.closureFiles);
  const entryFile = useStudioStore((s) => s.entryFile);
  // #3014: an ink project provably cannot reach the mounted `.brink`
  // stdlib — `compilation_closure_files` never includes it — so the
  // Library section is hidden entirely rather than listing files that
  // are, by construction, not part of the story. A `.brink` entry (or no
  // compile yet, entryFile null) keeps the section.
  const projectIsInk = entryFile !== null && entryFile.endsWith(".ink");
  const activeDocKey = useStudioStore((s) => s.activeDocKey);
  const collapsed = useStudioStore((s) => s.collapsed);
  const selectedKeys = useStudioStore((s) => s.selectedKeys);
  const focusedKey = useStudioStore((s) => s.focusedKey);
  const openTarget = useStudioStore((s) => s.openTarget);
  const toggleCollapsed = useStudioStore((s) => s.toggleCollapsed);
  const libraryExpanded = useStudioStore((s) => s.libraryExpanded);
  const toggleLibraryExpanded = useStudioStore((s) => s.toggleLibraryExpanded);
  const selectKey = useStudioStore((s) => s.selectKey);
  const clearSelection = useStudioStore((s) => s.clearSelection);
  const setFocusedKey = useStudioStore((s) => s.setFocusedKey);
  const applyMoveResult = useStudioStore((s) => s.applyMoveResult);
  const deleteFile = useStudioStore((s) => s.deleteFile);
  const deleteFolder = useStudioStore((s) => s.deleteFolder);
  const renameFile = useStudioStore((s) => s.renameFile);
  const moveFiles = useStudioStore((s) => s.moveFiles);
  const renameFolder = useStudioStore((s) => s.renameFolder);
  const undo = useStudioStore((s) => s.undo);
  const undoStack = useStudioStore((s) => s.undoStack);
  const addFile = useStudioStore((s) => s.addFile);
  const storeApi = useStudioStoreApi();

  const [inputActive, setInputActive] = useState(false);
  /** Directory prefix the New File input is pre-filled with ("New file here"). */
  const [newFileDir, setNewFileDir] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    target: ContextMenuTarget;
  } | null>(null);

  // Pending delete confirmation (file or folder).
  const [pendingDelete, setPendingDelete] = useState<{
    message: string;
    run: () => void;
  } | null>(null);

  // The row (file path or folder prefix) currently being inline-renamed.
  const [renaming, setRenaming] = useState<{ key: string; isFolder: boolean } | null>(null);

  // Drag state
  const [dragState, setDragState] = useState<DragState | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  // Whether the "move to project root" drop zone is currently hovered.
  const [rootDropActive, setRootDropActive] = useState(false);

  const flatRows = buildFlatRows(outline, collapsed);

  // ── Helpers ─────────────────────────────────────────────────────

  // Whether file rename/move is available (gates file dragging). Read
  // non-reactively — the project is bound once and outline updates re-render.
  const canRenameFiles = storeApi.getState()._project?.canRenameFiles() ?? false;

  // The shared symbol-action dispatcher (play-from-here + structural refactors),
  // used here for both the context menu and the drag-drop reorder/move handlers.
  // File/folder lifecycle actions are handled in handleContextMenuAction.
  const executeAction = useCallback(
    (action: ContextMenuAction) =>
      dispatchSymbolAction(storeApi.getState(), applyMoveResult, action),
    [storeApi, applyMoveResult],
  );

  // ── Tab open helpers ────────────────────────────────────────────

  const handleOpenUnpinned = useCallback(
    (target: TabTarget) => {
      openTarget(target, false);
    },
    [openTarget],
  );

  const handleOpenPinned = useCallback(
    (target: TabTarget) => {
      openTarget(target, true);
    },
    [openTarget],
  );

  // ── Click handler ───────────────────────────────────────────────

  const handleRowClick = useCallback(
    (key: string, target: TabTarget, e: React.MouseEvent) => {
      const isMulti = e.ctrlKey || e.metaKey;
      if (isMulti) {
        selectKey(key, true);
        return; // Do NOT open tab on ctrl/cmd+click
      }
      selectKey(key, false);
      handleOpenUnpinned(target);
    },
    [selectKey, handleOpenUnpinned],
  );

  // Library rows (mounted std/ files) never join `selectedKeys`: they have
  // no drag/rename/delete/move affordances, so there is nothing a
  // multi-select could do with one, and routing them through `selectKey`
  // would silently co-select a mounted path into an otherwise-ordinary
  // project-file drag/move — `handleDragStart` expands a drag to every
  // selected key, and that path has no visual indicator here (`isSelected`
  // is always `false` for a Library row) so the user would never see it
  // happen (issue #2343 review finding). A click here only opens the file
  // and clears any existing project-file selection instead.
  const handleLibraryFileClick = useCallback(
    (target: TabTarget) => {
      clearSelection();
      handleOpenUnpinned(target);
    },
    [clearSelection, handleOpenUnpinned],
  );

  // ── New file input ──────────────────────────────────────────────

  /** Open the inline New File input, optionally pre-filled with a directory
   *  prefix ("New file here" on a file/folder row). Cursor lands at the end. */
  const openNewFileInput = useCallback(
    (dir: string) => {
      if (inputActive) return;
      setNewFileDir(dir);
      setInputActive(true);
      requestAnimationFrame(() => {
        const input = inputRef.current;
        if (!input) return;
        input.focus();
        const end = input.value.length;
        // SELECT-INVARIANT Binder.newFileInput.cursorToEnd: a zero-width
        // range (start === end) places the caret, it does not select any
        // text — there is nothing typed here for it to clobber even though
        // it runs inside this deferred frame, and `end` is read fresh from
        // `input.value` at fire time, not a value captured before the frame.
        input.setSelectionRange(end, end);
      });
    },
    [inputActive],
  );

  const handleNewClick = useCallback(() => openNewFileInput(""), [openNewFileInput]);

  const cancelInput = useCallback(() => {
    setInputActive(false);
    setNewFileDir("");
  }, []);

  const confirmInput = useCallback(() => {
    const input = inputRef.current;
    if (!input) return;
    let name = input.value.trim();
    setInputActive(false);
    setNewFileDir("");
    if (!name) return;
    if (!name.includes(".")) {
      name += ".ink";
    }
    void addFile(name);
  }, [addFile]);

  const handleInputKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        confirmInput();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancelInput();
      }
    },
    [confirmInput, cancelInput],
  );

  // ── Inline rename commit ────────────────────────────────────────

  const commitRename = useCallback(
    (value: string) => {
      const target = renaming;
      setRenaming(null);
      if (!target) return;
      const trimmed = value.trim();
      if (!trimmed) return;

      if (target.isFolder) {
        const oldPrefix = target.key; // e.g. "scenes/act1/"
        const newPrefix = parentOf(oldPrefix) + trimmed.replace(/\/$/, "") + "/";
        if (newPrefix === oldPrefix) return;
        const paths = outline.map((f) => f.path).filter((p) => p.startsWith(oldPrefix));
        void renameFolder(oldPrefix, newPrefix, paths);
      } else {
        const oldPath = target.key;
        let name = trimmed;
        if (!name.includes(".")) name += ".ink";
        const newPath = dirOf(oldPath) + name;
        if (newPath === oldPath) return;
        void renameFile(oldPath, newPath);
      }
    },
    [renaming, outline, renameFile, renameFolder],
  );

  const cancelRename = useCallback(() => setRenaming(null), []);

  // ── Context menu handler ────────────────────────────────────────

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, row: FlatRow) => {
      e.preventDefault();
      e.stopPropagation();

      let target: ContextMenuTarget;
      if (row.kind === "file") {
        const project = storeApi.getState()._project;
        target = {
          kind: "file",
          path: row.path,
          canDelete: project?.canDeleteFiles() ?? false,
          canRename: project?.canRenameFiles() ?? false,
        };
      } else if (row.kind === "folder") {
        // Every file under the folder prefix (recursive); the folder row's
        // `path` is the directory key with a trailing slash.
        const paths = outline
          .map((f) => f.path)
          .filter((p) => p.startsWith(row.path));
        const project = storeApi.getState()._project;
        target = {
          kind: "folder",
          prefix: row.path,
          paths,
          canDelete: project?.canDeleteFiles() ?? false,
          canRename: project?.canRenameFiles() ?? false,
        };
      } else {
        target = {
          kind: row.kind,
          path: row.path,
          knot: row.knot!,
          stitch: row.stitch,
          index: row.index,
          siblingCount: row.siblingCount,
        };
      }
      setContextMenu({ x: e.clientX, y: e.clientY, target });
    },
    [storeApi, outline],
  );

  const handleContextMenuAction = useCallback(
    (action: ContextMenuAction) => {
      setContextMenu(null);
      switch (action.type) {
        case "newFileInFolder":
          openNewFileInput(action.dir);
          return;
        case "renameFile":
          setRenaming({ key: action.path, isFolder: false });
          return;
        case "renameFolder":
          setRenaming({ key: action.prefix, isFolder: true });
          return;
        case "deleteFile":
          setPendingDelete({
            message: `Delete ${action.path}?`,
            run: () => void deleteFile(action.path),
          });
          return;
        case "deleteFolder": {
          const n = action.paths.length;
          setPendingDelete({
            message: `Delete ${action.prefix.replace(/\/$/, "")}/ and its ${n} file${n === 1 ? "" : "s"}?`,
            run: () => void deleteFolder(action.prefix, action.paths),
          });
          return;
        }
        default:
          void executeAction(action);
      }
    },
    [executeAction, openNewFileInput, deleteFile, deleteFolder, storeApi],
  );

  // ── Keyboard handler ────────────────────────────────────────────

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        clearSelection();
        setContextMenu(null);
        return;
      }

      // Undo: Ctrl/Cmd+Z
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && undoStack.length > 0) {
        e.preventDefault();
        void undo();
        return;
      }

      // F2: rename the focused file/folder row.
      if (e.key === "F2" && focusedKey) {
        const row = flatRows.find((r) => r.key === focusedKey);
        if (row && (row.kind === "file" || row.kind === "folder")) {
          e.preventDefault();
          setRenaming({ key: row.key, isFolder: row.kind === "folder" });
        }
        return;
      }

      const focusIdx = focusedKey ? flatRows.findIndex((r) => r.key === focusedKey) : -1;

      if (e.key === "ArrowDown" && !e.altKey) {
        e.preventDefault();
        const next = focusIdx + 1 < flatRows.length ? focusIdx + 1 : 0;
        setFocusedKey(flatRows[next]!.key);
        return;
      }

      if (e.key === "ArrowUp" && !e.altKey) {
        e.preventDefault();
        const prev = focusIdx > 0 ? focusIdx - 1 : flatRows.length - 1;
        setFocusedKey(flatRows[prev]!.key);
        return;
      }

      if (e.key === "Enter" && focusedKey) {
        e.preventDefault();
        const row = flatRows.find((r) => r.key === focusedKey);
        if (row?.kind === "folder") {
          toggleCollapsed(row.key);
        } else if (row) {
          const target = buildTarget(row, outline);
          if (target) handleOpenUnpinned(target);
        }
        return;
      }

      // Alt+Arrow: reorder focused item
      if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        e.preventDefault();
        if (!focusedKey) return;
        const row = flatRows.find((r) => r.key === focusedKey);
        if (!row || row.kind === "file" || row.kind === "folder") return;
        const direction = e.key === "ArrowDown" ? 1 : -1;

        if (row.kind === "stitch") {
          void executeAction({
            type: "reorderStitch",
            path: row.path,
            knot: row.knot!,
            stitch: row.stitch!,
            direction,
          });
        } else if (row.kind === "knot") {
          void executeAction({
            type: "reorderKnot",
            path: row.path,
            knot: row.knot!,
            direction,
          });
        }
      }
    },
    [
      clearSelection,
      undoStack,
      undo,
      focusedKey,
      flatRows,
      setFocusedKey,
      handleOpenUnpinned,
      outline,
      executeAction,
      toggleCollapsed,
    ],
  );

  // ── Drag handlers ───────────────────────────────────────────────

  const handleDragStart = useCallback(
    (e: React.DragEvent, row: FlatRow) => {
      if (row.kind === "folder") {
        e.preventDefault();
        return;
      }
      if (row.kind === "file") {
        // Dragging a file moves it (into a folder); when the dragged row is
        // part of a multi-selection, the whole selection moves together.
        // Knot/stitch reorder is unaffected (handlers branch on sourceKind).
        const keys = selectedKeys.has(row.key) ? [...selectedKeys] : [row.key];
        setDragState({ sourceKeys: keys, sourceKind: "file", sourcePath: row.path });
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", row.key);
        return;
      }
      const keys = selectedKeys.has(row.key) ? [...selectedKeys] : [row.key];
      setDragState({
        sourceKeys: keys,
        sourceKind: row.kind,
        sourcePath: row.path,
        sourceParent: row.knot,
      });
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", row.key);
    },
    [selectedKeys],
  );

  const handleDragEnd = useCallback(() => {
    setDragState(null);
    setDropTarget(null);
    setRootDropActive(false);
  }, []);

  // ── Root drop zone (move a nested file out to the project root) ──

  const handleRootDragOver = useCallback(
    (e: React.DragEvent) => {
      if (dragState?.sourceKind !== "file") return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      setRootDropActive(true);
    },
    [dragState],
  );

  const handleRootDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      const ds = dragState;
      setDragState(null);
      setDropTarget(null);
      setRootDropActive(false);
      if (ds?.sourceKind !== "file") return;
      // Move every selected file out to the root; moveFiles skips ones already
      // there.
      void moveFiles(ds.sourceKeys, "");
    },
    [dragState, moveFiles],
  );

  const handleDragOver = useCallback(
    (e: React.DragEvent, row: FlatRow) => {
      if (!dragState) return;
      e.preventDefault();

      // File drag: only folders are drop targets (move into folder).
      if (dragState.sourceKind === "file") {
        if (row.kind === "folder") {
          e.dataTransfer.dropEffect = "move";
          setDropTarget({ kind: "into", targetKey: row.key });
        } else {
          e.dataTransfer.dropEffect = "none";
          setDropTarget(null);
        }
        return;
      }

      // Determine drop kind
      if (row.kind === "file") {
        e.dataTransfer.dropEffect = "none";
        setDropTarget(null);
        return;
      }

      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      const y = e.clientY - rect.top;
      const isTop = y < rect.height * 0.3;
      const isBottom = y > rect.height * 0.7;

      if (dragState.sourceKind === "stitch") {
        if (row.kind === "knot") {
          // Dropping stitch onto knot = reparent (drop into)
          e.dataTransfer.dropEffect = "move";
          setDropTarget({ kind: "into", targetKey: row.key });
        } else if (row.kind === "stitch") {
          // Dropping stitch onto stitch = reorder or reparent
          if (row.knot === dragState.sourceParent) {
            // Same knot: reorder
            e.dataTransfer.dropEffect = "move";
            if (isTop) {
              setDropTarget({ kind: "between", afterKey: row.key, targetKey: "before" });
            } else {
              setDropTarget({ kind: "between", afterKey: row.key, targetKey: "after" });
            }
          } else {
            // Different knot: reparent into that knot
            e.dataTransfer.dropEffect = "move";
            setDropTarget({ kind: "into", targetKey: `${row.path}::${row.knot}` });
          }
        }
      } else if (dragState.sourceKind === "knot") {
        if (row.kind === "knot") {
          if (isTop || isBottom) {
            // Reorder between knots
            e.dataTransfer.dropEffect = "move";
            setDropTarget({ kind: "between", afterKey: row.key, targetKey: isTop ? "before" : "after" });
          } else {
            // Demote into this knot
            e.dataTransfer.dropEffect = "move";
            setDropTarget({ kind: "into", targetKey: row.key });
          }
        } else {
          e.dataTransfer.dropEffect = "none";
          setDropTarget(null);
        }
      }
    },
    [dragState],
  );

  const handleDrop = useCallback(
    (e: React.DragEvent, _row: FlatRow) => {
      e.preventDefault();
      if (!dragState || !dropTarget) return;

      // File drag onto a folder = move (keys change; includes rewrite). All
      // selected files move together; a single drag is a batch of one.
      if (dragState.sourceKind === "file") {
        if (dropTarget.kind === "into" && dropTarget.targetKey) {
          void moveFiles(dragState.sourceKeys, dropTarget.targetKey); // "folder/"
        }
        setDragState(null);
        setDropTarget(null);
        return;
      }

      const path = dragState.sourcePath;
      // All selected items move together (in their current relative order).
      const draggedNames = dragState.sourceKeys.map(lastSegment);
      const side = dropTarget.targetKey === "after" ? "after" : "before";

      if (dragState.sourceKind === "stitch") {
        const srcKnot = dragState.sourceParent!;

        if (dropTarget.kind === "into") {
          // Reparent every selected stitch into the target knot. Sequential so
          // each move sees the source updated by the previous one.
          const destParts = dropTarget.targetKey!.split("::");
          const destKnot = destParts[1] ?? destParts[0]!;
          if (destKnot !== srcKnot) {
            void (async () => {
              for (const stitch of draggedNames) {
                await executeAction({ type: "moveStitch", path, srcKnot, stitch, destKnot });
              }
            })();
          }
        } else if (dropTarget.afterKey) {
          // Reorder within the knot to the exact dropped position.
          const siblings = stitchNamesIn(outline, path, srcKnot);
          const order = computeReorder(siblings, draggedNames, lastSegment(dropTarget.afterKey), side);
          void executeAction({ type: "reorderStitches", path, knot: srcKnot, order });
        }
      } else if (dragState.sourceKind === "knot") {
        if (dropTarget.kind === "into") {
          // Demote every selected knot into the target knot.
          const destParts = dropTarget.targetKey!.split("::");
          const destKnot = destParts[1] ?? destParts[0]!;
          void (async () => {
            for (const knot of draggedNames) {
              if (knot !== destKnot) {
                await executeAction({ type: "demoteKnot", path, knot, destKnot });
              }
            }
          })();
        } else if (dropTarget.afterKey) {
          const siblings = knotNamesIn(outline, path);
          const order = computeReorder(siblings, draggedNames, lastSegment(dropTarget.afterKey), side);
          void executeAction({ type: "reorderKnots", path, order });
        }
      }

      setDragState(null);
      setDropTarget(null);
    },
    [dragState, dropTarget, executeAction, outline, moveFiles],
  );

  // ── Drop line helper ───────────────────────────────────────────

  function dropLineFor(rowKey: string): "before" | "after" | null {
    if (!dropTarget || dropTarget.kind !== "between") return null;
    if (dropTarget.afterKey !== rowKey) return null;
    return dropTarget.targetKey === "before" ? "before" : "after";
  }

  // ── Render helpers ──────────────────────────────────────────────

  function renderStitch(
    path: string,
    knot: DocumentSymbol,
    stitch: DocumentSymbol,
    row: FlatRow,
    depth: number,
  ) {
    const stitchId = row.key;
    const isActive = activeDocKey === stitchId;
    const target: TabTarget = {
      kind: "symbol",
      path,
      name: stitch.name,
      start: stitch.full_start,
      end: stitch.full_end,
    };

    return (
      <BinderRow
        key={stitchId}
        rowKey={stitchId}
        depth={depth}
        kind="stitch"
        label={stitch.name}
        expandable={false}
        isExpanded={false}
        isActive={isActive}
        isSelected={selectedKeys.has(stitchId)}
        isFocused={focusedKey === stitchId}
        isDragging={dragState?.sourceKeys.includes(stitchId) ?? false}
        isDropInto={dropTarget?.kind === "into" && dropTarget.targetKey === stitchId}
        dropLinePosition={dropLineFor(stitchId)}
        draggable={true}
        onChevronClick={() => {}}
        onClick={(e) => handleRowClick(stitchId, target, e)}
        onDoubleClick={() => handleOpenPinned(target)}
        onContextMenu={(e) => handleContextMenu(e, row)}
        onDragStart={(e) => handleDragStart(e, row)}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleDragOver(e, row)}
        onDrop={(e) => handleDrop(e, row)}
      />
    );
  }

  function renderKnot(path: string, knot: DocumentSymbol, row: FlatRow, depth: number) {
    const knotKey = row.key;
    const stitches = knot.children.filter((c) => c.kind === "stitch");
    const hasStitches = stitches.length > 0;
    const isExpanded = !collapsed.has(knotKey);
    const isActive = activeDocKey === knotKey;
    const target: TabTarget = {
      kind: "symbol",
      path,
      name: knot.name,
      start: knot.full_start,
      end: knot.full_end,
    };

    return (
      <div key={knotKey}>
        <BinderRow
          rowKey={knotKey}
          depth={depth}
          kind="knot"
          isFunction={knot.detail === "function"}
          label={knot.name}
          expandable={hasStitches}
          isExpanded={isExpanded}
          isActive={isActive}
          isSelected={selectedKeys.has(knotKey)}
          isFocused={focusedKey === knotKey}
          isDragging={dragState?.sourceKeys.includes(knotKey) ?? false}
          isDropInto={dropTarget?.kind === "into" && dropTarget.targetKey === knotKey}
          dropLinePosition={dropLineFor(knotKey)}
          draggable={true}
          onChevronClick={() => toggleCollapsed(knotKey)}
          onClick={(e) => handleRowClick(knotKey, target, e)}
          onDoubleClick={() => handleOpenPinned(target)}
          onContextMenu={(e) => handleContextMenu(e, row)}
          onDragStart={(e) => handleDragStart(e, row)}
          onDragEnd={handleDragEnd}
          onDragOver={(e) => handleDragOver(e, row)}
          onDrop={(e) => handleDrop(e, row)}
        />
        {hasStitches &&
          isExpanded &&
          stitches.map((s) => {
            const sRow = flatRows.find((r) => r.key === `${path}::${knot.name}::${s.name}`);
            if (!sRow) return null;
            return renderStitch(path, knot, s, sRow, depth + 1);
          })}
      </div>
    );
  }

  function renderFile(file: FileOutline, depth: number) {
    const knots = file.symbols.filter((s) => s.kind === "knot");
    const hasChildren = knots.length > 0;
    const fileKey = file.path;
    const isExpanded = !collapsed.has(fileKey);
    const isActive = activeDocKey === fileKey;
    const target: TabTarget = { kind: "file", path: file.path };
    const fileRow = flatRows.find((r) => r.key === fileKey);
    // Scope marks (#3021 — Binder.dc.html): the entry is the project's
    // anchor; a source file outside the compile closure is on disk, not
    // in the story ("not included"), and renders dimmed with a badge.
    const isEntry = entryFile !== null && file.path === entryFile;
    const notIncluded = !isEntry && isOutOfScope(file.path, closureFiles, rawOutline);

    return (
      <div key={fileKey}>
        <BinderRow
          rowKey={fileKey}
          depth={depth}
          kind="file"
          label={displayName(file.path)}
          badge={
            isEntry
              ? { text: "entry", tone: "entry" }
              : notIncluded
                ? { text: "not included", tone: "muted" }
                : undefined
          }
          dimmed={notIncluded}
          expandable={hasChildren}
          isExpanded={isExpanded}
          isActive={isActive}
          isSelected={selectedKeys.has(fileKey)}
          isFocused={focusedKey === fileKey}
          isDragging={dragState?.sourceKind === "file" && dragState.sourceKeys.includes(fileKey)}
          isDropInto={false}
          dropLinePosition={null}
          draggable={canRenameFiles}
          editing={
            renaming && !renaming.isFolder && renaming.key === fileKey
              ? { initial: displayName(file.path), onCommit: commitRename, onCancel: cancelRename }
              : undefined
          }
          onChevronClick={() => toggleCollapsed(fileKey)}
          onClick={(e) => handleRowClick(fileKey, target, e)}
          onDoubleClick={() => handleOpenPinned(target)}
          onContextMenu={(e) => (fileRow ? handleContextMenu(e, fileRow) : e.preventDefault())}
          onDragStart={(e) => fileRow && handleDragStart(e, fileRow)}
          onDragEnd={handleDragEnd}
          onDragOver={(e) => fileRow && handleDragOver(e, fileRow)}
          onDrop={(e) => fileRow && handleDrop(e, fileRow)}
        />
        {isExpanded &&
          knots.map((k) => {
            const kRow = flatRows.find((r) => r.key === `${file.path}::${k.name}`);
            if (!kRow) return null;
            return renderKnot(file.path, k, kRow, depth + 1);
          })}
      </div>
    );
  }

  function renderFolder(folder: FolderNode, depth: number) {
    const isExpanded = !collapsed.has(folder.key);
    const folderRow = flatRows.find((r) => r.key === folder.key);
    return (
      <div key={folder.key}>
        <BinderRow
          rowKey={folder.key}
          depth={depth}
          kind="folder"
          label={folder.name}
          expandable={true}
          isExpanded={isExpanded}
          isActive={false}
          isSelected={selectedKeys.has(folder.key)}
          isFocused={focusedKey === folder.key}
          isDragging={false}
          isDropInto={dropTarget?.kind === "into" && dropTarget.targetKey === folder.key}
          dropLinePosition={null}
          draggable={false}
          editing={
            renaming?.isFolder && renaming.key === folder.key
              ? { initial: folder.name, onCommit: commitRename, onCancel: cancelRename }
              : undefined
          }
          onChevronClick={() => toggleCollapsed(folder.key)}
          onClick={() => {
            setFocusedKey(folder.key);
            toggleCollapsed(folder.key);
          }}
          onDoubleClick={() => {}}
          onContextMenu={(e) => (folderRow ? handleContextMenu(e, folderRow) : e.preventDefault())}
          onDragStart={() => {}}
          onDragEnd={handleDragEnd}
          onDragOver={(e) => folderRow && handleDragOver(e, folderRow)}
          onDrop={(e) => folderRow && handleDrop(e, folderRow)}
        />
        {isExpanded && renderTree(folder, depth + 1)}
      </div>
    );
  }

  function renderTree(level: TreeLevel, depth: number) {
    return (
      <>
        {level.folders.map((folder) => renderFolder(folder, depth))}
        {level.files.map((file) => renderFile(file, depth))}
      </>
    );
  }

  // ── Library section (mounted std/ files, issue #2306/#2343) ──────
  //
  // A visually distinct, read-only counterpart to the project's own file
  // tree: browsable (folders expand/collapse) and openable (click/double-
  // click opens the file exactly like a project file — reads work through
  // the normal `openTarget` path), but no drag, no context menu, and no
  // inline rename/new-file affordances. Files render at folder granularity
  // only (no knot/stitch drill-down): opening a file shows its full
  // contents, including every knot/stitch, in the same read-only editor
  // view — see `DocumentSessions.slotExtensions` (`EditorState.readOnly`).
  // Actual edit refusal lives at the wasm/session layer regardless of what
  // this component renders; this section exists so a mounted file is
  // discoverable at all instead of the pre-#2343 "hidden entirely".

  function renderLibraryFolder(folder: FolderNode, depth: number) {
    const key = libraryFolderKey(folder.key);
    const isExpanded = !collapsed.has(key);
    return (
      <div key={key}>
        <BinderRow
          rowKey={key}
          depth={depth}
          kind="folder"
          label={folder.name}
          expandable
          isExpanded={isExpanded}
          isActive={false}
          isSelected={false}
          isFocused={false}
          isDragging={false}
          isDropInto={false}
          dropLinePosition={null}
          draggable={false}
          onChevronClick={() => toggleCollapsed(key)}
          onClick={() => toggleCollapsed(key)}
          onDoubleClick={() => {}}
          onContextMenu={(e) => e.preventDefault()}
          onDragStart={(e) => e.preventDefault()}
          onDragEnd={() => {}}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => e.preventDefault()}
        />
        {isExpanded && renderLibraryTree(folder, depth + 1)}
      </div>
    );
  }

  function renderLibraryFile(file: FileOutline, depth: number) {
    const fileKey = file.path;
    const isActive = activeDocKey === fileKey;
    const target: TabTarget = { kind: "file", path: file.path };
    return (
      <BinderRow
        key={fileKey}
        rowKey={fileKey}
        depth={depth}
        kind="file"
        label={displayName(file.path)}
        expandable={false}
        isExpanded={false}
        isActive={isActive}
        isSelected={false}
        isFocused={false}
        isDragging={false}
        isDropInto={false}
        dropLinePosition={null}
        draggable={false}
        onChevronClick={() => {}}
        onClick={() => handleLibraryFileClick(target)}
        onDoubleClick={() => handleOpenPinned(target)}
        onContextMenu={(e) => e.preventDefault()}
        onDragStart={(e) => e.preventDefault()}
        onDragEnd={() => {}}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => e.preventDefault()}
      />
    );
  }

  function renderLibraryTree(level: TreeLevel, depth: number) {
    return (
      <>
        {level.folders.map((folder) => renderLibraryFolder(folder, depth))}
        {level.files.map((file) => renderLibraryFile(file, depth))}
      </>
    );
  }

  const libraryTree = buildBinderTree(libraryFiles);

  return (
    <div
      ref={containerRef}
      className="brink-binder"
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      {renderTree(buildBinderTree(outline), 0)}
      {libraryFiles.length > 0 && !projectIsInk && (
        <div className="brink-binder-library-section">
          <BinderRow
            rowKey={LIBRARY_ROW_KEY}
            depth={0}
            kind="library"
            label="Library"
            expandable
            isExpanded={libraryExpanded}
            isActive={false}
            isSelected={false}
            isFocused={false}
            isDragging={false}
            isDropInto={false}
            dropLinePosition={null}
            draggable={false}
            onChevronClick={toggleLibraryExpanded}
            onClick={toggleLibraryExpanded}
            onDoubleClick={() => {}}
            onContextMenu={(e) => e.preventDefault()}
            onDragStart={(e) => e.preventDefault()}
            onDragEnd={() => {}}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => e.preventDefault()}
          />
          {libraryExpanded && renderLibraryTree(libraryTree, 1)}
        </div>
      )}
      {dragState?.sourceKind === "file" && dragState.sourceKeys.some((k) => k.includes("/")) && (
        <div
          className={"brink-binder-root-drop" + (rootDropActive ? " active" : "")}
          onDragOver={handleRootDragOver}
          onDragLeave={() => setRootDropActive(false)}
          onDrop={handleRootDrop}
        >
          Move to project root
        </div>
      )}
      {entryFile !== null && (
        <div className="brink-binder-legend">
          <div className="brink-binder-legend-row">
            <span className="brink-binder-legend-swatch legend-entry" />
            <span>Entry point — what the project compiles from</span>
          </div>
          <div className="brink-binder-legend-row">
            <span className="brink-binder-legend-swatch legend-muted" />
            <span>On disk, not reached by any INCLUDE</span>
          </div>
        </div>
      )}
      <div className="brink-binder-row brink-binder-new" onClick={handleNewClick}>
        + New file
      </div>
      {inputActive && (
        <div className="brink-binder-input-wrapper">
          <input
            ref={inputRef}
            className="brink-tab-input"
            type="text"
            placeholder="filename.ink"
            defaultValue={newFileDir}
            size={16}
            onKeyDown={handleInputKeyDown}
            onBlur={cancelInput}
          />
        </div>
      )}
      {contextMenu && (
        <BinderContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          target={contextMenu.target}
          outline={outline}
          onAction={handleContextMenuAction}
          onClose={() => setContextMenu(null)}
        />
      )}
      <Overlay
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        className="brink-binder-confirm"
      >
        <div className="brink-binder-confirm-message">{pendingDelete?.message}</div>
        <div className="brink-binder-confirm-actions">
          <button
            type="button"
            className="brink-binder-confirm-cancel"
            onClick={() => setPendingDelete(null)}
          >
            Cancel
          </button>
          <button
            type="button"
            className="brink-binder-confirm-delete"
            onClick={() => {
              pendingDelete?.run();
              setPendingDelete(null);
            }}
          >
            Delete
          </button>
        </div>
      </Overlay>
    </div>
  );
}

// ── Helper: build TabTarget from flat row ───────────────────────────

function buildTarget(row: FlatRow, outline: FileOutline[]): TabTarget | null {
  if (row.kind === "file") {
    return { kind: "file", path: row.path };
  }
  const file = outline.find((f) => f.path === row.path);
  if (!file) return null;

  if (row.kind === "knot") {
    const knot = file.symbols.find((s) => s.kind === "knot" && s.name === row.knot);
    if (!knot) return null;
    return {
      kind: "symbol",
      path: row.path,
      name: knot.name,
      start: knot.full_start,
      end: knot.full_end,
    };
  }

  if (row.kind === "stitch") {
    const knot = file.symbols.find((s) => s.kind === "knot" && s.name === row.knot);
    const stitch = knot?.children.find((c) => c.kind === "stitch" && c.name === row.stitch);
    if (!stitch) return null;
    return {
      kind: "symbol",
      path: row.path,
      name: stitch.name,
      start: stitch.full_start,
      end: stitch.full_end,
    };
  }

  return null;
}

export const Binder = memo(BinderInner);
