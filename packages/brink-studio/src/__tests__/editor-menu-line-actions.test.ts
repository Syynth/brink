/**
 * Line-context menu items (context-menu spec, structural rows): INCLUDE
 * lines offer Open File, foldable lines offer Fold, and the classifier
 * gives the host the line kind for studio-side items (TODO panel).
 */

import { describe, expect, it, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { brinkStudio, classifyLine, lineActionsAt } from "@brink-lang/editor";

const minimal = {
  compile: () => ({ ok: true, diagnostics: [] }) as never,
  getSemanticTokens: () => [],
  getTokenTypeNames: () => [],
};

function mount(doc: string, extra: object = {}): EditorView {
  return new EditorView({
    state: EditorState.create({
      doc,
      extensions: [brinkStudio({ ...minimal, ...extra } as Parameters<typeof brinkStudio>[0])],
    }),
    parent: document.body,
  });
}

describe("editor menu line actions", () => {
  it("INCLUDE line offers Open <file>", () => {
    const nav = vi.fn();
    const view = mount("INCLUDE scenes/intro.ink\nHello\n");
    const actions = lineActionsAt(view, 4, { onNavigateToFile: nav, onPlayFrom: () => {} });
    expect(actions.map((a) => a.label)).toContain("Open intro.ink");
    actions.find((a) => a.label === "Open intro.ink")!.run();
    expect(nav).toHaveBeenCalledWith({ file: "scenes/intro.ink", start: 0, end: 0 });
    view.destroy();
  });

  it("a foldable line offers Fold; a folded one offers Unfold", () => {
    const doc = "=== k ===\nline one\nline two\n";
    const view = mount(doc, {
      getFoldingRanges: () => [
        {
          start_line: 1,
          end_line: 2,
          kind: "structural",
          collapsed_text: null,
          from_line_start: true,
        },
      ],
    });
    const pos = doc.indexOf("line one");
    const actions = lineActionsAt(view, pos, { onPlayFrom: () => {} });
    const fold = actions.find((a) => a.label === "Fold");
    expect(fold, "foldable line must offer Fold").toBeDefined();
    fold!.run();
    const after = lineActionsAt(view, pos, { onPlayFrom: () => {} });
    expect(after.map((a) => a.label)).toContain("Unfold");
    view.destroy();
  });

  it("TODO lines classify as todo (the host keys the panel item off this)", () => {
    expect(classifyLine("TODO: fix this").type).toBe("todo");
  });
});
