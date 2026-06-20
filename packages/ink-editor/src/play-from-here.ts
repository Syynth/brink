/**
 * "Play from here" (#186) — a hover-revealed gutter ▶ run-icon and a right-click
 * menu on knot/stitch declarations, like an IDE test-runner gutter. Clicking
 * starts a fresh play session entered at that knot/stitch.
 *
 * This is a thin editor affordance over an existing capability: the studio wires
 * `onPlayFrom(inkPath)` to `store.openSession({ path })`, which enters the path
 * via the runtime's `choose_path_string`. The path passed out is the qualified
 * ink name (`knot` or `knot.stitch`) — the same dotted path ink uses — computed
 * from the header lines, not a UI key.
 */

import { StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { EditorView, GutterMarker, ViewPlugin, gutter } from "@codemirror/view";
import { elementTypeField, ElementType } from "./element-type.js";
import { openPopover } from "./widget-popover.js";

export interface PlayFromHereOptions {
  /** Start a session at `inkPath` (`knot` or `knot.stitch`). */
  onPlayFrom: (inkPath: string, label?: string) => void;
}

// ── Path computation ────────────────────────────────────────────────

/** The declared name from a header line: `=== name ===` / `= name(params)` → `name`. */
export function headerName(text: string): string | null {
  const stripped = text.trim().replace(/^=+/, "").replace(/=+$/, "").trim();
  const name = stripped.split("(")[0]?.trim() ?? "";
  return name || null;
}

/**
 * Pure core of the path computation, testable without an `EditorState`. The
 * qualified ink path for the knot/stitch declared on `lineNo` (1-based) over the
 * parallel `texts`/`types` arrays (0-based), or `null` if that line isn't a
 * header. A stitch resolves to `knot.stitch` by walking back to its enclosing
 * knot header.
 */
export function qualifiedInkPath(
  texts: readonly string[],
  types: readonly (ElementType | undefined)[],
  lineNo: number,
): string | null {
  const type = types[lineNo - 1];
  const name = headerName(texts[lineNo - 1] ?? "");
  if (!name) return null;

  if (type === ElementType.KnotHeader) return name;
  if (type === ElementType.StitchHeader) {
    for (let i = lineNo - 1; i >= 1; i--) {
      if (types[i - 1] === ElementType.KnotHeader) {
        const knot = headerName(texts[i - 1] ?? "");
        return knot ? `${knot}.${name}` : name;
      }
    }
    return name; // stitch with no enclosing knot — best effort
  }
  return null;
}

/** The qualified ink path for the header on `lineNo` (1-based), or `null`. */
function inkPathForLine(state: EditorState, lineNo: number): string | null {
  const infos = state.field(elementTypeField, false);
  if (!infos) return null;
  const texts: string[] = [];
  for (let i = 1; i <= state.doc.lines; i++) texts.push(state.doc.line(i).text);
  return qualifiedInkPath(
    texts,
    infos.map((info) => info.type),
    lineNo,
  );
}

function isHeaderLine(state: EditorState, lineNo: number): boolean {
  const type = state.field(elementTypeField, false)?.[lineNo - 1]?.type;
  return type === ElementType.KnotHeader || type === ElementType.StitchHeader;
}

// ── Hover tracking ──────────────────────────────────────────────────

/** The 1-based line currently revealing its run-icon, or `null`. */
const setHoverLine = StateEffect.define<number | null>();

const hoverLineField = StateField.define<number | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setHoverLine)) return e.value;
    return value;
  },
});

/** Map a mouse y-coordinate to its 1-based document line, or `null`. */
function lineAtPointer(view: EditorView, clientX: number, clientY: number): number | null {
  // Resolve against the content's left edge so a pointer over the gutter still
  // maps to the row it sits beside.
  const contentLeft = view.contentDOM.getBoundingClientRect().left + 1;
  const pos =
    view.posAtCoords({ x: clientX, y: clientY }) ??
    view.posAtCoords({ x: contentLeft, y: clientY }, false);
  if (pos == null) return null;
  return view.state.doc.lineAt(pos).number;
}

// ── Gutter marker ───────────────────────────────────────────────────

class PlayMarker extends GutterMarker {
  override toDOM(): HTMLElement {
    const btn = document.createElement("button");
    btn.className = "brink-play-gutter-icon";
    btn.title = "Play from here";
    btn.setAttribute("aria-label", "Play from here");
    btn.textContent = "▶";
    return btn;
  }
}
const playMarker = new PlayMarker();

// ── Right-click menu ────────────────────────────────────────────────

