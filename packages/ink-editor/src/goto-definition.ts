import { type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location } from "@brink/wasm-types";
import { findReferencesAt } from "./references.js";

export interface GotoDefinitionOptions {
  gotoDefinition: (source: string, offset: number) => Location | null;
  /** Called when the definition is in a different file. */
  onNavigateToFile?: (location: Location) => void;
  /** Returns the current active file path. */
  getActiveFile?: () => string;
  /** When provided, cmd-clicking the *definition itself* runs Find
   *  References instead of a no-op self-navigation (ruled 2026-08-24:
   *  "you're already there"). Same callbacks the Shift-Alt-F surface
   *  uses; without them the click keeps plain navigation. */
  findReferences?: (source: string, offset: number) => Location[];
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
 * the click still selects the declaration. Returns false when nothing
 * resolves (the click stays an ordinary click).
 */
export function gotoOrReferencesAt(
  view: EditorView,
  pos: number,
  options: GotoDefinitionOptions,
): boolean {
  const source = view.state.doc.toString();

  let location: Location | null;
  try {
    location = options.gotoDefinition(source, pos);
  } catch {
    return false;
  }

  if (!location) return false;

  const activeFile = options.getActiveFile?.();
  const sameFile = activeFile === undefined || location.file === activeFile;
  const atDefinition = sameFile && pos >= location.start && pos <= location.end;
  if (atDefinition && options.findReferences) {
    const { findReferences, onShowReferences, gotoDefinition } = options;
    if (findReferencesAt(view, pos, { findReferences, onShowReferences, gotoDefinition })) {
      return true;
    }
  }

  navigateToLocation(view, location, options);
  return true;
}

export function gotoDefinitionExtension(options: GotoDefinitionOptions): Extension {
  return EditorView.domEventHandlers({
    click(event: MouseEvent, view: EditorView) {
      if (!(event.ctrlKey || event.metaKey)) return false;

      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;

      if (!gotoOrReferencesAt(view, pos, options)) return false;
      event.preventDefault();
      return true;
    },
  });
}
