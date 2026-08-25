/**
 * HIR overlay initial-refresh regression (#494).
 *
 * The overlay's projection StateField seeds at view creation — before the
 * first async compile/analysis completes — so on a passive load
 * `getHirProjection` returns the empty projection and, since the field only
 * recomputed on doc-changing transactions, the overlay stayed blank until the
 * user's first edit.
 *
 * Covers both halves of the fix:
 * 1. `refreshHirOverlay(view)` / `refreshHirOverlayEffect` — the exported
 *    host seam: re-reads the projection without a doc change.
 * 2. `DocumentSessions` — automatic refresh: delivering a compile result
 *    dispatches the effect to every mounted view, so passive loads paint as
 *    soon as the initial compile lands.
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts), whose
 * `hir_spans_doc` is settable via `setMockHirProjection` — empty by default,
 * mirroring a real session before its first analysis.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
  hirOverlayExtension,
  refreshHirOverlay,
} from "@brink-lang/editor";
import type { HirProjection } from "@brink/wasm-types";
import { initWasm } from "@brink-lang/web";
import { setMockHirProjection } from "../__mocks__/brink-web";

const EMPTY: HirProjection = { spans: [], lines: [] };

const MAIN_INK = "=== start ===\nHello world\n-> done\n";

/** A projection for MAIN_INK: a knot container over all three lines plus a
 *  divert reference on the last line. */
const PROJECTION: HirProjection = {
  spans: [
    {
      start_line: 0,
      start_char: 0,
      end_line: 2,
      end_char: 7,
      kind: "knot",
      container: true,
      depth: 0,
      def_id: "$k1",
      handle: 1,
    },
    {
      start_line: 2,
      start_char: 3,
      end_line: 2,
      end_char: 7,
      kind: "divert",
      container: false,
      depth: 1,
      target_id: "$k2",
    },
  ],
  lines: [
    [{ kind: "knot", handle: 1, depth: 0 }],
    [{ kind: "knot", handle: 1, depth: 0 }],
    [{ kind: "knot", handle: 1, depth: 0 }],
  ],
};

function markEls(v: EditorView): HTMLElement[] {
  return Array.from(v.dom.querySelectorAll<HTMLElement>("[data-hir-kind]"));
}

function railEls(v: EditorView): HTMLElement[] {
  return Array.from(v.dom.querySelectorAll<HTMLElement>(".brink-hir-rail"));
}

// ── 1. The exported host seam ───────────────────────────────────────

describe("refreshHirOverlay", () => {
  let view: EditorView | null = null;

  afterEach(() => {
    view?.destroy();
    view = null;
  });

  function mount(getHirProjection: () => HirProjection): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: MAIN_INK,
        extensions: [hirOverlayExtension({ getHirProjection })],
      }),
      parent: document.body,
    });
    return view;
  }

  it("re-reads the projection without a doc change", () => {
    let projection = EMPTY;
    const v = mount(() => projection);

    // Seeded before "analysis": nothing rendered.
    expect(markEls(v)).toHaveLength(0);
    expect(railEls(v)).toHaveLength(0);

    // The projection populates (compile/analysis completed); no doc change.
    projection = PROJECTION;
    refreshHirOverlay(v);

    const marks = markEls(v);
    expect(marks).toHaveLength(1);
    expect(marks[0].getAttribute("data-hir-kind")).toBe("divert");
    expect(marks[0].classList.contains("brink-hir-divert")).toBe(true);
    expect(railEls(v).length).toBeGreaterThan(0);
    expect(v.dom.querySelector('[data-hir-rails="knot"]')).not.toBeNull();
    expect(v.state.doc.toString()).toBe(MAIN_INK); // truly no edit
  });

  it("keeps the last-good state when a refresh fetch throws (R5)", () => {
    let projection: HirProjection | null = PROJECTION;
    const v = mount(() => {
      if (projection === null) throw new Error("transient");
      return projection;
    });
    expect(markEls(v)).toHaveLength(1);

    projection = null;
    refreshHirOverlay(v);
    expect(markEls(v)).toHaveLength(1); // not dropped
  });
});

// ── 2. DocumentSessions automatic refresh ───────────────────────────

