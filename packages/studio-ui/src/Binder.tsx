import { memo, useCallback, useRef, useState } from "react";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import {
  BinderContextMenu,
  type ContextMenuAction,
  type ContextMenuTarget,
} from "./BinderContextMenu.js";
import type { FileOutline, DocumentSymbol, MoveResult } from "@brink/wasm-types";
import type { TabTarget } from "@brink/studio-store";

// ── Icons ──────────────────────────────────────────────────────────

const ICON_FILE = "\ud83d\udcc4";
const ICON_KNOT = "\u25c6";
const ICON_STITCH = "\u25c7";

function iconChar(kind: string): string {
  switch (kind) {
    case "file":
      return ICON_FILE;
    case "knot":
      return ICON_KNOT;
    case "stitch":
      return ICON_STITCH;
    default:
      return "\u00b7";
  }
}

function iconClass(kind: string): string {
  switch (kind) {
    case "file":
      return "brink-binder-icon-file";
    case "knot":
      return "brink-binder-icon-knot";
    case "stitch":
      return "brink-binder-icon-stitch";
    default:
      return "";
  }
}

function displayName(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash >= 0 ? path.substring(slash + 1) : path;
}

// ── Drag state types ────────────────────────────────────────────────

interface DragState {
  sourceKeys: string[];
  sourceKind: "knot" | "stitch";
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
  onChevronClick: () => void;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onDragStart: (e: React.DragEvent) => void;
  onDragEnd: () => void;
  onDragOver: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent) => void;
}

