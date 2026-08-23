/**
 * The brink.toml form view (#3015): renders above the raw text editor for
 * any open brink.toml, offers the project's ACTUAL files for entry (a
 * typo'd entry reproduces #3010 — free text is how that happens), applies
 * comment-preserving structured edits through the session, and flags a
 * configured value that names a missing file instead of rewriting it.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ConfigFormPanel, InkFileDocument, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [], mounted: false },
  { path: "scenes/harbour.ink", symbols: [], mounted: false },
  { path: "screenplay.brink", symbols: [], mounted: false },
  { path: "std/lib.brink", symbols: [], mounted: true },
];

const CONFIG = `# keep me\n[project]\nentry = "main.ink"\n`;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function fakeProject(initial: string) {
  let source = initial;
  const applied: string[] = [];
  return {
    applied,
    getSource: () => source,
    project: {
      getEntryFile: () => "main.ink",
      getSession: () => ({
        getFileSource: (p: string) => (p === "brink.toml" ? source : null),
      }),
      applyEdit: (_path: string, next: string) => {
        source = next;
        applied.push(next);
        return true;
      },
    },
  };
}

function seededStore(project: unknown) {
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  store.setState({
    _project: project as never,
    _documents: {
      refreshExternal: vi.fn(),
      triggerCompile: vi.fn(),
      mountView: () => () => {},
    } as never,
  });
  return store;
}

function mountPanel(store: ReturnType<typeof createStudioStore>) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(StoreProvider, {
        store,
        children: createElement(ConfigFormPanel, { path: "brink.toml" }),
      }),
    );
  });
}

function select(key: string): HTMLSelectElement {
  const el = container!.querySelector<HTMLSelectElement>(`select[data-config-key="${key}"]`);
  if (el === null) throw new Error(`select not found: ${key}`);
  return el;
}

describe("ConfigFormPanel", () => {
  it("derives field values from the file and offers real project files for entry", () => {
    const { project } = fakeProject(CONFIG);
    mountPanel(seededStore(project));
    expect(select("entry").value).toBe("main.ink");
    const entryOptions = [...select("entry").options].map((o) => o.value);
    expect(entryOptions).toContain("scenes/harbour.ink");
    expect(entryOptions).toContain("screenplay.brink");
    expect(entryOptions).not.toContain("std/lib.brink");
    // Conventions offers .brink modules only.
    const convOptions = [...select("conventions").options].map((o) => o.value);
    expect(convOptions).toContain("screenplay.brink");
    expect(convOptions).not.toContain("main.ink");
  });

  it("applies a comment-preserving edit and recompiles", () => {
    const { project, applied } = fakeProject(CONFIG);
    const store = seededStore(project);
    mountPanel(store);
    act(() => {
      const el = select("entry");
      el.value = "scenes/harbour.ink";
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(applied).toHaveLength(1);
    expect(applied[0]).toBe(`# keep me\n[project]\nentry = "scenes/harbour.ink"\n`);
    const docs = store.getState()._documents as unknown as {
      refreshExternal: ReturnType<typeof vi.fn>;
      triggerCompile: ReturnType<typeof vi.fn>;
    };
    expect(docs.refreshExternal).toHaveBeenCalledWith("brink.toml");
    expect(docs.triggerCompile).toHaveBeenCalledTimes(1);
    // The select reflects the new value immediately (session re-read).
    expect(select("entry").value).toBe("scenes/harbour.ink");
  });

  it("flags a configured entry that names a missing file instead of rewriting it", () => {
    const { project } = fakeProject(`[project]\nentry = "gone.ink"\n`);
    mountPanel(seededStore(project));
    const entry = select("entry");
    expect(entry.value).toBe("gone.ink");
    const missing = [...entry.options].find((o) => o.value === "gone.ink");
    expect(missing?.label).toBe("gone.ink (missing)");
  });

  it("clears a key via (not set)", () => {
    const { project, applied } = fakeProject(CONFIG);
    mountPanel(seededStore(project));
    act(() => {
      const el = select("entry");
      el.value = "";
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(applied[0]).toBe(`# keep me\n[project]\n`);
  });
});

describe("InkFileDocument config integration", () => {
  it("renders the form panel for an open brink.toml, and not for a story file", () => {
    const { project } = fakeProject(CONFIG);
    const store = seededStore(project);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    const doc = (docId: string) =>
      createElement(InkFileDocument, { doc: { typeId: "ink-file", docId, title: docId }, groupId: "g1" });
    act(() => {
      root!.render(createElement(StoreProvider, { store, children: doc("brink.toml") }));
    });
    expect(container.querySelector(".brink-config-form")).not.toBeNull();
    act(() => {
      root!.render(createElement(StoreProvider, { store, children: doc("main.ink") }));
    });
    expect(container.querySelector(".brink-config-form")).toBeNull();
  });
});