describe("DocumentSessions HIR overlay refresh on compile delivery", () => {
  const containers: HTMLElement[] = [];
  let documents: DocumentSessions | null = null;

  afterEach(() => {
    setMockHirProjection(null);
    documents?.dispose();
    documents = null;
    for (const el of containers) el.remove();
    containers.length = 0;
  });

  function mount(docs: DocumentSessions, docKey: string, groupId: string): EditorView {
    const container = document.createElement("div");
    document.body.appendChild(container);
    containers.push(container);
    docs.mountView(docKey, groupId, container);
    const dom = container.querySelector(".cm-editor");
    const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
    if (!view) throw new Error("no editor mounted");
    return view;
  }

  it("populates a passively loaded view when the compile result lands", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    documents = new DocumentSessions(project);

    const view = mount(documents, "main.ink", "g1");

    // Passive load: the field seeded from the pre-analysis (empty) projection.
    expect(markEls(view)).toHaveLength(0);
    expect(railEls(view)).toHaveLength(0);

    // Analysis completes and the compile result is delivered — no doc change,
    // no user edit.
    setMockHirProjection(JSON.stringify(PROJECTION));
    documents.triggerCompile();
    await new Promise((r) => setTimeout(r, 0)); // W4: async compile landing

    const marks = markEls(view);
    expect(marks).toHaveLength(1);
    expect(marks[0].getAttribute("data-hir-kind")).toBe("divert");
    expect(railEls(view).length).toBeGreaterThan(0);
    expect(view.state.doc.toString()).toBe(MAIN_INK);
  });
});

// ── 3. View mounts AFTER the compile was delivered (#518) ───────────

describe("DocumentSessions HIR overlay refresh on view mount", () => {
  const containers: HTMLElement[] = [];
  let documents: DocumentSessions | null = null;

  afterEach(() => {
    setMockHirProjection(null);
    documents?.dispose();
    documents = null;
    for (const el of containers) el.remove();
    containers.length = 0;
  });

  function mountWithDispose(
    docs: DocumentSessions,
    docKey: string,
    groupId: string,
  ): { view: EditorView; dispose: () => void } {
    const container = document.createElement("div");
    document.body.appendChild(container);
    containers.push(container);
    const dispose = docs.mountView(docKey, groupId, container);
    const dom = container.querySelector(".cm-editor");
    const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
    if (!view) throw new Error("no editor mounted");
    return { view, dispose };
  }

  function mount(docs: DocumentSessions, docKey: string, groupId: string): EditorView {
    return mountWithDispose(docs, docKey, groupId).view;
  }

  async function makeSessions(): Promise<DocumentSessions> {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    documents = new DocumentSessions(project);
    return documents;
  }

  it("populates a view that mounts after the delivered compile — no doc edit", async () => {
    const docs = await makeSessions();

    // The compile result lands while NO view is mounted (the external
    // embedder order, #518: initialize → triggerCompile → the framework
    // commits the editor mount afterwards). deliverCompile's per-view
    // refresh loop finds nothing to refresh.
    setMockHirProjection(JSON.stringify(PROJECTION));
    docs.triggerCompile();
    await new Promise((r) => setTimeout(r, 0)); // W4: async compile landing

    // A passive load never compiles again, and no edit happens — the mount
    // itself must self-serve the missed refresh.
    const view = mount(docs, "main.ink", "g1");

    const marks = markEls(view);
    expect(marks).toHaveLength(1);
    expect(marks[0].getAttribute("data-hir-kind")).toBe("divert");
    expect(railEls(view).length).toBeGreaterThan(0);
    expect(view.state.doc.toString()).toBe(MAIN_INK); // truly no edit
  });

  it("populates a REMOUNTED view whose cached state predates the compile", async () => {
    const docs = await makeSessions();

    // Mounted before any analysis/compile: the overlay field seeds empty …
    const first = mountWithDispose(docs, "main.ink", "g1");
    expect(markEls(first.view)).toHaveLength(0);

    // … and that (blank) EditorState is cached at unmount.
    first.dispose();

    // The compile result lands while the slot has no view — deliverCompile
    // skips it (the refresh is dropped, not queued).
    setMockHirProjection(JSON.stringify(PROJECTION));
    docs.triggerCompile();
    await new Promise((r) => setTimeout(r, 0)); // W4: async compile landing

    // Remount reuses the cached EditorState (content unchanged), so the
    // overlay field's create() never re-runs — without the mount refresh the
    // blank value cached at unmount would persist until the first edit.
    const view = mount(docs, "main.ink", "g1");

    const marks = markEls(view);
    expect(marks).toHaveLength(1);
    expect(marks[0].getAttribute("data-hir-kind")).toBe("divert");
    expect(railEls(view).length).toBeGreaterThan(0);
    expect(view.state.doc.toString()).toBe(MAIN_INK);
  });
});