function BinderRow({
  depth,
  kind,
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
    (kind === "file" ? " brink-binder-file-row" : "") +
    (kind === "knot" ? " brink-binder-knot" : "") +
    (kind === "stitch" ? " brink-binder-stitch" : "") +
    (isActive ? " brink-binder-active" : "") +
    (isSelected ? " brink-binder-selected" : "") +
    (isFocused ? " brink-binder-focused" : "") +
    (isDragging ? " brink-binder-dragging" : "") +
    (isDropInto ? " brink-binder-drop-into" : "");

  const chevronClass =
    "brink-binder-chevron" +
    (expandable ? (isExpanded ? "" : " collapsed") : " leaf");

  return (
    <>
      {dropLinePosition === "before" && <div className="brink-binder-drop-line" />}
      <div
        className={rowClass}
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
        <span className={"brink-binder-icon " + iconClass(kind)}>{iconChar(kind)}</span>
        <span className="brink-binder-label">{label}</span>
      </div>
      {dropLinePosition === "after" && <div className="brink-binder-drop-line" />}
    </>
  );
}

// ── Flat row key list builder ───────────────────────────────────────

interface FlatRow {
  key: string;
  kind: "file" | "knot" | "stitch";
  path: string;
  knot?: string;
  stitch?: string;
  index: number;
  siblingCount: number;
}

function buildFlatRows(outline: FileOutline[], collapsed: Set<string>): FlatRow[] {
  const rows: FlatRow[] = [];
  for (const file of outline) {
    rows.push({
      key: file.path,
      kind: "file",
      path: file.path,
      index: 0,
      siblingCount: 1,
    });
    if (collapsed.has(file.path)) continue;
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
  }
  return rows;
}

// ── Main Binder component ──────────────────────────────────────────

function BinderInner() {
  const outline = useStudioStore((s) => s.outline);
  const activeTabId = useStudioStore((s) => s.activeTabId);
  const collapsed = useStudioStore((s) => s.collapsed);
  const selectedKeys = useStudioStore((s) => s.selectedKeys);
  const focusedKey = useStudioStore((s) => s.focusedKey);
  const openTab = useStudioStore((s) => s.openTab);
  const toggleCollapsed = useStudioStore((s) => s.toggleCollapsed);
  const selectKey = useStudioStore((s) => s.selectKey);
  const clearSelection = useStudioStore((s) => s.clearSelection);
  const setFocusedKey = useStudioStore((s) => s.setFocusedKey);
  const applyMoveResult = useStudioStore((s) => s.applyMoveResult);
  const undo = useStudioStore((s) => s.undo);
  const undoStack = useStudioStore((s) => s.undoStack);
  const addFile = useStudioStore((s) => s.addFile);
  const storeApi = useStudioStoreApi();

  const [inputActive, setInputActive] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    target: ContextMenuTarget;
  } | null>(null);

  // Drag state
  const [dragState, setDragState] = useState<DragState | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);

  const flatRows = buildFlatRows(outline, collapsed);

  // ── Helpers ─────────────────────────────────────────────────────

  const getSession = useCallback(() => {
    const state = storeApi.getState();
    return state._project?.getSession();
  }, [storeApi]);

  const executeAction = useCallback(
    async (action: ContextMenuAction) => {
      const session = getSession();
      if (!session) return;

      let result: MoveResult;
      let description: string;

      switch (action.type) {
        case "reorderStitch":
          result = session.reorderStitch(action.path, action.knot, action.stitch, action.direction);
          description = `Reorder ${action.stitch} ${action.direction > 0 ? "down" : "up"}`;
          break;
        case "reorderKnot":
          result = session.reorderKnot(action.path, action.knot, action.direction);
          description = `Reorder ${action.knot} ${action.direction > 0 ? "down" : "up"}`;
          break;
        case "moveStitch":
          result = session.moveStitch(action.path, action.srcKnot, action.stitch, action.destKnot);
          description = `Move ${action.stitch} to ${action.destKnot}`;
          break;
        case "promoteStitch":
          result = session.promoteStitch(action.path, action.knot, action.stitch);
          description = `Promote ${action.stitch} to knot`;
          break;
        case "demoteKnot":
          result = session.demoteKnot(action.path, action.knot, action.destKnot);
          description = `Demote ${action.knot} into ${action.destKnot}`;
          break;
      }

      if (result.ok && result.path) {
        await applyMoveResult(result, description, [result.path]);
      }
    },
    [getSession, applyMoveResult],
  );

  // ── Tab open helpers ────────────────────────────────────────────

  const handleOpenUnpinned = useCallback(
    (target: TabTarget) => {
      void openTab(target, false);
    },
    [openTab],
  );

  const handleOpenPinned = useCallback(
    (target: TabTarget) => {
      void openTab(target, true);
    },
    [openTab],
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

  // ── New file input ──────────────────────────────────────────────

  const handleNewClick = useCallback(() => {
    if (inputActive) return;
    setInputActive(true);
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [inputActive]);

  const cancelInput = useCallback(() => {
    setInputActive(false);
  }, []);

  const confirmInput = useCallback(() => {
    const input = inputRef.current;
    if (!input) return;
    let name = input.value.trim();
    if (!name) {
      setInputActive(false);
      return;
    }
    if (!name.includes(".")) {
      name += ".ink";
    }
    setInputActive(false);
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

  // ── Context menu handler ────────────────────────────────────────

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, row: FlatRow) => {
      if (row.kind === "file") return; // No context menu for files
      e.preventDefault();
      e.stopPropagation();
      setContextMenu({
        x: e.clientX,
        y: e.clientY,
        target: {
          kind: row.kind,
          path: row.path,
          knot: row.knot!,
          stitch: row.stitch,
          index: row.index,
          siblingCount: row.siblingCount,
        },
      });
    },
    [],
  );

  const handleContextMenuAction = useCallback(
    (action: ContextMenuAction) => {
      setContextMenu(null);
      void executeAction(action);
    },
    [executeAction],
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
        if (row) {
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
        if (!row || row.kind === "file") return;
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
    ],
  );

  // ── Drag handlers ───────────────────────────────────────────────

  const handleDragStart = useCallback(
    (e: React.DragEvent, row: FlatRow) => {
      if (row.kind === "file") {
        e.preventDefault();
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
  }, []);

  const handleDragOver = useCallback(
    (e: React.DragEvent, row: FlatRow) => {
      if (!dragState) return;
      e.preventDefault();

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
    (e: React.DragEvent, row: FlatRow) => {
      e.preventDefault();
      if (!dragState || !dropTarget) return;

      const sourceKey = dragState.sourceKeys[0]!;
      const sourceParts = sourceKey.split("::");

      const path = dragState.sourcePath;

      if (dragState.sourceKind === "stitch") {
        const srcKnot = sourceParts[1]!;
        const stitch = sourceParts[2]!;

        if (dropTarget.kind === "into") {
          // Move to different knot
          const destParts = dropTarget.targetKey!.split("::");
          const destKnot = destParts[1] ?? destParts[0]!;
          if (destKnot !== srcKnot) {
            void executeAction({ type: "moveStitch", path, srcKnot, stitch, destKnot });
          }
        } else {
          // Reorder within same knot
          const direction =
            dropTarget.targetKey === "after" ? 1 : -1;
          void executeAction({
            type: "reorderStitch",
            path,
            knot: srcKnot,
            stitch,
            direction,
          });
        }
      } else if (dragState.sourceKind === "knot") {
        const knot = sourceParts[1]!;

        if (dropTarget.kind === "into") {
          // Demote into target knot
          const destParts = dropTarget.targetKey!.split("::");
          const destKnot = destParts[1] ?? destParts[0]!;
          void executeAction({ type: "demoteKnot", path, knot, destKnot });
        } else {
          // Reorder
          const direction = dropTarget.targetKey === "after" ? 1 : -1;
          void executeAction({ type: "reorderKnot", path, knot, direction });
        }
      }

      setDragState(null);
      setDropTarget(null);
    },
    [dragState, dropTarget, executeAction],
  );

  // ── Drop line helper ───────────────────────────────────────────

  function dropLineFor(rowKey: string): "before" | "after" | null {
    if (!dropTarget || dropTarget.kind !== "between") return null;
    if (dropTarget.afterKey !== rowKey) return null;
    return dropTarget.targetKey === "before" ? "before" : "after";
  }

  // ── Render helpers ──────────────────────────────────────────────

  function renderStitch(path: string, knot: DocumentSymbol, stitch: DocumentSymbol, row: FlatRow) {
    const stitchId = row.key;
    const isActive = activeTabId === stitchId;
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
        depth={2}
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

  function renderKnot(path: string, knot: DocumentSymbol, row: FlatRow) {
    const knotKey = row.key;
    const stitches = knot.children.filter((c) => c.kind === "stitch");
    const hasStitches = stitches.length > 0;
    const isExpanded = !collapsed.has(knotKey);
    const isActive = activeTabId === knotKey;
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
          depth={1}
          kind="knot"
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
            return renderStitch(path, knot, s, sRow);
          })}
      </div>
    );
  }

  function renderFile(file: FileOutline) {
    const knots = file.symbols.filter((s) => s.kind === "knot");
    const hasChildren = knots.length > 0;
    const fileKey = file.path;
    const isExpanded = !collapsed.has(fileKey);
    const isActive = activeTabId === fileKey;
    const target: TabTarget = { kind: "file", path: file.path };

    return (
      <div key={fileKey}>
        <BinderRow
          rowKey={fileKey}
          depth={0}
          kind="file"
          label={displayName(file.path)}
          expandable={hasChildren}
          isExpanded={isExpanded}
          isActive={isActive}
          isSelected={selectedKeys.has(fileKey)}
          isFocused={focusedKey === fileKey}
          isDragging={false}
          isDropInto={false}
          dropLinePosition={null}
          draggable={false}
          onChevronClick={() => toggleCollapsed(fileKey)}
          onClick={(e) => handleRowClick(fileKey, target, e)}
          onDoubleClick={() => handleOpenPinned(target)}
          onContextMenu={(e) => e.preventDefault()}
          onDragStart={() => {}}
          onDragEnd={() => {}}
          onDragOver={() => {}}
          onDrop={() => {}}
        />
        {isExpanded &&
          knots.map((k) => {
            const kRow = flatRows.find((r) => r.key === `${file.path}::${k.name}`);
            if (!kRow) return null;
            return renderKnot(file.path, k, kRow);
          })}
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="brink-binder"
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      {outline.map((file) => renderFile(file))}
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
