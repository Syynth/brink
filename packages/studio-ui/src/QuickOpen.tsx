/**
 * Quick-open (docs/studio-shell-spec.md §6): Mod-P over binder items — files,
 * knots, stitches from the compile outline — on the shared QuickPick surface.
 * Picks dispatch `editor.reveal` with source locations taken straight from
 * the outline (no symbol re-resolution: per-file precision for free).
 */

import { useCallback, useEffect, useState } from "react";
import {
  EDITOR_REVEAL_COMMAND_ID,
  QuickPick,
  useShell,
  type QuickPickItem,
} from "@brink/studio-shell";
import type { FileOutline, DocumentSymbol } from "@brink/wasm-types";
import { useStudioStore } from "./StoreContext.js";

export const QUICK_OPEN_COMMAND_ID = "quickOpen.toggle";

export interface QuickOpenItem extends QuickPickItem {
  file: string;
  span: { start: number; end: number };
}

/**
 * Flatten the outline into pick items: one per file, one per symbol with its
 * qualified name. Deterministic: outline order (file, then symbol order,
 * depth-first). Exported for tests.
 */
export function buildQuickOpenItems(outline: readonly FileOutline[]): QuickOpenItem[] {
  const items: QuickOpenItem[] = [];
  // The author's manuscript only — the same set the Binder tree shows, and
  // the same filter Continuous view needs (#3136). Mounted `std/` library
  // files are not somewhere you navigate to while writing, and listing them
  // put two `std/conventions/screenplay.brink` symbols under one React key.
  for (const file of outline) {
    if (file.mounted || file.path === "brink.toml") continue;
    items.push({
      key: `file:${file.path}`,
      title: file.path,
      detail: "file",
      file: file.path,
      span: { start: 0, end: 0 },
    });
    const walk = (symbols: readonly DocumentSymbol[], prefix: string): void => {
      for (const symbol of symbols) {
        const qualified = prefix === "" ? symbol.name : `${prefix}.${symbol.name}`;
        items.push({
          // Span-qualified: a name is not unique within a file (two knots can
          // declare the same stitch name), and a duplicate React key silently
          // drops or duplicates rows rather than erroring.
          key: `sym:${file.path}:${qualified}:${symbol.start}`,
          title: qualified,
          detail: `${symbol.kind} · ${file.path}`,
          searchText: `${qualified} ${file.path}`,
          file: file.path,
          span: { start: symbol.start, end: symbol.end },
        });
        walk(symbol.children, qualified);
      }
    };
    walk(file.symbols, "");
  }
  return items;
}

export function QuickOpen() {
  const { commands } = useShell();
  const outline = useStudioStore((s) => s.outline);
  const [open, setOpen] = useState(false);

  useEffect(
    () =>
      commands.register({
        id: QUICK_OPEN_COMMAND_ID,
        title: "Go to File or Symbol",
        // Mod-P (browser print is interceptable) with Mod-E as the alternate
        // in case a browser claims it (#107 multi-binding pattern).
        keybinding: ["Mod-P", "Mod-E"],
        run: () => setOpen((wasOpen) => !wasOpen),
      }),
    [commands],
  );

  const close = useCallback(() => setOpen(false), []);
  const items = open ? buildQuickOpenItems(outline) : [];

  return (
    <QuickPick
      open={open}
      onClose={close}
      items={items}
      onPick={(item) =>
        commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
          kind: "source",
          file: item.file,
          span: item.span,
        })
      }
      placeholder="Go to file or symbol…"
      emptyText="No matches"
      ariaLabel="Quick open"
    />
  );
}
