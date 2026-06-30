import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { DocumentSymbol, FileOutline } from "@brink/wasm-types";

// ── Types ───────────────────────────────────────────────────────────

export type ContextMenuTarget =
  | {
      kind: "knot" | "stitch";
      path: string;
      knot: string;
      stitch?: string;
      /** Position in sibling list */
      index: number;
      /** Total siblings */
      siblingCount: number;
    }
  | {
      kind: "file";
      path: string;
      /** Whether the provider supports deletion (hides Delete when false). */
      canDelete: boolean;
      /** Whether files can be renamed/moved (hides Rename when false). */
      canRename: boolean;
    }
  | {
      kind: "folder";
      /** Directory key with trailing slash, e.g. "scenes/act1/". */
      prefix: string;
      /** All file paths under the folder (recursive). */
      paths: string[];
      canDelete: boolean;
      canRename: boolean;
    };

interface MenuItem {
  label: string;
  disabled?: boolean;
  action?: () => void;
  submenu?: MenuItem[];
}

interface Props {
  x: number;
  y: number;
  target: ContextMenuTarget;
  outline: FileOutline[];
  onAction: (action: ContextMenuAction) => void;
  onClose: () => void;
}

export type ContextMenuAction =
  | { type: "playFromHere"; path: string; inkPath: string; label: string }
  | { type: "renameSymbol"; path: string; knot: string; stitch?: string; source?: "editor" | "graph" }
  | { type: "reorderStitch"; path: string; knot: string; stitch: string; direction: number }
  | { type: "reorderKnot"; path: string; knot: string; direction: number }
  | { type: "reorderStitches"; path: string; knot: string; order: string[] }
  | { type: "reorderKnots"; path: string; order: string[] }
  | { type: "moveStitch"; path: string; srcKnot: string; stitch: string; destKnot: string }
  | { type: "promoteStitch"; path: string; knot: string; stitch: string }
  | { type: "demoteKnot"; path: string; knot: string; destKnot: string }
  | { type: "deleteFile"; path: string }
  | { type: "deleteFolder"; prefix: string; paths: string[] }
  | { type: "newFileInFolder"; dir: string }
  | { type: "renameFile"; path: string }
  | { type: "renameFolder"; prefix: string };

/** Directory of a file path, with trailing slash; "" for a root-level file. */
function dirOf(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash >= 0 ? path.substring(0, slash + 1) : "";
}

// ── Component ───────────────────────────────────────────────────────

