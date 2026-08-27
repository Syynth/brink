/**
 * @brink/studio-shell — layout persistence (docs/studio-shell-spec.md §7.1).
 *
 * The durable half of the layout store — placements, open occupants, dock
 * sizes, maximized — round-trips through a versioned localStorage key.
 * Transient presentation (tier, drawers, narrow overlay) never persists.
 *
 * Loading is lenient: a corrupt, foreign-version, or partially-invalid
 * payload yields null (defaults win) rather than an error. Unknown
 * tool-window ids are NOT filtered here — applying the snapshot before the
 * provider's registry sync lets syncFromRegistry drop unknown ids and seed
 * new ones, which is exactly the specced behavior.
 */

import type { EditorViewId, ShellLayoutState, ShellLayoutStore } from "./layout-store.js";
import { DOCK_SECTION_IDS, type DockSectionId, type Placement } from "./toolwindow.js";

export const LAYOUT_STORAGE_KEY = "brink-studio.layout.v1";

const SNAPSHOT_VERSION = 1;

/** Debounce for writes — drags and splitter moves come in bursts. */
const WRITE_DEBOUNCE_MS = 250;

export interface LayoutSnapshot {
  placements: Record<string, Placement>;
  open: Record<DockSectionId, string | null>;
  dockSizes: { left: number; right: number; bottom: number };
  maximized: string | null;
  editorView: EditorViewId;
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

/** The persisted subset of the current state. */
export function snapshotLayout(state: ShellLayoutState): LayoutSnapshot {
  return {
    placements: state.placements,
    open: state.open,
    dockSizes: state.dockSizes,
    maximized: state.maximized,
    editorView: state.editorView,
  };
}

/** Load + validate a snapshot. Never throws; invalid payloads yield null. */
export function loadLayoutSnapshot(storage: Pick<Storage, "getItem">): LayoutSnapshot | null {
  let raw: string | null;
  try {
    raw = storage.getItem(LAYOUT_STORAGE_KEY);
  } catch {
    return null;
  }
  if (raw === null || raw === "") return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isRecord(parsed) || parsed.version !== SNAPSHOT_VERSION) return null;

  const placements = readPlacements(parsed.placements);
  const open = readOpen(parsed.open);
  const dockSizes = readDockSizes(parsed.dockSizes);
  if (placements === null || open === null || dockSizes === null) return null;

  // Open occupants must be placed in the section claiming them; anything
  // else (manual edits, future drift) falls back per-section.
  for (const key of DOCK_SECTION_IDS) {
    const occupant = open[key];
    if (occupant === null) continue;
    const placement = placements[occupant];
    if (placement === undefined || `${placement.dock}.${placement.section}` !== key) {
      open[key] = null;
    }
  }

  const maximized = parsed.maximized;
  // An unknown view id (an older payload, a hand edit, a view that has since
  // been removed) falls back to "code" rather than rejecting the whole
  // snapshot — losing your dock layout over an unreadable view name would be
  // a worse trade than starting in the default view.
  const editorView: EditorViewId =
    parsed.editorView === "single" || parsed.editorView === "continuous"
      ? parsed.editorView
      : "code";
  return {
    placements,
    open,
    dockSizes,
    maximized: typeof maximized === "string" ? maximized : null,
    editorView,
  };
}

/**
 * Subscribe the store to storage: debounced writes of the durable subset,
 * flushed on pagehide so quick closes don't lose the last change. Returns a
 * detach function (also flushes).
 */
export function attachLayoutPersistence(
  store: ShellLayoutStore,
  storage: StorageLike,
  target: Pick<Window, "addEventListener" | "removeEventListener"> | null =
    typeof window === "undefined" ? null : window,
): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;

  const write = (): void => {
    timer = null;
    const snapshot = snapshotLayout(store.getState());
    try {
      storage.setItem(
        LAYOUT_STORAGE_KEY,
        JSON.stringify({ version: SNAPSHOT_VERSION, ...snapshot }),
      );
    } catch {
      // Quota/denied storage — persistence silently degrades to in-session.
    }
  };
  const flush = (): void => {
    if (timer !== null) {
      clearTimeout(timer);
      write();
    }
  };
  const schedule = (): void => {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(write, WRITE_DEBOUNCE_MS);
  };

  const unsubscribe = store.subscribe((state, previous) => {
    if (
      state.placements !== previous.placements ||
      state.open !== previous.open ||
      state.dockSizes !== previous.dockSizes ||
      state.maximized !== previous.maximized ||
      state.editorView !== previous.editorView
    ) {
      schedule();
    }
  });
  target?.addEventListener("pagehide", flush);

  return () => {
    unsubscribe();
    target?.removeEventListener("pagehide", flush);
    flush();
  };
}

// ── Lenient field readers ────────────────────────────────────────────

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readPlacements(value: unknown): Record<string, Placement> | null {
  if (!isRecord(value)) return null;
  const out: Record<string, Placement> = {};
  for (const [id, p] of Object.entries(value)) {
    if (!isRecord(p)) return null;
    const { dock, section } = p;
    if (dock !== "left" && dock !== "right" && dock !== "bottom") return null;
    if (section !== "start" && section !== "end") return null;
    out[id] = { dock, section };
  }
  return out;
}

function readOpen(value: unknown): Record<DockSectionId, string | null> | null {
  if (!isRecord(value)) return null;
  const out = {} as Record<DockSectionId, string | null>;
  for (const key of DOCK_SECTION_IDS) {
    const occupant = value[key];
    if (occupant !== null && typeof occupant !== "string" && occupant !== undefined) {
      return null;
    }
    out[key] = typeof occupant === "string" ? occupant : null;
  }
  return out;
}

function readDockSizes(value: unknown): LayoutSnapshot["dockSizes"] | null {
  if (!isRecord(value)) return null;
  const sizes = { left: 0, right: 0, bottom: 0 };
  for (const dock of ["left", "right", "bottom"] as const) {
    const px = value[dock];
    if (typeof px !== "number" || !Number.isFinite(px) || px <= 0) return null;
    sizes[dock] = Math.round(px);
  }
  return sizes;
}
