import { memo } from "react";
import type { SidebarView } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";

// ── View registry ──────────────────────────────────────────────────
//
// One entry per dockable sidebar view. The activity bar renders a button
// per entry; the docked panel / drawer renders the active view's body.

interface ViewDef {
  id: SidebarView;
  label: string;
  icon: string;
}

export const SIDEBAR_VIEWS: ViewDef[] = [
  { id: "binder", label: "Binder", icon: "☰" },
  { id: "state", label: "State", icon: "{}" },
  { id: "program", label: "Program", icon: "⌗" },
];

/** Display label for a sidebar view (used as the panel header title). */
export function sidebarViewLabel(view: SidebarView): string {
  return SIDEBAR_VIEWS.find((v) => v.id === view)?.label ?? "";
}

// ── Component ───────────────────────────────────────────────────────

function ActivityBarInner() {
  const activeSidebarView = useStudioStore((s) => s.activeSidebarView);
  const setSidebarView = useStudioStore((s) => s.setSidebarView);

  return (
    <div className="studio-activitybar" role="tablist" aria-label="Sidebar views">
      {SIDEBAR_VIEWS.map((v) => {
        const active = v.id === activeSidebarView;
        return (
          <button
            key={v.id}
            type="button"
            role="tab"
            aria-selected={active}
            aria-label={v.label}
            title={v.label}
            className={"studio-activitybar-btn" + (active ? " active" : "")}
            onClick={() => setSidebarView(v.id)}
          >
            {v.icon}
          </button>
        );
      })}
    </div>
  );
}

export const ActivityBar = memo(ActivityBarInner);
