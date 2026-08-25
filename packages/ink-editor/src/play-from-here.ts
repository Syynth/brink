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
import type { Location } from "@brink/wasm-types";
import { foldEffect, foldable, foldedRanges, unfoldEffect } from "@codemirror/language";
import { navigateToLocation } from "./goto-definition.js";
import { findReferencesAt } from "./references.js";
import { startInlineRename } from "./rename.js";
import { elementTypeField, ElementType } from "./element-type.js";

export interface PlayFromHereOptions {
  /** Start a session at `inkPath` (`knot` or `knot.stitch`) — the gutter ▶. */
  onPlayFrom: (inkPath: string, label?: string) => void;
  /** Right-click a knot/stitch declaration: open the shared symbol context
   *  menu (play-from-here + structural refactors) at the pointer. The host
   *  fills in the file path. */
  onSymbolContextMenu?: (info: { knot: string; stitch?: string }, x: number, y: number) => void;
  /** Right-click anywhere else: the editor-owned text menu request (position
   *  + Cut/Copy/Paste/Select All bound to this view). When provided, the
   *  native context menu never appears inside the editor. */
  onTextContextMenu?: (request: TextMenuRequest) => void;
  /** Identity resolution for the menu's Navigate/Rename group — the same
   *  callbacks the cmd-click / Shift-Alt-F / F2 surfaces use. */
  gotoDefinition?: (source: string, offset: number) => Location | null | Promise<Location | null>;
  findReferences?: (source: string, offset: number) => Location[] | Promise<Location[]>;
  /** Host references surface (the Search panel) — see references.ts. */
  onShowReferences?: (
    symbol: string,
    locations: Location[],
    declaration?: Location | null,
  ) => void;
  getActiveFile?: () => string;
  onNavigateToFile?: (location: Location) => void;
  /** Whether the inline-rename surface is mounted (gates the Rename item). */
  renameEnabled?: boolean;
  /** Per-token rename gate — the same query F2 uses. A token goto-definition
   *  resolves but prepareRename refuses (externals: the host-binding
   *  contract) gets Navigate items but NO dead Rename item. */
  prepareRename?: (source: string, offset: number) => Location | null | Promise<Location | null>;
}

// ── Path computation ────────────────────────────────────────────────

/** The declared name from a header line: `=== name ===` / `= name(params)` → `name`. */
export function headerName(text: string): string | null {
  const stripped = text.trim().replace(/^=+/, "").replace(/=+$/, "").trim();
  // `=== function name(params) ===` — the keyword is not part of the path
  // (this bug ate every right-click on a function header, #3054 review).
  const sansKeyword = stripped.replace(/^function\s+/, "");
  const name = sansKeyword.split("(")[0]?.trim() ?? "";
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
    // An SVG triangle, not the ▶ text glyph — the glyph's size and
    // baseline vary by font and always sat off-center in the slot.
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 12 12");
    svg.setAttribute("width", "9");
    svg.setAttribute("height", "9");
    svg.setAttribute("aria-hidden", "true");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", "M3.2 1.8 L10 6 L3.2 10.2 Z");
    path.setAttribute("fill", "currentColor");
    path.setAttribute("stroke", "currentColor");
    path.setAttribute("stroke-width", "1");
    path.setAttribute("stroke-linejoin", "round");
    svg.appendChild(path);
    btn.appendChild(svg);
    return btn;
  }
}
const playMarker = new PlayMarker();

// ── Right-click target ──────────────────────────────────────────────

/** The knot/stitch declared on `lineNo`, split into parts, or `null`. */
function symbolAtLine(
  state: EditorState,
  lineNo: number,
): { knot: string; stitch?: string } | null {
  const path = inkPathForLine(state, lineNo);
  if (!path) return null;
  const dot = path.indexOf(".");
  return dot >= 0 ? { knot: path.slice(0, dot), stitch: path.slice(dot + 1) } : { knot: path };
}

// ── Extension ───────────────────────────────────────────────────────

/** The editor-owned half of the plain-text context menu: position, state,
 *  and the actions bound to the right view. The studio renders the menu and
 *  invokes these; clipboard access degrades gracefully where denied. */
