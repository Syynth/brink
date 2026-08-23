/**
 * The out-of-scope editor banner + status note (#3017 — compare
 * `docs/design/project-open-flow/ScopeBanner.dc.html`): a source file the
 * latest compile's closure does not contain gets a banner above its
 * editor ("not analyzed", with "Add INCLUDE to <entry>" for the ink flow)
 * and a "— file not analyzed" status-bar note. Absent diagnostics look
 * identical to clean diagnostics; these are what make the difference
 * visible.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { InkFileDocument, ScopeNoteSegment, StoreProvider, isOutOfScope } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [], mounted: false },
  { path: "offcuts.ink", symbols: [], mounted: false },
  { path: "std/screenplay.brink", symbols: [], mounted: true },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function seedStore() {
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  store.getState().setClosureFiles(["main.ink"]);
  return store;
}

/** A minimal ProjectSession stand-in for the entry lookup + edit path. */
function fakeProject(entrySource: string) {
  const applied: Array<{ path: string; source: string }> = [];
  const project = {
    getEntryFile: () => "main.ink",
    getSession: () => ({ getFileSource: (p: string) => (p === "main.ink" ? entrySource : null) }),
    applyEdit: (path: string, source: string) => {
      applied.push({ path, source });
      return true;
    },
  };
  return { project, applied };
}

function mount(store: ReturnType<typeof createStudioStore>, element: React.ReactElement) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: element }));
  });
}

function inkDoc(docId: string) {
  return createElement(InkFileDocument, {
    doc: { typeId: "ink-file", docId, title: docId },
    groupId: "g1",
    active: true,
  });
}

describe("isOutOfScope", () => {
  const outline = OUTLINE;
  it("is false before the first compile — an empty closure asserts nothing", () => {
    expect(isOutOfScope("offcuts.ink", [], outline)).toBe(false);
  });
  it("is true for a source file the closure omits", () => {
    expect(isOutOfScope("offcuts.ink", ["main.ink"], outline)).toBe(true);
  });
  it("is false for closure members, non-source files, and mounted stdlib", () => {
    expect(isOutOfScope("main.ink", ["main.ink"], outline)).toBe(false);
    expect(isOutOfScope("brink.toml", ["main.ink"], outline)).toBe(false);
    expect(isOutOfScope("std/screenplay.brink", ["main.ink"], outline)).toBe(false);
  });
});

describe("InkFileDocument scope banner", () => {
  it("shows the banner for an out-of-closure file, with Add INCLUDE for the ink flow", () => {
    const store = seedStore();
    const { project } = fakeProject("Hello.\n");
    store.setState({ _project: project as never });
    mount(store, inkDoc("offcuts.ink"));
    const banner = container!.querySelector(".brink-scope-banner");
    expect(banner).not.toBeNull();
    expect(banner?.textContent).toContain("Not included in the project");
    expect(
      container!.querySelector<HTMLButtonElement>(".scope-banner-include")?.textContent,
    ).toBe("Add INCLUDE to main.ink");
  });

  it("shows no banner for a file in the closure", () => {
    const store = seedStore();
    mount(store, inkDoc("main.ink"));
    expect(container!.querySelector(".brink-scope-banner")).toBeNull();
  });

  it("Add INCLUDE inserts into the entry, refreshes its view, and recompiles", () => {
    const store = seedStore();
    const { project, applied } = fakeProject("Hello.\n");
    const refreshExternal = vi.fn();
    const triggerCompile = vi.fn();
    store.setState({
      _project: project as never,
      _documents: { refreshExternal, triggerCompile, mountView: () => () => {} } as never,
    });
    mount(store, inkDoc("offcuts.ink"));
    act(() => {
      container!.querySelector<HTMLButtonElement>(".scope-banner-include")?.click();
    });
    expect(applied).toEqual([{ path: "main.ink", source: "INCLUDE offcuts.ink\nHello.\n" }]);
    expect(refreshExternal).toHaveBeenCalledWith("main.ink");
    expect(triggerCompile).toHaveBeenCalledTimes(1);
  });

  it("the banner appears even on a fragment (symbol) document of the file", () => {
    const store = seedStore();
    mount(store, inkDoc("offcuts.ink::abandoned_branch"));
    expect(container!.querySelector(".brink-scope-banner")).not.toBeNull();
  });
});

describe("ScopeNoteSegment", () => {
  it("renders the note only while the active document's file is out of scope", () => {
    const store = seedStore();
    store.setState({ activeDocKey: "offcuts.ink" });
    mount(store, createElement(ScopeNoteSegment));
    expect(container!.textContent).toContain("— file not analyzed");
    act(() => {
      store.setState({ activeDocKey: "main.ink" });
    });
    expect(container!.textContent).not.toContain("file not analyzed");
  });
});
