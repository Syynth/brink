import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { DocumentSymbol, FileOutline } from "@brink/wasm-types";

// ── Types ───────────────────────────────────────────────────────────

export interface ContextMenuTarget {
  kind: "knot" | "stitch";
  path: string;
  knot: string;
  stitch?: string;
  /** Position in sibling list */
  index: number;
  /** Total siblings */
  siblingCount: number;
}

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
  | { type: "reorderStitch"; path: string; knot: string; stitch: string; direction: number }
  | { type: "reorderKnot"; path: string; knot: string; direction: number }
  | { type: "moveStitch"; path: string; srcKnot: string; stitch: string; destKnot: string }
  | { type: "promoteStitch"; path: string; knot: string; stitch: string }
  | { type: "demoteKnot"; path: string; knot: string; destKnot: string };

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
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKey);
    };
  }, [onClose]);

  // Get knots from same file for submenu
  const fileOutline = outline.find((f) => f.path === target.path);
  const allKnots: DocumentSymbol[] = fileOutline?.symbols.filter((s) => s.kind === "knot") ?? [];

  const items: MenuItem[] = [];

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
      // Check for name collision
      return !k.children.some(
        (c) => c.kind === "stitch" && c.name === target.stitch,
      );
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
  } else {
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
    const hasStitches =
      knotNode?.children.some((c) => c.kind === "stitch") ?? false;

    if (!hasStitches) {
      const demoteTargets = allKnots.filter((k) => {
        if (k.name === target.knot) return false;
        return !k.children.some(
          (c) => c.kind === "stitch" && c.name === target.knot,
        );
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
  }

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

export const BinderContextMenu = memo(BinderContextMenuInner);
