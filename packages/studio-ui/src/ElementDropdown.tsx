/**
 * Element conversion dropdown, on the shell overlay primitive (spec §7.7).
 *
 * The overlay owns anchoring (flips above the status bar at the viewport
 * edge), Escape/outside-click dismissal, and focus return — replacing the
 * old manual position:fixed rect-tracking and its dismiss-on-scroll
 * workaround (floating-ui's autoUpdate repositions instead). This component
 * keeps the list, keyboard model (arrows/Enter/shortcut letters), and the
 * class names the e2e suite clicks.
 */

import { useCallback, useEffect, useState, memo } from "react";
import { Overlay } from "@brink/studio-shell";
import { CONVERTIBLE_TYPES } from "@brink/ink-operations";

interface ElementDropdownProps {
  open: boolean;
  anchor: HTMLElement | null;
  onSelect: (sigil: string) => void;
  onDismiss: () => void;
}

function ElementDropdownInner({ open, anchor, onSelect, onDismiss }: ElementDropdownProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);

  // Reset selection when the dropdown opens.
  useEffect(() => {
    if (open) setSelectedIndex(0);
  }, [open]);

  // Arrow/Enter/shortcut-letter navigation. Escape is the overlay's job.
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
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
      const match = CONVERTIBLE_TYPES.find((t) => t.key === e.key.toLowerCase());
      if (match) {
        e.preventDefault();
        onSelect(match.sigil);
      }
    },
    [selectedIndex, onSelect],
  );

  useEffect(() => {
    if (!open) return;
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [open, handleKeyDown]);

  return (
    <Overlay open={open} onClose={onDismiss} anchor={anchor} placement="top-start">
      <div className="brink-element-dropdown">
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
    </Overlay>
  );
}

export const ElementDropdown = memo(ElementDropdownInner);
