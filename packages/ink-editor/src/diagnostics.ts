import { StateEffect, type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import type { Diagnostic } from "@codemirror/lint";
import { diagnosticSources, publishDiagnostics } from "./diagnostic-sources.js";
import { renderDiagnosticMessage } from "./diagnostic-anatomy.js";
import type { CompileResult } from "@brink/wasm-types";
import { perfSpan, perfTime } from "./perf/probe.js";

/**
 * Re-publish compile diagnostics without a document change (#3260).
 *
 * The squiggles are published by this extension's own ViewPlugin, which
 * wakes on `docChanged`. A compile that lands for some OTHER reason — a
 * `brink.toml` edit changing `[lints]`, a suppression written into a
 * sibling file — has no document change in THIS view to wake it, so the
 * squiggles stayed as they were until the author typed or reopened the
 * file. Suppressing a diagnostic project-wide and watching it sit there is
 * how that was reported.
 *
 * The prose checker already had exactly this seam (`refreshProseEffect`)
 * for the same reason; this is its compile-side twin.
 */
export const refreshDiagnosticsEffect = StateEffect.define<void>();

export interface DiagnosticsOptions {
  /**
   * Produce a project compile for the current source. May be synchronous
   * or async (W2a of `docs/editor-worker-spec.md` — the studio wiring
   * rides the async session facade); an async result lands only if the
   * document hasn't changed since it was requested (a change reschedules
   * a fresh compile anyway) and the view is still alive. A rejected
   * promise is dropped silently: it means the compile was superseded or
   * the session is tearing down — a newer compile or unmount follows.
   */
  compile: (source: string) => CompileResult | Promise<CompileResult>;
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
  return [
    // Carried WITH the producer, never wired separately — see
    // diagnostic-sources.ts.
    diagnosticSources,
    ViewPlugin.fromClass(
    class {
      private timeout: ReturnType<typeof setTimeout> | null = null;
      /** Bumped on every doc change — an async compile landing only
       *  applies if the doc it compiled is still the doc on screen (a
       *  change rescheduled a fresh compile that will land after it). */
      private docGen = 0;
      private destroyed = false;

      constructor(private readonly view: EditorView) {
        // Compile the freshly shown document (initial load / tab switch).
        this.schedule();
      }

      update(update: ViewUpdate): void {
        if (update.docChanged) {
          this.docGen += 1;
          this.schedule();
          return;
        }
        // A refresh carries no document change, so `docGen` must NOT move:
        // bumping it would invalidate an in-flight compile whose offsets are
        // still correct, and drop its result.
        if (
          update.transactions.some((t) =>
            t.effects.some((e) => e.is(refreshDiagnosticsEffect)),
          )
        ) {
          this.schedule();
        }
      }

      destroy(): void {
        this.destroyed = true;
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

        const endCycle = perfSpan("cm.diagnostics.compileCycle");
        const source = this.view.state.doc.toString();
        const endCompile = perfSpan("cm.diagnostics.compile");
        const produced = options.compile(source);
        if (produced instanceof Promise) {
          const gen = this.docGen;
          void produced.then(
            (result) => {
              endCompile();
              // Landing guards (spec §5.3, whole-project class): dead view,
              // torn-down plugin, or a doc that moved on — a newer compile
              // is already scheduled/landing in each of those cases.
              if (this.destroyed || gen !== this.docGen || !this.view.dom.isConnected) {
                endCycle();
                return;
              }
              this.land(source, result, endCycle);
            },
            () => {
              // Rejection means superseded/cancelled (a newer compile or an
              // unmount follows) or a compile fault the host already
              // surfaced through its own channel — nothing to land here.
              endCompile();
              endCycle();
            },
          );
          return;
        }
        endCompile();
        this.land(source, produced, endCycle);
      }

      private land(source: string, result: CompileResult, endCycle: () => void): void {
        // A project compile reports diagnostics for every file; keep only the
        // ones belonging to the file shown here so an INCLUDEd file's errors
        // don't land on this tab at the wrong offsets.
        const activeFile = options.getActiveFile?.();

        const diags: Diagnostic[] = [];
        if (result.warnings) {
          for (const w of result.warnings) {
            if (activeFile !== undefined && w.file !== activeFile) continue;
            // TODO author notes (#3050): the brink-todo line band is their
            // entire in-editor presentation — a squiggle under the note
            // would double-mark it. They still reach Problems + TODOs.
            if (w.code === "E189") continue;
            const from = Math.min(w.start, source.length);
            const to = Math.min(w.end, source.length);
            const severity =
              w.severity === "Error"
                ? "error"
                : w.severity === "Warning"
                  ? "warning"
                  : "info";
            diags.push({
              from,
              to: Math.max(to, from),
              severity,
              message: w.message,
              // The code was computed and then dropped. It is the one thing
              // that lets an author look a diagnostic up, and the anatomy
              // has a slot for it.
              source: w.code,
              renderMessage: () => renderDiagnosticMessage(severity, w.message),
            });
          }
        }
        if (result.error) {
          diags.push({
            from: 0,
            to: 0,
            severity: "error",
            message: result.error,
            renderMessage: () => renderDiagnosticMessage("error", result.error ?? ""),
          });
        }

        // Published rather than `setDiagnostics`-ed: the prose checker is a
        // second producer into the same set, and `setDiagnostics` REPLACES —
        // whichever landed last would erase the other's squiggles, silently
        // and intermittently. See diagnostic-sources.ts.
        perfTime("cm.diagnostics.setDiagnostics", () =>
          publishDiagnostics(this.view, "compile", diags),
        );
        // The studio's compile fan-out (outline, story graph, player reload,
        // store sweeps) all runs inside this callback — the span separates
        // "compiling" from "reacting to the compile".
        perfTime("cm.diagnostics.onCompile", () => options.onCompile?.(result));
        endCycle();
      }
    },
    ),
  ];
}
