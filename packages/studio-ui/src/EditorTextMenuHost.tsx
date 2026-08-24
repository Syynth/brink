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
import type { EditorTextMenuRequest } from "@brink/studio-store";
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
  // The dismiss contract (global capture-phase Escape included) must exist
  // ONLY while a menu is open — hoisted into an inner component so the
  // listeners mount with the menu. The first cut ran the hook here
  // unconditionally and swallowed every Escape in the app (drag cancel,
  // maximize restore, keymap defaults — four E2E reds).
  if (!textMenu) return null;
  return <EditorTextMenu menu={textMenu} onClose={closeTextMenu} />;
}

function EditorTextMenu({
  menu: textMenu,
  onClose: closeTextMenu,
}: {
  menu: EditorTextMenuRequest;
  onClose: () => void;
}): ReactElement {
  const menuRef = useRef<HTMLDivElement>(null);
  useContextMenuDismiss(menuRef, closeTextMenu);

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
