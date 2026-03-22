import { type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { CompileResult } from "@brink/wasm-types";

export interface DiagnosticsOptions {
  compile: (source: string) => CompileResult;
  onCompile?: (result: CompileResult) => void;
}

export function diagnosticsExtension(options: DiagnosticsOptions): Extension {
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

  return EditorView.updateListener.of((update) => {
    if (!update.docChanged) return;

    if (compileTimeout !== null) {
      clearTimeout(compileTimeout);
    }
    compileTimeout = setTimeout(() => doCompile(update.view), 500);
  });
}
