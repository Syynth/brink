/**
 * Draft status through the studio surfaces (#3145).
 *
 * The definition — `matches a [project] drafts glob && !reachable from the
 * entry` — is computed and tested on the Rust side (`draft_paths`); the
 * mock's own agreement with the Rust glob dialect is pinned by
 * `drafts-glob-dialect.test.ts`. What this file covers is the half neither
 * of those can: what the STUDIO does once a file is a draft.
 *
 * Ruled 2026-08-27 — a file's name and its draft status never appear
 * apart, and a draft shows no "not included" banner (that is the whole
 * point of declaring it). Both are asserted here against real components.
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  Binder,
  DocumentIcon,
  InkFileDocument,
  StoreProvider,
  inkFileRef,
} from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [], mounted: false },
  { path: "offcuts.ink", symbols: [], mounted: false },
  { path: "scratch/cut.ink", symbols: [], mounted: false },
];

const CLOSURE = ["main.ink"];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function seededStore(drafts: string[]) {
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  store.getState().setClosureFiles(CLOSURE);
  store.getState().setEntryFile("main.ink");
  store.getState().setDraftFiles(drafts);
  return store;
}

function mount(store: ReturnType<typeof createStudioStore>, child: React.ReactNode) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: child }));
  });
}

function rowFor(path: string): HTMLElement | null {
  for (const el of container!.querySelectorAll("[data-binder-row-key]")) {
    if (el.getAttribute("data-binder-row-key") === path) return el as HTMLElement;
  }
  return null;
}

describe("the Binder row (#3145)", () => {
  it("draws a draft with the draft icon and no `not included` badge", () => {
    mount(seededStore(["scratch/cut.ink"]), createElement(Binder));

    const draft = rowFor("scratch/cut.ink");
    // Decision log 2026-08-27: the status lives in the ICON, not a badge.
    expect(draft?.querySelector(".brink-file-icon-draft")).not.toBeNull();
    // The distinction that matters: "not included" states a problem, and a
    // draft is not one, so that badge must not appear.
    expect(draft?.querySelector(".brink-binder-badge-muted")).toBeNull();
    // Not dimmed either — dimming reads as "lesser", and a draft is
    // deliberate rather than degraded.
    expect(draft?.classList.contains("brink-binder-dimmed")).toBe(false);
  });

  it("leaves an unmarked out-of-scope file saying `not included`", () => {
    // The control: draft status must be doing the work here, not the mere
    // fact of being outside the closure.
    mount(seededStore(["scratch/cut.ink"]), createElement(Binder));

    const offcuts = rowFor("offcuts.ink");
    expect(offcuts?.querySelector(".brink-binder-badge-muted")?.textContent).toBe("not included");
    expect(offcuts?.querySelector(".brink-file-icon-draft")).toBeNull();
    expect(offcuts?.classList.contains("brink-binder-dimmed")).toBe(true);
  });

  it("shows no draft icon when nothing is a draft", () => {
    mount(seededStore([]), createElement(Binder));
    expect(container!.querySelector(".brink-file-icon-draft")).toBeNull();
  });
});

describe("the out-of-scope banner (#3145)", () => {
  it("is suppressed for a draft", () => {
    mount(
      seededStore(["scratch/cut.ink"]),
      createElement(InkFileDocument, {
        doc: inkFileRef({ kind: "file", path: "scratch/cut.ink" }),
        groupId: "g1",
        active: true,
      }),
    );
    expect(container!.querySelector(".brink-scope-banner")).toBeNull();
  });

  it("still shows for an out-of-scope file that is not a draft", () => {
    // Planted control for the assertion above: without this, a banner that
    // never rendered in this harness at all would look like a pass.
    mount(
      seededStore(["scratch/cut.ink"]),
      createElement(InkFileDocument, {
        doc: inkFileRef({ kind: "file", path: "offcuts.ink" }),
        groupId: "g1",
        active: true,
      }),
    );
    expect(container!.querySelector(".brink-scope-banner")).not.toBeNull();
  });
});

describe("DocumentIcon — the shell's documentIcon (#3145)", () => {
  it("draws the draft variant for a draft file", () => {
    mount(
      seededStore(["scratch/cut.ink"]),
      createElement(DocumentIcon, { doc: inkFileRef({ kind: "file", path: "scratch/cut.ink" }) }),
    );
    expect(container!.querySelector(".brink-file-icon-draft")).not.toBeNull();
  });

  it("draws the ordinary file icon for a non-draft file", () => {
    // Not "renders nothing": every file gets an icon now, so the assertion
    // has to distinguish the two icons rather than presence from absence.
    mount(
      seededStore(["scratch/cut.ink"]),
      createElement(DocumentIcon, { doc: inkFileRef({ kind: "file", path: "main.ink" }) }),
    );
    expect(container!.querySelector(".brink-doc-icon")).not.toBeNull();
    expect(container!.querySelector(".brink-file-icon-draft")).toBeNull();
  });

  it("renders nothing for a document that is not a file", () => {
    // Settings and the Story Graph name themselves, not a path — a mark
    // there would be claiming something about a document with no file.
    mount(
      seededStore(["scratch/cut.ink"]),
      createElement(DocumentIcon, { doc: { typeId: "settings", docId: "scratch/cut.ink" } }),
    );
    expect(container!.querySelector(".brink-doc-icon")).toBeNull();
  });

  it("follows a symbol (fragment) document back to its file", () => {
    // Fragment doc ids are `path::symbol`; the draft set holds paths.
    mount(
      seededStore(["scratch/cut.ink"]),
      createElement(DocumentIcon, {
        doc: inkFileRef({
          kind: "symbol",
          path: "scratch/cut.ink",
          name: "intro",
          start: 0,
          end: 0,
        }),
      }),
    );
    expect(container!.querySelector(".brink-file-icon-draft")).not.toBeNull();
  });
});
