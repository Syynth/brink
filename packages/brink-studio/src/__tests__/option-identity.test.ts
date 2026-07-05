/**
 * Option identity on choice/body lines (#364). The element-type post-pass
 * assigns every Choice line and its ChoiceBody lines an option path — the full
 * lineage of zero-based option indices through the weave — emitted as
 * `data-option-path` (contract) and `data-option` (convenience innermost
 * index) CM6 line attributes, so hosts can render per-branch rails without
 * re-deriving the weave. Gathers close their level's groups; nested weaves
 * are first-class.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import {
  assignOptionPaths,
  ElementType,
  elementTypeField,
  DocumentSessions,
  InMemoryFileProvider,
  ProjectSession,
  type LineInfo,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

function li(type: ElementType, depth = 0, sticky = false): LineInfo {
  return { type, depth, sticky, standalone: false };
}

function paths(infos: LineInfo[]): (readonly number[] | undefined)[] {
  return infos.map((i) => i.optionPath);
}

describe("assignOptionPaths (pure post-pass)", () => {
  it("numbers consecutive options at the same depth", () => {
    const infos = [
      li(ElementType.Choice, 1),
      li(ElementType.Choice, 1),
      li(ElementType.Choice, 1),
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], [1], [2]]);
  });

  it("gives body lines their owning option's path", () => {
    const infos = [
      li(ElementType.Choice, 1),
      li(ElementType.ChoiceBody, 1),
      li(ElementType.ChoiceBody, 1),
      li(ElementType.Choice, 1),
      li(ElementType.ChoiceBody, 1),
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], [0], [0], [1], [1]]);
  });

  it("tracks nested weaves as full lineage paths", () => {
    const infos = [
      li(ElementType.Choice, 1), // * A            → [0]
      li(ElementType.Choice, 2), // ** A1           → [0, 0]
      li(ElementType.ChoiceBody, 2), //   body       → [0, 0]
      li(ElementType.Choice, 2), // ** A2           → [0, 1]
      li(ElementType.Choice, 3), // *** A2a         → [0, 1, 0]
      li(ElementType.Choice, 1), // * B             → [1]
      li(ElementType.Choice, 2), // ** B1           → [1, 0]
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([
      [0],
      [0, 0],
      [0, 0],
      [0, 1],
      [0, 1, 0],
      [1],
      [1, 0],
    ]);
  });

  it("closes the level's group at a gather — the next option starts a new group", () => {
    const infos = [
      li(ElementType.Choice, 1), // * A     → [0]
      li(ElementType.Choice, 1), // * B     → [1]
      li(ElementType.Gather, 1), // -        (closes depth-1 group)
      li(ElementType.Choice, 1), // * C     → [0]  (new group)
      li(ElementType.ChoiceBody, 1), //      → [0]
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], [1], undefined, [0], [0]]);
  });

  it("an inner gather closes only the inner group; the parent stays open", () => {
    const infos = [
      li(ElementType.Choice, 1), // * A          → [0]
      li(ElementType.Choice, 2), // ** a         → [0, 0]
      li(ElementType.Choice, 2), // ** b         → [0, 1]
      li(ElementType.Gather, 2), // - -           (closes depth-2 group)
      li(ElementType.ChoiceBody, 1), //   still A's body → [0]
      li(ElementType.Choice, 2), // ** c         → [0, 0]  (new inner group)
      li(ElementType.Choice, 1), // * B          → [1]
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], [0, 0], [0, 1], undefined, [0], [0, 0], [1]]);
  });

  it("counts sticky (+) choices in the same groups as (*) choices", () => {
    const infos = [
      li(ElementType.Choice, 1, false), // * A   → [0]
      li(ElementType.Choice, 1, true), //  + B   → [1]
      li(ElementType.Choice, 1, false), // * C   → [2]
      li(ElementType.Choice, 2, true), //  ++ c  → [2, 0]
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], [1], [2], [2, 0]]);
  });

  it("resets the weave at knot and stitch headers", () => {
    const infos = [
      li(ElementType.Choice, 1), // * A       → [0]
      li(ElementType.KnotHeader), //           (reset)
      li(ElementType.Choice, 1), // * X       → [0]
      li(ElementType.Choice, 1), // * Y       → [1]
      li(ElementType.StitchHeader), //         (reset)
      li(ElementType.Choice, 1), // * Z       → [0]
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], undefined, [0], [1], undefined, [0]]);
  });

  it("leaves non-weave lines untouched and tolerates a leading gather", () => {
    const infos = [
      li(ElementType.Gather, 1), // gather with nothing open — no crash
      li(ElementType.NarrativeText),
      li(ElementType.Divert),
      li(ElementType.Choice, 1),
      li(ElementType.Logic),
      li(ElementType.ChoiceBody, 1),
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([
      undefined,
      undefined,
      undefined,
      [0],
      undefined,
      [0],
    ]);
  });

  it("interleaved non-weave lines don't close groups", () => {
    const infos = [
      li(ElementType.Choice, 1), //     * A      → [0]
      li(ElementType.Comment), //       // note
      li(ElementType.Choice, 1), //     * B      → [1]
    ];
    assignOptionPaths(infos);
    expect(paths(infos)).toEqual([[0], undefined, [1]]);
  });
});

// ── End-to-end: classification → line attributes in the DOM ─────────
// Note: vitest runs against the mocked wasm session (src/__mocks__/brink-web.ts),
// whose `line_contexts_doc` returns `[]` — so line classification here goes
// through the editor's regex fallback. That fallback classifies choices and
// gathers (with depth) but not choice-body narrative; ChoiceBody path
// inheritance is proven by the pure unit tests above, and the attribute
// emission below is type-agnostic (driven purely by `optionPath` presence).

const MAIN = [
  "-> start",
  "=== start ===",
  "* Option A",
  "    A body line.",
  "    * * Nested A1",
  "        A1 body.",
  "    * * Nested A2",
  "* Option B",
  "- Gathered.",
  "+ Option C",
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

async function mount(): Promise<Harness> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": MAIN });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const documents = new DocumentSessions(project);
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "g1", container);
  documents.setFocused("main.ink", "g1");
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return { project, documents, view, container, dispose };
}

describe("option identity end-to-end (#364)", () => {
  let h: Harness;

  beforeEach(async () => {
    h = await mount();
  });

  afterEach(() => {
    h.dispose();
    h.container.remove();
  });

  it("assigns option paths through the mounted editor's classification", () => {
    const infos = h.view.state.field(elementTypeField);
    // Lines are 1-based in the doc; infos are 0-based.
    expect(infos[2].optionPath).toEqual([0]); //       * Option A
    expect(infos[4].optionPath).toEqual([0, 0]); //    * * Nested A1
    expect(infos[6].optionPath).toEqual([0, 1]); //    * * Nested A2
    expect(infos[7].optionPath).toEqual([1]); //       * Option B
    expect(infos[8].optionPath).toBeUndefined(); //    - Gathered.
    expect(infos[9].optionPath).toEqual([0]); //       + Option C (new group)
    expect(infos[10].optionPath).toBeUndefined(); //   -> END
  });

  it("emits data-option-path and data-option line attributes in the DOM", () => {
    const byText = (needle: string): HTMLElement => {
      const lines = Array.from(h.view.dom.querySelectorAll<HTMLElement>(".cm-line"));
      const el = lines.find((l) => (l.textContent ?? "").includes(needle));
      if (!el) throw new Error(`no rendered line containing ${JSON.stringify(needle)}`);
      return el;
    };

    const optionA = byText("Option A");
    expect(optionA.getAttribute("data-option-path")).toBe("0");
    expect(optionA.getAttribute("data-option")).toBe("0");

    const nestedA2 = byText("Nested A2");
    expect(nestedA2.getAttribute("data-option-path")).toBe("0.1");
    expect(nestedA2.getAttribute("data-option")).toBe("1");

    const optionB = byText("Option B");
    expect(optionB.getAttribute("data-option-path")).toBe("1");

    // The gather closes the group: no identity on the gather itself, and the
    // sticky option after it starts a new group at 0.
    const gather = byText("Gathered.");
    expect(gather.getAttribute("data-option-path")).toBeNull();
    const optionC = byText("Option C");
    expect(optionC.getAttribute("data-option-path")).toBe("0");
    expect(optionC.getAttribute("data-option")).toBe("0");
  });
});
