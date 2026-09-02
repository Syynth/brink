/**
 * Regression test for #426: `computeLineInfos` (`@brink-lang/editor`'s
 * `element-type.ts`) silently dropped dialect classification whenever a wasm
 * document handle was present but `handle.lineContexts()` (wasm's
 * `line_contexts_doc`) returned `[]` — handle absent/not-yet-synced, or (as
 * exercised here) a host mock. That branch filled every line via the bare
 * regex `classifyLine` and never ran `applyDialectFallback`, unlike the
 * no-handle branch a few lines below it, so a mounted editor with a dialect
 * active (the default — `AT_CUE_DIALECT` — per `DocumentSessions`) would
 * render plain narrative lines for what should be character cues and chained
 * dialogue, with no diagnostic.
 *
 * This suite mounts a real editor through `ProjectSession`/`DocumentSessions`
 * against the package's mocked wasm session
 * (`src/__mocks__/brink-web.ts`), whose `line_contexts_doc` always returns
 * `"[]"` — the exact `contexts.length === 0` path the bug lived in.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import {
  ElementType,
  elementTypeField,
  DocumentSessions,
  InMemoryFileProvider,
  ProjectSession,
  AT_CUE_DIALECT,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

// `@Alice:<>` is the at-cue dialect's "character" source form (pinned by
// `tests/dialect_fixtures/at_cue.json`'s `cue-basic` case); the narrative
// line right after it chains to "dialogue", carrying the speaker.
const MAIN = ["=== start ===", "@Alice:<>", "Hello there.", "-> END", ""].join("\n");

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
  // The at-cue preset is opted into explicitly (RULED 2026-08-30: no
  // dialect by default — a host that never overrides now gets NONE),
  // matching a real host that never overrides the dialect.
  const documents = new DocumentSessions(project, {}, [], { dialect: AT_CUE_DIALECT });
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "g1", container);
  documents.setFocused("main.ink", "g1");
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return { project, documents, view, container, dispose };
}

describe("dialect classification survives an empty line_contexts_doc (#426)", () => {
  let h: Harness;

  beforeEach(async () => {
    h = await mount();
  });

  afterEach(() => {
    h.dispose();
    h.container.remove();
  });

  it("still classifies a character cue and its chained dialogue via the TS fallback", () => {
    const infos = h.view.state.field(elementTypeField);
    // 0-based line indices: 0 = "=== start ===", 1 = "@Alice:<>",
    // 2 = "Hello there.", 3 = "-> END".
    const cue = infos[1];
    expect(cue.type).toBe(ElementType.Character);
    expect(cue.dialect).toBeDefined();
    expect(cue.dialect?.kind).toBe("character");
    expect(Object.fromEntries(cue.dialect?.attrs ?? [])).toEqual({ speaker: "Alice" });

    const dialogue = infos[2];
    expect(dialogue.type).toBe(ElementType.Dialogue);
    expect(dialogue.dialect).toBeDefined();
    expect(dialogue.dialect?.kind).toBe("dialogue");
    expect(Object.fromEntries(dialogue.dialect?.attrs ?? [])).toEqual({ speaker: "Alice" });
  });
});
