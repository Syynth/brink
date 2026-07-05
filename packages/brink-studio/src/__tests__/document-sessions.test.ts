/**
 * DocumentSessions unit tests (issue #90): per-(document, group) views over
 * wasm document handles — mount/unmount lifecycle, fragment splice-back,
 * live same-document mirroring (CM6 sync-dispatch), fragment⇄file mirroring
 * via change specs, invalidation, and focused-view tracking.
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts), which
 * implements the document-handle API with in-memory splicing.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
  documentHandleFacet,
  syncAnnotation,
  type DocTarget,
  type DocumentCallbacks,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

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

const START_KNOT_OFFSET = MAIN_INK.indexOf("=== start ===");
const START_KNOT_END = MAIN_INK.indexOf("=== story ===");
const STORY_KNOT_OFFSET = MAIN_INK.indexOf("=== story ===");
const STORY_KNOT_END = MAIN_INK.length;

const START_KNOT_TEXT = MAIN_INK.slice(START_KNOT_OFFSET, START_KNOT_END);
const STORY_KNOT_TEXT = MAIN_INK.slice(STORY_KNOT_OFFSET, STORY_KNOT_END);

const START_TARGET: DocTarget = {
  kind: "symbol",
  path: "main.ink",
  name: "start",
  start: START_KNOT_OFFSET,
  end: START_KNOT_END,
};

// ── Harness ─────────────────────────────────────────────────────────

interface Mounted {
  view: EditorView;
  dispose: () => void;
  container: HTMLElement;
}

class Harness {
  readonly project: ProjectSession;
  readonly documents: DocumentSessions;
  readonly callbacks: DocumentCallbacks;
  private readonly containers: HTMLElement[] = [];

  constructor(project: ProjectSession, callbacks: DocumentCallbacks) {
    this.project = project;
    this.callbacks = callbacks;
    this.documents = new DocumentSessions(project, callbacks);
  }

  mount(docKey: string, groupId: string): Mounted {
    const container = document.createElement("div");
    document.body.appendChild(container);
    this.containers.push(container);
    const dispose = this.documents.mountView(docKey, groupId, container);
    const view = viewIn(container);
    return { view, dispose, container };
  }

  cleanup(): void {
    this.documents.dispose();
    for (const el of this.containers) el.remove();
  }
}

function viewIn(container: HTMLElement): EditorView {
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return view;
}

function typeAt(view: EditorView, pos: number, text: string): void {
  view.dispatch({ changes: { from: pos, insert: text } });
}

function docText(view: EditorView): string {
  return view.state.doc.toString();
}

async function createHarness(
  files: Record<string, string> = { "main.ink": MAIN_INK },
  callbacks: DocumentCallbacks = {},
): Promise<Harness> {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const entryFile = Object.keys(files)[0]!;
  const project = new ProjectSession({ provider, entryFile });
  await project.initialize();
  return new Harness(project, callbacks);
}

// ── Tests ───────────────────────────────────────────────────────────

describe("DocumentSessions", () => {
  let harness: Harness;

  beforeEach(async () => {
    harness = await createHarness();
  });

  afterEach(() => {
    harness.cleanup();
  });

  describe("mounting", () => {
    it("mounts a file view with the full file content", () => {
      const { view } = harness.mount("main.ink", "group-1");
      expect(docText(view)).toBe(MAIN_INK);
    });

    it("mounts a symbol view with the fragment content (live resolution)", () => {
      harness.documents.noteTarget(START_TARGET);
      const { view } = harness.mount("main.ink::start", "group-1");
      expect(docText(view)).toBe(START_KNOT_TEXT);
    });

    it("resolves symbol ranges from the session even without a hint", () => {
      const { view } = harness.mount("main.ink::story", "group-1");
      expect(docText(view)).toBe(STORY_KNOT_TEXT);
    });

    it("degrades an unknown symbol to the full file", () => {
      const { view } = harness.mount("main.ink::missing", "group-1");
      expect(docText(view)).toBe(MAIN_INK);
    });

    it("unmount closes the handle; remount restores content and selection", () => {
      const mounted = harness.mount("main.ink", "group-1");
      mounted.view.dispatch({ selection: { anchor: 10 } });
      mounted.dispose();

      const again = harness.mount("main.ink", "group-1");
      expect(docText(again.view)).toBe(MAIN_INK);
      expect(again.view.state.selection.main.head).toBe(10);
    });

    it("remount rebuilds when the file changed underneath", () => {
      const mounted = harness.mount("main.ink", "group-1");
      mounted.dispose();

      harness.project.getSession().updateFile("main.ink", "fresh content\n");
      const again = harness.mount("main.ink", "group-1");
      expect(docText(again.view)).toBe("fresh content\n");
    });
  });

  describe("splice-back (fragment handles)", () => {
    it("edits in a symbol view splice into the full file", () => {
      const { view } = harness.mount("main.ink::start", "group-1");
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "=== start ===\nModified content!\n" },
      });

      const full = harness.project.getSession().getFileSource("main.ink")!;
      expect(full).toContain("Modified content!");
      expect(full).toContain("// Welcome to brink studio!");
      expect(full).toContain("=== story ===");
      expect(full).toContain("Once upon a time.");
      // Content before the knot is byte-identical.
      expect(full.slice(0, full.indexOf("=== start ==="))).toBe(
        MAIN_INK.slice(0, START_KNOT_OFFSET),
      );
    });

    it("successive edits keep the fragment range in sync", () => {
      const { view } = harness.mount("main.ink::start", "group-1");
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "=== start ===\nEdit 1.\n" },
      });
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "=== start ===\nEdit 2 is longer!\n" },
      });

      const full = harness.project.getSession().getFileSource("main.ink")!;
      expect(full).toContain("Edit 2 is longer!");
      expect(full).not.toContain("Edit 1.");
      expect(full).toContain("=== story ===");
    });
  });

  describe("same-document mirroring (split)", () => {
    it("forwards edits between two views of the same file", () => {
      const a = harness.mount("main.ink", "group-1");
      const b = harness.mount("main.ink", "group-2");

      typeAt(a.view, 0, "// typed in A\n");
      expect(docText(b.view)).toBe(docText(a.view));

      typeAt(b.view, 5, "typed in B ");
      expect(docText(a.view)).toBe(docText(b.view));
    });

    it("mirrored transactions don't echo or count as edits in the sibling", async () => {
      const edited = vi.fn();
      const h = await createHarness(
        { "main.ink": MAIN_INK },
        { onDocEdited: edited },
      );
      try {
        const a = h.mount("main.ink", "group-1");
        const b = h.mount("main.ink", "group-2");

        typeAt(a.view, 0, "x");
        // Contents converge and stay put (no ping-pong growth).
        expect(docText(b.view)).toBe(docText(a.view));
        expect(docText(a.view)).toBe("x" + MAIN_INK);
        // Only the source view reports a user edit (auto-pin stays per-view).
        expect(edited).toHaveBeenCalledTimes(1);
        expect(edited).toHaveBeenCalledWith("main.ink", "group-1");
      } finally {
        h.cleanup();
      }
    });

    it("keeps selection per-view", () => {
      const a = harness.mount("main.ink", "group-1");
      const b = harness.mount("main.ink", "group-2");
      b.view.dispatch({ selection: { anchor: 20 } });

      a.view.dispatch({ changes: { from: 0, insert: "ab" }, selection: { anchor: 2 } });
      expect(a.view.state.selection.main.head).toBe(2);
      // B's selection stays its own (mapped through the mirrored change).
      expect(b.view.state.selection.main.head).toBe(22);
    });

    it("mirrors between two views of the same fragment", () => {
      harness.documents.noteTarget(START_TARGET);
      const a = harness.mount("main.ink::start", "group-1");
      const b = harness.mount("main.ink::start", "group-2");
      expect(docText(b.view)).toBe(START_KNOT_TEXT);

      typeAt(a.view, START_KNOT_TEXT.indexOf("Hello"), "Oh! ");
      expect(docText(b.view)).toBe(docText(a.view));

      // The file got exactly one splice.
      const full = harness.project.getSession().getFileSource("main.ink")!;
      expect(full).toContain("Oh! Hello, world!");
      expect(full.indexOf("Oh! ")).toBe(full.lastIndexOf("Oh! "));
    });
  });

  describe("fragment⇄file mirroring", () => {
    it("fragment edits live-appear in a full-file view of the same file", () => {
      const file = harness.mount("main.ink", "group-1");
      harness.documents.noteTarget(START_TARGET);
      const frag = harness.mount("main.ink::start", "group-2");

      typeAt(frag.view, START_KNOT_TEXT.indexOf("Hello"), "Why, ");
      expect(docText(file.view)).toBe(
        harness.project.getSession().getFileSource("main.ink")!,
      );
      expect(docText(file.view)).toContain("Why, Hello, world!");
    });

    it("file edits outside the fragment shift its range without touching it", () => {
      harness.documents.noteTarget(START_TARGET);
      const frag = harness.mount("main.ink::start", "group-1");
      const file = harness.mount("main.ink", "group-2");

      // Edit before the fragment in the file view.
      typeAt(file.view, 0, "// prefix\n");
      // Fragment content unchanged...
      expect(docText(frag.view)).toBe(START_KNOT_TEXT);
      // ...and further fragment edits splice at the *new* location.
      typeAt(frag.view, START_KNOT_TEXT.indexOf("Hello"), "Hey! ");
      const full = harness.project.getSession().getFileSource("main.ink")!;
      expect(full.startsWith("// prefix\n")).toBe(true);
      expect(full).toContain("Hey! Hello, world!");
      expect(full.indexOf("Hey! ")).toBe(full.lastIndexOf("Hey! "));
    });

    it("file edits inside the fragment refresh the fragment view", () => {
      harness.documents.noteTarget(START_TARGET);
      const frag = harness.mount("main.ink::start", "group-1");
      const file = harness.mount("main.ink", "group-2");

      const pos = MAIN_INK.indexOf("Hello");
      typeAt(file.view, pos, "Well ");
      expect(docText(frag.view)).toContain("Well Hello, world!");
    });
  });

  describe("auto-import in a fragment view (#312 F)", () => {
    // The fragment-view completion-accept path: an out-of-scope divert accepted
    // in a symbol tab must (1) prepend the INCLUDE to the whole file and (2)
    // rebase the open fragment view so the SUBSEQUENT symbol dispatch splices at
    // the shifted range — NOT clobber the INCLUDE line / knot header. This is
    // the regression the raw-applyEdit path caused (stale fragment offsets).
    const FILES = {
      "main.ink": "=== start ===\nThe cursor is here.\n",
      "economy.ink": "=== trade ===\nbuy.\n-> END\n",
    };
    const START_FRAG: DocTarget = {
      kind: "symbol",
      path: "main.ink",
      name: "start",
      start: 0,
      end: FILES["main.ink"].length,
    };

    // Read the live wasm handle out of a mounted view's facet.
    function handleOf(view: EditorView) {
      const slot = view.state.facet(documentHandleFacet);
      const handle = slot?.handle;
      if (!handle) throw new Error("no handle on view");
      return handle;
    }

    it("prepends the INCLUDE and rebases the fragment so the next push doesn't corrupt the file", async () => {
      const h = await createHarness(FILES);
      try {
        h.documents.noteTarget(START_FRAG);
        const { view } = h.mount("main.ink::start", "group-1");
        const handle = handleOf(view);
        expect(handle.fragmentRange()).not.toBeNull();

        // Accept an out-of-scope completion → apply-and-rebase the INCLUDE.
        const result = handle.autoImportApply("economy.ink");
        expect(result.ok).toBe(true);
        expect(result.already_reachable).toBe(false);
        // The apply path returns no edit for the caller to dispatch.
        expect(result.edit ?? null).toBeNull();

        // The whole file now carries the INCLUDE above the untouched knot.
        expect(h.project.getSession().getFileSource("main.ink")).toBe(
          "INCLUDE economy.ink\n=== start ===\nThe cursor is here.\n",
        );

        // Now the completion dispatches the symbol into the FRAGMENT view. This
        // triggers pushSource → update_document, splicing at the rebased range.
        const body = view.state.doc.toString();
        const cursor = body.indexOf("here.") + "here.".length;
        view.dispatch({ changes: { from: cursor, insert: " -> trade" } });

        // INCLUDE + knot header intact; only the body changed.
        expect(h.project.getSession().getFileSource("main.ink")).toBe(
          "INCLUDE economy.ink\n=== start ===\nThe cursor is here. -> trade\n",
        );
      } finally {
        h.cleanup();
      }
    });

    it("adds no INCLUDE when the file already reaches the target (idempotent)", async () => {
      const h = await createHarness({
        "main.ink": "INCLUDE economy.ink\n=== start ===\nbody.\n",
        "economy.ink": "=== trade ===\n-> END\n",
      });
      try {
        const frag: DocTarget = {
          kind: "symbol",
          path: "main.ink",
          name: "start",
          start: 0,
          end: "INCLUDE economy.ink\n=== start ===\nbody.\n".length,
        };
        h.documents.noteTarget(frag);
        const { view } = h.mount("main.ink::start", "group-1");
        const handle = handleOf(view);

        const result = handle.autoImportApply("economy.ink");
        expect(result.already_reachable).toBe(true);

        // Exactly one INCLUDE, and the fragment push still lands correctly.
        const cursor = view.state.doc.toString().indexOf("body.") + "body.".length;
        view.dispatch({ changes: { from: cursor, insert: " -> trade" } });
        const full = h.project.getSession().getFileSource("main.ink")!;
        expect(full.match(/INCLUDE economy\.ink/g) ?? []).toHaveLength(1);
        expect(full).toContain("=== start ===");
        expect(full).toContain("body. -> trade");
      } finally {
        h.cleanup();
      }
    });
  });

  describe("invalidateFile", () => {
    it("reloads mounted file views from the session", () => {
      const { view } = harness.mount("main.ink", "group-1");
      harness.project.getSession().updateFile("main.ink", "replaced\n");
      harness.documents.invalidateFile("main.ink");
      expect(docText(view)).toBe("replaced\n");
    });

    it("re-resolves symbol views, degrading to the file when gone", () => {
      harness.documents.noteTarget(START_TARGET);
      const { view } = harness.mount("main.ink::start", "group-1");
      const without = "// no knots anymore\n-> END\n";
      harness.project.getSession().updateFile("main.ink", without);
      harness.documents.invalidateFile("main.ink");
      expect(docText(view)).toBe(without);
    });
  });

  describe("renameSymbolDoc (#305)", () => {
    it("re-keys an open symbol view to the renamed knot", () => {
      harness.documents.noteTarget(START_TARGET);
      const { view } = harness.mount("main.ink::start", "group-1");
      expect(docText(view)).toBe(START_KNOT_TEXT);

      // Rename `start` → `begin` in the file, then re-key the open symbol view —
      // the same flow the studio runs after applyMoveResult.
      const renamed = MAIN_INK.replace("=== start ===", "=== begin ===");
      harness.project.getSession().updateFile("main.ink", renamed);
      harness.documents.invalidateFile("main.ink"); // `start` gone → degrades
      harness.documents.renameSymbolDoc("main.ink", "start", "begin");

      // The same view now shows the renamed knot, re-resolved by the new name —
      // not the degraded full file.
      const begin = renamed.slice(
        renamed.indexOf("=== begin ==="),
        renamed.indexOf("=== story ==="),
      );
      expect(docText(view)).toBe(begin);
    });
  });

  describe("focus tracking", () => {
    it("reports the focused view via onFocusedViewChange", async () => {
      const focusChanges: (EditorView | null)[] = [];
      const h = await createHarness(
        { "main.ink": MAIN_INK },
        { onFocusedViewChange: (v) => focusChanges.push(v) },
      );
      try {
        const a = h.mount("main.ink", "group-1");
        h.documents.setFocused("main.ink", "group-1");
        expect(h.documents.getFocusedView()).toBe(a.view);
        expect(focusChanges.at(-1)).toBe(a.view);

        const b = h.mount("main.ink::story", "group-2");
        h.documents.setFocused("main.ink::story", "group-2");
        expect(h.documents.getFocusedView()).toBe(b.view);
      } finally {
        h.cleanup();
      }
    });

    it("applies focus side effects when the view mounts after setFocused", () => {
      harness.documents.setFocused("main.ink", "group-1");
      expect(harness.documents.getFocusedView()).toBeNull();
      const { view } = harness.mount("main.ink", "group-1");
      expect(harness.documents.getFocusedView()).toBe(view);
    });
  });

  describe("insertAtCursor (StudioApi insertText path, spec §8.2)", () => {
    it("inserts at the cursor in the focused view and moves the cursor after", () => {
      const { view } = harness.mount("main.ink", "group-1");
      harness.documents.setFocused("main.ink", "group-1");
      view.dispatch({ selection: { anchor: 3 } });
      harness.documents.insertAtCursor("EXTERNAL has(item)\n");
      expect(docText(view)).toBe(
        `${MAIN_INK.slice(0, 3)}EXTERNAL has(item)\n${MAIN_INK.slice(3)}`,
      );
      expect(view.state.selection.main.head).toBe(3 + "EXTERNAL has(item)\n".length);
    });

    it("replaces a selection", () => {
      const { view } = harness.mount("main.ink", "group-1");
      harness.documents.setFocused("main.ink", "group-1");
      view.dispatch({ selection: { anchor: 0, head: 2 } });
      harness.documents.insertAtCursor("XY");
      expect(docText(view)).toBe(`XY${MAIN_INK.slice(2)}`);
    });

    it("is a no-op when no view is focused", () => {
      const { view } = harness.mount("main.ink", "group-1");
      expect(harness.documents.getFocusedView()).toBeNull();
      harness.documents.insertAtCursor("nope");
      expect(docText(view)).toBe(MAIN_INK);
    });
  });

  describe("reveal", () => {
    it("selects and scrolls once the view is mounted", () => {
      harness.documents.revealAt("main.ink", 30);
      const { view } = harness.mount("main.ink", "group-1");
      expect(view.state.selection.main.head).toBe(30);
    });

    it("applies immediately to an already-mounted view", () => {
      const { view } = harness.mount("main.ink", "group-1");
      harness.documents.revealAt("main.ink", 12);
      expect(view.state.selection.main.head).toBe(12);
    });
  });

  describe("view state (#347)", () => {
    it("reads cursor + scroll from the live view when mounted", () => {
      const { view } = harness.mount("main.ink", "group-1");
      view.dispatch({ selection: { anchor: 5, head: 12 } });
      view.scrollDOM.scrollTop = 40;
      expect(harness.documents.viewState("main.ink", "group-1")).toEqual({
        anchor: 5,
        head: 12,
        scrollTop: 40,
      });
    });

    it("reads from the cached slot after unmount", () => {
      const mounted = harness.mount("main.ink", "group-1");
      mounted.view.dispatch({ selection: { anchor: 5, head: 12 } });
      mounted.view.scrollDOM.scrollTop = 40;
      mounted.dispose(); // backgrounded: EditorState + scroll snapshot only

      expect(harness.documents.viewState("main.ink", "group-1")).toEqual({
        anchor: 5,
        head: 12,
        scrollTop: 40,
      });
    });

    it("snapshots every open tab, not just the focused view", () => {
      // Two docKeys in two groups, neither ever focused.
      const a = harness.mount("main.ink", "group-1");
      const b = harness.mount("main.ink::story", "group-2");
      a.view.dispatch({ selection: { anchor: 2, head: 7 } });
      b.view.dispatch({ selection: { anchor: 1, head: 4 } });
      b.dispose(); // one live, one cached

      expect(harness.documents.viewState("main.ink", "group-1")).toMatchObject({
        anchor: 2,
        head: 7,
      });
      expect(harness.documents.viewState("main.ink::story", "group-2")).toMatchObject({
        anchor: 1,
        head: 4,
      });
    });

    it("prefers a mounted slot when groupId is omitted", () => {
      const a = harness.mount("main.ink", "group-1");
      const b = harness.mount("main.ink", "group-2");
      a.view.dispatch({ selection: { anchor: 3 } });
      b.view.dispatch({ selection: { anchor: 9 } });
      b.dispose();

      expect(harness.documents.viewState("main.ink")).toMatchObject({ anchor: 3 });
    });

    it("returns null for an unknown docKey", () => {
      expect(harness.documents.viewState("nope.ink")).toBeNull();
      expect(harness.documents.viewState("main.ink", "group-1")).toBeNull();
    });

    it("restores selection + scroll on the next mount", () => {
      harness.documents.restoreViewState("main.ink", {
        anchor: 3,
        head: 9,
        scrollTop: 25,
      });
      const { view } = harness.mount("main.ink", "group-1");
      expect(view.state.selection.main.anchor).toBe(3);
      expect(view.state.selection.main.head).toBe(9);
      expect(view.scrollDOM.scrollTop).toBe(25);
    });

    it("applies immediately to an already-mounted view", () => {
      const { view } = harness.mount("main.ink", "group-1");
      harness.documents.restoreViewState("main.ink", {
        anchor: 4,
        head: 11,
        scrollTop: 15,
      });
      expect(view.state.selection.main.anchor).toBe(4);
      expect(view.state.selection.main.head).toBe(11);
      expect(view.scrollDOM.scrollTop).toBe(15);
    });

    it("restores every open tab independently (queued per docKey)", () => {
      harness.documents.restoreViewState("main.ink", {
        anchor: 2,
        head: 8,
        scrollTop: 10,
      });
      harness.documents.restoreViewState("main.ink::story", {
        anchor: 1,
        head: 5,
        scrollTop: 6,
      });

      const a = harness.mount("main.ink", "group-1");
      const b = harness.mount("main.ink::story", "group-2");
      expect(a.view.state.selection.main.anchor).toBe(2);
      expect(a.view.state.selection.main.head).toBe(8);
      expect(a.view.scrollDOM.scrollTop).toBe(10);
      expect(b.view.state.selection.main.anchor).toBe(1);
      expect(b.view.state.selection.main.head).toBe(5);
      expect(b.view.scrollDOM.scrollTop).toBe(6);
    });

    it("clamps a stale selection to the document length", () => {
      harness.documents.restoreViewState("main.ink", {
        anchor: 100_000,
        head: 100_000,
        scrollTop: 0,
      });
      const { view } = harness.mount("main.ink", "group-1");
      expect(view.state.selection.main.head).toBe(view.state.doc.length);
    });

    it("keeps the scroll snapshot across an in-session remount", () => {
      const mounted = harness.mount("main.ink", "group-1");
      mounted.view.scrollDOM.scrollTop = 33;
      mounted.dispose();

      const again = harness.mount("main.ink", "group-1");
      expect(again.view.scrollDOM.scrollTop).toBe(33);
    });
  });

  describe("slot pruning", () => {
    it("drops cached states for closed tabs but keeps mounted ones", () => {
      const a = harness.mount("main.ink", "group-1");
      const b = harness.mount("main.ink::story", "group-1");
      b.dispose(); // backgrounded: cached state only

      harness.documents.retainSlots(
        new Set([DocumentSessions.slotId("main.ink", "group-1")]),
        new Set(["main.ink"]),
      );

      // The pruned slot remounts from scratch (content from the session).
      const again = harness.mount("main.ink::story", "group-1");
      expect(docText(again.view)).toBe(STORY_KNOT_TEXT);
      expect(docText(a.view)).toBe(MAIN_INK);
    });
  });

  describe("compile", () => {
    it("delivers compile results once per distinct result", async () => {
      const results: unknown[] = [];
      const h = await createHarness(
        { "main.ink": MAIN_INK },
        { onCompileResult: (r) => results.push(r) },
      );
      try {
        h.documents.triggerCompile();
        h.documents.triggerCompile(); // cached + reference-equal → collapsed
        expect(results).toHaveLength(1);

        h.project.getSession().updateFile("main.ink", "-> END\n");
        h.documents.triggerCompile();
        expect(results).toHaveLength(2);
      } finally {
        h.cleanup();
      }
    });
  });

  describe("syncAnnotation export", () => {
    it("is defined (mirror transactions are annotated with it)", () => {
      expect(syncAnnotation).toBeDefined();
    });
  });
});