export interface TextMenuRequest {
  x: number;
  y: number;
  hasSelection: boolean;
  cut: () => void;
  copy: () => void;
  paste: () => void;
  selectAll: () => void;
  /** The clicked line's element kind (`todo`, `include`, …) — lets the
   *  host contribute studio-side items (e.g. "Show in TODOs Panel"). */
  lineType?: string;
  /** Editor-side line-context items (context-menu spec, structural rows):
   *  Open File on INCLUDEs, Fold/Unfold on foldable regions. */
  lineActions?: LineMenuAction[];
  /** Present when the click landed on an identity-bearing token (a divert
   *  target, VAR/CONST/list/label/param/EXTERNAL reference or declaration —
   *  the test is "goto-definition resolves here"). Actions are bound to the
   *  raising view; the menu's Navigate/Rename group renders from this. */
  identity?: IdentityMenuSection;
}

/** One line-context menu item, bound to the raising view. */
export interface LineMenuAction {
  label: string;
  run: () => void;
}

/** The identity group of the editor context menu (context-menu spec: the
 *  Navigate · Rename rows every identity token shares). */
export interface IdentityMenuSection {
  /** The token's word at the click, for display ("Rename 'gold'…"). */
  name: string;
  gotoDefinition: () => void;
  /** Absent when the host wired no findReferences. */
  findReferences?: () => void;
  /** Absent when the inline-rename surface isn't mounted. */
  rename?: () => void;
}

function selectionText(view: EditorView): string {
  return view.state.sliceDoc(
    view.state.selection.main.from,
    view.state.selection.main.to,
  );
}

/** The identity group for the token at `pos`, or undefined off-identity.
 *  "Goto-definition resolves here" is the identity test: exactly the tokens
 *  with definitions (references and declarations alike) get the group. */
async function identitySectionAt(
  view: EditorView,
  pos: number,
  options: PlayFromHereOptions,
): Promise<IdentityMenuSection | undefined> {
  if (!options.gotoDefinition) return undefined;
  const source = view.state.doc.toString();
  let location: Location | null;
  try {
    location = await options.gotoDefinition(source, pos);
  } catch {
    return undefined;
  }
  if (!location) return undefined;
  const word = view.state.wordAt(pos);
  const name = word ? view.state.sliceDoc(word.from, word.to) : "";
  const loc = location;
  const { findReferences, onShowReferences, gotoDefinition } = options;
  return {
    name,
    gotoDefinition: () => navigateToLocation(view, loc, options),
    findReferences: findReferences
      ? () => {
          void findReferencesAt(view, pos, { findReferences, onShowReferences, gotoDefinition });
        }
      : undefined,
    rename:
      options.renameEnabled && (await renameableAt(view, pos, options))
        ? () => {
            void startInlineRename(view, pos);
          }
        : undefined,
  };
}

/** Whether prepareRename accepts this offset — mirrors the F2 gate, so the
 *  menu never offers a Rename that would silently no-op (externals). */
async function renameableAt(
  view: EditorView,
  pos: number,
  options: PlayFromHereOptions,
): Promise<boolean> {
  if (!options.prepareRename) return true;
  try {
    return (await options.prepareRename(view.state.doc.toString(), pos)) !== null;
  } catch {
    return false;
  }
}

/** Line-context items for the line at `pos` (context-menu spec, structural
 *  rows): INCLUDE lines open their file; foldable regions fold/unfold.
 *  Exported for tests (jsdom has no layout, so the pointer path can't be
 *  driven there). */
