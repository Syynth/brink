import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { defaultKeymap } from "@codemirror/commands";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { brinkStudio, type BrinkStudioOptions } from "./extensions.js";
import type { CompileResult } from "../wasm.js";

export type { BrinkStudioOptions };

export interface BrinkEditorOptions extends BrinkStudioOptions {
  initialContent: string;
}

export interface BrinkEditorHandle {
  setContent(content: string): void;
  getContent(): string;
  triggerCompile(): void;
  destroy(): void;
  readonly view: EditorView;
}

export function createBrinkEditor(
  container: HTMLElement,
  options: BrinkEditorOptions,
): BrinkEditorHandle {
  let compileTimeout: ReturnType<typeof setTimeout> | null = null;

  function doCompile(view: EditorView): void {
    const source = view.state.doc.toString();
    const result = options.compile(source);

    const diags: Diagnostic[] = [];
    if (result.warnings) {
      for (const w of result.warnings) {
        const from = Math.min(w.start, source.length);
        const to = Math.min(w.end, source.length);
        diags.push({
          from,
          to: Math.max(to, from),
          severity: w.severity === "Error" ? "error" : "warning",
          message: w.message,
        });
      }
    }
    if (result.error) {
      diags.push({
        from: 0,
        to: 0,
        severity: "error",
        message: result.error,
      });
    }

    view.dispatch(setDiagnostics(view.state, diags));
    options.onCompile?.(result);
  }

  const state = EditorState.create({
    doc: options.initialContent,
    extensions: [
      brinkStudio(options),
      basicSetup,
      keymap.of(defaultKeymap),
    ],
  });

  const view = new EditorView({ state, parent: container });

  // Initial compile after a short delay
  setTimeout(() => doCompile(view), 100);

  return {
    get view() {
      return view;
    },

    setContent(content: string) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: content },
      });
    },

    getContent(): string {
      return view.state.doc.toString();
    },

    triggerCompile() {
      if (compileTimeout !== null) {
        clearTimeout(compileTimeout);
      }
      doCompile(view);
    },

    destroy() {
      if (compileTimeout !== null) {
        clearTimeout(compileTimeout);
      }
      view.destroy();
    },
  };
}
