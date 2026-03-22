import { memo, useCallback, useRef, useState } from "react";
import { useStudioStore } from "./StoreContext.js";
import { ElementDropdown } from "./ElementDropdown.js";
import type { LineInfo } from "@brink/studio-store";
import { ElementTypeEnum } from "@brink/studio-store";

// ── Element labels ─────────────────────────────────────────────────

const ELEMENT_LABELS: Record<number, string> = {
  0: "Knot Header",
  1: "Stitch Header",
  2: "Narrative",
  3: "Choice",
  4: "Choice Body",
  5: "Gather",
  6: "Divert",
  7: "Logic",
  8: "Variable",
  9: "Comment",
  10: "Include",
  11: "External",
  12: "Tag",
  13: "Blank",
  14: "Character",
  15: "Parenthetical",
  16: "Dialogue",
};

function elementLabel(info: LineInfo): string {
  let label = ELEMENT_LABELS[info.type] ?? "Unknown";
  if (
    (info.type === ElementTypeEnum.Choice || info.type === ElementTypeEnum.Gather) &&
    info.depth > 1
  ) {
    label += ` \u00b7 ${info.depth}`;
  }
  if (info.type === ElementTypeEnum.Choice && info.sticky) {
    label += " (+)";
  }
  return label;
}

// ── Component ──────────────────────────────────────────────────────

function StatusBarInner() {
  const cursor = useStudioStore((s) => s.cursor);
  const lineInfo = useStudioStore((s) => s.currentLineInfo);
  const hints = useStudioStore((s) => s.currentLineHints);
  const diagnostics = useStudioStore((s) => s.diagnostics);
  const convertLine = useStudioStore((s) => s.convertLineToType);

  const [dropdownVisible, setDropdownVisible] = useState(false);
  const [anchorRect, setAnchorRect] = useState<DOMRect | null>(null);
  const btnRef = useRef<HTMLButtonElement>(null);

  const handleElementClick = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (btnRef.current) {
      setAnchorRect(btnRef.current.getBoundingClientRect());
    }
    setDropdownVisible((v) => !v);
  }, []);

  const handleSelect = useCallback(
    (sigil: string) => {
      setDropdownVisible(false);
      convertLine(sigil);
    },
    [convertLine],
  );

  const handleDismiss = useCallback(() => {
    setDropdownVisible(false);
  }, []);

  // Key hints
  const hintText = hints.map((h) => `${h.key}: ${h.hint}`).join("  \u00b7  ");

  // Diagnostics
  let diagText = "";
  let diagClass = "brink-status-diag";
  if (diagnostics.errors > 0 || diagnostics.warnings > 0) {
    const parts: string[] = [];
    if (diagnostics.errors > 0) {
      parts.push(`${diagnostics.errors} error${diagnostics.errors > 1 ? "s" : ""}`);
    }
    if (diagnostics.warnings > 0) {
      parts.push(`${diagnostics.warnings} warning${diagnostics.warnings > 1 ? "s" : ""}`);
    }
    diagText = parts.join(", ");
    diagClass += diagnostics.errors > 0 ? " has-errors" : " has-warnings";
  }

  return (
    <div className="brink-statusbar">
      <div className="brink-status-left">
        <span className="brink-status-keyhint">{hintText}</span>
      </div>
      <div className="brink-status-right">
        <span className={diagClass}>{diagText}</span>
        <button
          ref={btnRef}
          className="brink-status-element-btn"
          onClick={handleElementClick}
        >
          {lineInfo ? elementLabel(lineInfo) : "Blank"}
        </button>
        <span className="brink-status-cursor">
          {cursor.line}:{cursor.col}
        </span>
      </div>
      <ElementDropdown
        visible={dropdownVisible}
        onSelect={handleSelect}
        onDismiss={handleDismiss}
        anchorRect={anchorRect}
      />
    </div>
  );
}

export const StatusBar = memo(StatusBarInner);