function BinderContextMenuInner({ x, y, target, outline, onAction, onClose }: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [submenuFor, setSubmenuFor] = useState<string | null>(null);

  // Close on click-outside or Escape
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    // The menu is fixed-positioned at the click coords, so a page scroll / resize
    // / focus loss would strand it — close on those. Use a *non-capturing*
    // scroll listener so only genuine page scroll closes it; a capturing one
    // also fires on inner-element scrolls (e.g. CodeMirror's scroller emits one
    // on right-click), which would dismiss the menu the instant it opens.
    const close = () => onClose();
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKey);
    window.addEventListener("scroll", close);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKey);
      window.removeEventListener("scroll", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [onClose]);

  const items: MenuItem[] = buildItems(target, outline, onAction);

  const handleItemClick = useCallback(
    (item: MenuItem) => {
      if (item.disabled || !item.action) return;
      item.action();
      onClose();
    },
    [onClose],
  );

  return (
    <div
      ref={menuRef}
      className="brink-context-menu"
      style={{ left: x, top: y }}
    >
      {items.map((item, i) => {
        if (item.label === "---") {
          return <div key={i} className="brink-context-menu-separator" />;
        }
        if (item.submenu) {
          return (
            <div
              key={item.label}
              className={
                "brink-context-menu-item brink-context-menu-has-submenu" +
                (submenuFor === item.label ? " active" : "")
              }
              onMouseEnter={() => setSubmenuFor(item.label)}
              onMouseLeave={() => setSubmenuFor(null)}
            >
              <span>{item.label}</span>
              <span className="brink-context-menu-arrow">{"\u25b6"}</span>
              {submenuFor === item.label && (
                <div className="brink-context-menu brink-context-submenu">
                  {item.submenu.map((sub) => (
                    <div
                      key={sub.label}
                      className="brink-context-menu-item"
                      onClick={() => {
                        if (sub.action) {
                          sub.action();
                          onClose();
                        }
                      }}
                    >
                      {sub.label}
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        }
        return (
          <div
            key={item.label}
            className={
              "brink-context-menu-item" + (item.disabled ? " disabled" : "")
            }
            onClick={() => handleItemClick(item)}
          >
            {item.label}
          </div>
        );
      })}
    </div>
  );
}

// ── Menu construction ───────────────────────────────────────────────

/** Build the menu items for a target. Files/folders get lifecycle actions
 *  (New file here, Delete); knots/stitches get the structural-move actions. */
function buildItems(
  target: ContextMenuTarget,
  outline: FileOutline[],
  onAction: (action: ContextMenuAction) => void,
): MenuItem[] {
  if (target.kind === "file") {
    const items: MenuItem[] = [
      {
        label: "New file here",
        action: () => onAction({ type: "newFileInFolder", dir: dirOf(target.path) }),
      },
    ];
    if (target.canRename) {
      items.push({
        label: "Rename",
        action: () => onAction({ type: "renameFile", path: target.path }),
      });
    }
    if (target.canDelete) {
      items.push({ label: "---" });
      items.push({
        label: "Delete",
        action: () => onAction({ type: "deleteFile", path: target.path }),
      });
    }
    return items;
  }

  if (target.kind === "folder") {
    const items: MenuItem[] = [
      {
        label: "New file here",
        action: () => onAction({ type: "newFileInFolder", dir: target.prefix }),
      },
    ];
    if (target.canRename) {
      items.push({
        label: "Rename folder",
        disabled: target.paths.length === 0,
        action: () => onAction({ type: "renameFolder", prefix: target.prefix }),
      });
    }
    if (target.canDelete) {
      items.push({ label: "---" });
      items.push({
        label: `Delete folder (${target.paths.length})`,
        disabled: target.paths.length === 0,
        action: () =>
          onAction({ type: "deleteFolder", prefix: target.prefix, paths: target.paths }),
      });
    }
    return items;
  }

  // Knot / stitch: structural-move actions, scoped to the file's knots.
  const fileOutline = outline.find((f) => f.path === target.path);
  const allKnots: DocumentSymbol[] = fileOutline?.symbols.filter((s) => s.kind === "knot") ?? [];
  const items: MenuItem[] = [];

  // Play from here — start a fresh session entered at this knot/stitch. The ink
  // path is the qualified name `choose_path_string` expects (`knot` or
  // `knot.stitch`), not the binder row key.
  const inkPath = target.kind === "stitch" ? `${target.knot}.${target.stitch}` : target.knot;
  items.push({
    label: "Play from here",
    action: () =>
      onAction({ type: "playFromHere", path: target.path, inkPath, label: inkPath }),
  });
  items.push({ label: "---" });

  // Rename — safe-by-default; the prompt flips to a breakage report if the
  // rename would introduce diagnostics (#305).
  items.push({
    label: "Rename…",
    action: () =>
      onAction({
        type: "renameSymbol",
        path: target.path,
        knot: target.knot,
        stitch: target.kind === "stitch" ? target.stitch : undefined,
      }),
  });
  items.push({ label: "---" });

  if (target.kind === "stitch") {
    items.push({
      label: "Move Up",
      disabled: target.index === 0,
      action: () =>
        onAction({
          type: "reorderStitch",
          path: target.path,
          knot: target.knot,
          stitch: target.stitch!,
          direction: -1,
        }),
    });
    items.push({
      label: "Move Down",
      disabled: target.index >= target.siblingCount - 1,
      action: () =>
        onAction({
          type: "reorderStitch",
          path: target.path,
          knot: target.knot,
          stitch: target.stitch!,
          direction: 1,
        }),
    });
    items.push({ label: "---" });

    // Move to submenu — knots excluding current parent, excluding name collisions
    const moveTargets = allKnots.filter((k) => {
      if (k.name === target.knot) return false;
      return !k.children.some((c) => c.kind === "stitch" && c.name === target.stitch);
    });
    if (moveTargets.length > 0) {
      items.push({
        label: "Move to",
        submenu: moveTargets.map((k) => ({
          label: k.name,
          action: () =>
            onAction({
              type: "moveStitch",
              path: target.path,
              srcKnot: target.knot,
              stitch: target.stitch!,
              destKnot: k.name,
            }),
        })),
      });
    }

    items.push({
      label: "Promote to Knot",
      disabled: allKnots.some((k) => k.name === target.stitch),
      action: () =>
        onAction({
          type: "promoteStitch",
          path: target.path,
          knot: target.knot,
          stitch: target.stitch!,
        }),
    });
    return items;
  }

  // Knot context menu
  items.push({
    label: "Move Up",
    disabled: target.index === 0,
    action: () =>
      onAction({ type: "reorderKnot", path: target.path, knot: target.knot, direction: -1 }),
  });
  items.push({
    label: "Move Down",
    disabled: target.index >= target.siblingCount - 1,
    action: () =>
      onAction({ type: "reorderKnot", path: target.path, knot: target.knot, direction: 1 }),
  });
  items.push({ label: "---" });

  // Demote into submenu — sibling knots excluding self and collision check
  const knotNode = allKnots.find((k) => k.name === target.knot);
  const hasStitches = knotNode?.children.some((c) => c.kind === "stitch") ?? false;

  if (!hasStitches) {
    const demoteTargets = allKnots.filter((k) => {
      if (k.name === target.knot) return false;
      return !k.children.some((c) => c.kind === "stitch" && c.name === target.knot);
    });
    if (demoteTargets.length > 0) {
      items.push({
        label: "Demote into",
        submenu: demoteTargets.map((k) => ({
          label: k.name,
          action: () =>
            onAction({
              type: "demoteKnot",
              path: target.path,
              knot: target.knot,
              destKnot: k.name,
            }),
        })),
      });
    }
  }
  return items;
}

export const BinderContextMenu = memo(BinderContextMenuInner);