/** A one-item DOM context menu anchored at the pointer. */
function openPlayMenu(
  view: EditorView,
  clientX: number,
  clientY: number,
  inkPath: string,
  onPlayFrom: (p: string, label?: string) => void,
): void {
  const anchor = document.createElement("div");
  anchor.style.cssText = `position:fixed;left:${clientX}px;top:${clientY}px;width:0;height:0;`;
  (view.dom.closest<HTMLElement>(".brink-studio") ?? document.body).appendChild(anchor);

  const handle = openPopover(
    anchor,
    (container) => {
      const item = document.createElement("button");
      item.className = "brink-play-menu-item";
      item.textContent = `Play from ${inkPath}`;
      item.addEventListener("click", () => {
        onPlayFrom(inkPath, inkPath);
        handle.close();
      });
      container.appendChild(item);
    },
    () => anchor.remove(),
  );
}

// ── Extension ───────────────────────────────────────────────────────

export function playFromHereExtension(options: PlayFromHereOptions): Extension {
  const { onPlayFrom } = options;

  const playGutter = gutter({
    class: "brink-play-gutter",
    lineMarker(view, line) {
      const hovered = view.state.field(hoverLineField);
      if (hovered == null) return null;
      const lineNo = view.state.doc.lineAt(line.from).number;
      if (lineNo !== hovered || !isHeaderLine(view.state, lineNo)) return null;
      return playMarker;
    },
    lineMarkerChange(update) {
      return (
        update.startState.field(hoverLineField) !== update.state.field(hoverLineField) ||
        update.startState.field(elementTypeField, false) !==
          update.state.field(elementTypeField, false)
      );
    },
    // Reserve the column so text doesn't shift when the icon appears.
    initialSpacer: () => playMarker,
    domEventHandlers: {
      mousedown(view, line, event) {
        const lineNo = view.state.doc.lineAt(line.from).number;
        const path = inkPathForLine(view.state, lineNo);
        if (!path) return false;
        (event as MouseEvent).preventDefault();
        onPlayFrom(path, path);
        return true;
      },
    },
  });

  // Track the hovered header line on the *whole* editor DOM (content + gutters),
  // so moving from the line into the gutter to click the ▶ keeps it revealed —
  // a contentDOM-only `mouseleave` would hide it the instant the pointer crosses
  // into the gutter.
  const hoverTracker = ViewPlugin.fromClass(
    class {
      private readonly onMove = (e: MouseEvent): void => {
        const lineNo = lineAtPointer(this.view, e.clientX, e.clientY);
        const next = lineNo != null && isHeaderLine(this.view.state, lineNo) ? lineNo : null;
        if (this.view.state.field(hoverLineField) !== next) {
          this.view.dispatch({ effects: setHoverLine.of(next) });
        }
      };
      private readonly onLeave = (): void => {
        if (this.view.state.field(hoverLineField) !== null) {
          this.view.dispatch({ effects: setHoverLine.of(null) });
        }
      };
      constructor(readonly view: EditorView) {
        view.dom.addEventListener("mousemove", this.onMove);
        view.dom.addEventListener("mouseleave", this.onLeave);
      }
      destroy(): void {
        this.view.dom.removeEventListener("mousemove", this.onMove);
        this.view.dom.removeEventListener("mouseleave", this.onLeave);
      }
    },
  );

  const contextMenu = EditorView.domEventHandlers({
    contextmenu(event, view) {
      const lineNo = lineAtPointer(view, event.clientX, event.clientY);
      if (lineNo == null) return false;
      const path = inkPathForLine(view.state, lineNo);
      if (!path) return false;
      event.preventDefault();
      openPlayMenu(view, event.clientX, event.clientY, path, onPlayFrom);
      return true;
    },
  });

  return [hoverLineField, playGutter, hoverTracker, contextMenu, playFromHereTheme];
}

const playFromHereTheme = EditorView.baseTheme({
  ".cm-gutter.brink-play-gutter": {
    cursor: "pointer",
  },
  ".brink-play-gutter-icon": {
    all: "unset",
    boxSizing: "border-box",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    // Intrinsic size — `initialSpacer` measures this to reserve the column
    // width, so a percentage width here would collapse the gutter to zero.
    width: "1.2em",
    height: "100%",
    padding: "0 2px",
    fontSize: "0.7em",
    lineHeight: "1",
    color: "var(--bs-accent, #3b82f6)",
    cursor: "pointer",
    opacity: "0.85",
  },
  ".brink-play-gutter-icon:hover": {
    opacity: "1",
  },
  ".brink-play-menu-item": {
    all: "unset",
    display: "block",
    padding: "4px 12px",
    cursor: "pointer",
    whiteSpace: "nowrap",
    fontSize: "0.85em",
    color: "var(--bs-fg, inherit)",
  },
  ".brink-play-menu-item:hover": {
    background: "var(--bs-accent, #3b82f6)",
    color: "#fff",
  },
});
