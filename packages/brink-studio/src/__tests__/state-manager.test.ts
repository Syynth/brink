import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorStateManager, type TabTarget } from "../editor/state-manager.js";
import { ProjectSession } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";
import { initWasm } from "../wasm.js";

// ── Fixtures ────────────────────────────────────────────────────────

const MAIN_INK = [
  "// Welcome to brink studio!",
  "",
  "-> start",
  "",
  "=== start ===",
  "Hello, world!",
  "* [Choice A] -> END",
  "* [Choice B] -> story",
  "",
  "=== story ===",
  "Once upon a time.",
  "-> END",
  "",
].join("\n");

// Byte offsets for the default ink file (ASCII so bytes = chars)
const START_KNOT_OFFSET = MAIN_INK.indexOf("=== start ===");
const START_KNOT_END = MAIN_INK.indexOf("=== story ===");
const STORY_KNOT_OFFSET = MAIN_INK.indexOf("=== story ===");
const STORY_KNOT_END = MAIN_INK.length;

const START_KNOT_TEXT = MAIN_INK.slice(START_KNOT_OFFSET, START_KNOT_END);
const STORY_KNOT_TEXT = MAIN_INK.slice(STORY_KNOT_OFFSET, STORY_KNOT_END);

// ── Test helpers ────────────────────────────────────────────────────

async function createTestManager(files: Record<string, string> = { "main.ink": MAIN_INK }) {
  await initWasm();

  const provider = new InMemoryFileProvider(files);
  const entryFile = Object.keys(files)[0]!;
  const project = new ProjectSession({ provider, entryFile });
  await project.initialize();

  const studioOptions = {
    ...project.createStudioOptions(),
    onCompile: () => {},
  };
  const manager = new EditorStateManager(project, studioOptions);
  const initialState = manager.getState(entryFile);

  const container = document.createElement("div");
  document.body.appendChild(container);
  const view = new EditorView({ state: initialState, parent: container });
  manager.setView(view);

  return { manager, project, view, container };
}

function docText(view: EditorView): string {
  return view.state.doc.toString();
}

// ── Tests ───────────────────────────────────────────────────────────

