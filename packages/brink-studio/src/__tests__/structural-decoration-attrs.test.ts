/**
 * Line-decoration structural placement is taxonomy, not inline styles (#414).
 *
 * `screenplay.ts`'s line-decoration pass used to stamp two data-driven looks
 * as inline `style` attributes: `padding-left` for weave depth indent (on
 * choices/gathers at depth > 1) and `text-align: right` for standalone
 * diverts. Both beat host stylesheets, so a headless host (`theme: false`)
 * couldn't restyle them — a leak in the #363 headless contract. This test
 * proves the fix: the pass emits `data-depth="N"` / `brink-divert-standalone`
 * instead, and — critically — never emits a `style` attribute on any line,
 * regardless of `theme`.
 */

import { describe, it, expect, afterEach } from "vitest";
import {
  DocumentSessions,
  InMemoryFileProvider,
  ProjectSession,
  type DocumentSessionsOptions,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

const MAIN = [
  "-> start",
  "=== start ===",
  "* Option A",
  "    * * Nested A1",
  "        A1 body.",
  "- Gathered.",
  "-> tunnel ->",
  "-> END",
  "",
].join("\n");

interface Harness {
  project: ProjectSession;
  documents: DocumentSessions;
  view: EditorView;
  container: HTMLElement;
  dispose: () => void;
}

async function mount(options?: DocumentSessionsOptions): Promise<Harness> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": MAIN });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const documents = new DocumentSessions(project, undefined, undefined, options);
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "g1", container);
  documents.setFocused("main.ink", "g1");
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return { project, documents, view, container, dispose };
}

describe("structural line-decoration attrs (#414)", () => {
  let h: Harness;

  afterEach(() => {
    h.dispose();
    h.container.remove();
  });

  const byText = (view: EditorView, needle: string): HTMLElement => {
    const lines = Array.from(view.dom.querySelectorAll<HTMLElement>(".cm-line"));
    const el = lines.find((l) => (l.textContent ?? "").includes(needle));
    if (!el) throw new Error(`no rendered line containing ${JSON.stringify(needle)}`);
    return el;
  };

  /**
   * The #414/#363 contract, refined for the indent guides (ruled
   * 2026-08-23): OUR line-decoration pass never emits a style attribute,
   * but the opted-in `@replit/codemirror-indentation-markers` extension
   * carries its gradient via ONE inline CUSTOM PROPERTY
   * (`--indent-markers: ...`) per indented line, consumed by its own
   * stylesheet. A custom property is a data carrier, not a layout
   * imposition — a host restyles or disables it (`indentGuides: false`)
   * freely. So the audit becomes: a line's inline style, when present,
   * declares only known custom-property carriers — `--indent-markers`
   * (the guides) and `--line-indent` (the wrapped-line hanging indent,
   * same ruling) — any real CSS property (padding, text-align, ...)
   * still fails.
   */
  const CARRIER_PROPS = new Set(["--indent-markers", "--line-indent"]);
  const assertNoImposedInlineStyle = (line: HTMLElement): void => {
    if (!line.hasAttribute("style")) return;
    const style = line.style;
    for (let i = 0; i < style.length; i++) {
      const prop = style.item(i);
      expect(
        CARRIER_PROPS.has(prop),
        `unexpected inline property "${prop}" on: ${line.textContent}`,
      ).toBe(true);
    }
  };

  it("carries data-depth (not inline padding-left) on nested choices/gathers", async () => {
    h = await mount();
    const nested = byText(h.view, "Nested A1");
    expect(nested.getAttribute("data-depth")).toBe("2");
    assertNoImposedInlineStyle(nested);
  });

  it("carries brink-divert-standalone (not inline text-align) on a standalone divert, but not a tunnel call", async () => {
    h = await mount();
    const standalone = byText(h.view, "-> END");
    expect(standalone.className).toContain("brink-divert-standalone");
    assertNoImposedInlineStyle(standalone);

    const tunnel = byText(h.view, "-> tunnel ->");
    expect(tunnel.className).not.toContain("brink-divert-standalone");
    assertNoImposedInlineStyle(tunnel);
  });

  it("emits no imposed inline style on any classified line, headless or themed", async () => {
    h = await mount({ theme: false });
    const lines = Array.from(h.view.dom.querySelectorAll<HTMLElement>(".cm-line"));
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) {
      assertNoImposedInlineStyle(line);
    }
  });

  it("with indentGuides: false, the only inline carrier left is --line-indent (hanging indent)", async () => {
    h = await mount({ theme: false, indentGuides: false });
    const lines = Array.from(h.view.dom.querySelectorAll<HTMLElement>(".cm-line"));
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) {
      if (!line.hasAttribute("style")) continue;
      for (let i = 0; i < line.style.length; i++) {
        expect(line.style.item(i)).toBe("--line-indent");
      }
    }
  });
});
