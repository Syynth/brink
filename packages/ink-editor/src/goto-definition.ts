import { type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location } from "@brink/wasm-types";

export interface GotoDefinitionOptions {
  gotoDefinition: (source: string, offset: number) => Location | null;
  /** Called when the definition is in a different file. */
  onNavigateToFile?: (location: Location) => void;
  /** Returns the current active file path. */
  getActiveFile?: () => string;
}

export function gotoDefinitionExtension(options: GotoDefinitionOptions): Extension {
  return EditorView.domEventHandlers({
    click(event: MouseEvent, view: EditorView) {
      if (!(event.ctrlKey || event.metaKey)) return false;

      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;

      const source = view.state.doc.toString();

      let location: Location | null;
      try {
        location = options.gotoDefinition(source, pos);
      } catch {
        return false;
      }

      if (!location) return false;

      const activeFile = options.getActiveFile?.();
      if (activeFile && location.file !== activeFile && options.onNavigateToFile) {
        options.onNavigateToFile(location);
        event.preventDefault();
        return true;
      }

      // Same-file navigation
      view.dispatch({
        selection: { anchor: location.start },
        effects: EditorView.scrollIntoView(location.start, { y: "center" }),
      });

      event.preventDefault();
      return true;
    },
  });
}