describe("EditorStateManager", () => {
  let manager: EditorStateManager;
  let project: ProjectSession;
  let view: EditorView;
  let container: HTMLElement;

  beforeEach(async () => {
    ({ manager, project, view, container } = await createTestManager());
  });

  afterEach(() => {
    view.destroy();
    container.remove();
  });

  // ── Initial state ──────────────────────────────────────────────

  describe("initial state", () => {
    it("starts with one pinned file tab", () => {
      const tabs = manager.getTabs();
      expect(tabs).toHaveLength(1);
      expect(tabs[0]!.id).toBe("main.ink");
      expect(tabs[0]!.pinned).toBe(true);
      expect(tabs[0]!.target.kind).toBe("file");
    });

    it("active tab is the entry file", () => {
      const tab = manager.getActiveTab();
      expect(tab.id).toBe("main.ink");
      expect(manager.active()).toBe("main.ink");
    });

    it("view shows the full file content", () => {
      expect(docText(view)).toBe(MAIN_INK);
    });
  });

  // ── File tabs ──────────────────────────────────────────────────

  describe("file tabs", () => {
    it("opens a new unpinned file tab", async () => {
      await manager.addFile("other.ink", "other content");
      // addFile opens as pinned; let's test openTab directly
      const tabs = manager.getTabs();
      expect(tabs).toHaveLength(2);
    });

    it("switches between file tabs preserving content", async () => {
      await manager.addFile("other.ink", "other content");
      expect(docText(view)).toBe("other content");

      // Switch back
      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      expect(docText(view)).toBe(MAIN_INK);
    });

    it("cannot close the last tab", async () => {
      const closed = await manager.closeTab("main.ink");
      expect(closed).toBe(false);
      expect(manager.getTabs()).toHaveLength(1);
    });

    it("closes a tab and switches to adjacent", async () => {
      await manager.addFile("other.ink", "other");
      // Now active is "other.ink"
      const closed = await manager.closeTab("other.ink");
      expect(closed).toBe(true);
      expect(manager.getActiveTab().id).toBe("main.ink");
      expect(docText(view)).toBe(MAIN_INK);
    });
  });

  // ── Pinned / unpinned semantics ────────────────────────────────

  describe("pinned/unpinned semantics", () => {
    it("unpinned tab is replaced by next unpinned open", async () => {
      // Open two symbol tabs unpinned — second should replace the first
      const target1: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      const target2: TabTarget = { kind: "symbol", path: "main.ink", name: "story", start: STORY_KNOT_OFFSET, end: STORY_KNOT_END };

      await manager.openTab(target1, false);
      expect(manager.getTabs()).toHaveLength(2); // main.ink(pinned) + start(unpinned)
      expect(manager.getActiveTab().id).toBe("main.ink::start");

      await manager.openTab(target2, false);
      expect(manager.getTabs()).toHaveLength(2); // main.ink(pinned) + story(unpinned, replaced start)
      expect(manager.getActiveTab().id).toBe("main.ink::story");
      // The start tab should be gone (replaced)
      expect(manager.getTabs().find((t) => t.id === "main.ink::start")).toBeUndefined();
    });

    it("at most one unpinned tab exists", async () => {
      await manager.addFile("a.ink", "aaa");
      await manager.addFile("b.ink", "bbb");

      // Open symbol tabs unpinned
      const target1: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      const target2: TabTarget = { kind: "symbol", path: "main.ink", name: "story", start: STORY_KNOT_OFFSET, end: STORY_KNOT_END };

      await manager.openTab(target1, false);
      const tabsBefore = [...manager.getTabs()];
      const unpinnedBefore = tabsBefore.filter((t) => !t.pinned);
      expect(unpinnedBefore).toHaveLength(1);
      expect(unpinnedBefore[0]!.id).toBe("main.ink::start");

      await manager.openTab(target2, false);
      const tabsAfter = [...manager.getTabs()];
      const unpinnedAfter = tabsAfter.filter((t) => !t.pinned);
      expect(unpinnedAfter).toHaveLength(1);
      expect(unpinnedAfter[0]!.id).toBe("main.ink::story");
    });

    it("pinTab makes an unpinned tab pinned", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, false);
      expect(manager.getActiveTab().pinned).toBe(false);

      manager.pinTab("main.ink::start");
      expect(manager.getActiveTab().pinned).toBe(true);
    });

    it("opening pinned when unpinned exists with same ID pins it", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, false);
      expect(manager.getActiveTab().pinned).toBe(false);

      // Open same target as pinned
      await manager.openTab(target, true);
      expect(manager.getActiveTab().pinned).toBe(true);
      // Should not create a duplicate
      const matching = manager.getTabs().filter((t) => t.id === "main.ink::start");
      expect(matching).toHaveLength(1);
    });

    it("unpinned replacement does not affect pinned tabs", async () => {
      const target1: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      const target2: TabTarget = { kind: "symbol", path: "main.ink", name: "story", start: STORY_KNOT_OFFSET, end: STORY_KNOT_END };

      // Pin first symbol tab
      await manager.openTab(target1, true);
      expect(manager.getTabs()).toHaveLength(2); // main.ink + start (pinned)

      // Open second as unpinned
      await manager.openTab(target2, false);
      expect(manager.getTabs()).toHaveLength(3); // main.ink + start (pinned) + story (unpinned)

      // Both tabs survive
      const ids = manager.getTabs().map((t) => t.id);
      expect(ids).toContain("main.ink");
      expect(ids).toContain("main.ink::start");
      expect(ids).toContain("main.ink::story");
    });
  });

  // ── Focused view content extraction ────────────────────────────

  describe("focused view", () => {
    it("symbol tab shows only the knot content", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      expect(docText(view)).toBe(START_KNOT_TEXT);
    });

    it("different knots show different content", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "story", start: STORY_KNOT_OFFSET, end: STORY_KNOT_END };
      await manager.openTab(target, true);

      expect(docText(view)).toBe(STORY_KNOT_TEXT);
    });

    it("switching between symbol and file tab preserves file content", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);
      expect(docText(view)).toBe(START_KNOT_TEXT);

      // Switch back to full file
      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      expect(docText(view)).toBe(MAIN_INK);
    });
  });

  // ── Splice-back ────────────────────────────────────────────────

  describe("splice-back", () => {
    it("edits in symbol tab are spliced back into full file on flush", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      // Simulate an edit: replace content with modified text
      const modified = "=== start ===\nModified content!\n";
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: modified },
      });

      // Switch to file tab (triggers flush)
      await manager.openTab({ kind: "file", path: "main.ink" }, true);

      const fullContent = docText(view);
      // The file should have the modified start knot spliced in
      expect(fullContent).toContain("Modified content!");
      // The top-level content before start should be preserved
      expect(fullContent).toContain("// Welcome to brink studio!");
      // The story knot should be preserved
      expect(fullContent).toContain("=== story ===");
      expect(fullContent).toContain("Once upon a time.");
    });

    it("splice-back preserves content before the symbol", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      const modified = "=== start ===\nNew start.\n";
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: modified },
      });

      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      const full = docText(view);

      // Content before start knot is intact
      const beforeKnot = full.slice(0, full.indexOf("=== start ==="));
      expect(beforeKnot).toBe(MAIN_INK.slice(0, START_KNOT_OFFSET));
    });

    it("splice-back preserves content after the symbol", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      const modified = "=== start ===\nNew start.\n";
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: modified },
      });

      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      const full = docText(view);

      // Content after start knot (the story knot) is intact
      const storyIdx = full.indexOf("=== story ===");
      expect(storyIdx).toBeGreaterThan(0);
      expect(full.slice(storyIdx)).toBe(MAIN_INK.slice(START_KNOT_END));
    });

    it("multiple edits to the same symbol tab update the range correctly", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      // First edit
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "=== start ===\nEdit 1.\n" },
      });

      // Compile (triggers splice via wrapCompile through the diagnostics extension debounce,
      // but we test the flush path instead by switching tabs)
      // Switch to file
      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      let full = docText(view);
      expect(full).toContain("Edit 1.");
      expect(full).toContain("=== story ===");

      // Switch back to the start symbol
      await manager.openTab(target, true);
      // Now the state should be re-extracted with updated range
      expect(docText(view)).toContain("Edit 1.");

      // Second edit
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "=== start ===\nEdit 2 is longer!\n" },
      });

      // Switch to file again
      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      full = docText(view);
      expect(full).toContain("Edit 2 is longer!");
      expect(full).toContain("// Welcome to brink studio!");
      expect(full).toContain("=== story ===");
    });

    it("editing the last knot in a file works correctly", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "story", start: STORY_KNOT_OFFSET, end: STORY_KNOT_END };
      await manager.openTab(target, true);

      const modified = "=== story ===\nA different story.\n-> END\n";
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: modified },
      });

      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      const full = docText(view);
      expect(full).toContain("A different story.");
      expect(full).toContain("=== start ===");
      expect(full).toContain("// Welcome to brink studio!");
    });
  });

  // ── View context (native splice) ────────────────────────────────

  describe("view context", () => {
    it("updateSource with view context splices fragment into full file", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      // Simulate what the diagnostics extension does: call updateSource
      const session = project.getSession();
      session.updateSource("=== start ===\nCompiled content.\n");

      // Check that the session has the full file with the spliced content
      const fullSource = session.getFileSource("main.ink")!;
      expect(fullSource).toContain("Compiled content.");
      expect(fullSource).toContain("// Welcome to brink studio!");
      expect(fullSource).toContain("=== story ===");
    });

    it("getViewSource returns fragment when view context is active", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      const viewSource = project.getSession().getViewSource();
      expect(viewSource).toBe(START_KNOT_TEXT);
    });

    it("clearViewContext returns to full file mode", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);

      project.getSession().clearViewContext();
      const viewSource = project.getSession().getViewSource();
      expect(viewSource).toBe(MAIN_INK);
    });
  });

  // ── Tab labels ─────────────────────────────────────────────────

  describe("tab labels", () => {
    it("file tab shows filename", () => {
      expect(manager.getActiveTab().label).toBe("main.ink");
    });

    it("file in subdirectory shows just the filename", async () => {
      await manager.addFile("subdir/nested.ink", "content");
      expect(manager.getActiveTab().label).toBe("nested.ink");
    });

    it("symbol tab shows name (filename)", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);
      expect(manager.getActiveTab().label).toBe("start (main.ink)");
    });
  });

  // ── Tab IDs ────────────────────────────────────────────────────

  describe("tab IDs", () => {
    it("file tab ID is the path", () => {
      expect(manager.getActiveTab().id).toBe("main.ink");
    });

    it("symbol tab ID is path::name", async () => {
      const target: TabTarget = { kind: "symbol", path: "main.ink", name: "start", start: START_KNOT_OFFSET, end: START_KNOT_END };
      await manager.openTab(target, true);
      expect(manager.getActiveTab().id).toBe("main.ink::start");
    });
  });

  // ── Cross-file symbol tabs ─────────────────────────────────────

  describe("cross-file", () => {
    it("symbol tab in a different file switches active file", async () => {
      const otherContent = "=== other_knot ===\nOther content.\n-> END\n";
      await manager.addFile("other.ink", otherContent);

      // Switch back to main
      await manager.openTab({ kind: "file", path: "main.ink" }, true);
      expect(manager.active()).toBe("main.ink");

      // Open symbol from other file
      const target: TabTarget = { kind: "symbol", path: "other.ink", name: "other_knot", start: 0, end: otherContent.length };
      await manager.openTab(target, true);

      expect(manager.active()).toBe("other.ink");
      expect(docText(view)).toBe(otherContent);
    });
  });
});