export function lineActionsAt(
  view: EditorView,
  pos: number,
  options: PlayFromHereOptions,
): LineMenuAction[] {
  const actions: LineMenuAction[] = [];
  const line = view.state.doc.lineAt(pos);

  const info = view.state.field(elementTypeField, false)?.[line.number - 1];
  if (info?.type === ElementType.Include && options.onNavigateToFile) {
    const m = /^\s*INCLUDE\s+(.+?)\s*$/.exec(line.text);
    const target = m?.[1];
    if (target !== undefined && target !== "") {
      const { onNavigateToFile } = options;
      actions.push({
        label: `Open ${target.split("/").pop() ?? target}`,
        run: () => onNavigateToFile({ file: target, start: 0, end: 0 }),
      });
    }
  }

  // Fold/Unfold — CM's registered fold service decides what's foldable; a
  // fold already anchored in this line offers Unfold instead.
  let folded: { from: number; to: number } | null = null;
  foldedRanges(view.state).between(line.from, line.to, (from, to) => {
    folded = { from, to };
    return false;
  });
  if (folded !== null) {
    const range: { from: number; to: number } = folded;
    actions.push({
      label: "Unfold",
      run: () => view.dispatch({ effects: unfoldEffect.of(range) }),
    });
  } else {
    const range = foldable(view.state, line.from, line.to);
    if (range) {
      actions.push({
        label: "Fold",
        run: () => view.dispatch({ effects: foldEffect.of(range) }),
      });
    }
  }
  return actions;
}

async function buildTextMenuRequest(
  view: EditorView,
  x: number,
  y: number,
  options: PlayFromHereOptions,
): Promise<TextMenuRequest> {
  const hasSelection = !view.state.selection.main.empty;
  const pos = view.posAtCoords({ x, y });
  const lineInfo =
    pos == null
      ? undefined
      : view.state.field(elementTypeField, false)?.[view.state.doc.lineAt(pos).number - 1];
  return {
    identity: pos == null ? undefined : await identitySectionAt(view, pos, options),
    lineType: lineInfo?.type,
    lineActions: pos == null ? undefined : lineActionsAt(view, pos, options),
    x,
    y,
    hasSelection,
    copy: () => {
      void navigator.clipboard?.writeText(selectionText(view)).catch(() => {});
    },
    cut: () => {
      const text = selectionText(view);
      void navigator.clipboard?.writeText(text).catch(() => {});
      view.dispatch(view.state.replaceSelection(""));
      view.focus();
    },
    paste: () => {
      void navigator.clipboard
        ?.readText()
        .then((text) => {
          view.dispatch(view.state.replaceSelection(text));
          view.focus();
        })
        .catch(() => {});
    },
    selectAll: () => {
      view.dispatch({
        selection: { anchor: 0, head: view.state.doc.length },
      });
      view.focus();
    },
  };
}

export function playFromHereExtension(options: PlayFromHereOptions): Extension {
  const { onPlayFrom, onSymbolContextMenu } = options;

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
      // The GLOBAL editor context-menu entry point (the ruled architecture,
      // docs/editor-context-menu-spec.md): the native menu never appears
      // inside the editor — resolve the click to the richest context we
      // have and dispatch. Headers get the shared symbol menu; everything
      // else gets the text menu (Cut/Copy/Paste/Select All re-provided as
      // ours). Provider-per-token-kind grows from here, matrix row by row.
      const { onTextContextMenu } = options;
      if (!onSymbolContextMenu && !onTextContextMenu) return false;
      event.preventDefault();
      const lineNo = lineAtPointer(view, event.clientX, event.clientY);
      const info =
        lineNo == null || !onSymbolContextMenu ? null : symbolAtLine(view.state, lineNo);
      if (info && onSymbolContextMenu) {
        onSymbolContextMenu(info, event.clientX, event.clientY);
        return true;
      }
      if (onTextContextMenu) {
        // Identity/rename gating resolves on the worker road (#3110): the
        // menu opens one landing later — imperceptible next to the click.
        void buildTextMenuRequest(view, event.clientX, event.clientY, options).then(
          (request) => {
            if (view.dom.isConnected) onTextContextMenu(request);
          },
        );
      }
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
    width: "16px",
    // First-row height (see the fold marker) — headers can soft-wrap.
    height: "1lh",
    borderRadius: "3px",
    color: "var(--bs-success, #22c55e)",
    cursor: "pointer",
    opacity: "0.9",
  },
  ".brink-play-gutter-icon:hover": {
    opacity: "1",
    backgroundColor: "rgb(var(--bs-success-rgb, 34 197 94) / 18%)",
  },
  ".brink-play-gutter-icon:focus-visible": {
    outline: "1px solid var(--bs-accent, #3b82f6)",
    outlineOffset: "-1px",
  },
});
