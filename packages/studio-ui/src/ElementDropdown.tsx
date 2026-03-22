import { useCallback, useEffect, useRef, useState, memo } from "react";
import { CONVERTIBLE_TYPES } from "@brink/ink-operations";

interface ElementDropdownProps {
  visible: boolean;
  onSelect: (sigil: string) => void;
  onDismiss: () => void;
  anchorRect: DOMRect | null;
}

function ElementDropdownInner({ visible, onSelect, onDismiss, anchorRect }: ElementDropdownProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  // Reset selection when dropdown opens
  useEffect(() => {
    if (visible) {
      setSelectedIndex(0);
    }
  }, [visible]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!visible) return;

      if (e.key === "Escape") {
        e.preventDefault();
        onDismiss();
        return;
      }

      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((i) => (i + 1) % CONVERTIBLE_TYPES.length);
        return;
      }

      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((i) => (i - 1 + CONVERTIBLE_TYPES.length) % CONVERTIBLE_TYPES.length);
        return;
      }

      if (e.key === "Enter") {
        e.preventDefault();
        const item = CONVERTIBLE_TYPES[selectedIndex];
        if (item) onSelect(item.sigil);
        return;
      }

      // Shortcut key match
      const lower = e.key.toLowerCase();
      const match = CONVERTIBLE_TYPES.find((t) => t.key === lower);
      if (match) {
        e.preventDefault();
        onSelect(match.sigil);
      }
    },
    [visible, selectedIndex, onSelect, onDismiss],
  );

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // Dismiss on outside click
  useEffect(() => {
    if (!visible) return;
    function handleClick(e: MouseEvent) {
      if (listRef.current && !listRef.current.contains(e.target as Node)) {
        onDismiss();
      }
    }
    document.addEventListener("click", handleClick);
    return () => document.removeEventListener("click", handleClick);
  }, [visible, onDismiss]);

  if (!visible || !anchorRect) return null;

  const style: React.CSSProperties = {
    position: "fixed",
    left: anchorRect.left,
    bottom: window.innerHeight - anchorRect.top + 4,
  };

  return (
    <div className="brink-element-dropdown" ref={listRef} style={style}>
      {CONVERTIBLE_TYPES.map((item, index) => (
        <button
          key={item.sigil}
          className={
            "brink-element-dropdown-item" + (index === selectedIndex ? " selected" : "")
          }
          onMouseDown={(e) => {
            e.preventDefault();
            onSelect(item.sigil);
          }}
          onMouseEnter={() => setSelectedIndex(index)}
        >
          {item.label}
          <span className="brink-element-dropdown-key">{item.key}</span>
        </button>
      ))}
    </div>
  );
}

export const ElementDropdown = memo(ElementDropdownInner);
