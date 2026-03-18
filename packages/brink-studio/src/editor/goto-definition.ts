import { type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location } from "../wasm.js";

export interface GotoDefinitionOptions {
  gotoDefinition: (source: string, offset: number) => Location | null;
  onNavigateToFile?: (location: Location) => void;
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

      // Navigate within the file
      view.dispatch({
        selection: { anchor: location.start },
        effects: EditorView.scrollIntoView(location.start, { y: "center" }),
      });

      // Briefly highlight the target range
      event.preventDefault();
      return true;
    },
  });
}
