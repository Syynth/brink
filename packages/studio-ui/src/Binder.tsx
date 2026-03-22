import { memo, useCallback, useRef, useState } from "react";
import { useStudioStore } from "./StoreContext.js";
import type { FileOutline, DocumentSymbol } from "@brink/wasm-types";
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

// ── Row component ──────────────────────────────────────────────────

interface RowProps {
  depth: number;
  kind: string;
  label: string;
  expandable: boolean;
  isExpanded: boolean;
  isActive: boolean;
  onChevronClick: () => void;
  onClick: () => void;
  onDoubleClick: () => void;
}

function BinderRow({
  depth,
  kind,
  label,
  expandable,
  isExpanded,
  isActive,
  onChevronClick,
  onClick,
  onDoubleClick,
}: RowProps) {
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleClick = useCallback(() => {
    if (clickTimer.current) clearTimeout(clickTimer.current);
    clickTimer.current = setTimeout(() => {
      clickTimer.current = null;
      onClick();
    }, 200);
  }, [onClick]);

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
    (isActive ? " brink-binder-active" : "");

  const chevronClass =
    "brink-binder-chevron" +
    (expandable ? (isExpanded ? "" : " collapsed") : " leaf");

  return (
    <div className={rowClass} onClick={handleClick} onDoubleClick={handleDoubleClick}>
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
  );
}

// ── Main Binder component ──────────────────────────────────────────

function BinderInner() {
  const outline = useStudioStore((s) => s.outline);
  const activeTabId = useStudioStore((s) => s.activeTabId);
  const collapsed = useStudioStore((s) => s.collapsed);
  const openTab = useStudioStore((s) => s.openTab);
  const toggleCollapsed = useStudioStore((s) => s.toggleCollapsed);
  const addFile = useStudioStore((s) => s.addFile);

  const [inputActive, setInputActive] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

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

  function renderStitch(path: string, stitch: DocumentSymbol) {
    const stitchId = `${path}::${stitch.name}`;
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
        depth={2}
        kind="stitch"
        label={stitch.name}
        expandable={false}
        isExpanded={false}
        isActive={isActive}
        onChevronClick={() => {}}
        onClick={() => handleOpenUnpinned(target)}
        onDoubleClick={() => handleOpenPinned(target)}
      />
    );
  }

  function renderKnot(path: string, knot: DocumentSymbol) {
    const knotKey = `${path}::${knot.name}`;
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
          depth={1}
          kind="knot"
          label={knot.name}
          expandable={hasStitches}
          isExpanded={isExpanded}
          isActive={isActive}
          onChevronClick={() => toggleCollapsed(knotKey)}
          onClick={() => handleOpenUnpinned(target)}
          onDoubleClick={() => handleOpenPinned(target)}
        />
        {hasStitches && isExpanded && stitches.map((s) => renderStitch(path, s))}
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
          depth={0}
          kind="file"
          label={displayName(file.path)}
          expandable={hasChildren}
          isExpanded={isExpanded}
          isActive={isActive}
          onChevronClick={() => toggleCollapsed(fileKey)}
          onClick={() => handleOpenUnpinned(target)}
          onDoubleClick={() => handleOpenPinned(target)}
        />
        {isExpanded && knots.map((k) => renderKnot(file.path, k))}
      </div>
    );
  }

  return (
    <div className="brink-binder">
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
    </div>
  );
}

export const Binder = memo(BinderInner);
