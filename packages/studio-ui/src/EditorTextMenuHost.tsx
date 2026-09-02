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
import { ensureToolWindowOpen, useShell } from "@brink/studio-shell";
import type { EditorTextMenuRequest } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import { saveEditorSettings } from "./SettingsDocument.js";
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
  const { layout } = useShell();
  useContextMenuDismiss(menuRef, closeTextMenu);
  const formGlyph = useStudioStore((s) => s.formGlyph);
  const autoOpenForm = useStudioStore((s) => s.autoOpenForm);
  const showGutters = useStudioStore((s) => s.showGutters);
  const showInlayHints = useStudioStore((s) => s.showInlayHints);
  const editorFontSize = useStudioStore((s) => s.editorFontSize);
  const appFontSize = useStudioStore((s) => s.appFontSize);
  const setShowGutters = useStudioStore((s) => s.setShowGutters);
  // Break on write (W18/#3311, maintainer follow-up): the editor menu
  // offers the same verb the Debugger panel's variable rows carry, when
  // the clicked identifier is a known GLOBAL — by the live session's
  // debug mirror when one runs, else the compiled program model (so the
  // verb works before any session; rows arm on bind).
  const dataBreakpoints = useStudioStore((s) => s.dataBreakpoints);
  const dataBreakpointToggle = useStudioStore((s) => s.dataBreakpointToggle);
  const identityIsGlobal = useStudioStore((s) => {
    const name = textMenu.identity?.name;
    if (name === undefined || name === "") return false;
    return (
      (s.debugState?.globals.some((g) => g.name === name) ?? false) ||
      (s.programModel?.globals.some((g) => g.name === name) ?? false)
    );
  });

  // Group order per the context-menu spec: Navigate · Rename · Text.
  const identity = textMenu.identity;
  const identityItems: TextMenuItem[] = identity
    ? [
        { label: "Go to Definition", shortcut: "⌘Click", run: identity.gotoDefinition },
        ...(identity.findReferences
          ? [{ label: "Find References", shortcut: "⇧⌥F", run: identity.findReferences }]
          : []),
        ...(identity.rename
          ? [
              {
                label: identity.name === "" ? "Rename…" : `Rename '${identity.name}'…`,
                shortcut: "F2",
                run: identity.rename,
              },
            ]
          : []),
        ...(identityIsGlobal
          ? [
              {
                label: dataBreakpoints.some((r) => r.name === identity.name)
                  ? `Remove Break on Write '${identity.name}'`
                  : `Break on Write '${identity.name}'`,
                shortcut: "",
                run: () => dataBreakpointToggle(identity.name),
              },
            ]
          : []),
      ]
    : [];
  // Context group (spec order: … · Context-specific · Text): editor-side
  // line actions (Open File, Fold/Unfold) plus studio-side additions the
  // editor can't know about (panels).
  const contextItems: TextMenuItem[] = (textMenu.lineActions ?? []).map((a) => ({
    label: a.label,
    shortcut: "",
    run: a.run,
  }));
  if (textMenu.lineType === "todo") {
    contextItems.push({
      label: "Show in TODOs Panel",
      shortcut: "",
      run: () => ensureToolWindowOpen(layout, "todos"),
    });
  }

  const textItems: TextMenuItem[] = [
    { label: "Cut", shortcut: "⌘X", disabled: !textMenu.hasSelection, run: textMenu.cut },
    { label: "Copy", shortcut: "⌘C", disabled: !textMenu.hasSelection, run: textMenu.copy },
    { label: "Paste", shortcut: "⌘V", run: textMenu.paste },
    { label: "Select All", shortcut: "⌘A", run: textMenu.selectAll },
    // View toggle (below the edit group's separator): mirrors the Settings
    // checkbox, persisting through the same editor-settings payload.
    {
      label: showGutters ? "Hide Gutters" : "Show Gutters",
      shortcut: "",
      run: () => {
        setShowGutters(!showGutters);
        saveEditorSettings(window.localStorage, {
          formGlyph,
          autoOpenForm,
          showGutters: !showGutters,
          showInlayHints,
          fontSize: editorFontSize,
          appFontSize,
        });
      },
    },
  ];

  return (
    <div
      ref={menuRef}
      className="brink-context-menu brink-text-menu"
      style={{ left: textMenu.x, top: textMenu.y }}
      role="menu"
    >
      {identityItems.map((item) => (
        <div key={item.label} role="presentation">
          <div
            role="menuitem"
            className="brink-context-menu-item"
            onClick={() => {
              closeTextMenu();
              item.run();
            }}
          >
            {item.label}
            <span className="brink-context-menu-shortcut">{item.shortcut}</span>
          </div>
        </div>
      ))}
      {identityItems.length > 0 && <div className="brink-context-menu-separator" />}
      {contextItems.map((item) => (
        <div key={item.label} role="presentation">
          <div
            role="menuitem"
            className="brink-context-menu-item"
            onClick={() => {
              closeTextMenu();
              item.run();
            }}
          >
            {item.label}
            {item.shortcut !== "" && (
              <span className="brink-context-menu-shortcut">{item.shortcut}</span>
            )}
          </div>
        </div>
      ))}
      {contextItems.length > 0 && <div className="brink-context-menu-separator" />}
      {textItems.map((item, i) => (
        <div key={item.label} role="presentation">
          {(i === 3 || i === 4) && <div className="brink-context-menu-separator" />}
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
