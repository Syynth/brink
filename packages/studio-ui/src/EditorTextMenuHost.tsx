/**
 * The editor's plain-text context menu (docs/editor-context-menu-spec.md).
 *
 * The editor's global `contextmenu` handler suppresses the native menu
 * everywhere inside the editor; text contexts raise an
 * `EditorTextMenuRequest` through the store, and this host renders the
 * replacement — Cut / Copy / Paste / Select All with their shortcuts, the
 * actions already bound to the raising view. Same chrome and dismiss
 * contract as the shared symbol menu.
 */

import { useRef, type ReactElement } from "react";
import { useStudioStore } from "./StoreContext.js";
import { useContextMenuDismiss } from "./BinderContextMenu.js";

interface TextMenuItem {
  label: string;
  shortcut: string;
  disabled?: boolean;
  run: () => void;
}

export function EditorTextMenuHost(): ReactElement | null {
  const textMenu = useStudioStore((s) => s.textMenu);
  const closeTextMenu = useStudioStore((s) => s.closeTextMenu);
  const menuRef = useRef<HTMLDivElement>(null);

  useContextMenuDismiss(menuRef, closeTextMenu);

  if (!textMenu) return null;

  const items: TextMenuItem[] = [
    { label: "Cut", shortcut: "⌘X", disabled: !textMenu.hasSelection, run: textMenu.cut },
    { label: "Copy", shortcut: "⌘C", disabled: !textMenu.hasSelection, run: textMenu.copy },
    { label: "Paste", shortcut: "⌘V", run: textMenu.paste },
    { label: "Select All", shortcut: "⌘A", run: textMenu.selectAll },
  ];

  return (
    <div
      ref={menuRef}
      className="brink-context-menu brink-text-menu"
      style={{ left: textMenu.x, top: textMenu.y }}
      role="menu"
    >
      {items.map((item, i) => (
        <div key={item.label} role="presentation">
          {i === 3 && <div className="brink-context-menu-separator" />}
          <div
            role="menuitem"
            aria-disabled={item.disabled || undefined}
            className={
              "brink-context-menu-item" + (item.disabled ? " is-disabled" : "")
            }
            onClick={() => {
              if (item.disabled) return;
              closeTextMenu();
              item.run();
            }}
          >
            {item.label}
            <span className="brink-context-menu-shortcut">{item.shortcut}</span>
          </div>
        </div>
      ))}
    </div>
  );
}
