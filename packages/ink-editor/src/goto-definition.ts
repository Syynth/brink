import { EditorSelection, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location } from "@brink/wasm-types";
import { findReferencesAt } from "./references.js";
import { includePathAt } from "./element-type.js";

export interface GotoDefinitionOptions {
  /** Sync or async (#3110 — the studio wiring rides the worker road). */
  gotoDefinition: (source: string, offset: number) => Location | null | Promise<Location | null>;
  /** Called when the definition is in a different file. */
  onNavigateToFile?: (location: Location) => void;
  /** Returns the current active file path. */
  getActiveFile?: () => string;
  /** When provided, cmd-clicking the *definition itself* runs Find
   *  References instead of a no-op self-navigation (ruled 2026-08-24:
   *  "you're already there"). Same callbacks the Shift-Alt-F surface
   *  uses; without them the click keeps plain navigation. */
  findReferences?: (source: string, offset: number) => Location[] | Promise<Location[]>;
  onShowReferences?: (
    symbol: string,
    locations: Location[],
    declaration?: Location | null,
  ) => void;
}

/** Navigate to a resolved definition location — cross-file via the host's
 *  onNavigateToFile, same-file via selection + centered scroll. Shared by
 *  the cmd-click handler and the context menu's Go to Definition item. */
export function navigateToLocation(
  view: EditorView,
  location: Location,
  options: Pick<GotoDefinitionOptions, "getActiveFile" | "onNavigateToFile">,
): void {
  const activeFile = options.getActiveFile?.();
  if (activeFile && location.file !== activeFile && options.onNavigateToFile) {
    options.onNavigateToFile(location);
    return;
  }
  view.dispatch({
    selection: { anchor: location.start },
    effects: EditorView.scrollIntoView(location.start, { y: "center" }),
  });
}

/**
 * The cmd-click action at `pos`: navigate to the definition — unless `pos`
 * is *inside* the definition's own span (same file), where navigation
 * would be a no-op; there, run Find References instead (ruled 2026-08-24).
 * Falls back to navigation when references are unavailable or empty, so
 * the click still selects the declaration. When no definition resolves but
 * `pos` sits on the path text of an INCLUDE line, opens that file (ruled
 * 2026-08-24). Returns false when nothing resolves (the click stays an
 * ordinary click).
 */
export async function gotoOrReferencesAt(
  view: EditorView,
  pos: number,
  options: GotoDefinitionOptions,
): Promise<boolean> {
  const doc = view.state.doc;
  const source = doc.toString();

  let location: Location | null;
  try {
    location = await options.gotoDefinition(source, pos);
  } catch {
    return false;
  }
  // Stale landing (#3110): the doc moved (or the view died) while the
  // worker resolved — the offsets no longer address this text.
  if (!view.dom.isConnected || view.state.doc !== doc) return false;

  if (!location) {
    const include = includePathAt(view.state, pos);
    if (include !== null && options.onNavigateToFile) {
      options.onNavigateToFile({ file: include, start: 0, end: 0 });
      return true;
    }
    return false;
  }

  const activeFile = options.getActiveFile?.();
  const sameFile = activeFile === undefined || location.file === activeFile;
  const atDefinition = sameFile && pos >= location.start && pos <= location.end;
  if (atDefinition && options.findReferences) {
    const { findReferences, onShowReferences, gotoDefinition } = options;
    if (await findReferencesAt(view, pos, { findReferences, onShowReferences, gotoDefinition })) {
      return true;
    }
    if (!view.dom.isConnected || view.state.doc !== doc) return false;
  }

  navigateToLocation(view, location, options);
  return true;
}

export function gotoDefinitionExtension(options: GotoDefinitionOptions): Extension {
  return EditorView.domEventHandlers({
    // Handled on MOUSEDOWN, not click: CM6's own cmd/ctrl-mousedown adds a
    // multi-cursor and preventDefaults, which suppresses the browser's
    // `click` event entirely — a click-bound handler never fires from a
    // real pointer. Claiming mousedown (return true) also keeps CM from
    // adding that stray cursor.
    mousedown(event: MouseEvent, view: EditorView) {
      if (event.button !== 0) return false;
      if (!(event.ctrlKey || event.metaKey)) return false;

      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;

      // The resolution rides the worker road (#3110), so the answer is
      // async — but the gesture must be claimed NOW (keeps CM's own
      // cmd-mousedown from adding a stray cursor). When nothing resolves,
      // emulate what CM would have done: add the multi-cursor ourselves.
      event.preventDefault();
      void gotoOrReferencesAt(view, pos, options).then((handled) => {
        if (handled || !view.dom.isConnected) return;
        view.dispatch({
          // SELECT-INVARIANT-EXEMPT GotoDefinition.emulatedMulticursor: CM6
          // EditorSelection (state), not the DOM Selection API — no text
          // input is involved; this re-creates exactly the multi-cursor
          // CM's own cmd-mousedown would have added had the async
          // resolution (#3110) not claimed the gesture.
          selection: view.state.selection.addRange(EditorSelection.cursor(pos)),
        });
      });
      return true;
    },
  });
}
