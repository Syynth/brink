import { type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { CompileResult } from "@brink/wasm-types";

export interface DiagnosticsOptions {
  compile: (source: string) => CompileResult;
  onCompile?: (result: CompileResult) => void;
  /**
   * Path of the file shown in this editor. A project compile returns
   * diagnostics for every file (entry + INCLUDEs); only those belonging to this
   * file are placed as squiggles here. Project-wide counts come from `onCompile`.
   */
  getActiveFile?: () => string;
}

/**
 * Debounced compile → CM6 lint diagnostics.
 *
 * Implemented as a ViewPlugin so its pending timer is cancelled in `destroy()`:
 * when the editor unmounts (`view.destroy()`) or a tab switch replaces the
 * state (the manager builds each tab's state with a fresh extension set, so the
 * previous plugin is destroyed). This prevents both dispatching into a detached
 * view and splicing a stale tab's source into the now-active file's session.
 */
export function diagnosticsExtension(options: DiagnosticsOptions): Extension {
  return ViewPlugin.fromClass(
    class {
      private timeout: ReturnType<typeof setTimeout> | null = null;

      constructor(private readonly view: EditorView) {
        // Compile the freshly shown document (initial load / tab switch).
        this.schedule();
      }

      update(update: ViewUpdate): void {
        if (update.docChanged) this.schedule();
      }

      destroy(): void {
        this.cancel();
      }

      private schedule(): void {
        this.cancel();
        this.timeout = setTimeout(() => {
          this.timeout = null;
          this.doCompile();
        }, 500);
      }

      private cancel(): void {
        if (this.timeout !== null) {
          clearTimeout(this.timeout);
          this.timeout = null;
        }
      }

      private doCompile(): void {
        // The view may have been detached/replaced between scheduling and now.
        if (!this.view.dom.isConnected) return;

        const source = this.view.state.doc.toString();
        const result = options.compile(source);

        // A project compile reports diagnostics for every file; keep only the
        // ones belonging to the file shown here so an INCLUDEd file's errors
        // don't land on this tab at the wrong offsets.
        const activeFile = options.getActiveFile?.();

        const diags: Diagnostic[] = [];
        if (result.warnings) {
          for (const w of result.warnings) {
            if (activeFile !== undefined && w.file !== activeFile) continue;
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

        this.view.dispatch(setDiagnostics(this.view.state, diags));
        options.onCompile?.(result);
      }
    },
  );
}
