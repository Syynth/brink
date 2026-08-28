/**
 * The diagnostic anatomy (#3255).
 *
 * Two producers reach one tooltip surface, and before this they arrived
 * looking like two different products. The design decision is that the
 * SHAPE is shared — same slots, same order, whichever filled them — so
 * these tests assert the produced DOM rather than any styling.
 *
 * The CSS depends on that DOM in a way nothing else checks: the stylesheet
 * puts the message and the source tag on their own rows by making
 * CodeMirror's first and last children full-width, and lets the action
 * buttons flow between them. If `renderMessage` stopped producing
 * `.cm-diag-body`, or the compile path stopped setting `source`, the layout
 * would silently collapse back to one inline run — which is exactly the
 * "fixes are fiddly to click" complaint this replaced.
 */
import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { forEachDiagnostic, type Diagnostic } from "@codemirror/lint";
import type { CompileResult } from "@brink/wasm-types";
import { renderDiagnosticMessage } from "../diagnostic-anatomy.js";
import { diagnosticsExtension } from "../diagnostics.js";

describe("renderDiagnosticMessage", () => {
  it("emits a label and a title as separate elements", () => {
    const el = renderDiagnosticMessage("warning", "`roll` shadows a built-in function.");
    expect(el.className).toBe("cm-diag-body");
    expect(el.querySelector(".cm-diag-label")?.textContent).toBe("warning");
    expect(el.querySelector(".cm-diag-title")?.textContent).toBe(
      "`roll` shadows a built-in function.",
    );
  });

  it("gives the label a per-kind class, so severity is a word AND a colour", () => {
    // The rail colour alone fails a colourblind reader and fails a
    // screenshot pasted into an issue — which is how most of these are
    // reported.
    expect(
      renderDiagnosticMessage("error", "x").querySelector(".cm-diag-label")?.className,
    ).toContain("cm-diag-label-error");
    expect(
      renderDiagnosticMessage("Spelling", "x").querySelector(".cm-diag-label")?.className,
    ).toContain("cm-diag-label-spelling");
  });

  it("lowercases the label so it reads as a category, not a shout", () => {
    expect(
      renderDiagnosticMessage("Spelling", "x").querySelector(".cm-diag-label")?.textContent,
    ).toBe("spelling");
  });

  it("sets the message as text, never as markup", () => {
    // Diagnostic messages carry author content (identifiers, prose). Any
    // path that put them in as HTML would be an injection through a
    // spelling mistake.
    const el = renderDiagnosticMessage("info", "<img src=x onerror=alert(1)>");
    expect(el.querySelector("img")).toBeNull();
    expect(el.querySelector(".cm-diag-title")?.textContent).toContain("<img");
  });
});

// ── The compile producer ───────────────────────────────────────────

function resultWith(warnings: CompileResult["warnings"]): CompileResult {
  return { ok: true, warnings } as CompileResult;
}

describe("compile diagnostics carry the anatomy", () => {
  let view: EditorView | null = null;

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    view?.destroy();
    view = null;
    vi.useRealTimers();
  });

  function mount(result: CompileResult): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: "hello\n",
        extensions: [
          diagnosticsExtension({
            compile: () => result,
            getActiveFile: () => "main.ink",
          }),
        ],
      }),
      parent: document.body,
    });
    return view;
  }

  function first(v: EditorView): Diagnostic | null {
    let found: Diagnostic | null = null;
    forEachDiagnostic(v.state, (d) => {
      found ??= d;
    });
    return found;
  }

  it("passes the diagnostic code through as the source tag", async () => {
    // The code was computed and then dropped on the floor. It is the one
    // thing that lets an author look a diagnostic up, and the anatomy has a
    // slot for it.
    const v = mount(
      resultWith([
        {
          start: 0,
          end: 2,
          message: "name shadows a built-in function: `roll`",
          severity: "Warning",
          code: "E123",
          file: "main.ink",
        },
      ]),
    );
    await vi.advanceTimersByTimeAsync(500);
    expect(first(v)?.source).toBe("E123");
  });

  it("renders through the shared anatomy, labelled by severity", async () => {
    const v = mount(
      resultWith([
        {
          start: 0,
          end: 2,
          message: "boom",
          severity: "Warning",
          code: "E123",
          file: "main.ink",
        },
      ]),
    );
    await vi.advanceTimersByTimeAsync(500);
    const dom = first(v)?.renderMessage?.(v);
    expect(dom?.querySelector(".cm-diag-label")?.textContent).toBe("warning");
    expect(dom?.querySelector(".cm-diag-title")?.textContent).toBe("boom");
  });

  it("gives a whole-compile error the anatomy too", async () => {
    const v = mount({ ok: false, warnings: [], error: "could not parse" } as CompileResult);
    await vi.advanceTimersByTimeAsync(500);
    const dom = first(v)?.renderMessage?.(v);
    expect(dom?.querySelector(".cm-diag-label")?.textContent).toBe("error");
    expect(dom?.querySelector(".cm-diag-title")?.textContent).toBe("could not parse");
  });
});
