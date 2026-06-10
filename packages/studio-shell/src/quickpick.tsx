/**
 * @brink/studio-shell — generic quick-pick surface (docs/studio-shell-spec.md §6).
 *
 * The fuzzy input-over-list overlay shared by the command palette and
 * quick-open ("reuses the same overlay component with a different provider").
 * Owns query/selection state and the keyboard model; providers supply items
 * and handle picks.
 */

import { useEffect, useRef, useState } from "react";
import { Overlay } from "./overlay.js";

export interface QuickPickItem {
  /** Stable identity within the list. */
  key: string;
  /** Primary display text; preferred match target. */
  title: string;
  /** Right-aligned hint (keybinding, file path, kind). */
  detail?: string;
  /** Secondary match target (command id, qualified name). */
  searchText?: string;
}

/**
 * Case-insensitive subsequence ranking: earlier and more compact matches
 * first, title matches preferred over searchText, ties by input order.
 */
export function rankQuickPickItems<T extends QuickPickItem>(
  items: readonly T[],
  query: string,
): T[] {
  const q = query.trim().toLowerCase();
  if (q === "") return [...items];

  const scored: { item: T; score: number }[] = [];
  for (const item of items) {
    let score = subsequenceScore(item.title.toLowerCase(), q);
    if (item.searchText !== undefined) {
      score = Math.min(score, subsequenceScore(item.searchText.toLowerCase(), q) + 1);
    }
    if (score !== Number.POSITIVE_INFINITY) scored.push({ item, score });
  }
  scored.sort((a, b) => a.score - b.score);
  return scored.map((s) => s.item);
}

/** Lower is better; Infinity if `query` is not a subsequence of `text`. */
function subsequenceScore(text: string, query: string): number {
  let pos = text.indexOf(query[0] ?? "");
  if (pos === -1) return Number.POSITIVE_INFINITY;
  const start = pos;
  for (let i = 1; i < query.length; i++) {
    pos = text.indexOf(query.charAt(i), pos + 1);
    if (pos === -1) return Number.POSITIVE_INFINITY;
  }
  // Contiguity dominates, then earliness.
  return (pos - start - (query.length - 1)) * 100 + start;
}

export interface QuickPickProps<T extends QuickPickItem> {
  open: boolean;
  onClose(): void;
  /** Unfiltered items; the picker ranks them against its query. */
  items: readonly T[];
  onPick(item: T): void;
  placeholder: string;
  emptyText: string;
  ariaLabel: string;
}

export function QuickPick<T extends QuickPickItem>({
  open,
  onClose,
  items,
  onPick,
  placeholder,
  emptyText,
  ariaLabel,
}: QuickPickProps<T>) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Reset transient state every time the picker opens.
  useEffect(() => {
    if (open) {
      setQuery("");
      setSelected(0);
      inputRef.current?.focus();
    }
  }, [open]);

  const ranked = open ? rankQuickPickItems(items, query) : [];
  const clampedSelected = Math.min(selected, Math.max(0, ranked.length - 1));

  const pick = (item: T): void => {
    // Close first so focus returns before the pick's effects land.
    onClose();
    onPick(item);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (ranked.length === 0) return;
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setSelected((ranked.length + clampedSelected + delta) % ranked.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const item = ranked[clampedSelected];
      if (item) pick(item);
    }
  };

  return (
    <Overlay open={open} onClose={onClose} className="shell-palette">
      <input
        ref={inputRef}
        className="shell-palette-input"
        type="text"
        placeholder={placeholder}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setSelected(0);
        }}
        onKeyDown={onKeyDown}
        aria-label={ariaLabel}
      />
      <ul className="shell-palette-list" role="listbox">
        {ranked.map((item, index) => (
          <li
            key={item.key}
            role="option"
            aria-selected={index === clampedSelected}
            className={"shell-palette-item" + (index === clampedSelected ? " selected" : "")}
            onMouseEnter={() => setSelected(index)}
            onClick={() => pick(item)}
          >
            <span className="title">{item.title}</span>
            {item.detail !== undefined && <span className="binding">{item.detail}</span>}
          </li>
        ))}
        {ranked.length === 0 && <li className="shell-palette-empty">{emptyText}</li>}
      </ul>
    </Overlay>
  );
}
